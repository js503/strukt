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
    use std::os::unix::fs::symlink;

    use super::*;

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

        assert!(!outside.path().join("created.txt").exists());
        assert!(!outside.path().join("moved.txt").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("source.txt")).unwrap(),
            "source"
        );
    }
}
