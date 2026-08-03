use std::fs;

use strukt_session::{
    PaneLifecycle, PersistedCatalog, SessionCatalog, SessionStore, SessionStoreError,
};

#[test]
fn live_catalog_is_persisted_and_restored_only_as_stopped_definitions() {
    let data = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = SessionStore::at(data.path());
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "backend", workspace.path())
        .unwrap();
    let pane = catalog
        .session(session)
        .unwrap()
        .active_window()
        .unwrap()
        .focused_pane()
        .id();
    catalog
        .set_pane_lifecycle(1, session, pane, PaneLifecycle::Running)
        .unwrap();

    store
        .save(&PersistedCatalog::new(&catalog, Vec::new()).unwrap())
        .unwrap();
    let restored = store.load().unwrap().unwrap();
    let pane = restored
        .catalog()
        .session(session)
        .unwrap()
        .active_window()
        .unwrap()
        .focused_pane();
    assert_eq!(pane.lifecycle(), &PaneLifecycle::Stopped);
    assert_eq!(pane.generation(), 0);
    assert!(!workspace.path().join(".strukt").exists());
}

#[test]
fn corrupt_current_catalog_falls_back_to_last_valid() {
    let data = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = SessionStore::at(data.path());
    let mut first = SessionCatalog::new();
    first.create_session(0, "first", workspace.path()).unwrap();
    store
        .save(&PersistedCatalog::new(&first, Vec::new()).unwrap())
        .unwrap();
    let mut second = SessionCatalog::new();
    second
        .create_session(0, "second", workspace.path())
        .unwrap();
    store
        .save(&PersistedCatalog::new(&second, Vec::new()).unwrap())
        .unwrap();

    fs::write(store.current_path(), b"not json").unwrap();
    let restored = store.load().unwrap().unwrap();
    assert_eq!(
        restored.catalog().sessions().next().unwrap().name(),
        "first"
    );
}

#[test]
fn unknown_top_level_fields_survive_load_and_save() {
    let data = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = SessionStore::at(data.path());
    let mut catalog = SessionCatalog::new();
    catalog.create_session(0, "one", workspace.path()).unwrap();
    store
        .save(&PersistedCatalog::new(&catalog, Vec::new()).unwrap())
        .unwrap();
    let mut json: serde_json::Value =
        serde_json::from_slice(&fs::read(store.current_path()).unwrap()).unwrap();
    json["future_catalog"] = serde_json::json!({"kept": true});
    fs::write(
        store.current_path(),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();

    let restored = store.load().unwrap().unwrap();
    store.save(&restored).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(store.current_path()).unwrap()).unwrap();
    assert_eq!(saved["future_catalog"]["kept"], true);
}

#[test]
fn persisted_json_excludes_runtime_and_sensitive_fields() {
    let data = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = SessionStore::at(data.path());
    let mut catalog = SessionCatalog::new();
    catalog.create_session(0, "one", workspace.path()).unwrap();
    store
        .save(&PersistedCatalog::new(&catalog, Vec::new()).unwrap())
        .unwrap();
    let json = fs::read_to_string(store.current_path()).unwrap();
    for forbidden in [
        "terminal_input",
        "command_history",
        "environment",
        "process_id",
        "process_handle",
        "authentication_secret",
        "endpoint_identity",
        "clipboard",
        "selection",
    ] {
        assert!(
            !json.contains(forbidden),
            "persisted forbidden field {forbidden}"
        );
    }
}

#[test]
fn unsupported_schema_and_oversized_records_fail_closed() {
    let data = tempfile::tempdir().unwrap();
    let store = SessionStore::at(data.path());
    fs::create_dir_all(store.current_path().parent().unwrap()).unwrap();
    fs::write(
        store.current_path(),
        br#"{"schema_version":99,"catalog":{},"histories":[]}"#,
    )
    .unwrap();
    assert!(matches!(
        store.load(),
        Err(SessionStoreError::UnsupportedSchema(99))
    ));

    fs::write(store.current_path(), vec![b'x'; 32 * 1024 * 1024 + 1]).unwrap();
    assert!(matches!(store.load(), Err(SessionStoreError::TooLarge)));
}
