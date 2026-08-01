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

#[test]
fn legacy_state_migrates_with_empty_contributions_and_unknown_values_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(directory.path()).unwrap());
    let mut value = serde_json::to_value(&state).unwrap();
    value.as_object_mut().unwrap().remove("contributions");
    let migrated: WorkspaceState = serde_json::from_value(value).unwrap();
    assert!(migrated.contributions.is_empty());

    let mut extended = migrated;
    extended.contributions.insert(
        "future.plugin".into(),
        serde_json::json!({"opaque": [1, 2, 3]}),
    );
    let restored: WorkspaceState =
        serde_json::from_slice(&serde_json::to_vec(&extended).unwrap()).unwrap();
    assert_eq!(restored.contributions, extended.contributions);
}
