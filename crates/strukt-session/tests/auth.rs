use strukt_session::{HandshakeChallenge, ServiceInstanceId, ServiceSecret};

#[test]
fn authentication_proof_binds_version_instance_endpoint_and_nonce() {
    let secret = ServiceSecret::from_bytes([7; 32]);
    let instance = ServiceInstanceId::new().unwrap();
    let challenge = HandshakeChallenge::new(1, instance, "strukt-session-test", [3; 32]).unwrap();
    let proof = secret.prove(&challenge).unwrap();

    assert!(secret.verify(&challenge, &proof));
    assert!(!ServiceSecret::from_bytes([8; 32]).verify(&challenge, &proof));
    let changed = HandshakeChallenge::new(1, instance, "strukt-session-other", [3; 32]).unwrap();
    assert!(!secret.verify(&changed, &proof));
}

#[test]
fn generated_secrets_and_nonces_are_distinct_and_redacted() {
    let first = ServiceSecret::generate().unwrap();
    let second = ServiceSecret::generate().unwrap();
    let instance = ServiceInstanceId::new().unwrap();
    let challenge = HandshakeChallenge::new(1, instance, "endpoint", [9; 32]).unwrap();
    assert_ne!(
        first.prove(&challenge).unwrap(),
        second.prove(&challenge).unwrap()
    );
    assert_eq!(format!("{first:?}"), "ServiceSecret([REDACTED])");

    let first = HandshakeChallenge::generate(1, instance, "endpoint").unwrap();
    let second = HandshakeChallenge::generate(1, instance, "endpoint").unwrap();
    assert_ne!(first.client_nonce(), second.client_nonce());
}

#[test]
fn endpoint_identity_is_bounded_and_nul_free() {
    let instance = ServiceInstanceId::new().unwrap();
    assert!(HandshakeChallenge::new(1, instance, "", [0; 32]).is_err());
    assert!(HandshakeChallenge::new(1, instance, "bad\0endpoint", [0; 32]).is_err());
    assert!(HandshakeChallenge::new(1, instance, "x".repeat(257), [0; 32]).is_err());
}
