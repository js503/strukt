use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use strukt_language::ResolvedCommand;
use strukt_persistence::{
    ApprovalSnapshot, LANGUAGE_CONTRIBUTION_ID, LanguageSelectionSnapshot, LanguageSessionSnapshot,
    WorkspaceStore, language_contribution, set_language_contribution,
};
use strukt_workspace::{WorkspaceRoot, WorkspaceState};

#[test]
fn language_snapshot_contains_only_configuration_and_presentation() {
    let snapshot = snapshot(true);
    let json = serde_json::to_string(&snapshot).unwrap();

    for forbidden in [
        "diagnostic",
        "source_text",
        "process",
        "stderr",
        "completion",
        "hover",
    ] {
        assert!(
            !json.contains(forbidden),
            "persisted forbidden field {forbidden}"
        );
    }
}

#[test]
fn contribution_round_trips_and_restores_with_no_running_servers() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::open(root_dir.path()).unwrap();
    let mut state = WorkspaceState::new(root);
    let snapshot = snapshot(false);
    state
        .contributions
        .insert("plugin.future".to_owned(), serde_json::json!({"kept":true}));

    set_language_contribution(&mut state, &snapshot).unwrap();
    let restored = language_contribution(&state)
        .unwrap()
        .unwrap()
        .restore()
        .unwrap();

    assert_eq!(restored.running_servers(), 0);
    assert!(!restored.problems_visible());
    assert_eq!(restored.selections()[0].descriptor_id(), "rust-analyzer");
    assert_eq!(state.contributions["plugin.future"]["kept"], true);
}

#[test]
fn saved_fingerprint_matches_only_the_exact_rediscovered_command() {
    let original = command(&["--stdio"]);
    let changed = command(&["--stdio", "--unsafe"]);
    let approval = ApprovalSnapshot::new("rust", original.fingerprint()).unwrap();

    assert!(approval.matches(&original));
    assert!(!approval.matches(&changed));
}

#[test]
fn invalid_language_contribution_falls_back_to_last_valid_workspace_snapshot() {
    let data = tempfile::tempdir().unwrap();
    let root_dir = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::open(root_dir.path()).unwrap();
    let mut state = WorkspaceState::new(root.clone());
    let store = WorkspaceStore::at(data.path());

    set_language_contribution(&mut state, &snapshot(true)).unwrap();
    store.save(&state).unwrap();
    set_language_contribution(&mut state, &snapshot(false)).unwrap();
    store.save(&state).unwrap();

    let current = store.current_path(root.id());
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
    json["state"]["contributions"][LANGUAGE_CONTRIBUTION_ID]["schema_version"] =
        serde_json::json!(99);
    fs::write(&current, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    let loaded = store.load(root.id()).unwrap().unwrap();
    let restored = language_contribution(&loaded.state).unwrap().unwrap();
    assert!(restored.problems_visible);
}

#[test]
fn duplicate_language_ids_and_unsupported_schema_are_rejected() {
    let selection = LanguageSelectionSnapshot::enabled("rust", "rust-analyzer").unwrap();
    assert!(
        LanguageSessionSnapshot::new(vec![selection.clone(), selection], Vec::new(), true).is_err()
    );
    let invalid: LanguageSessionSnapshot = serde_json::from_value(serde_json::json!({
        "schema_version": 2,
        "selections": [],
        "approvals": [],
        "problems_visible": true
    }))
    .unwrap();
    assert!(invalid.restore().is_err());

    let selections = (0..129)
        .map(|index| LanguageSelectionSnapshot::enabled(format!("lang-{index}"), "server").unwrap())
        .collect();
    assert!(LanguageSessionSnapshot::new(selections, Vec::new(), true).is_err());
}

#[test]
fn unknown_language_fields_survive_a_round_trip() {
    let snapshot: LanguageSessionSnapshot = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "selections": [{
            "language_id": "rust",
            "descriptor_id": "rust-analyzer",
            "enabled": true,
            "future_selection": {"kept": true}
        }],
        "approvals": [],
        "problems_visible": true,
        "future_session": [1, 2, 3]
    }))
    .unwrap();
    snapshot.restore().unwrap();
    let encoded = serde_json::to_value(snapshot).unwrap();
    assert_eq!(encoded["future_session"], serde_json::json!([1, 2, 3]));
    assert_eq!(encoded["selections"][0]["future_selection"]["kept"], true);
}

fn snapshot(problems_visible: bool) -> LanguageSessionSnapshot {
    LanguageSessionSnapshot::new(
        vec![LanguageSelectionSnapshot::enabled("rust", "rust-analyzer").unwrap()],
        vec![ApprovalSnapshot::new("rust", command(&["--stdio"]).fingerprint()).unwrap()],
        problems_visible,
    )
    .unwrap()
}

fn command(arguments: &[&str]) -> ResolvedCommand {
    ResolvedCommand::new(
        PathBuf::from("/workspace/tools/rust-analyzer"),
        arguments.iter().map(OsString::from).collect(),
    )
    .unwrap()
}
