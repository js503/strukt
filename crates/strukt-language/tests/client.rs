use std::time::Duration;

use strukt_language::{
    ClientError, ClientTimeout, FeatureRequestKind, LanguageClient, LanguageServerState,
    LspPosition, PositionEncoding, ResponseDisposition, ServerCapabilities,
    ServerRequestDisposition, SynchronizationKind,
};

#[test]
fn initialization_is_first_and_captures_only_supported_capabilities() {
    let mut client = LanguageClient::new("workspace", "rust-analyzer");
    assert_eq!(
        client.did_open("file:///workspace/main.rs", 1, "fn main() {}"),
        Err(ClientError::NotReady)
    );

    let initialize = client.start(Duration::ZERO).unwrap();
    assert_eq!(initialize.method(), "initialize");
    assert_eq!(client.state(), LanguageServerState::Starting);
    let capabilities = all_capabilities(PositionEncoding::Utf8);
    client
        .accept_initialize(initialize.id().unwrap(), capabilities)
        .unwrap();

    assert_eq!(client.state(), LanguageServerState::Ready);
    assert_eq!(client.capabilities(), Some(capabilities));
    assert_eq!(client.take_outbound().unwrap().method(), "initialized");
}

#[test]
fn stale_response_cannot_cross_document_revision_or_server_generation() {
    let mut client = ready_client();
    client
        .did_open("file:///workspace/main.rs", 4, "fn main() {}")
        .unwrap();
    while client.take_outbound().is_some() {}
    let request = client
        .request_feature(
            FeatureRequestKind::Hover,
            "file:///workspace/main.rs",
            4,
            LspPosition::new(0, 3),
            Duration::from_secs(1),
        )
        .unwrap();
    client
        .did_change(
            "file:///workspace/main.rs",
            5,
            "fn changed() {}",
            Duration::from_millis(1_100),
        )
        .unwrap();
    client.restart_generation(Duration::from_secs(2));

    assert_eq!(
        client.accept_feature_response(&request),
        ResponseDisposition::Stale
    );
}

#[test]
fn full_document_changes_coalesce_to_the_latest_revision() {
    let mut client = ready_client();
    client
        .did_open("file:///workspace/main.rs", 1, "one")
        .unwrap();
    while client.take_outbound().is_some() {}
    client
        .did_change(
            "file:///workspace/main.rs",
            2,
            "two",
            Duration::from_millis(10),
        )
        .unwrap();
    client
        .did_change(
            "file:///workspace/main.rs",
            3,
            "three",
            Duration::from_millis(100),
        )
        .unwrap();

    assert_eq!(client.flush_changes(Duration::from_millis(349)), 0);
    assert_eq!(client.flush_changes(Duration::from_millis(350)), 1);
    let change = client.take_outbound().unwrap();
    assert_eq!(change.method(), "textDocument/didChange");
    assert_eq!(change.params()["textDocument"]["version"], 3);
    assert_eq!(change.params()["contentChanges"][0]["text"], "three");
}

#[test]
fn crash_restarts_are_bounded_with_documented_backoff() {
    let mut client = ready_client();
    assert_eq!(
        client.process_exited(Duration::from_secs(1)),
        Some(Duration::from_millis(250))
    );
    client.start(Duration::from_secs(2)).unwrap();
    assert_eq!(
        client.process_exited(Duration::from_secs(3)),
        Some(Duration::from_secs(1))
    );
    client.start(Duration::from_secs(4)).unwrap();
    assert_eq!(
        client.process_exited(Duration::from_secs(5)),
        Some(Duration::from_secs(4))
    );
    client.start(Duration::from_secs(6)).unwrap();
    assert_eq!(client.process_exited(Duration::from_secs(7)), None);
    assert_eq!(client.state(), LanguageServerState::Failed);
}

#[test]
fn graceful_shutdown_orders_shutdown_before_exit() {
    let mut client = ready_client();
    let shutdown = client.begin_shutdown(Duration::from_secs(1)).unwrap();
    assert_eq!(shutdown.method(), "shutdown");
    assert_eq!(client.state(), LanguageServerState::Stopping);
    client.accept_shutdown(shutdown.id().unwrap()).unwrap();
    assert_eq!(client.take_outbound().unwrap().method(), "exit");
    client.finish_shutdown();
    assert_eq!(client.state(), LanguageServerState::Stopped);
}

#[test]
fn initialize_request_and_shutdown_timeouts_fail_closed() {
    let mut initializing = LanguageClient::new("workspace", "rust-analyzer");
    initializing.start(Duration::ZERO).unwrap();
    assert_eq!(
        initializing.poll_timeouts(Duration::from_secs(10)),
        vec![ClientTimeout::Initialize]
    );
    assert_eq!(initializing.state(), LanguageServerState::Failed);

    let mut stopping = ready_client();
    stopping.begin_shutdown(Duration::from_secs(2)).unwrap();
    assert_eq!(
        stopping.poll_timeouts(Duration::from_secs(4)),
        vec![ClientTimeout::Shutdown]
    );
    assert_eq!(stopping.state(), LanguageServerState::Stopped);
}

#[test]
fn ordinary_requests_timeout_and_empty_clients_idle_shutdown() {
    let mut client = ready_client();
    client
        .did_open("file:///workspace/main.rs", 1, "text")
        .unwrap();
    client.take_outbound();
    let request = client
        .request_feature(
            FeatureRequestKind::Hover,
            "file:///workspace/main.rs",
            1,
            LspPosition::new(0, 0),
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(
        client.poll_timeouts(Duration::from_secs(6)),
        vec![ClientTimeout::Request(request.id())]
    );
    while client.take_outbound().is_some() {}
    client
        .did_close("file:///workspace/main.rs", Duration::from_secs(7))
        .unwrap();
    while client.take_outbound().is_some() {}
    assert_eq!(
        client.poll_timeouts(Duration::from_secs(37)),
        vec![ClientTimeout::IdleShutdown]
    );
    assert_eq!(client.state(), LanguageServerState::Stopping);
    assert_eq!(client.take_outbound().unwrap().method(), "shutdown");
}

#[test]
fn initialization_errors_protocol_failures_and_server_requests_are_isolated() {
    let mut failed = LanguageClient::new("workspace-a", "rust-analyzer");
    let initialize = failed.start(Duration::ZERO).unwrap();
    failed.reject_initialize(initialize.id().unwrap()).unwrap();
    assert_eq!(failed.state(), LanguageServerState::Failed);

    let mut healthy = ready_client();
    assert_eq!(
        healthy.handle_server_request("workspace/configuration"),
        ServerRequestDisposition::Configuration(Vec::new())
    );
    assert_eq!(
        healthy.handle_server_request("workspace/executeClientCommand"),
        ServerRequestDisposition::MethodNotFound
    );
    healthy.fail_protocol();
    assert_eq!(healthy.state(), LanguageServerState::Failed);
    assert_eq!(failed.state(), LanguageServerState::Failed);
}

fn ready_client() -> LanguageClient {
    let mut client = LanguageClient::new("workspace", "rust-analyzer");
    let initialize = client.start(Duration::ZERO).unwrap();
    client
        .accept_initialize(
            initialize.id().unwrap(),
            all_capabilities(PositionEncoding::Utf16),
        )
        .unwrap();
    client.take_outbound();
    client
}

fn all_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities::new(
        SynchronizationKind::Full,
        [
            FeatureRequestKind::Completion,
            FeatureRequestKind::Hover,
            FeatureRequestKind::Definition,
        ],
        encoding,
    )
}
