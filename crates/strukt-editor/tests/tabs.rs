use strukt_editor::{
    CloseDecision, CloseOutcome, DiskRevision, DocumentStatus, EditKind, EditTransaction,
    EditorWorkspace, OpenDisposition, RelativeDocumentPath, Revision,
};
use strukt_workspace::WorkspaceId;

fn workspace_id() -> WorkspaceId {
    serde_json::from_str(&format!("\"{}\"", "a".repeat(64))).unwrap()
}

fn open(workspace: &mut EditorWorkspace, path: &str, disposition: OpenDisposition) {
    workspace
        .open(
            RelativeDocumentPath::new(path).unwrap(),
            path,
            DiskRevision::new(format!("disk-{path}")),
            false,
            disposition,
        )
        .unwrap();
}

#[test]
fn clean_preview_is_replaced_and_existing_path_is_reused() {
    let mut workspace = EditorWorkspace::new(workspace_id());
    open(&mut workspace, "one.rs", OpenDisposition::Preview);
    let first = workspace.active_document_id().unwrap();
    open(&mut workspace, "two.rs", OpenDisposition::Preview);
    assert_eq!(workspace.document_count(), 1);
    assert_ne!(workspace.active_document_id(), Some(first));
    let second = workspace.active_document_id().unwrap();
    open(&mut workspace, "two.rs", OpenDisposition::Pinned);
    assert_eq!(workspace.document_count(), 1);
    assert_eq!(workspace.active_document_id(), Some(second));
    assert!(workspace.view_state().tabs[0].pinned);
}

#[test]
fn editing_a_preview_pins_it_before_another_preview_opens() {
    let mut workspace = EditorWorkspace::new(workspace_id());
    open(&mut workspace, "one.rs", OpenDisposition::Preview);
    let first = workspace.active_document_id().unwrap();
    workspace
        .edit(
            first,
            EditTransaction::insert(Revision::INITIAL, 0, "x"),
            EditKind::Typing,
            0,
            1,
        )
        .unwrap();
    open(&mut workspace, "two.rs", OpenDisposition::Preview);
    assert_eq!(workspace.document_count(), 2);
    assert_eq!(
        workspace.document(first).unwrap().status(),
        &DocumentStatus::Dirty
    );
}

#[test]
fn recovered_conflicted_and_missing_documents_cannot_be_preview_replaced() {
    let mut workspace = EditorWorkspace::new(workspace_id());
    open(&mut workspace, "recovered.rs", OpenDisposition::Preview);
    let recovered = workspace.active_document_id().unwrap();
    workspace.mark_recovered(recovered).unwrap();
    open(&mut workspace, "conflict.rs", OpenDisposition::Preview);
    let conflict = workspace.active_document_id().unwrap();
    workspace
        .edit(
            conflict,
            EditTransaction::insert(Revision::INITIAL, 0, "x"),
            EditKind::Typing,
            0,
            1,
        )
        .unwrap();
    let revision = workspace.document(conflict).unwrap().revision();
    workspace
        .observe_disk_change(conflict, revision, DiskRevision::new("changed"), "disk")
        .unwrap();
    open(&mut workspace, "missing.rs", OpenDisposition::Preview);
    let missing = workspace.active_document_id().unwrap();
    let revision = workspace.document(missing).unwrap().revision();
    workspace.observe_missing(missing, revision).unwrap();
    open(&mut workspace, "final.rs", OpenDisposition::Preview);
    assert_eq!(workspace.document_count(), 4);
    assert!(
        workspace
            .view_state()
            .tabs
            .iter()
            .find(|tab| tab.id == recovered)
            .unwrap()
            .pinned
    );
}

#[test]
fn close_decisions_preserve_dirty_work_and_choose_an_active_fallback() {
    let mut workspace = EditorWorkspace::new(workspace_id());
    open(&mut workspace, "one.rs", OpenDisposition::Pinned);
    let first = workspace.active_document_id().unwrap();
    open(&mut workspace, "two.rs", OpenDisposition::Pinned);
    let second = workspace.active_document_id().unwrap();
    workspace
        .edit(
            second,
            EditTransaction::insert(Revision::INITIAL, 0, "x"),
            EditKind::Other,
            0,
            1,
        )
        .unwrap();
    assert_eq!(
        workspace.request_close(second).unwrap(),
        CloseOutcome::NeedsDecision
    );
    assert_eq!(
        workspace
            .resolve_close(second, CloseDecision::Cancel)
            .unwrap(),
        CloseOutcome::Cancelled
    );
    assert_eq!(workspace.active_document_id(), Some(second));
    assert_eq!(
        workspace
            .resolve_close(second, CloseDecision::Save)
            .unwrap(),
        CloseOutcome::SaveRequired
    );
    assert_eq!(workspace.document_count(), 2);
    assert_eq!(
        workspace
            .resolve_close(second, CloseDecision::Discard)
            .unwrap(),
        CloseOutcome::Closed
    );
    assert_eq!(workspace.active_document_id(), Some(first));
}
