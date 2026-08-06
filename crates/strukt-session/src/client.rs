use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use thiserror::Error;

use crate::{
    EndpointIdentity, LocalEndpoint, LocalStream, PaneId, PaneScreenSnapshot,
    ProviderCatalogSnapshot, ProviderError, RendezvousStore, RequestBody, RequestEnvelope,
    RequestIdGenerator, ResponseBody, ResponseEnvelope, ServiceInstanceId, ServiceSecret,
    decode_cbor, encode_cbor,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const RETRY_DELAYS: [Duration; 7] = [
    Duration::from_millis(25),
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_secs(2),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientConnectIntent {
    ExplicitCreate,
    ExplicitAttach,
    ExplicitRestart,
    Reconnect,
}

impl ClientConnectIntent {
    const fn may_start_service(self) -> bool {
        !matches!(self, Self::Reconnect)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientHealth {
    Stopped,
    Connecting,
    Ready,
    Stale,
    Failed,
}

pub trait ProviderConnection: Send {
    fn service_instance(&self) -> ServiceInstanceId;

    /// Sends one request and waits for its matching response.
    ///
    /// # Errors
    ///
    /// Returns transport, framing, protocol, or provider errors.
    fn exchange(&mut self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError>;
}

pub trait ClientBackend: Send + Sync {
    /// Connects to an already-running service.
    ///
    /// # Errors
    ///
    /// Returns an error when discovery, authentication, or transport setup fails.
    fn connect(&self) -> Result<Box<dyn ProviderConnection>, ClientError>;

    /// Launches the exact configured service helper.
    ///
    /// # Errors
    ///
    /// Returns an error when the helper cannot be started.
    fn start_service(&self) -> Result<(), ClientError>;

    fn wait(&self, duration: Duration);
}

pub struct SessionClient {
    backend: Arc<dyn ClientBackend>,
    connection: Option<Box<dyn ProviderConnection>>,
    request_ids: RequestIdGenerator,
    in_flight: bool,
    health: ClientHealth,
    service_instance: Option<ServiceInstanceId>,
    catalog: Option<ProviderCatalogSnapshot>,
    snapshots: HashMap<PaneId, PaneScreenSnapshot>,
}

impl SessionClient {
    /// Creates a production client without connecting to or launching the service.
    ///
    /// # Errors
    ///
    /// Returns an error when either trusted path is not absolute and non-traversing.
    pub fn new(
        application_data: impl Into<PathBuf>,
        helper: impl Into<PathBuf>,
    ) -> Result<Self, ClientError> {
        let application_data = validate_absolute(application_data.into())?;
        let helper = validate_absolute(helper.into())?;
        let backend = Arc::new(LocalClientBackend {
            application_data,
            helper,
        });
        Ok(Self::from_backend(backend))
    }

    /// Creates a client around an injected transport backend for deterministic tests.
    ///
    /// # Errors
    ///
    /// Returns an error when either trusted path is not absolute and non-traversing.
    pub fn with_backend<B: ClientBackend + 'static>(
        application_data: PathBuf,
        helper: PathBuf,
        backend: Arc<B>,
    ) -> Result<Self, ClientError> {
        validate_absolute(application_data)?;
        validate_absolute(helper)?;
        Ok(Self::from_backend(backend))
    }

    fn from_backend(backend: Arc<dyn ClientBackend>) -> Self {
        Self {
            backend,
            connection: None,
            request_ids: RequestIdGenerator::new(),
            in_flight: false,
            health: ClientHealth::Stopped,
            service_instance: None,
            catalog: None,
            snapshots: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn health(&self) -> ClientHealth {
        self.health
    }

    #[must_use]
    pub const fn catalog(&self) -> Option<&ProviderCatalogSnapshot> {
        self.catalog.as_ref()
    }

    #[must_use]
    pub fn snapshot(&self, pane: PaneId) -> Option<&PaneScreenSnapshot> {
        self.snapshots.get(&pane)
    }

    #[must_use]
    pub fn accepts_service_instance(&self, instance: ServiceInstanceId) -> bool {
        self.service_instance == Some(instance)
    }

    /// Reserves the single connection lane and constructs a blocking connect job.
    ///
    /// # Errors
    ///
    /// Returns an error when another client job is already in flight or request
    /// identifiers are exhausted.
    pub fn begin_connect(
        &mut self,
        intent: ClientConnectIntent,
    ) -> Result<ClientConnectJob, ClientError> {
        self.reserve()?;
        self.health = ClientHealth::Connecting;
        let request_id = self.next_request_id()?;
        Ok(ClientConnectJob {
            backend: Arc::clone(&self.backend),
            intent,
            request_id,
            expected_catalog_revision: self.catalog_revision(),
        })
    }

    /// Applies a completed connect job to client state.
    ///
    /// # Errors
    ///
    /// Returns the connect, attach, response-correlation, or service-instance error.
    pub fn finish_connect(
        &mut self,
        completion: ClientConnectCompletion,
    ) -> Result<(), ClientError> {
        self.in_flight = false;
        let (connection, response) = match completion.result {
            Ok(value) => value,
            Err(error) => {
                self.health = if self.catalog.is_some() {
                    ClientHealth::Stale
                } else {
                    ClientHealth::Failed
                };
                return Err(error);
            }
        };
        let instance = connection.service_instance();
        let snapshot = match attached_snapshot(&response, completion.request_id, instance) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.health = if self.catalog.is_some() {
                    ClientHealth::Stale
                } else {
                    ClientHealth::Failed
                };
                return Err(error);
            }
        };
        self.service_instance = Some(instance);
        self.catalog = Some(snapshot);
        self.connection = Some(connection);
        self.health = ClientHealth::Ready;
        Ok(())
    }

    /// Reserves the connection lane and constructs a blocking request job.
    ///
    /// # Errors
    ///
    /// Returns an error when disconnected, stale, or another request is in flight.
    pub fn begin_request(&mut self, body: RequestBody) -> Result<ClientRequestJob, ClientError> {
        self.reserve()?;
        if self.health != ClientHealth::Ready {
            self.in_flight = false;
            return Err(ClientError::Unavailable);
        }
        let request_id = self.next_request_id()?;
        let Some(connection) = self.connection.take() else {
            self.in_flight = false;
            return Err(ClientError::Unavailable);
        };
        Ok(ClientRequestJob {
            connection,
            request: RequestEnvelope::new(request_id, self.catalog_revision(), body),
        })
    }

    /// Constructs the explicit detach request; dropping the app does not imply detach.
    ///
    /// # Errors
    ///
    /// Returns the same state errors as [`Self::begin_request`].
    pub fn begin_detach(&mut self) -> Result<ClientRequestJob, ClientError> {
        self.begin_request(RequestBody::Detach)
    }

    /// Applies a completed request and returns its typed response body.
    ///
    /// # Errors
    ///
    /// Returns transport, provider, response-correlation, or stale-service errors.
    pub fn finish_request(
        &mut self,
        completion: ClientRequestCompletion,
    ) -> Result<ResponseBody, ClientError> {
        self.in_flight = false;
        let connection = completion.connection;
        let response = match completion.result {
            Ok(response) => response,
            Err(error) => {
                self.mark_transport_lost(error.to_string());
                return Err(error);
            }
        };
        if response.request_id() != completion.request_id {
            self.mark_transport_lost("response request identifier mismatch");
            return Err(ClientError::Protocol);
        }
        let body = match response.result().clone() {
            Ok(body) => body,
            Err(error) => {
                self.connection = Some(connection);
                return Err(ClientError::Provider(error));
            }
        };
        match &body {
            ResponseBody::Catalog(snapshot)
            | ResponseBody::Attached(snapshot)
            | ResponseBody::CatalogChanged(snapshot) => {
                if snapshot.service_instance() != connection.service_instance() {
                    self.mark_transport_lost("stale service response");
                    return Err(ClientError::StaleService);
                }
                self.service_instance = Some(snapshot.service_instance());
                self.catalog = Some(snapshot.clone());
            }
            ResponseBody::PaneSnapshot(snapshot) => {
                // The request body carries the pane identity; service snapshots are
                // installed by the app with `apply_snapshot` after routing it.
                let _ = snapshot;
            }
            ResponseBody::Detached => {
                self.connection = None;
                self.service_instance = None;
                self.health = ClientHealth::Stopped;
                return Ok(body);
            }
            _ => {}
        }
        self.connection = Some(connection);
        Ok(body)
    }

    pub fn mark_transport_lost(&mut self, _detail: impl AsRef<str>) {
        self.connection = None;
        self.in_flight = false;
        self.health = if self.catalog.is_some() {
            ClientHealth::Stale
        } else {
            ClientHealth::Failed
        };
    }

    pub fn apply_snapshot(&mut self, pane: PaneId, snapshot: PaneScreenSnapshot) -> bool {
        let is_newer = self.snapshots.get(&pane).is_none_or(|current| {
            snapshot.generation() > current.generation()
                || (snapshot.generation() == current.generation()
                    && snapshot.output_revision() > current.output_revision())
        });
        if is_newer {
            self.snapshots.insert(pane, snapshot);
        }
        is_newer
    }

    fn reserve(&mut self) -> Result<(), ClientError> {
        if self.in_flight {
            return Err(ClientError::RequestInFlight);
        }
        self.in_flight = true;
        Ok(())
    }

    fn next_request_id(&mut self) -> Result<u64, ClientError> {
        match self.request_ids.next_id() {
            Ok(id) => Ok(id),
            Err(error) => {
                self.in_flight = false;
                Err(ClientError::ProtocolDetail(error.to_string()))
            }
        }
    }

    fn catalog_revision(&self) -> u64 {
        self.catalog
            .as_ref()
            .map_or(0, |snapshot| snapshot.catalog().revision())
    }
}

pub struct ClientConnectJob {
    backend: Arc<dyn ClientBackend>,
    intent: ClientConnectIntent,
    request_id: u64,
    expected_catalog_revision: u64,
}

impl ClientConnectJob {
    #[must_use]
    pub fn run(self) -> ClientConnectCompletion {
        let mut attempted_start = false;
        let mut retries = RETRY_DELAYS.iter();
        let result = loop {
            match self.backend.connect() {
                Ok(mut connection) => {
                    let request = RequestEnvelope::new(
                        self.request_id,
                        self.expected_catalog_revision,
                        RequestBody::Attach,
                    );
                    break connection
                        .exchange(request)
                        .map(|response| (connection, response));
                }
                Err(error) => {
                    if self.intent.may_start_service() && !attempted_start {
                        attempted_start = true;
                        if let Err(start_error) = self.backend.start_service() {
                            break Err(start_error);
                        }
                    } else if attempted_start || !self.intent.may_start_service() {
                        // Continue through the bounded retry schedule below.
                    } else {
                        break Err(error);
                    }
                }
            }
            let Some(delay) = retries.next() else {
                break Err(ClientError::Unavailable);
            };
            self.backend.wait(*delay);
        };
        ClientConnectCompletion {
            request_id: self.request_id,
            result,
        }
    }
}

pub struct ClientConnectCompletion {
    request_id: u64,
    result: Result<(Box<dyn ProviderConnection>, ResponseEnvelope), ClientError>,
}

pub struct ClientRequestJob {
    connection: Box<dyn ProviderConnection>,
    request: RequestEnvelope,
}

impl ClientRequestJob {
    #[must_use]
    pub fn run(mut self) -> ClientRequestCompletion {
        let request_id = self.request.request_id();
        let result = self.connection.exchange(self.request);
        ClientRequestCompletion {
            request_id,
            connection: self.connection,
            result,
        }
    }
}

pub struct ClientRequestCompletion {
    request_id: u64,
    connection: Box<dyn ProviderConnection>,
    result: Result<ResponseEnvelope, ClientError>,
}

struct LocalClientBackend {
    application_data: PathBuf,
    helper: PathBuf,
}

impl ClientBackend for LocalClientBackend {
    fn connect(&self) -> Result<Box<dyn ProviderConnection>, ClientError> {
        let record = RendezvousStore::at(self.application_data.clone())
            .load()?
            .ok_or(ClientError::Unavailable)?;
        let instance = record.service_instance();
        let identity = EndpointIdentity::from_record(
            &self.application_data,
            instance,
            record.endpoint_identity(),
        )?;
        let secret =
            ServiceSecret::load_from(self.application_data.join(record.secret_reference()))?;
        let stream = LocalEndpoint::connect_authenticated(&identity, instance, &secret)?;
        Ok(Box::new(LocalProviderConnection { instance, stream }))
    }

    fn start_service(&self) -> Result<(), ClientError> {
        Command::new(&self.helper)
            .arg("--app-data")
            .arg(&self.application_data)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(ClientError::Io)
    }

    fn wait(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

struct LocalProviderConnection {
    instance: ServiceInstanceId,
    stream: LocalStream,
}

impl ProviderConnection for LocalProviderConnection {
    fn service_instance(&self) -> ServiceInstanceId {
        self.instance
    }

    fn exchange(&mut self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        let frame = encode_cbor(&request, MAX_FRAME_BYTES)?;
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        let mut header = [0_u8; 4];
        self.stream.read_exact(&mut header)?;
        let length = u32::from_be_bytes(header) as usize;
        if length > MAX_FRAME_BYTES {
            return Err(ClientError::Protocol);
        }
        let mut payload = vec![0_u8; length];
        self.stream.read_exact(&mut payload)?;
        decode_cbor(&payload).map_err(ClientError::from)
    }
}

fn attached_snapshot(
    response: &ResponseEnvelope,
    request_id: u64,
    instance: ServiceInstanceId,
) -> Result<ProviderCatalogSnapshot, ClientError> {
    if response.request_id() != request_id {
        return Err(ClientError::Protocol);
    }
    match response.result() {
        Ok(ResponseBody::Attached(snapshot)) if snapshot.service_instance() == instance => {
            Ok(snapshot.clone())
        }
        Ok(ResponseBody::Attached(_)) => Err(ClientError::StaleService),
        Ok(_) => Err(ClientError::Protocol),
        Err(error) => Err(ClientError::Provider(error.clone())),
    }
}

fn validate_absolute(path: PathBuf) -> Result<PathBuf, ClientError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ClientError::InvalidPath);
    }
    Ok(path)
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("session service is unavailable")]
    Unavailable,
    #[error("another session request is already in flight")]
    RequestInFlight,
    #[error("session service transport was lost")]
    TransportLost,
    #[error("session response came from a stale service instance")]
    StaleService,
    #[error("session client protocol validation failed")]
    Protocol,
    #[error("trusted session path is invalid")]
    InvalidPath,
    #[error("session client protocol failed: {0}")]
    ProtocolDetail(String),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("session client IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("session endpoint failed: {0}")]
    Endpoint(#[from] crate::EndpointError),
    #[error("session rendezvous failed: {0}")]
    Rendezvous(#[from] crate::RendezvousError),
    #[error("session authentication failed: {0}")]
    Authentication(#[from] crate::AuthenticationError),
    #[error("session framing failed: {0}")]
    Frame(#[from] crate::FrameError),
}
