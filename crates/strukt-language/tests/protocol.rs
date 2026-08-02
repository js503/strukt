use serde_json::json;
use strukt_language::{
    IncomingMessage, ProtocolError, RequestIdAllocator, ResponseRouter, bounded_error_text,
    parse_message,
};

#[test]
fn request_ids_are_monotonic_and_responses_route_by_id() {
    let mut ids = RequestIdAllocator::default();
    let first = ids.next_id().unwrap();
    let second = ids.next_id().unwrap();
    assert!(second > first);

    let response = parse_message(
        &serde_json::to_vec(&json!({"jsonrpc":"2.0","id":first.get(),"result":null})).unwrap(),
    )
    .unwrap();
    assert!(matches!(response, IncomingMessage::Response(message) if message.id() == first));
}

#[test]
fn notifications_and_server_requests_are_distinct() {
    let notification = parse_message(
        br#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{}}"#,
    )
    .unwrap();
    let request = parse_message(
        br#"{"jsonrpc":"2.0","id":7,"method":"workspace/configuration","params":{}}"#,
    )
    .unwrap();

    assert!(matches!(notification, IncomingMessage::Notification(_)));
    assert!(matches!(request, IncomingMessage::Request(_)));
}

#[test]
fn invalid_versions_and_ambiguous_messages_are_rejected() {
    assert_eq!(
        parse_message(br#"{"jsonrpc":"1.0","id":1,"result":null}"#),
        Err(ProtocolError::InvalidMessage)
    );
    assert_eq!(
        parse_message(br#"{"jsonrpc":"2.0","id":1,"method":"x","result":null}"#),
        Err(ProtocolError::InvalidMessage)
    );
}

#[test]
fn response_router_rejects_duplicate_and_unknown_ids() {
    let mut ids = RequestIdAllocator::default();
    let id = ids.next_id().unwrap();
    let mut router = ResponseRouter::default();
    router.register(id).unwrap();
    let IncomingMessage::Response(response) = parse_message(
        &serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id.get(),"result":null})).unwrap(),
    )
    .unwrap() else {
        panic!("expected response");
    };

    router.accept(&response).unwrap();
    assert_eq!(
        router.accept(&response),
        Err(ProtocolError::UnexpectedResponse { id: id.get() })
    );
}

#[test]
fn protocol_error_text_is_bounded_without_copying_the_payload() {
    let payload = "x".repeat(20_000);
    let bounded = bounded_error_text(&payload);
    assert!(bounded.len() <= 4_096);
    assert!(bounded.ends_with('…'));
}
