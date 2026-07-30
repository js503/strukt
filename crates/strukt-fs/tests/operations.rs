use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use strukt_fs::{FileOperation, OperationError, apply_operation};
use tempfile::tempdir;

fn assert_already_exists(result: Result<(), OperationError>) {
    match result {
        Err(OperationError::Io(error)) => assert_eq!(error.kind(), ErrorKind::AlreadyExists),
        other => panic!("expected AlreadyExists IO error, got {other:?}"),
    }
}

fn assert_rejected(result: &Result<(), OperationError>) {
    assert!(result.is_err(), "operation unexpectedly succeeded");
}

fn assert_no_duplicate_staging_entries(root: &Path) {
    let staging_entries = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| {
            name.to_string_lossy()
                .starts_with(".strukt-duplicate-stage-")
        })
        .collect::<Vec<_>>();
    assert!(
        staging_entries.is_empty(),
        "unexpected duplicate staging entries: {staging_entries:?}"
    );
}

#[test]
fn create_rename_and_duplicate_stay_inside_the_workspace() {
    let root = tempdir().unwrap();

    apply_operation(root.path(), FileOperation::CreateFile("notes.txt".into())).unwrap();
    fs::write(root.path().join("notes.txt"), "workspace notes").unwrap();
    apply_operation(
        root.path(),
        FileOperation::Rename {
            from: "notes.txt".into(),
            to: "renamed.txt".into(),
        },
    )
    .unwrap();
    apply_operation(
        root.path(),
        FileOperation::Duplicate {
            from: "renamed.txt".into(),
            to: "copy.txt".into(),
        },
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.path().join("renamed.txt")).unwrap(),
        "workspace notes"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("copy.txt")).unwrap(),
        "workspace notes"
    );
    assert_no_duplicate_staging_entries(root.path());
}

#[test]
fn parent_traversal_is_rejected_without_creating_an_outside_file() {
    let parent = tempdir().unwrap();
    let root_path = parent.path().join("workspace");
    fs::create_dir(&root_path).unwrap();
    let outside_path = parent.path().join("escape.txt");

    assert_rejected(&apply_operation(
        &root_path,
        FileOperation::CreateFile("../escape.txt".into()),
    ));

    assert!(!outside_path.exists());
}

#[test]
fn permanent_delete_requires_an_explicit_operation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("delete.txt"), "content").unwrap();

    apply_operation(
        root.path(),
        FileOperation::DeletePermanently("delete.txt".into()),
    )
    .unwrap();

    assert!(!root.path().join("delete.txt").exists());
}

#[test]
fn rename_move_and_duplicate_never_overwrite_existing_destinations() {
    for operation in ["rename", "move", "duplicate"] {
        let root = tempdir().unwrap();
        fs::write(root.path().join("source.txt"), "source").unwrap();
        fs::write(root.path().join("destination.txt"), "destination").unwrap();

        let operation = match operation {
            "rename" => FileOperation::Rename {
                from: "source.txt".into(),
                to: "destination.txt".into(),
            },
            "move" => FileOperation::Move {
                from: "source.txt".into(),
                to: "destination.txt".into(),
            },
            "duplicate" => FileOperation::Duplicate {
                from: "source.txt".into(),
                to: "destination.txt".into(),
            },
            _ => unreachable!(),
        };

        assert_already_exists(apply_operation(root.path(), operation));
        assert_eq!(
            fs::read_to_string(root.path().join("source.txt")).unwrap(),
            "source"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("destination.txt")).unwrap(),
            "destination"
        );
    }
}

#[test]
fn duplicating_a_directory_into_its_descendant_is_rejected_without_mutation() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("source")).unwrap();
    fs::write(root.path().join("source/original.txt"), "original").unwrap();

    assert_rejected(&apply_operation(
        root.path(),
        FileOperation::Duplicate {
            from: "source".into(),
            to: "source/copy".into(),
        },
    ));

    assert!(!root.path().join("source/copy").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("source/original.txt")).unwrap(),
        "original"
    );
}

#[test]
fn empty_or_current_directory_targets_cannot_mutate_the_workspace_root() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("preserved.txt"), "preserved").unwrap();

    for target in [Path::new(""), Path::new(".")] {
        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::DeletePermanently(target.into()),
        ));
        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::Rename {
                from: target.into(),
                to: "renamed-root".into(),
            },
        ));
    }

    assert!(root.path().is_dir());
    assert_eq!(
        fs::read_to_string(root.path().join("preserved.txt")).unwrap(),
        "preserved"
    );
    assert!(!root.path().join("renamed-root").exists());
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    const FIFO_HELPER_ROOT: &str = "STRUKT_FS_FIFO_HELPER_ROOT";

    #[test]
    fn duplicate_preserves_executable_permissions() {
        let root = tempdir().unwrap();
        let source = root.path().join("script.sh");
        fs::write(&source, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();

        apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "script.sh".into(),
                to: "script-copy.sh".into(),
            },
        )
        .unwrap();

        let copied_mode = fs::metadata(root.path().join("script-copy.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(copied_mode & 0o777, 0o755);
    }

    #[test]
    fn duplicate_preserves_directory_permissions_after_copying_children() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("child.txt"), "child").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o555)).unwrap();

        apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "source".into(),
                to: "copy".into(),
            },
        )
        .unwrap();

        let copied_mode = fs::metadata(root.path().join("copy"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(copied_mode & 0o777, 0o555);
        assert_eq!(
            fs::read_to_string(root.path().join("copy/child.txt")).unwrap(),
            "child"
        );
    }

    #[test]
    fn special_fifo_is_rejected_promptly_without_creating_a_destination() {
        let root = tempdir().unwrap();
        let fifo = root.path().join("source.fifo");
        let status = Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(status.success());

        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "unix::fifo_duplicate_helper", "--nocapture"])
            .env(FIFO_HELPER_ROOT, root.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "FIFO helper rejected incorrectly");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("FIFO duplicate did not return promptly");
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(!root.path().join("copy.fifo").exists());
    }

    #[test]
    fn fifo_duplicate_helper() {
        let Some(root) = std::env::var_os(FIFO_HELPER_ROOT) else {
            return;
        };
        let root = Path::new(&root).to_path_buf();
        match apply_operation(
            &root,
            FileOperation::Duplicate {
                from: "source.fifo".into(),
                to: "copy.fifo".into(),
            },
        ) {
            Err(OperationError::Io(error)) => assert_eq!(error.kind(), ErrorKind::InvalidInput),
            other => panic!("expected InvalidInput IO error, got {other:?}"),
        }
        assert!(!root.join("copy.fifo").exists());
    }

    #[test]
    fn failed_directory_duplicate_removes_its_destination_for_retry() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("a-copied-first.txt"), "first").unwrap();
        let unreadable = source.join("z-unreadable.txt");
        fs::write(&unreadable, "content").unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

        let result = apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "source".into(),
                to: "copy".into(),
            },
        );

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o600)).unwrap();
        assert_rejected(&result);
        assert!(!root.path().join("copy").exists());
        assert_no_duplicate_staging_entries(root.path());

        apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "source".into(),
                to: "copy".into(),
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("copy/z-unreadable.txt")).unwrap(),
            "content"
        );
        assert_no_duplicate_staging_entries(root.path());
    }

    #[test]
    fn duplicate_rejects_symlinks_that_escape_the_workspace() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        symlink(
            outside.path().join("secret.txt"),
            root.path().join("link.txt"),
        )
        .unwrap();

        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "link.txt".into(),
                to: "copy.txt".into(),
            },
        ));
        assert!(!root.path().join("copy.txt").exists());
    }

    #[test]
    fn permanent_delete_of_an_outside_directory_symlink_removes_only_the_link() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("preserved.txt"), "preserved").unwrap();
        symlink(outside.path(), root.path().join("outside-link")).unwrap();

        apply_operation(
            root.path(),
            FileOperation::DeletePermanently("outside-link".into()),
        )
        .unwrap();

        assert!(!root.path().join("outside-link").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("preserved.txt")).unwrap(),
            "preserved"
        );
    }

    #[test]
    fn nested_symlink_rejects_directory_duplicate_before_destination_creation() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir(root.path().join("source")).unwrap();
        fs::write(root.path().join("source/original.txt"), "original").unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        symlink(
            outside.path().join("secret.txt"),
            root.path().join("source/nested-link"),
        )
        .unwrap();

        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "source".into(),
                to: "copy".into(),
            },
        ));

        assert!(!root.path().join("copy").exists());
    }

    #[test]
    fn in_root_symlink_alias_destination_is_rejected_without_mutation() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("source")).unwrap();
        fs::write(root.path().join("source/original.txt"), "original").unwrap();
        symlink(root.path().join("source"), root.path().join("alias")).unwrap();

        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "source".into(),
                to: "alias/copy".into(),
            },
        ));

        assert!(!root.path().join("source/copy").exists());
        assert_no_duplicate_staging_entries(root.path());
    }

    #[test]
    fn symlinked_parent_outside_root_is_rejected_for_create_and_destination_paths() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), root.path().join("outside-parent")).unwrap();
        fs::write(root.path().join("source.txt"), "source").unwrap();

        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::CreateFile("outside-parent/created.txt".into()),
        ));
        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::Move {
                from: "source.txt".into(),
                to: "outside-parent/moved.txt".into(),
            },
        ));
        assert_rejected(&apply_operation(
            root.path(),
            FileOperation::Duplicate {
                from: "outside-parent/secret.txt".into(),
                to: "copied-secret.txt".into(),
            },
        ));

        assert!(!outside.path().join("created.txt").exists());
        assert!(!outside.path().join("moved.txt").exists());
        assert!(!root.path().join("copied-secret.txt").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("source.txt")).unwrap(),
            "source"
        );
    }
}

#[test]
fn case_insensitive_alias_destination_is_rejected_when_the_filesystem_has_one() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("Source")).unwrap();
    fs::write(root.path().join("Source/original.txt"), "original").unwrap();
    let canonical_source = fs::canonicalize(root.path().join("Source")).unwrap();
    let Ok(canonical_alias) = fs::canonicalize(root.path().join("source")) else {
        return;
    };
    if canonical_alias != canonical_source {
        return;
    }

    assert_rejected(&apply_operation(
        root.path(),
        FileOperation::Duplicate {
            from: "Source".into(),
            to: "source/copy".into(),
        },
    ));

    assert!(!root.path().join("Source/copy").exists());
    assert_no_duplicate_staging_entries(root.path());
}

#[cfg(windows)]
#[test]
fn permanent_delete_of_an_outside_directory_symlink_removes_only_the_link() {
    use std::os::windows::fs::symlink_dir;

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("preserved.txt"), "preserved").unwrap();
    if let Err(error) = symlink_dir(outside.path(), root.path().join("outside-link")) {
        if error.raw_os_error() == Some(1314) {
            return;
        }
        panic!("failed to create Windows directory symlink: {error}");
    }

    apply_operation(
        root.path(),
        FileOperation::DeletePermanently("outside-link".into()),
    )
    .unwrap();

    assert!(!root.path().join("outside-link").exists());
    assert_eq!(
        fs::read_to_string(outside.path().join("preserved.txt")).unwrap(),
        "preserved"
    );
}
