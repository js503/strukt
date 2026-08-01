use serde_json::Value;
use strukt_persistence::{
    TerminalSessionSnapshot, TerminalStoreError, WorkspaceSnapshot, WorkspaceStore,
    set_terminal_contribution, terminal_contribution,
};
use strukt_terminal::{PaneState, SplitAxis, TerminalWorkspace};
use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

#[test]
fn terminal_snapshot_round_trips_layout_without_runtime_content() {
    let snapshot = nested_terminal_snapshot();
    let value = serde_json::to_value(&snapshot).unwrap();
    let encoded = value.to_string();
    for forbidden in ["scrollback", "environment", "command", "child_id", "output"] {
        assert!(!encoded.contains(forbidden));
    }
    assert_eq!(
        serde_json::from_value::<TerminalSessionSnapshot>(value).unwrap(),
        snapshot
    );

    let restored = snapshot.restore().unwrap();
    assert!(
        restored
            .panes()
            .all(|pane| matches!(pane.state(), PaneState::Stopped))
    );
}

#[test]
fn unknown_fields_survive_the_terminal_contribution_round_trip() {
    let value: Value =
        serde_json::json!({"schema_version":1,"tabs":[],"active_tab":null,"future":{"kept":true}});
    let decoded: TerminalSessionSnapshot = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}

#[test]
fn unsupported_versions_and_invalid_layouts_are_rejected() {
    let mut unsupported = nested_terminal_snapshot();
    unsupported.schema_version = 99;
    assert_eq!(
        unsupported.restore().unwrap_err(),
        TerminalStoreError::UnsupportedSchema(99)
    );

    let mut invalid = nested_terminal_snapshot();
    invalid.tabs[0].focused_pane = strukt_terminal::TerminalPaneId::new();
    assert!(matches!(
        invalid.restore(),
        Err(TerminalStoreError::InvalidLayout(_))
    ));
}

#[test]
fn workspace_contribution_preserves_opaque_siblings() {
    let project = tempdir().unwrap();
    let root = WorkspaceRoot::open(project.path()).unwrap();
    let mut state = WorkspaceState::new(root);
    state.contributions.insert(
        "future.plugin".into(),
        serde_json::json!({"opaque":[1,2,3]}),
    );
    let snapshot = nested_terminal_snapshot();

    set_terminal_contribution(&mut state, &snapshot).unwrap();

    assert_eq!(terminal_contribution(&state).unwrap(), Some(snapshot));
    assert_eq!(
        state.contributions["future.plugin"],
        serde_json::json!({"opaque":[1,2,3]})
    );
}

#[test]
fn corrupt_terminal_contribution_falls_back_to_last_valid_workspace() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let root = WorkspaceRoot::open(project.path()).unwrap();
    let mut original = WorkspaceState::new(root);
    set_terminal_contribution(&mut original, &nested_terminal_snapshot()).unwrap();
    let mut changed = original.clone();
    changed.explorer.show_hidden = true;
    let store = WorkspaceStore::at(app_data.path());
    store.save(&original).unwrap();
    store.save(&changed).unwrap();

    let mut invalid = changed;
    invalid.contributions.insert(
        strukt_persistence::TERMINAL_CONTRIBUTION_ID.into(),
        serde_json::json!({"schema_version":99,"tabs":[],"active_tab":null}),
    );
    let current = WorkspaceSnapshot {
        schema_version: 1,
        state: invalid,
    };
    std::fs::write(
        store.current_path(original.root.id()),
        serde_json::to_vec(&current).unwrap(),
    )
    .unwrap();

    assert_eq!(
        store.load(original.root.id()).unwrap().unwrap().state,
        original
    );
}

fn nested_terminal_snapshot() -> TerminalSessionSnapshot {
    let mut workspace = TerminalWorkspace::default();
    workspace.create_tab("build", "/workspace").unwrap();
    workspace.split_focused(SplitAxis::Vertical).unwrap();
    TerminalSessionSnapshot::from_workspace(&workspace)
}
