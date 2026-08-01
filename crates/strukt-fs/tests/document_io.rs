use std::fs;

use strukt_editor::DiskRevision;
use strukt_fs::{
    DocumentIoError, DocumentKind, ReadOptions, SaveMode, SaveRequest, read_document, save_document,
};
use strukt_workspace::WorkspaceRoot;
use tempfile::TempDir;

struct Fixture {
    directory: TempDir,
    root: WorkspaceRoot,
}

impl Fixture {
    fn new(path: &str, bytes: &[u8]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(path), bytes).unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        Self { directory, root }
    }

    fn read(&self, path: &str) -> Vec<u8> {
        fs::read(self.directory.path().join(path)).unwrap()
    }
}

#[test]
fn reads_utf8_and_preserves_crlf() {
    let fixture = Fixture::new("file.txt", b"one\r\ntwo\r\n");
    let opened = read_document(&fixture.root, "file.txt", ReadOptions::default()).unwrap();

    assert_eq!(opened.text.as_deref(), Some("one\r\ntwo\r\n"));
    assert_eq!(opened.size, 10);
    assert_eq!(
        opened.kind,
        DocumentKind::Text {
            read_only: false,
            truncated: false
        }
    );
}

#[test]
fn classifies_nul_and_invalid_utf8_without_exposing_text() {
    let binary = Fixture::new("binary", b"text\0payload");
    let opened = read_document(&binary.root, "binary", ReadOptions::default()).unwrap();
    assert_eq!(opened.kind, DocumentKind::Binary);
    assert!(opened.text.is_none());

    let invalid = Fixture::new("invalid", &[0xf0, 0x28, 0x8c, 0x28]);
    let opened = read_document(&invalid.root, "invalid", ReadOptions::default()).unwrap();
    assert_eq!(opened.kind, DocumentKind::InvalidUtf8);
    assert!(opened.text.is_none());
}

#[test]
fn large_files_open_as_truncated_preview_unless_explicitly_overridden() {
    let content = vec![b'x'; 4 * 1024 * 1024 + 1];
    let fixture = Fixture::new("large.txt", &content);
    let opened = read_document(&fixture.root, "large.txt", ReadOptions::default()).unwrap();

    assert_eq!(opened.size, content.len() as u64);
    assert_eq!(opened.text.as_ref().unwrap().len(), 1024 * 1024);
    assert_eq!(
        opened.kind,
        DocumentKind::Text {
            read_only: true,
            truncated: true
        }
    );

    let forced = read_document(
        &fixture.root,
        "large.txt",
        ReadOptions {
            force_full: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert_eq!(forced.text.as_ref().unwrap().len(), content.len());
    assert_eq!(
        forced.kind,
        DocumentKind::Text {
            read_only: false,
            truncated: false
        }
    );
}

#[test]
fn streamed_large_preview_keeps_utf8_boundaries_and_validates_the_full_file() {
    let valid = Fixture::new("unicode.txt", "abcédef".as_bytes());
    let opened = read_document(
        &valid.root,
        "unicode.txt",
        ReadOptions {
            max_editable_bytes: 4,
            preview_bytes: 4,
            force_full: false,
        },
    )
    .unwrap();
    assert_eq!(opened.text.as_deref(), Some("abc"));
    assert_eq!(
        opened.disk_revision,
        DiskRevision::new(blake3::hash("abcédef".as_bytes()).to_hex().to_string())
    );

    let mut invalid = vec![b'a'; 1024 * 1024];
    invalid.push(0xff);
    let invalid = Fixture::new("invalid-large.txt", &invalid);
    let opened = read_document(
        &invalid.root,
        "invalid-large.txt",
        ReadOptions {
            max_editable_bytes: 1024,
            preview_bytes: 16,
            force_full: false,
        },
    )
    .unwrap();
    assert_eq!(opened.kind, DocumentKind::InvalidUtf8);
    assert!(opened.text.is_none());
}

#[test]
fn rejects_traversal_and_non_regular_files() {
    let fixture = Fixture::new("file.txt", b"text");
    fs::create_dir(fixture.directory.path().join("directory")).unwrap();
    assert!(matches!(
        read_document(&fixture.root, "../outside", ReadOptions::default()),
        Err(DocumentIoError::OutsideRoot(_))
    ));

    assert!(matches!(
        read_document(&fixture.root, "directory", ReadOptions::default()),
        Err(DocumentIoError::NotRegularFile(_))
    ));
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("file.txt", b"text");
    let outside = tempfile::NamedTempFile::new().unwrap();
    symlink(outside.path(), fixture.directory.path().join("escape")).unwrap();

    assert!(matches!(
        read_document(&fixture.root, "escape", ReadOptions::default()),
        Err(DocumentIoError::Symlink(_))
    ));
}

#[cfg(unix)]
#[test]
fn rejects_fifo_before_opening_it() {
    use std::process::Command;

    let fixture = Fixture::new("file.txt", b"text");
    assert!(
        Command::new("mkfifo")
            .arg(fixture.directory.path().join("pipe"))
            .status()
            .unwrap()
            .success()
    );

    assert!(matches!(
        read_document(&fixture.root, "pipe", ReadOptions::default()),
        Err(DocumentIoError::NotRegularFile(_))
    ));
}

#[test]
fn rejects_replaced_workspace_root() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("file.txt"), b"retained").unwrap();
    let root = WorkspaceRoot::open(&original).unwrap();
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("file.txt"), b"replacement").unwrap();

    assert!(matches!(
        read_document(&root, "file.txt", ReadOptions::default()),
        Err(DocumentIoError::WorkspaceChanged)
    ));
}

#[cfg(unix)]
#[test]
fn save_does_not_follow_a_replaced_parent_outside_the_workspace() {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().unwrap();
    let workspace = parent.path().join("workspace");
    let outside = parent.path().join("outside");
    fs::create_dir_all(workspace.join("nested")).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(workspace.join("nested/file.txt"), b"before").unwrap();
    fs::write(outside.join("file.txt"), b"outside").unwrap();
    let root = WorkspaceRoot::open(&workspace).unwrap();
    let opened = read_document(&root, "nested/file.txt", ReadOptions::default()).unwrap();
    fs::remove_file(workspace.join("nested/file.txt")).unwrap();
    fs::remove_dir(workspace.join("nested")).unwrap();
    symlink(&outside, workspace.join("nested")).unwrap();

    assert!(
        save_document(
            &root,
            &SaveRequest::new("nested/file.txt", b"local".to_vec(), opened.disk_revision),
        )
        .is_err()
    );
    assert_eq!(fs::read(outside.join("file.txt")).unwrap(), b"outside");
}

#[test]
fn saves_only_the_expected_disk_revision() {
    let fixture = Fixture::new("file.txt", b"before");
    let opened = read_document(&fixture.root, "file.txt", ReadOptions::default()).unwrap();
    let saved = save_document(
        &fixture.root,
        &SaveRequest::new("file.txt", b"after".to_vec(), opened.disk_revision.clone()),
    )
    .unwrap();

    assert_eq!(fixture.read("file.txt"), b"after");
    assert_ne!(saved.disk_revision, opened.disk_revision);
}

#[test]
fn changed_disk_revision_is_never_knowingly_overwritten() {
    let fixture = Fixture::new("file.txt", b"before");
    let opened = read_document(&fixture.root, "file.txt", ReadOptions::default()).unwrap();
    fs::write(fixture.directory.path().join("file.txt"), b"external").unwrap();

    let error = save_document(
        &fixture.root,
        &SaveRequest::new("file.txt", b"local".to_vec(), opened.disk_revision),
    )
    .unwrap_err();
    assert!(matches!(error, DocumentIoError::SaveConflict { .. }));
    assert_eq!(fixture.read("file.txt"), b"external");
}

#[test]
fn force_save_is_explicit() {
    let fixture = Fixture::new("file.txt", b"before");
    fs::write(fixture.directory.path().join("file.txt"), b"external").unwrap();

    save_document(
        &fixture.root,
        &SaveRequest::new("file.txt", b"local".to_vec(), DiskRevision::new("stale"))
            .with_mode(SaveMode::Force),
    )
    .unwrap();
    assert_eq!(fixture.read("file.txt"), b"local");
}

#[cfg(unix)]
#[test]
fn save_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("file.txt", b"before");
    fs::set_permissions(
        fixture.directory.path().join("file.txt"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    let opened = read_document(&fixture.root, "file.txt", ReadOptions::default()).unwrap();

    save_document(
        &fixture.root,
        &SaveRequest::new("file.txt", b"after".to_vec(), opened.disk_revision),
    )
    .unwrap();

    assert_eq!(
        fs::metadata(fixture.directory.path().join("file.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
}
