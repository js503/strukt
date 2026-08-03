use strukt_session::{
    EventEnvelope, EventGuard, FrameDecoder, FrameError, PaneId, RequestBody, RequestEnvelope,
    RequestIdGenerator, ResponseBody, ResponseEnvelope, ServiceInstanceId, SessionId, WindowId,
    WireError, decode_cbor, encode_cbor,
};

#[test]
fn decoder_handles_fragmented_and_combined_frames() {
    let first = encode_cbor(&RequestEnvelope::new(1, 0, RequestBody::Catalog), 64 * 1024).unwrap();
    let second = encode_cbor(&RequestEnvelope::new(2, 0, RequestBody::Detach), 64 * 1024).unwrap();
    let mut decoder = FrameDecoder::new(64 * 1024);

    assert!(decoder.push(&first[..3]).unwrap().is_empty());
    let mut remainder = first[3..].to_vec();
    remainder.extend_from_slice(&second);
    let frames = decoder.push(&remainder).unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(
        decode_cbor::<RequestEnvelope>(&frames[0])
            .unwrap()
            .request_id(),
        1
    );
    assert_eq!(
        decode_cbor::<RequestEnvelope>(&frames[1])
            .unwrap()
            .request_id(),
        2
    );
}

#[test]
fn framing_rejects_oversized_and_malformed_payloads_without_retaining_them() {
    let mut decoder = FrameDecoder::new(8);
    assert_eq!(
        decoder.push(&12_u32.to_be_bytes()),
        Err(FrameError::FrameTooLarge)
    );
    assert_eq!(decoder.retained_bytes(), 0);
    assert!(matches!(
        decode_cbor::<RequestEnvelope>(&[0xff]),
        Err(FrameError::MalformedPayload)
    ));
}

#[test]
fn protocol_versions_request_ids_and_body_bounds_are_enforced() {
    let mut ids = RequestIdGenerator::new();
    assert_eq!(ids.next_id().unwrap(), 1);
    assert_eq!(ids.next_id().unwrap(), 2);

    let invalid = RequestEnvelope::with_version(99, 1, 0, RequestBody::Catalog);
    assert_eq!(invalid.validate(), Err(WireError::VersionIncompatible));
    let invalid_name = RequestEnvelope::new(
        3,
        0,
        RequestBody::CreateSession {
            name: "x".repeat(81),
            working_directory: "/workspace".into(),
        },
    );
    assert_eq!(invalid_name.validate(), Err(WireError::InvalidBody));
}

#[test]
fn response_and_event_guards_reject_stale_instances_and_generations() {
    let instance = ServiceInstanceId::new().unwrap();
    let other = ServiceInstanceId::new().unwrap();
    let session = SessionId::new().unwrap();
    let window = WindowId::new().unwrap();
    let pane = PaneId::new().unwrap();
    let guard = EventGuard::new(instance, session, window, pane, 4, 10);
    assert!(guard.matches(&EventEnvelope::pane_changed(
        instance, session, window, pane, 4, 11
    )));
    assert!(!guard.matches(&EventEnvelope::pane_changed(
        other, session, window, pane, 4, 11
    )));
    assert!(!guard.matches(&EventEnvelope::pane_changed(
        instance, session, window, pane, 3, 11
    )));

    let response = ResponseEnvelope::ok(7, ResponseBody::Detached);
    assert_eq!(response.request_id(), 7);
}
