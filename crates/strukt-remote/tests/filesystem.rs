use std::fs;

use strukt_remote::{
    RemoteDocumentKind, RemoteFilesystem, RemoteFilesystemError, RemotePath, RemoteWatchInput,
    RemoteWatchSequencer,
};
use tempfile::tempdir;

fn fixture() -> (tempfile::TempDir, RemoteFilesystem) {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\nneedle\n").unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(root.path().join("binary.bin"), b"a\0b").unwrap();
    fs::write(root.path().join("invalid.txt"), [0xff, 0xfe]).unwrap();
    fs::write(root.path().join(".hidden"), "hidden").unwrap();
    fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(root.path().join("ignored.txt"), "ignored needle").unwrap();
    let filesystem = RemoteFilesystem::open(root.path()).unwrap();
    (root, filesystem)
}

#[test]
fn directory_listing_is_deterministic_and_paged() {
    let (_root, filesystem) = fixture();
    let first = filesystem.list(&RemotePath::root(), None, 3).unwrap();
    assert_eq!(first.entries.len(), 3);
    assert!(first.next_cursor.is_some());
    let second = filesystem
        .list(&RemotePath::root(), first.next_cursor.as_deref(), 10)
        .unwrap();
    let mut names = first
        .entries
        .iter()
        .chain(&second.entries)
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>();
    let original = names.clone();
    names.sort();
    assert_eq!(original, names);
    assert!(
        filesystem
            .list(&RemotePath::root(), Some("bad"), 2)
            .is_err()
    );
    assert!(filesystem.list(&RemotePath::root(), None, 0).is_err());
}

#[test]
fn reads_classify_text_binary_and_invalid_utf8_with_revisions() {
    let (_root, filesystem) = fixture();
    let text = filesystem
        .read(&RemotePath::new("src/main.rs").unwrap())
        .unwrap();
    assert_eq!(text.kind, RemoteDocumentKind::Text);
    assert_eq!(text.bytes, b"fn main() {}\nneedle\n");
    assert_eq!(text.revision.len(), 64);
    assert_eq!(
        filesystem
            .read(&RemotePath::new("binary.bin").unwrap())
            .unwrap()
            .kind,
        RemoteDocumentKind::Binary
    );
    assert_eq!(
        filesystem
            .read(&RemotePath::new("invalid.txt").unwrap())
            .unwrap()
            .kind,
        RemoteDocumentKind::InvalidUtf8
    );
}

#[test]
fn conditional_atomic_save_detects_conflicts_and_preserves_new_data() {
    let (root, filesystem) = fixture();
    let path = RemotePath::new("src/main.rs").unwrap();
    let original = filesystem.read(&path).unwrap();
    let saved = filesystem
        .save(
            &path,
            b"fn main() { println!(\"remote\"); }\n",
            &original.revision,
            false,
        )
        .unwrap();
    assert_ne!(saved.revision, original.revision);
    assert_eq!(saved.bytes_written, 34);
    assert!(matches!(
        filesystem.save(&path, b"stale", &original.revision, false),
        Err(RemoteFilesystemError::Conflict { .. })
    ));
    assert_eq!(
        fs::read(root.path().join("src/main.rs")).unwrap(),
        b"fn main() { println!(\"remote\"); }\n"
    );
    filesystem
        .save(&path, b"forced", &original.revision, true)
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("src/main.rs")).unwrap(),
        b"forced"
    );
}

#[test]
fn enumeration_search_and_limits_reuse_workspace_policy() {
    let (_root, filesystem) = fixture();
    let report = filesystem.enumerate(false, false, 100).unwrap();
    assert!(report.paths.iter().any(|path| path == "src/main.rs"));
    assert!(!report.paths.iter().any(|path| path == ".hidden"));
    assert!(!report.paths.iter().any(|path| path == "ignored.txt"));

    let accepted = filesystem.enumerate(true, true, 100).unwrap();
    assert!(accepted.paths.iter().any(|path| path == ".hidden"));
    assert!(accepted.paths.iter().any(|path| path == "ignored.txt"));

    let result = filesystem.search("needle", false, 10).unwrap();
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path, "src/main.rs");
    assert_eq!(result.matches[0].line, 2);
    assert!(filesystem.search("needle", false, 0).is_err());
}

#[test]
fn watch_sequences_are_monotonic_and_overflow_requires_resync_generation() {
    let mut sequencer = RemoteWatchSequencer::new(2).unwrap();
    let first = sequencer.accept(RemoteWatchInput::Changed(vec![
        "src/lib.rs".into(),
        "src/main.rs".into(),
    ]));
    assert!(!first.stale);
    assert_eq!(first.generation, 1);
    assert_eq!(first.events[0].sequence, 0);
    let second = sequencer.accept(RemoteWatchInput::Changed(vec!["README.md".into()]));
    assert_eq!(second.events[0].sequence, 1);
    assert_eq!(sequencer.cursor(), (1, 2));

    let overflow = sequencer.accept(RemoteWatchInput::Changed(vec![
        "a".into(),
        "b".into(),
        "c".into(),
    ]));
    assert!(overflow.stale);
    assert_eq!(overflow.generation, 2);
    assert!(overflow.events.is_empty());
    assert_eq!(sequencer.cursor(), (2, 0));

    let recovered = sequencer.accept(RemoteWatchInput::Changed(vec!["fresh".into()]));
    assert_eq!(recovered.generation, 2);
    assert_eq!(recovered.events[0].sequence, 0);
}
