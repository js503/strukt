use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strukt_session::{
    ClientBackend, ClientConnectIntent, ClientError, ClientHealth, PaneId, PaneLifecycle,
    PaneScreenSnapshot, ProviderCapabilities, ProviderCatalogSnapshot, ProviderConnection,
    ProviderKind, RequestBody, RequestEnvelope, ResponseBody, ResponseEnvelope, ServiceInstanceId,
    SessionCatalog, SessionClient,
};
use strukt_terminal::{GridSize, TerminalModel};

#[test]
fn service_start_is_lazy_and_only_explicit_connect_intents_may_launch_it() {
    let backend = Arc::new(FakeBackend::default());
    let mut client = SessionClient::with_backend(
        PathBuf::from("/application-data"),
        PathBuf::from("/repository/bin/strukt-sessiond"),
        backend.clone(),
    )
    .expect("client");
    assert_eq!(backend.starts(), 0);
    assert_eq!(backend.connects(), 0);

    backend.fail_connects(1);
    backend.queue_connection(FakeConnection::attached_snapshot(empty_snapshot()));
    let completion = client
        .begin_connect(ClientConnectIntent::ExplicitAttach)
        .expect("connect job")
        .run();
    assert_eq!(backend.starts(), 1);
    assert_eq!(
        backend.start_arguments(),
        vec!["--app-data", "/application-data"]
    );
    client.finish_connect(completion).expect("finish connect");
    assert_eq!(client.health(), ClientHealth::Ready);

    client.mark_transport_lost("fixture disconnect");
    backend.fail_connects(9);
    let completion = client
        .begin_connect(ClientConnectIntent::Reconnect)
        .expect("reconnect job")
        .run();
    assert!(client.finish_connect(completion).is_err());
    assert_eq!(backend.starts(), 1, "background reconnect never launches");
    assert!(backend.max_sleep() <= Duration::from_secs(2));
}

#[test]
fn requests_are_monotonic_single_writer_and_detach_is_explicit() {
    let instance = ServiceInstanceId::new().expect("service instance");
    let backend = Arc::new(FakeBackend::default());
    backend.queue_connection(FakeConnection::new(
        instance,
        VecDeque::from([
            ResponseBody::Attached(snapshot(instance, SessionCatalog::new())),
            ResponseBody::PaneWritten,
            ResponseBody::Catalog(snapshot(instance, SessionCatalog::new())),
            ResponseBody::Detached,
        ]),
    ));
    let mut client = test_client(backend.clone());
    let completion = client
        .begin_connect(ClientConnectIntent::ExplicitAttach)
        .expect("connect job")
        .run();
    client.finish_connect(completion).expect("attach");

    let pane = PaneId::new().expect("pane");
    let request = client
        .begin_request(RequestBody::WritePane {
            pane,
            generation: 1,
            bytes: b"input".to_vec(),
        })
        .expect("request job")
        .run();
    client.finish_request(request).expect("write response");
    let catalog = client
        .begin_request(RequestBody::Catalog)
        .expect("catalog job");
    assert!(
        client.begin_request(RequestBody::Catalog).is_err(),
        "one in flight"
    );
    client
        .finish_request(catalog.run())
        .expect("catalog response");

    let detached = client.begin_detach().expect("detach job").run();
    client.finish_request(detached).expect("detach response");
    assert_eq!(client.health(), ClientHealth::Stopped);
    assert_eq!(backend.request_ids(), vec![1, 2, 3, 4]);
}

#[test]
fn transport_loss_freezes_catalog_and_newest_snapshots_while_stale_work_is_rejected() {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "local", directory)
        .expect("session");
    let pane = catalog
        .session(session)
        .expect("session")
        .active_window()
        .expect("window")
        .focused_pane()
        .id();
    let instance = ServiceInstanceId::new().expect("service instance");
    let backend = Arc::new(FakeBackend::default());
    backend.queue_connection(FakeConnection::attached_snapshot(snapshot(
        instance, catalog,
    )));
    let mut client = test_client(backend);
    let completion = client
        .begin_connect(ClientConnectIntent::ExplicitAttach)
        .expect("connect job")
        .run();
    client.finish_connect(completion).expect("attach");

    let newest = pane_snapshot(5);
    let stale = pane_snapshot(4);
    assert!(client.apply_snapshot(pane, newest.clone()));
    assert!(!client.apply_snapshot(pane, stale));
    client.mark_transport_lost("socket closed");

    assert_eq!(client.health(), ClientHealth::Stale);
    assert_eq!(
        client
            .catalog()
            .expect("frozen catalog")
            .catalog()
            .sessions()
            .count(),
        1
    );
    assert_eq!(client.snapshot(pane), Some(&newest));
    assert!(client.begin_request(RequestBody::Catalog).is_err());

    let other = ServiceInstanceId::new().expect("other instance");
    assert!(!client.accepts_service_instance(other));
    assert!(client.accepts_service_instance(instance));
}

fn test_client(backend: Arc<FakeBackend>) -> SessionClient {
    SessionClient::with_backend(
        PathBuf::from("/application-data"),
        PathBuf::from("/repository/bin/strukt-sessiond"),
        backend,
    )
    .expect("client")
}

fn empty_snapshot() -> ProviderCatalogSnapshot {
    snapshot(
        ServiceInstanceId::new().expect("service instance"),
        SessionCatalog::new(),
    )
}

fn snapshot(instance: ServiceInstanceId, catalog: SessionCatalog) -> ProviderCatalogSnapshot {
    ProviderCatalogSnapshot::new(
        instance,
        ProviderKind::NativeLocal,
        ProviderCapabilities::native_local(),
        catalog,
    )
}

fn pane_snapshot(output_revision: u64) -> PaneScreenSnapshot {
    let model = TerminalModel::new(GridSize::new(2, 20).expect("grid"), 10);
    PaneScreenSnapshot::from_terminal(
        &model.snapshot(0),
        output_revision,
        1,
        PaneLifecycle::Running,
        0,
        strukt_session::AttentionState::None,
    )
    .expect("pane snapshot")
}

#[derive(Default)]
struct FakeBackend {
    state: Mutex<FakeBackendState>,
}

#[derive(Default)]
struct FakeBackendState {
    failed_connects: usize,
    connects: usize,
    starts: usize,
    start_arguments: Vec<String>,
    sleeps: Vec<Duration>,
    connections: VecDeque<FakeConnection>,
    request_ids: Arc<Mutex<Vec<u64>>>,
}

impl FakeBackend {
    fn fail_connects(&self, count: usize) {
        self.state.lock().expect("backend").failed_connects = count;
    }

    fn queue_connection(&self, mut connection: FakeConnection) {
        connection.request_ids = self.state.lock().expect("backend").request_ids.clone();
        self.state
            .lock()
            .expect("backend")
            .connections
            .push_back(connection);
    }

    fn starts(&self) -> usize {
        self.state.lock().expect("backend").starts
    }

    fn connects(&self) -> usize {
        self.state.lock().expect("backend").connects
    }

    fn start_arguments(&self) -> Vec<String> {
        self.state.lock().expect("backend").start_arguments.clone()
    }

    fn max_sleep(&self) -> Duration {
        self.state
            .lock()
            .expect("backend")
            .sleeps
            .iter()
            .copied()
            .max()
            .unwrap_or_default()
    }

    fn request_ids(&self) -> Vec<u64> {
        self.state
            .lock()
            .expect("backend")
            .request_ids
            .lock()
            .expect("request ids")
            .clone()
    }
}

impl ClientBackend for FakeBackend {
    fn connect(&self) -> Result<Box<dyn ProviderConnection>, ClientError> {
        let mut state = self.state.lock().expect("backend");
        state.connects += 1;
        if state.failed_connects > 0 {
            state.failed_connects -= 1;
            return Err(ClientError::Unavailable);
        }
        state
            .connections
            .pop_front()
            .map(|connection| Box::new(connection) as Box<dyn ProviderConnection>)
            .ok_or(ClientError::Unavailable)
    }

    fn start_service(&self) -> Result<(), ClientError> {
        let mut state = self.state.lock().expect("backend");
        state.starts += 1;
        state.start_arguments = vec!["--app-data".to_owned(), "/application-data".to_owned()];
        Ok(())
    }

    fn wait(&self, duration: Duration) {
        self.state.lock().expect("backend").sleeps.push(duration);
    }
}

struct FakeConnection {
    instance: ServiceInstanceId,
    responses: VecDeque<ResponseBody>,
    request_ids: Arc<Mutex<Vec<u64>>>,
}

impl FakeConnection {
    fn new(instance: ServiceInstanceId, responses: VecDeque<ResponseBody>) -> Self {
        Self {
            instance,
            responses,
            request_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn attached_snapshot(snapshot: ProviderCatalogSnapshot) -> Self {
        Self::new(
            snapshot.service_instance(),
            VecDeque::from([ResponseBody::Attached(snapshot)]),
        )
    }
}

impl ProviderConnection for FakeConnection {
    fn service_instance(&self) -> ServiceInstanceId {
        self.instance
    }

    fn exchange(&mut self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        self.request_ids
            .lock()
            .expect("request ids")
            .push(request.request_id());
        let response = self
            .responses
            .pop_front()
            .ok_or(ClientError::TransportLost)?;
        Ok(ResponseEnvelope::ok(request.request_id(), response))
    }
}
