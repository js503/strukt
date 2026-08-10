use std::collections::BTreeSet;

use strukt_remote::{
    Capability, ClientHello, NegotiatedProtocol, OperationTracker, ProtocolError, ProtocolLimits,
    RemoteBuildTarget, RemoteError, RemoteErrorKind, RequestBody, RequestEnvelope, RequestId,
    ResponseBody, ResponseEnvelope, ServerHello, StreamChunk, negotiate,
};

fn capabilities(values: &[Capability]) -> BTreeSet<Capability> {
    values.iter().copied().collect()
}

fn hello(major: u16, minor: u16) -> (ClientHello, ServerHello) {
    let nonce = [7_u8; 32];
    (
        ClientHello {
            protocol_major: major,
            protocol_minor: minor,
            nonce,
            workspace_root: "/srv/strukt".into(),
            limits: ProtocolLimits::default(),
        },
        ServerHello {
            protocol_major: major,
            protocol_minor: minor.saturating_sub(1),
            nonce,
            helper_version: "0.1.0".into(),
            build_target: RemoteBuildTarget::LinuxX86_64,
            workspace_root: "/srv/strukt".into(),
            limits: ProtocolLimits {
                max_frame_bytes: 512 * 1_024,
                max_in_flight: 32,
                max_stream_chunk_bytes: 16 * 1_024,
                initial_stream_credit_bytes: 64 * 1_024,
            },
            capabilities: capabilities(&[
                Capability::Files,
                Capability::Search,
                Capability::Processes,
            ]),
        },
    )
}

#[test]
fn handshake_negotiates_minor_limits_and_capability_intersection() {
    let (client, server) = hello(1, 3);
    let supported = capabilities(&[Capability::Files, Capability::Git, Capability::Processes]);
    let negotiated = negotiate(&client, &server, &supported).unwrap();
    assert_eq!(negotiated.protocol_major, 1);
    assert_eq!(negotiated.protocol_minor, 2);
    assert_eq!(
        negotiated.capabilities,
        capabilities(&[Capability::Files, Capability::Processes])
    );
    assert_eq!(negotiated.limits.max_frame_bytes, 512 * 1_024);
    assert_eq!(negotiated.limits.max_in_flight, 32);
}

#[test]
fn handshake_rejects_major_nonce_and_limit_mismatches() {
    let (client, mut server) = hello(1, 2);
    server.protocol_major = 2;
    assert!(matches!(
        negotiate(&client, &server, &BTreeSet::new()),
        Err(ProtocolError::IncompatibleMajor)
    ));
    server.protocol_major = 1;
    server.nonce = [8; 32];
    assert!(matches!(
        negotiate(&client, &server, &BTreeSet::new()),
        Err(ProtocolError::NonceMismatch)
    ));
    server.nonce = client.nonce;
    server.limits.max_frame_bytes = 0;
    assert!(matches!(
        negotiate(&client, &server, &BTreeSet::new()),
        Err(ProtocolError::InvalidLimits)
    ));
}

#[test]
fn tracker_enforces_ids_sequence_credit_completion_and_cancellation() {
    let limits = ProtocolLimits {
        max_frame_bytes: 1_024,
        max_in_flight: 2,
        max_stream_chunk_bytes: 8,
        initial_stream_credit_bytes: 10,
    };
    let mut tracker = OperationTracker::new(limits).unwrap();
    let first = RequestId::new(1).unwrap();
    let second = RequestId::new(2).unwrap();
    tracker.register(first).unwrap();
    assert!(matches!(
        tracker.register(first),
        Err(ProtocolError::DuplicateRequest)
    ));
    tracker.register(second).unwrap();
    assert!(matches!(
        tracker.register(RequestId::new(3).unwrap()),
        Err(ProtocolError::TooManyInFlight)
    ));

    tracker
        .accept_chunk(&StreamChunk {
            request_id: first,
            sequence: 0,
            bytes: vec![1; 8],
        })
        .unwrap();
    assert!(matches!(
        tracker.accept_chunk(&StreamChunk {
            request_id: first,
            sequence: 2,
            bytes: vec![1],
        }),
        Err(ProtocolError::InvalidSequence)
    ));
    assert!(matches!(
        tracker.accept_chunk(&StreamChunk {
            request_id: first,
            sequence: 1,
            bytes: vec![1; 3],
        }),
        Err(ProtocolError::CreditExceeded)
    ));
    tracker.grant_credit(first, 4).unwrap();
    tracker
        .accept_chunk(&StreamChunk {
            request_id: first,
            sequence: 1,
            bytes: vec![1; 3],
        })
        .unwrap();
    tracker.complete(first).unwrap();
    assert!(matches!(
        tracker.accept_chunk(&StreamChunk {
            request_id: first,
            sequence: 2,
            bytes: vec![1],
        }),
        Err(ProtocolError::UnknownRequest)
    ));

    tracker.cancel(second).unwrap();
    assert!(matches!(
        tracker.accept_chunk(&StreamChunk {
            request_id: second,
            sequence: 0,
            bytes: vec![1],
        }),
        Err(ProtocolError::RequestCancelled)
    ));
}

#[test]
fn envelopes_and_typed_errors_round_trip_with_unknown_fields() {
    let request = RequestEnvelope {
        request_id: RequestId::new(9).unwrap(),
        generation: 4,
        body: RequestBody::ReadFile {
            path: "src/main.rs".into(),
            offset: 10,
            length: 20,
        },
    };
    let encoded = serde_json::to_value(&request).unwrap();
    let mut extended = encoded.as_object().unwrap().clone();
    extended.insert("future_field".into(), serde_json::json!(true));
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(extended.into()).unwrap(),
        request
    );

    let error = RemoteError::new(RemoteErrorKind::PermissionDenied, "bad\0path");
    assert!(!error.detail.contains('\0'));
    let response = ResponseEnvelope {
        request_id: request.request_id,
        generation: 4,
        body: ResponseBody::Error(error.clone()),
    };
    let round_trip: ResponseEnvelope =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    assert_eq!(round_trip, response);
    assert_eq!(error.kind, RemoteErrorKind::PermissionDenied);
}

#[test]
fn request_ids_and_limits_reject_invalid_values() {
    assert!(RequestId::new(0).is_err());
    assert!(ProtocolLimits::new(0, 1, 1, 1).is_err());
    assert!(ProtocolLimits::new(1, 0, 1, 1).is_err());
    assert!(ProtocolLimits::new(16, 1, 32, 1).is_err());
    let negotiated = NegotiatedProtocol {
        protocol_major: 1,
        protocol_minor: 0,
        limits: ProtocolLimits::default(),
        capabilities: BTreeSet::new(),
    };
    assert!(negotiated.limits.max_stream_chunk_bytes <= negotiated.limits.max_frame_bytes);
}
