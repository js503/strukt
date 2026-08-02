use std::fs;

use strukt_persistence::{
    EditorRecoveryStore, EditorSessionSnapshot, EditorSessionStore, EditorTabSnapshot, RecoveryKey,
    RecoveryKeyError, RecoveryKeyProvider, RecoveryMetadata, RecoveryPayload, RecoveryStoreError,
};

struct TestKeyProvider([u8; 32]);

impl RecoveryKeyProvider for TestKeyProvider {
    fn load_or_create(&self) -> Result<RecoveryKey, RecoveryKeyError> {
        Ok(RecoveryKey::new(self.0))
    }

    fn delete(&self) -> Result<(), RecoveryKeyError> {
        Ok(())
    }
}

struct UnavailableKeyProvider;

impl RecoveryKeyProvider for UnavailableKeyProvider {
    fn load_or_create(&self) -> Result<RecoveryKey, RecoveryKeyError> {
        Err(RecoveryKeyError::Unavailable("locked".into()))
    }

    fn delete(&self) -> Result<(), RecoveryKeyError> {
        Err(RecoveryKeyError::Unavailable("locked".into()))
    }
}

fn metadata(path: &str, baseline: &str) -> RecoveryMetadata {
    RecoveryMetadata::new("workspace-id", path, baseline)
}

fn payload(path: &str, baseline: &str, revision: u64, text: &str) -> RecoveryPayload {
    RecoveryPayload::new(metadata(path, baseline), revision, text)
}

#[test]
fn encrypted_payload_round_trips_with_unique_nonces() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let provider = TestKeyProvider([7; 32]);
    let first = payload("src/main.rs", "disk-1", 4, "unsaved");

    store.save(&provider, &first).unwrap();
    let first_envelope = fs::read(store.current_path(first.metadata())).unwrap();
    store.save(&provider, &first).unwrap();
    let second_envelope = fs::read(store.current_path(first.metadata())).unwrap();

    assert_ne!(first_envelope, second_envelope);
    assert_eq!(
        store.load(&provider, first.metadata()).unwrap(),
        Some(first)
    );
}

#[test]
fn metadata_is_authenticated_and_wrong_keys_do_not_decrypt() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let provider = TestKeyProvider([7; 32]);
    let record = payload("src/main.rs", "disk-1", 4, "unsaved");
    store.save(&provider, &record).unwrap();

    assert!(matches!(
        store.load(&TestKeyProvider([8; 32]), record.metadata()),
        Err(RecoveryStoreError::Authentication)
    ));

    let other = metadata("src/lib.rs", "disk-2");
    fs::create_dir_all(store.current_path(&other).parent().unwrap()).unwrap();
    fs::copy(
        store.current_path(record.metadata()),
        store.current_path(&other),
    )
    .unwrap();
    assert!(matches!(
        store.load(&provider, &other),
        Err(RecoveryStoreError::Authentication)
    ));
}

#[test]
fn corrupt_or_unsupported_current_envelope_falls_back_to_last_valid() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let provider = TestKeyProvider([7; 32]);
    let first = payload("src/main.rs", "disk-1", 1, "first");
    let second = payload("src/main.rs", "disk-1", 2, "second");
    store.save(&provider, &first).unwrap();
    store.save(&provider, &second).unwrap();

    fs::write(store.current_path(first.metadata()), b"not json").unwrap();
    assert_eq!(
        store.load(&provider, first.metadata()).unwrap(),
        Some(first.clone())
    );

    store.save(&provider, &second).unwrap();
    let path = store.current_path(first.metadata());
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    envelope["schema_version"] = 999.into();
    fs::write(path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert_eq!(
        store.load(&provider, first.metadata()).unwrap(),
        Some(first)
    );
}

#[test]
fn tampering_without_a_fallback_is_reported() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let provider = TestKeyProvider([7; 32]);
    let record = payload("src/main.rs", "disk-1", 1, "first");
    store.save(&provider, &record).unwrap();
    let path = store.current_path(record.metadata());
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let original = envelope["ciphertext"][0].as_u64().unwrap();
    envelope["ciphertext"][0] = ((original + 1) % 256).into();
    fs::write(path, serde_json::to_vec(&envelope).unwrap()).unwrap();

    assert!(matches!(
        store.load(&provider, record.metadata()),
        Err(RecoveryStoreError::Authentication)
    ));
}

#[test]
fn unavailable_keys_disable_recovery_without_plaintext_fallback() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let record = payload("src/main.rs", "disk-1", 1, "secret text");

    assert!(matches!(
        store.save(&UnavailableKeyProvider, &record),
        Err(RecoveryStoreError::Key(RecoveryKeyError::Unavailable(_)))
    ));
    assert!(!data.path().join("workspace-id").exists());
}

#[test]
fn confirmed_save_or_discard_deletes_current_and_fallback_records() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let provider = TestKeyProvider([7; 32]);
    let record = payload("src/main.rs", "disk-1", 1, "first");
    store.save(&provider, &record).unwrap();
    store.save(&provider, &record).unwrap();

    store.delete(record.metadata()).unwrap();

    assert_eq!(store.load(&provider, record.metadata()).unwrap(), None);
    assert!(!store.current_path(record.metadata()).exists());
    assert!(!store.backup_path(record.metadata()).exists());
}

#[test]
fn recovery_data_stays_in_application_data_not_the_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = EditorRecoveryStore::at(data.path());
    let provider = TestKeyProvider([7; 32]);
    let record = payload("src/main.rs", "disk-1", 1, "first");
    store.save(&provider, &record).unwrap();

    assert!(!workspace.path().join(".strukt").exists());
    assert!(
        store
            .current_path(record.metadata())
            .starts_with(data.path())
    );
}

#[test]
fn editor_session_layout_round_trips_and_falls_back_from_corruption() {
    let data = tempfile::tempdir().unwrap();
    let store = EditorSessionStore::at(data.path());
    let first = EditorSessionSnapshot::new(
        vec![EditorTabSnapshot::new("src/main.rs", 8, 3, 12.5)],
        Some("src/main.rs".into()),
        Some("src/main.rs".into()),
    );
    let second = EditorSessionSnapshot::new(
        vec![
            EditorTabSnapshot::new("src/main.rs", 9, 4, 13.0),
            EditorTabSnapshot::new("README.md", 0, 0, 0.0),
        ],
        Some("README.md".into()),
        None,
    );

    store.save("workspace-id", &first).unwrap();
    store.save("workspace-id", &second).unwrap();
    assert_eq!(store.load("workspace-id").unwrap(), Some(second));

    fs::write(store.current_path("workspace-id"), b"broken").unwrap();
    assert_eq!(store.load("workspace-id").unwrap(), Some(first));
}
