use std::collections::BTreeSet;
use std::io::Cursor;

use strukt_remote::{
    Capability, ClientHello, HelperClient, ProtocolLimits, RemoteClientError, RequestBody,
    ResponseBody, ResponseEnvelope, ServerHello, write_frame, write_preface,
};

fn server_stream(nonce: [u8; 32], generation: u64) -> Cursor<Vec<u8>> {
    let mut bytes = Vec::new();
    write_preface(&mut bytes).unwrap();
    write_frame(
        &mut bytes,
        &ServerHello {
            protocol_major: 1,
            protocol_minor: 0,
            nonce,
            helper_version: "0.1.0".into(),
            build_target: strukt_remote::RemoteBuildTarget::LinuxX86_64,
            workspace_root: "/workspace".into(),
            limits: ProtocolLimits::default(),
            capabilities: BTreeSet::from([Capability::Files, Capability::Git]),
        },
        strukt_remote::DEFAULT_FRAME_LIMIT,
    )
    .unwrap();
    write_frame(
        &mut bytes,
        &ResponseEnvelope {
            request_id: strukt_remote::RequestId::new(1).unwrap(),
            generation,
            body: ResponseBody::GitSummary {
                branch: Some("main".into()),
                detached: false,
                staged: 0,
                modified: 1,
                untracked: 0,
            },
        },
        strukt_remote::DEFAULT_FRAME_LIMIT,
    )
    .unwrap();
    Cursor::new(bytes)
}

#[test]
fn client_negotiates_capabilities_and_round_trips_a_typed_request() {
    let nonce = [7; 32];
    let reader = server_stream(nonce, 4);
    let writer = Vec::new();
    let hello = ClientHello {
        protocol_major: 1,
        protocol_minor: 0,
        nonce,
        workspace_root: "/workspace".into(),
        limits: ProtocolLimits::default(),
    };
    let mut client = HelperClient::connect(
        reader,
        writer,
        &hello,
        &BTreeSet::from([Capability::Files, Capability::Git, Capability::Processes]),
        4,
    )
    .unwrap();

    assert_eq!(
        client.capabilities(),
        &BTreeSet::from([Capability::Files, Capability::Git])
    );
    assert_eq!(client.workspace_root(), "/workspace");
    let response = client.request(RequestBody::GitSummary).unwrap();
    assert!(matches!(
        response,
        ResponseBody::GitSummary {
            branch: Some(branch),
            modified: 1,
            ..
        } if branch == "main"
    ));
    assert!(!client.into_writer().is_empty());
}

#[test]
fn client_rejects_stale_generations_before_publishing_results() {
    let nonce = [9; 32];
    let reader = server_stream(nonce, 3);
    let hello = ClientHello {
        protocol_major: 1,
        protocol_minor: 0,
        nonce,
        workspace_root: "/workspace".into(),
        limits: ProtocolLimits::default(),
    };
    let mut client = HelperClient::connect(
        reader,
        Vec::new(),
        &hello,
        &BTreeSet::from([Capability::Git]),
        4,
    )
    .unwrap();

    assert!(matches!(
        client.request(RequestBody::GitSummary),
        Err(RemoteClientError::StaleGeneration {
            expected: 4,
            actual: 3
        })
    ));
}
