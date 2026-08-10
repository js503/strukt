use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strukt_remote::{NativeSessionManager, PersistentProvider, SessionPayload};
use strukt_session::{
    ClientBackend, ClientError, ProviderCapabilities, ProviderCatalogSnapshot, ProviderConnection,
    ProviderKind, RequestBody, RequestEnvelope, ResponseBody, ResponseEnvelope, ServiceInstanceId,
    SessionCatalog, SessionClient, decode_cbor, encode_cbor,
};

const MAX_FRAME: usize = 1024 * 1024;

#[test]
fn native_proxy_starts_only_for_explicit_attach_and_preserves_correlation() {
    let instance = ServiceInstanceId::new().unwrap();
    let backend = Arc::new(FakeBackend::default());
    backend.fail_connects(1);
    backend.queue(FakeConnection::attached(instance));
    let manager = manager(backend.clone());

    let response = exchange(&manager, &RequestEnvelope::new(41, 0, RequestBody::Attach));
    assert_eq!(response.request_id(), 41);
    assert!(matches!(response.result(), Ok(ResponseBody::Attached(_))));
    assert_eq!(backend.starts(), 1);
}

#[test]
fn reconnect_never_starts_an_absent_native_service() {
    let backend = Arc::new(FakeBackend::default());
    backend.fail_connects(20);
    let manager = manager(backend.clone());
    let response = exchange(
        &manager,
        &RequestEnvelope::new(
            42,
            0,
            RequestBody::Reconnect {
                cursors: Vec::new(),
            },
        ),
    );
    assert_eq!(response.request_id(), 42);
    assert!(response.result().is_err());
    assert_eq!(backend.starts(), 0);
}

#[test]
fn native_proxy_rejects_wrong_provider_and_trailing_frames() {
    let manager = manager(Arc::new(FakeBackend::default()));
    let frame = encode_cbor(&RequestEnvelope::new(1, 0, RequestBody::Attach), MAX_FRAME).unwrap();
    let wrong = SessionPayload::new(PersistentProvider::Tmux, frame.clone()).unwrap();
    assert!(manager.exchange(&wrong).is_err());

    let mut combined = frame.clone();
    combined.extend_from_slice(&frame);
    let combined = SessionPayload::new(PersistentProvider::Native, combined).unwrap();
    assert!(manager.exchange(&combined).is_err());
}

fn manager(backend: Arc<FakeBackend>) -> NativeSessionManager {
    let client = SessionClient::with_backend(test_path("data"), test_path("sessiond"), backend)
        .expect("client");
    NativeSessionManager::from_client(client)
}

fn exchange(manager: &NativeSessionManager, request: &RequestEnvelope) -> ResponseEnvelope {
    let bytes = encode_cbor(&request, MAX_FRAME).unwrap();
    let payload = SessionPayload::new(PersistentProvider::Native, bytes).unwrap();
    let response = manager.exchange(&payload).unwrap();
    decode_framed(response.bytes())
}

fn decode_framed(bytes: &[u8]) -> ResponseEnvelope {
    let length = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert_eq!(length, bytes.len() - 4);
    decode_cbor(&bytes[4..]).unwrap()
}

fn test_path(name: &str) -> PathBuf {
    std::env::current_dir().unwrap().join(name)
}

#[derive(Default)]
struct FakeBackend {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    failed_connects: usize,
    starts: usize,
    connections: VecDeque<FakeConnection>,
}

impl FakeBackend {
    fn fail_connects(&self, count: usize) {
        self.state.lock().unwrap().failed_connects = count;
    }

    fn queue(&self, connection: FakeConnection) {
        self.state.lock().unwrap().connections.push_back(connection);
    }

    fn starts(&self) -> usize {
        self.state.lock().unwrap().starts
    }
}

impl ClientBackend for FakeBackend {
    fn connect(&self) -> Result<Box<dyn ProviderConnection>, ClientError> {
        let mut state = self.state.lock().unwrap();
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
        self.state.lock().unwrap().starts += 1;
        Ok(())
    }

    fn wait(&self, _duration: Duration) {}
}

struct FakeConnection {
    instance: ServiceInstanceId,
    responses: VecDeque<ResponseBody>,
}

impl FakeConnection {
    fn attached(instance: ServiceInstanceId) -> Self {
        Self {
            instance,
            responses: VecDeque::from([ResponseBody::Attached(ProviderCatalogSnapshot::new(
                instance,
                ProviderKind::NativeRemote,
                ProviderCapabilities::native_remote(),
                SessionCatalog::new(),
            ))]),
        }
    }
}

impl ProviderConnection for FakeConnection {
    fn service_instance(&self) -> ServiceInstanceId {
        self.instance
    }

    fn exchange(&mut self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        let body = self
            .responses
            .pop_front()
            .ok_or(ClientError::TransportLost)?;
        Ok(ResponseEnvelope::ok(request.request_id(), body))
    }
}
