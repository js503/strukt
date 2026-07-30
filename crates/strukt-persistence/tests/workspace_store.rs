use strukt_persistence::{WorkspaceSnapshot, WorkspaceStore};
use strukt_workspace::{WorkspaceRoot, WorkspaceState};
use tempfile::tempdir;

const CURRENT_SCHEMA: u32 = 1;

#[test]
fn snapshots_round_trip_without_touching_the_workspace() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let store = WorkspaceStore::at(app_data.path());

    store.save(&state).unwrap();
    let restored = store.load(state.root.id()).unwrap().unwrap();

    assert_eq!(restored.state, state);
    assert!(!project.path().join(".strukt").exists());
}

#[test]
fn legacy_path_only_snapshot_is_loaded_for_the_current_volume_aware_root() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let root = WorkspaceRoot::open(project.path()).unwrap();
    let state = WorkspaceState::new(root.clone());
    let store = WorkspaceStore::at(app_data.path());
    let snapshot = WorkspaceSnapshot {
        schema_version: CURRENT_SCHEMA,
        state,
    };
    let mut json = serde_json::to_value(snapshot).unwrap();
    json["state"]["root"]["id"] =
        serde_json::Value::String(root.legacy_path_id().as_str().to_owned());
    std::fs::write(
        store.current_path(root.legacy_path_id()),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();

    let restored = store.load_for_root(&root).unwrap().unwrap();

    assert_eq!(restored.state.root.id(), root.id());
    assert_eq!(restored.state.root.path(), root.path());
}

#[test]
fn malformed_current_snapshot_falls_back_to_last_valid_snapshot() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let store = WorkspaceStore::at(app_data.path());

    store.save(&state).unwrap();
    store.save(&state).unwrap();
    std::fs::write(store.current_path(state.root.id()), b"{broken").unwrap();

    assert_eq!(store.load(state.root.id()).unwrap().unwrap().state, state);
}

#[test]
fn saving_with_malformed_current_preserves_the_valid_backup() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let original = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let mut changed = original.clone();
    changed.explorer.show_hidden = true;
    let mut replacement = changed.clone();
    replacement.explorer.show_ignored = true;
    let store = WorkspaceStore::at(app_data.path());

    store.save(&original).unwrap();
    store.save(&changed).unwrap();
    std::fs::write(store.current_path(original.root.id()), b"{broken").unwrap();
    store.save(&replacement).unwrap();
    std::fs::write(store.current_path(original.root.id()), b"{broken-again").unwrap();

    assert_eq!(
        store.load(original.root.id()).unwrap().unwrap().state,
        original
    );
}

#[test]
fn a_snapshot_with_the_wrong_root_id_is_never_returned() {
    let app_data = tempdir().unwrap();
    let requested_project = tempdir().unwrap();
    let other_project = tempdir().unwrap();
    let requested = WorkspaceState::new(WorkspaceRoot::open(requested_project.path()).unwrap());
    let other = WorkspaceState::new(WorkspaceRoot::open(other_project.path()).unwrap());
    let store = WorkspaceStore::at(app_data.path());
    let wrong_snapshot = WorkspaceSnapshot {
        schema_version: CURRENT_SCHEMA,
        state: other,
    };

    std::fs::create_dir_all(app_data.path()).unwrap();
    std::fs::write(
        store.current_path(requested.root.id()),
        serde_json::to_vec(&wrong_snapshot).unwrap(),
    )
    .unwrap();

    assert_eq!(store.load(requested.root.id()).unwrap(), None);
}

#[test]
fn an_unsupported_schema_falls_back_to_the_last_valid_snapshot() {
    let app_data = tempdir().unwrap();
    let project = tempdir().unwrap();
    let state = WorkspaceState::new(WorkspaceRoot::open(project.path()).unwrap());
    let store = WorkspaceStore::at(app_data.path());

    store.save(&state).unwrap();
    store.save(&state).unwrap();
    let unsupported = WorkspaceSnapshot {
        schema_version: CURRENT_SCHEMA + 1,
        state: state.clone(),
    };
    std::fs::write(
        store.current_path(state.root.id()),
        serde_json::to_vec(&unsupported).unwrap(),
    )
    .unwrap();

    assert_eq!(store.load(state.root.id()).unwrap().unwrap().state, state);
}
