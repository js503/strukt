use std::fs;

use strukt_persistence::{RemoteConnectionRecord, RemoteHelperMetadata, RemoteStore};
use tempfile::tempdir;

fn record(id: &str, alias: &str, root: &str) -> RemoteConnectionRecord {
    RemoteConnectionRecord::new(
        id,
        alias,
        Some(alias.to_owned()),
        vec![root.to_owned()],
        None,
    )
    .unwrap()
}

#[test]
fn remote_records_round_trip_in_deterministic_order_without_secrets() {
    let data = tempdir().unwrap();
    let store = RemoteStore::at(data.path());
    let helper = RemoteHelperMetadata::new("0.1.0", "a".repeat(64)).unwrap();
    let second = RemoteConnectionRecord::new(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "zeta",
        Some("Build box".into()),
        vec!["~/work/app".into(), "/srv/repo".into()],
        Some(helper),
    )
    .unwrap();
    let first = record("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "alpha", "~/src");

    store.upsert(second.clone()).unwrap();
    store.upsert(first.clone()).unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded, vec![first, second]);
    let persisted = fs::read_to_string(store.current_path()).unwrap();
    for forbidden in [
        "password",
        "passphrase",
        "private_key",
        "agent_token",
        "SSH_AUTH_SOCK",
        "protocol_payload",
    ] {
        assert!(!persisted.contains(forbidden));
    }
}

#[test]
fn corrupt_current_falls_back_and_forget_is_explicit() {
    let data = tempdir().unwrap();
    let store = RemoteStore::at(data.path());
    let first = record("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "alpha", "~/src");
    let second = record("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "beta", "~/work");
    store.upsert(first.clone()).unwrap();
    store.upsert(second).unwrap();
    fs::write(store.current_path(), "{broken").unwrap();

    assert_eq!(store.load().unwrap(), vec![first.clone()]);
    assert!(store.forget(&first.connection_id).unwrap());
    assert!(store.load().unwrap().is_empty());
}

#[test]
fn records_reject_invalid_identifiers_aliases_roots_and_helper_metadata() {
    assert!(RemoteConnectionRecord::new("", "host", None, vec!["~/src".into()], None).is_err());
    assert!(
        RemoteConnectionRecord::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "-oProxyCommand=bad",
            None,
            vec!["~/src".into()],
            None,
        )
        .is_err()
    );
    assert!(
        RemoteConnectionRecord::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "host",
            None,
            vec!["relative".into()],
            None,
        )
        .is_err()
    );
    assert!(RemoteHelperMetadata::new("../bad", "a".repeat(64)).is_err());
}

#[cfg(unix)]
#[test]
fn remote_store_is_owner_readable_and_writable_only() {
    use std::os::unix::fs::PermissionsExt;

    let data = tempdir().unwrap();
    let store = RemoteStore::at(data.path());
    store
        .upsert(record("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "alpha", "~/src"))
        .unwrap();

    assert_eq!(
        fs::metadata(store.current_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
