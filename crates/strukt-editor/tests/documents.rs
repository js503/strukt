use strukt_editor::{
    DiskRevision, Document, DocumentError, DocumentStatus, EditKind, EditTransaction,
    RelativeDocumentPath, Revision,
};

fn document(path: &str, text: &str) -> Document {
    Document::new(
        RelativeDocumentPath::new(path).unwrap(),
        text,
        DiskRevision::new("disk-1"),
        false,
    )
}

#[test]
fn document_ids_are_unique_and_paths_are_normalized() {
    let first = document("src\\main.rs", "one");
    let second = document("src/main.rs", "two");
    assert_ne!(first.id(), second.id());
    assert_eq!(first.path().as_str(), "src/main.rs");
    assert_eq!(
        RelativeDocumentPath::new("../outside"),
        Err(DocumentError::InvalidPath("../outside".into()))
    );
}

#[test]
fn editing_marks_dirty_and_save_completion_sets_a_new_baseline() {
    let mut document = document("file.txt", "one");
    document
        .edit(
            EditTransaction::insert(Revision::INITIAL, 3, " local"),
            EditKind::Typing,
            3,
            9,
        )
        .unwrap();
    assert_eq!(document.status(), &DocumentStatus::Dirty);
    let revision = document.revision();
    document
        .complete_save(revision, DiskRevision::new("disk-2"))
        .unwrap();
    assert_eq!(document.status(), &DocumentStatus::Clean);
    assert_eq!(document.disk_revision(), &DiskRevision::new("disk-2"));
}

#[test]
fn document_scoped_undo_and_redo_update_dirty_state() {
    let mut document = document("file.txt", "one");
    document
        .edit(
            EditTransaction::insert(Revision::INITIAL, 3, " local"),
            EditKind::Other,
            3,
            9,
        )
        .unwrap();
    document.undo().unwrap();
    assert_eq!(document.text(), "one");
    assert_eq!(document.status(), &DocumentStatus::Clean);
    document.redo().unwrap();
    assert_eq!(document.text(), "one local");
    assert_eq!(document.status(), &DocumentStatus::Dirty);
}

#[test]
fn read_only_documents_refuse_edits() {
    let mut document = Document::new(
        RelativeDocumentPath::new("large.log").unwrap(),
        "preview",
        DiskRevision::new("disk-1"),
        true,
    );
    assert_eq!(
        document.edit(
            EditTransaction::insert(Revision::INITIAL, 0, "x"),
            EditKind::Other,
            0,
            1,
        ),
        Err(DocumentError::ReadOnly)
    );
}

#[test]
fn clean_external_change_reloads_content() {
    let mut document = document("file.txt", "one");
    document
        .observe_disk_change(document.revision(), DiskRevision::new("disk-2"), "disk")
        .unwrap();
    assert_eq!(document.text(), "disk");
    assert_eq!(document.status(), &DocumentStatus::Clean);
}

#[test]
fn dirty_external_change_preserves_local_content() {
    let mut document = document("src/main.rs", "one");
    document
        .edit(
            EditTransaction::insert(Revision::INITIAL, 3, " local"),
            EditKind::Typing,
            3,
            9,
        )
        .unwrap();
    document
        .observe_disk_change(document.revision(), DiskRevision::new("disk-2"), "disk")
        .unwrap();
    assert_eq!(document.text(), "one local");
    assert!(matches!(
        document.status(),
        DocumentStatus::Conflict { disk_text, .. } if disk_text == "disk"
    ));
}

#[test]
fn deleted_dirty_file_remains_recoverable_and_stale_events_are_rejected() {
    let mut document = document("file.txt", "one");
    document
        .edit(
            EditTransaction::insert(Revision::INITIAL, 3, " local"),
            EditKind::Typing,
            3,
            9,
        )
        .unwrap();
    assert_eq!(
        document.observe_missing(Revision::INITIAL),
        Err(DocumentError::StaleEvent {
            expected: document.revision(),
            actual: Revision::INITIAL,
        })
    );
    document.observe_missing(document.revision()).unwrap();
    assert_eq!(document.status(), &DocumentStatus::Missing);
    assert_eq!(document.text(), "one local");
    assert!(document.is_recoverable());
}
