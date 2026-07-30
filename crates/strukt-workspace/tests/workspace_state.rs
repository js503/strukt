use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

#[test]
fn new_workspace_uses_safe_explorer_defaults() {
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    assert!(state.explorer.visible);
    assert!(!state.explorer.show_hidden);
    assert!(!state.explorer.show_ignored);
    assert!(!state.stale_filesystem);
}
