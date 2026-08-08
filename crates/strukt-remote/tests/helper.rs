#![cfg(unix)]

use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;

use strukt_fs::CancellationToken;
use strukt_remote::{
    Capability, ClientHello, HelperServer, ProtocolLimits, RemoteErrorKind, RequestBody,
    RequestEnvelope, RequestId, ResponseBody, ResponseEnvelope, ServerHello, read_frame,
    read_preface, run_helper_stdio, write_frame, write_preface,
};
use tempfile::tempdir;

#[test]
fn real_helper_handshakes_and_serves_a_confined_file_over_stdio() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("README.md"), "remote fixture").unwrap();
    let hello = ClientHello {
        protocol_major: 1,
        protocol_minor: 0,
        nonce: [9; 32],
        workspace_root: root.path().to_string_lossy().into_owned(),
        limits: ProtocolLimits::default(),
    };
    let request = RequestEnvelope {
        request_id: RequestId::new(1).unwrap(),
        generation: 1,
        body: RequestBody::ReadFile {
            path: "README.md".into(),
            offset: 0,
            length: 1024,
        },
    };
    let mut input = Vec::new();
    write_preface(&mut input).unwrap();
    write_frame(&mut input, &hello, 1024 * 1024).unwrap();
    write_frame(&mut input, &request, 1024 * 1024).unwrap();
    let mut output = Vec::new();

    run_helper_stdio(&mut Cursor::new(input), &mut output).unwrap();

    let mut output = Cursor::new(output);
    read_preface(&mut output).unwrap();
    let server: ServerHello = read_frame(&mut output, 1024 * 1024).unwrap();
    assert_eq!(server.nonce, hello.nonce);
    assert_eq!(
        server.workspace_root,
        root.path().canonicalize().unwrap().to_string_lossy()
    );
    assert!(server.capabilities.contains(&Capability::Files));
    let response: ResponseEnvelope = read_frame(&mut output, 1024 * 1024).unwrap();
    assert_eq!(response.request_id, request.request_id);
    assert_eq!(response.generation, 1);
    match response.body {
        ResponseBody::Stream(chunk) => assert_eq!(chunk.bytes, b"remote fixture"),
        other => panic!("unexpected helper response: {other:?}"),
    }
}

#[test]
fn helper_rejects_incompatible_protocol_before_serving_requests() {
    let root = tempdir().unwrap();
    let hello = ClientHello {
        protocol_major: 99,
        protocol_minor: 0,
        nonce: [1; 32],
        workspace_root: root.path().to_string_lossy().into_owned(),
        limits: ProtocolLimits::default(),
    };
    let mut input = Vec::new();
    write_preface(&mut input).unwrap();
    write_frame(&mut input, &hello, 1024 * 1024).unwrap();
    let mut output = Vec::new();
    assert!(run_helper_stdio(&mut Cursor::new(input), &mut output).is_err());
}

#[test]
fn helper_advertises_no_persistent_session_capability() {
    let capabilities = strukt_remote::HelperServer::capabilities();
    assert_eq!(
        capabilities,
        BTreeSet::from([
            Capability::Files,
            Capability::Search,
            Capability::Git,
            Capability::Processes,
            Capability::Language,
        ])
    );
}

#[test]
fn helper_returns_a_typed_error_for_a_cancelled_operation() {
    let root = tempdir().unwrap();
    let helper = HelperServer::open(root.path()).unwrap();
    let request = RequestEnvelope {
        request_id: RequestId::new(7).unwrap(),
        generation: 2,
        body: RequestBody::Search {
            query: "needle".into(),
            include_ignored: false,
            limit: 10,
        },
    };
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let response = helper.handle_cancellable(&request, &cancellation);

    assert_eq!(response.request_id, request.request_id);
    assert_eq!(response.generation, request.generation);
    match response.body {
        ResponseBody::Error(error) => assert_eq!(error.kind, RemoteErrorKind::Cancelled),
        other => panic!("unexpected helper response: {other:?}"),
    }
}
