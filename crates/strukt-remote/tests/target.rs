use std::str::FromStr;

use strukt_remote::{ConnectionId, RemoteRoot, RemoteWorkspaceId, SshAlias};

#[test]
fn connection_ids_are_stable_lowercase_hex_values() {
    let id = ConnectionId::new().expect("OS randomness");
    let encoded = id.to_string();
    assert_eq!(encoded.len(), 32);
    assert!(encoded.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(ConnectionId::from_str(&encoded).unwrap(), id);
    assert!(ConnectionId::from_str(&encoded.to_uppercase()).is_err());
    assert!(ConnectionId::from_str("short").is_err());
}

#[test]
fn aliases_are_opaque_but_reject_option_and_control_injection() {
    for valid in ["ec2-development", "ubuntu@build-box", "dev.example.com"] {
        assert_eq!(SshAlias::new(valid).unwrap().as_str(), valid);
    }

    for invalid in ["", "-oProxyCommand=bad", "host\nother", "host\r", "a\0b"] {
        assert!(SshAlias::new(invalid).is_err(), "accepted {invalid:?}");
    }
    assert!(SshAlias::new("x".repeat(256)).is_err());
}

#[test]
fn remote_roots_normalize_without_permitting_escape() {
    assert_eq!(
        RemoteRoot::new("/srv/work/./strukt").unwrap().as_str(),
        "/srv/work/strukt"
    );
    assert_eq!(
        RemoteRoot::new("~/Development//strukt").unwrap().as_str(),
        "~/Development/strukt"
    );

    for invalid in [
        "",
        "relative/path",
        "/srv/../etc",
        "~/../other",
        "/srv/a\0b",
    ] {
        assert!(RemoteRoot::new(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn workspace_identity_includes_connection_and_root() {
    let first = ConnectionId::from_str("00000000000000000000000000000001").unwrap();
    let second = ConnectionId::from_str("00000000000000000000000000000002").unwrap();
    let root = RemoteRoot::new("/srv/strukt").unwrap();
    let other_root = RemoteRoot::new("/srv/other").unwrap();

    let identity = RemoteWorkspaceId::derive(first, &root);
    assert_eq!(identity, RemoteWorkspaceId::derive(first, &root));
    assert_ne!(identity, RemoteWorkspaceId::derive(second, &root));
    assert_ne!(identity, RemoteWorkspaceId::derive(first, &other_root));
    assert_eq!(identity.to_string().len(), 64);
}

#[test]
fn target_values_round_trip_through_json_with_validation() {
    let alias = SshAlias::new("ec2-development").unwrap();
    let root = RemoteRoot::new("~/Development/strukt").unwrap();
    let alias_json = serde_json::to_string(&alias).unwrap();
    let root_json = serde_json::to_string(&root).unwrap();
    assert_eq!(
        serde_json::from_str::<SshAlias>(&alias_json).unwrap(),
        alias
    );
    assert_eq!(
        serde_json::from_str::<RemoteRoot>(&root_json).unwrap(),
        root
    );
    assert!(serde_json::from_str::<SshAlias>("\"-bad\"").is_err());
    assert!(serde_json::from_str::<RemoteRoot>("\"relative\"").is_err());
}
