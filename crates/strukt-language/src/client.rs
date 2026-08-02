use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use serde_json::{Value, json};
use thiserror::Error;

use crate::{LspPosition, PositionEncoding, ProtocolError, RequestId, RequestIdAllocator};

const OUTBOUND_LIMIT: usize = 256;
const REQUEST_LIMIT: usize = 256;
const CHANGE_DELAY: Duration = Duration::from_millis(250);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const IDLE_SHUTDOWN_DELAY: Duration = Duration::from_secs(30);
const RESTART_WINDOW: Duration = Duration::from_mins(10);
const RESTART_DELAYS: [Duration; 3] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(4),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageServerState {
    Discovering,
    Starting,
    Ready,
    Degraded,
    Restarting,
    Failed,
    Stopping,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerCapabilities {
    synchronization: SynchronizationKind,
    features: u8,
    position_encoding: PositionEncoding,
}

impl ServerCapabilities {
    #[must_use]
    pub fn new(
        synchronization: SynchronizationKind,
        features: impl IntoIterator<Item = FeatureRequestKind>,
        position_encoding: PositionEncoding,
    ) -> Self {
        let features = features
            .into_iter()
            .fold(0, |flags, feature| flags | feature.flag());
        Self {
            synchronization,
            features,
            position_encoding,
        }
    }

    #[must_use]
    pub const fn supports(&self, kind: FeatureRequestKind) -> bool {
        self.features & kind.flag() != 0
    }

    #[must_use]
    pub const fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizationKind {
    None,
    Full,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutboundMessage {
    id: Option<RequestId>,
    method: String,
    params: Value,
}

impl OutboundMessage {
    #[must_use]
    pub const fn id(&self) -> Option<RequestId> {
        self.id
    }

    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    #[must_use]
    pub const fn params(&self) -> &Value {
        &self.params
    }

    #[must_use]
    pub fn json_rpc(&self) -> Value {
        let mut message = serde_json::Map::from_iter([
            ("jsonrpc".to_owned(), Value::String("2.0".to_owned())),
            ("method".to_owned(), Value::String(self.method.clone())),
            ("params".to_owned(), self.params.clone()),
        ]);
        if let Some(id) = self.id {
            message.insert("id".to_owned(), Value::from(id.get()));
        }
        Value::Object(message)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FeatureRequestKind {
    Completion,
    Hover,
    Definition,
}

impl FeatureRequestKind {
    const fn method(self) -> &'static str {
        match self {
            Self::Completion => "textDocument/completion",
            Self::Hover => "textDocument/hover",
            Self::Definition => "textDocument/definition",
        }
    }

    const fn flag(self) -> u8 {
        match self {
            Self::Completion => 1,
            Self::Hover => 1 << 1,
            Self::Definition => 1 << 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeatureRequest {
    id: RequestId,
    kind: FeatureRequestKind,
    document: String,
    revision: u64,
    position: LspPosition,
    generation: u64,
    deadline: Duration,
}

impl FeatureRequest {
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseDisposition {
    Applied,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientTimeout {
    Initialize,
    Request(RequestId),
    Shutdown,
    IdleShutdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerRequestDisposition {
    Configuration(Vec<Value>),
    MethodNotFound,
}

#[derive(Clone, Debug)]
struct DocumentState {
    revision: u64,
}

#[derive(Clone, Debug)]
struct PendingChange {
    revision: u64,
    text: String,
    due: Duration,
}

#[derive(Debug)]
pub struct LanguageClient {
    workspace_id: String,
    descriptor_id: String,
    state: LanguageServerState,
    generation: u64,
    ids: RequestIdAllocator,
    capabilities: Option<ServerCapabilities>,
    initialize_id: Option<RequestId>,
    initialize_deadline: Option<Duration>,
    shutdown_id: Option<RequestId>,
    shutdown_deadline: Option<Duration>,
    idle_deadline: Option<Duration>,
    outbound: VecDeque<OutboundMessage>,
    documents: HashMap<String, DocumentState>,
    pending_changes: HashMap<String, PendingChange>,
    requests: HashMap<RequestId, FeatureRequest>,
    crash_times: VecDeque<Duration>,
}

impl LanguageClient {
    #[must_use]
    pub fn new(workspace_id: impl Into<String>, descriptor_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            descriptor_id: descriptor_id.into(),
            state: LanguageServerState::Stopped,
            generation: 0,
            ids: RequestIdAllocator::default(),
            capabilities: None,
            initialize_id: None,
            initialize_deadline: None,
            shutdown_id: None,
            shutdown_deadline: None,
            idle_deadline: None,
            outbound: VecDeque::new(),
            documents: HashMap::new(),
            pending_changes: HashMap::new(),
            requests: HashMap::new(),
            crash_times: VecDeque::new(),
        }
    }

    #[must_use]
    pub const fn state(&self) -> LanguageServerState {
        self.state
    }

    #[must_use]
    pub const fn capabilities(&self) -> Option<ServerCapabilities> {
        self.capabilities
    }

    /// Begins a new process generation with an initialize request.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid lifecycle state or exhausted request IDs.
    pub fn start(&mut self, now: Duration) -> Result<OutboundMessage, ClientError> {
        if !matches!(
            self.state,
            LanguageServerState::Stopped | LanguageServerState::Restarting
        ) {
            return Err(ClientError::InvalidState);
        }
        self.generation = self.generation.saturating_add(1);
        self.state = LanguageServerState::Starting;
        self.capabilities = None;
        let id = self.ids.next_id()?;
        self.initialize_id = Some(id);
        self.initialize_deadline = Some(now + INITIALIZE_TIMEOUT);
        Ok(OutboundMessage {
            id: Some(id),
            method: "initialize".to_owned(),
            params: json!({
                "processId": null,
                "clientInfo": {"name": "strukt"},
                "workspaceFolders": [{"uri": self.workspace_id, "name": self.workspace_id}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-8", "utf-16"]},
                    "textDocument": {
                        "synchronization": {},
                        "completion": {},
                        "hover": {},
                        "definition": {}
                    }
                }
            }),
        })
    }

    /// Completes initialization for the active generation.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale ID, invalid state, or a full outbound queue.
    pub fn accept_initialize(
        &mut self,
        id: RequestId,
        capabilities: ServerCapabilities,
    ) -> Result<(), ClientError> {
        if self.state != LanguageServerState::Starting || self.initialize_id != Some(id) {
            return Err(ClientError::StaleResponse);
        }
        self.initialize_id = None;
        self.initialize_deadline = None;
        self.capabilities = Some(capabilities);
        self.state = LanguageServerState::Ready;
        self.push_outbound(OutboundMessage {
            id: None,
            method: "initialized".to_owned(),
            params: json!({}),
        })
    }

    /// Rejects initialization for the active generation.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale response identifier.
    pub fn reject_initialize(&mut self, id: RequestId) -> Result<(), ClientError> {
        if self.state != LanguageServerState::Starting || self.initialize_id != Some(id) {
            return Err(ClientError::StaleResponse);
        }
        self.fail_active_generation();
        Ok(())
    }

    #[must_use]
    pub fn handle_server_request(&self, method: &str) -> ServerRequestDisposition {
        if method == "workspace/configuration" {
            ServerRequestDisposition::Configuration(Vec::new())
        } else {
            ServerRequestDisposition::MethodNotFound
        }
    }

    pub fn fail_protocol(&mut self) {
        self.fail_active_generation();
    }

    /// Records and queues a full document-open notification.
    ///
    /// # Errors
    ///
    /// Returns an error when the client is not ready or the queue is full.
    pub fn did_open(
        &mut self,
        document: impl Into<String>,
        revision: u64,
        text: &str,
    ) -> Result<(), ClientError> {
        self.ensure_ready()?;
        let document = document.into();
        self.documents
            .insert(document.clone(), DocumentState { revision });
        self.idle_deadline = None;
        self.push_outbound(OutboundMessage {
            id: None,
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": document,
                    "languageId": self.descriptor_id,
                    "version": revision,
                    "text": text
                }
            }),
        })
    }

    /// Queues a save notification for a synchronized document.
    ///
    /// # Errors
    ///
    /// Returns an error when the client is not ready, the document is unknown,
    /// or the outbound queue is full.
    pub fn did_save(&mut self, document: &str) -> Result<(), ClientError> {
        self.ensure_ready()?;
        if !self.documents.contains_key(document) {
            return Err(ClientError::UnknownDocument);
        }
        self.push_outbound(OutboundMessage {
            id: None,
            method: "textDocument/didSave".to_owned(),
            params: json!({"textDocument": {"uri": document}}),
        })
    }

    /// Closes a synchronized document and schedules idle shutdown if it was the
    /// final open document.
    ///
    /// # Errors
    ///
    /// Returns an error when the client is not ready, the document is unknown,
    /// or the outbound queue is full.
    pub fn did_close(&mut self, document: &str, now: Duration) -> Result<(), ClientError> {
        self.ensure_ready()?;
        if self.documents.remove(document).is_none() {
            return Err(ClientError::UnknownDocument);
        }
        self.pending_changes.remove(document);
        self.invalidate_document_requests(document)?;
        self.push_outbound(OutboundMessage {
            id: None,
            method: "textDocument/didClose".to_owned(),
            params: json!({"textDocument": {"uri": document}}),
        })?;
        if self.documents.is_empty() {
            self.idle_deadline = Some(now + IDLE_SHUTDOWN_DELAY);
        }
        Ok(())
    }

    /// Records a revision and schedules its latest full text for coalescing.
    ///
    /// # Errors
    ///
    /// Returns an error when the client is not ready or the document is closed.
    pub fn did_change(
        &mut self,
        document: &str,
        revision: u64,
        text: impl Into<String>,
        now: Duration,
    ) -> Result<(), ClientError> {
        self.ensure_ready()?;
        let state = self
            .documents
            .get_mut(document)
            .ok_or(ClientError::UnknownDocument)?;
        if revision <= state.revision {
            return Err(ClientError::InvalidRevision);
        }
        state.revision = revision;
        self.pending_changes.insert(
            document.to_owned(),
            PendingChange {
                revision,
                text: text.into(),
                due: now + CHANGE_DELAY,
            },
        );
        self.invalidate_document_requests(document)?;
        Ok(())
    }

    /// Queues every coalesced document change due at `now`.
    ///
    /// Returns the number of queued changes.
    pub fn flush_changes(&mut self, now: Duration) -> usize {
        let mut due = self
            .pending_changes
            .iter()
            .filter_map(|(document, change)| (change.due <= now).then_some(document.clone()))
            .collect::<Vec<_>>();
        due.sort_unstable();
        let mut queued = 0;
        for document in due {
            let Some(change) = self.pending_changes.remove(&document) else {
                continue;
            };
            let message = OutboundMessage {
                id: None,
                method: "textDocument/didChange".to_owned(),
                params: json!({
                    "textDocument": {"uri": document, "version": change.revision},
                    "contentChanges": [{"text": change.text}]
                }),
            };
            if self.push_outbound(message).is_ok() {
                queued += 1;
            }
        }
        queued
    }

    /// Creates a revision- and generation-scoped language feature request.
    ///
    /// # Errors
    ///
    /// Returns an error when unsupported, stale, or bounded request state would
    /// be exceeded.
    pub fn request_feature(
        &mut self,
        kind: FeatureRequestKind,
        document: &str,
        revision: u64,
        position: LspPosition,
        now: Duration,
    ) -> Result<FeatureRequest, ClientError> {
        self.ensure_ready()?;
        if !self
            .capabilities
            .is_some_and(|capabilities| capabilities.supports(kind))
        {
            return Err(ClientError::UnsupportedFeature);
        }
        if self.documents.get(document).map(|state| state.revision) != Some(revision) {
            return Err(ClientError::InvalidRevision);
        }
        if self.requests.len() >= REQUEST_LIMIT {
            return Err(ClientError::TooManyRequests);
        }
        self.invalidate_feature_request(kind, document)?;
        let id = self.ids.next_id()?;
        let request = FeatureRequest {
            id,
            kind,
            document: document.to_owned(),
            revision,
            position,
            generation: self.generation,
            deadline: now + REQUEST_TIMEOUT,
        };
        self.push_outbound(OutboundMessage {
            id: Some(id),
            method: kind.method().to_owned(),
            params: json!({
                "textDocument": {"uri": document},
                "position": {"line": position.line, "character": position.character}
            }),
        })?;
        self.requests.insert(id, request.clone());
        Ok(request)
    }

    #[must_use]
    pub fn accept_feature_response(&mut self, request: &FeatureRequest) -> ResponseDisposition {
        let current = self.requests.remove(&request.id);
        let is_current = current.as_ref() == Some(request)
            && request.generation == self.generation
            && self
                .documents
                .get(&request.document)
                .is_some_and(|state| state.revision == request.revision)
            && self.state == LanguageServerState::Ready;
        if is_current {
            ResponseDisposition::Applied
        } else {
            ResponseDisposition::Stale
        }
    }

    pub fn restart_generation(&mut self, _now: Duration) {
        self.generation = self.generation.saturating_add(1);
        self.state = LanguageServerState::Restarting;
        self.capabilities = None;
        self.initialize_id = None;
        self.initialize_deadline = None;
        self.shutdown_id = None;
        self.shutdown_deadline = None;
        self.idle_deadline = None;
        self.outbound.clear();
        self.pending_changes.clear();
        self.requests.clear();
    }

    /// Records an unexpected process exit and returns the bounded restart delay.
    pub fn process_exited(&mut self, now: Duration) -> Option<Duration> {
        self.restart_generation(now);
        while self
            .crash_times
            .front()
            .is_some_and(|time| now.saturating_sub(*time) > RESTART_WINDOW)
        {
            self.crash_times.pop_front();
        }
        self.crash_times.push_back(now);
        let index = self.crash_times.len() - 1;
        let delay = RESTART_DELAYS.get(index).copied();
        if delay.is_none() {
            self.state = LanguageServerState::Failed;
        }
        delay
    }

    /// Begins graceful shutdown with a request.
    ///
    /// # Errors
    ///
    /// Returns an error when the client is not active or IDs are exhausted.
    pub fn begin_shutdown(&mut self, now: Duration) -> Result<OutboundMessage, ClientError> {
        if !matches!(
            self.state,
            LanguageServerState::Ready | LanguageServerState::Degraded
        ) {
            return Err(ClientError::InvalidState);
        }
        self.state = LanguageServerState::Stopping;
        self.pending_changes.clear();
        self.requests.clear();
        let id = self.ids.next_id()?;
        self.shutdown_id = Some(id);
        self.shutdown_deadline = Some(now + SHUTDOWN_TIMEOUT);
        Ok(OutboundMessage {
            id: Some(id),
            method: "shutdown".to_owned(),
            params: Value::Null,
        })
    }

    /// Accepts the shutdown response and queues the final exit notification.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale response or a full queue.
    pub fn accept_shutdown(&mut self, id: RequestId) -> Result<(), ClientError> {
        if self.state != LanguageServerState::Stopping || self.shutdown_id != Some(id) {
            return Err(ClientError::StaleResponse);
        }
        self.shutdown_id = None;
        self.shutdown_deadline = None;
        self.push_outbound(OutboundMessage {
            id: None,
            method: "exit".to_owned(),
            params: Value::Null,
        })
    }

    pub fn finish_shutdown(&mut self) {
        self.state = LanguageServerState::Stopped;
        self.capabilities = None;
        self.outbound.clear();
        self.pending_changes.clear();
        self.requests.clear();
        self.initialize_deadline = None;
        self.shutdown_deadline = None;
        self.idle_deadline = None;
    }

    /// Applies all lifecycle deadlines reached at `now`.
    #[must_use]
    pub fn poll_timeouts(&mut self, now: Duration) -> Vec<ClientTimeout> {
        let mut timeouts = Vec::new();
        if self
            .initialize_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.initialize_deadline = None;
            self.initialize_id = None;
            self.state = LanguageServerState::Failed;
            timeouts.push(ClientTimeout::Initialize);
        }

        let mut expired_requests = self
            .requests
            .iter()
            .filter_map(|(id, request)| (request.deadline <= now).then_some(*id))
            .collect::<Vec<_>>();
        expired_requests.sort_unstable();
        for id in expired_requests {
            self.requests.remove(&id);
            if self.push_cancel(id).is_err() && self.state == LanguageServerState::Ready {
                self.state = LanguageServerState::Degraded;
            }
            timeouts.push(ClientTimeout::Request(id));
        }

        if self
            .shutdown_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.shutdown_deadline = None;
            self.shutdown_id = None;
            self.state = LanguageServerState::Stopped;
            timeouts.push(ClientTimeout::Shutdown);
        }

        if self.state == LanguageServerState::Ready
            && self.idle_deadline.is_some_and(|deadline| deadline <= now)
        {
            self.idle_deadline = None;
            match self.begin_shutdown(now) {
                Ok(message) => {
                    if self.push_outbound(message).is_ok() {
                        timeouts.push(ClientTimeout::IdleShutdown);
                    } else {
                        self.state = LanguageServerState::Failed;
                    }
                }
                Err(_) => self.state = LanguageServerState::Failed,
            }
        }
        timeouts
    }

    pub fn take_outbound(&mut self) -> Option<OutboundMessage> {
        self.outbound.pop_front()
    }

    fn ensure_ready(&self) -> Result<(), ClientError> {
        if self.state == LanguageServerState::Ready {
            Ok(())
        } else {
            Err(ClientError::NotReady)
        }
    }

    fn push_outbound(&mut self, message: OutboundMessage) -> Result<(), ClientError> {
        if self.outbound.len() >= OUTBOUND_LIMIT {
            return Err(ClientError::OutboundQueueFull);
        }
        self.outbound.push_back(message);
        Ok(())
    }

    fn invalidate_document_requests(&mut self, document: &str) -> Result<(), ClientError> {
        let ids = self
            .requests
            .iter()
            .filter_map(|(id, request)| (request.document == document).then_some(*id))
            .collect::<Vec<_>>();
        for id in ids {
            self.requests.remove(&id);
            self.push_cancel(id)?;
        }
        Ok(())
    }

    fn invalidate_feature_request(
        &mut self,
        kind: FeatureRequestKind,
        document: &str,
    ) -> Result<(), ClientError> {
        let id = self.requests.iter().find_map(|(id, request)| {
            (request.kind == kind && request.document == document).then_some(*id)
        });
        if let Some(id) = id {
            self.requests.remove(&id);
            self.push_cancel(id)?;
        }
        Ok(())
    }

    fn push_cancel(&mut self, id: RequestId) -> Result<(), ClientError> {
        self.push_outbound(OutboundMessage {
            id: None,
            method: "$/cancelRequest".to_owned(),
            params: json!({"id": id.get()}),
        })
    }

    fn fail_active_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.state = LanguageServerState::Failed;
        self.capabilities = None;
        self.initialize_id = None;
        self.initialize_deadline = None;
        self.shutdown_id = None;
        self.shutdown_deadline = None;
        self.idle_deadline = None;
        self.outbound.clear();
        self.pending_changes.clear();
        self.requests.clear();
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ClientError {
    #[error("language client is not ready")]
    NotReady,
    #[error("language client state does not allow this operation")]
    InvalidState,
    #[error("response belongs to stale state")]
    StaleResponse,
    #[error("document is not synchronized")]
    UnknownDocument,
    #[error("document revision is stale or invalid")]
    InvalidRevision,
    #[error("language feature is not supported by the server")]
    UnsupportedFeature,
    #[error("outbound message queue is full")]
    OutboundQueueFull,
    #[error("outstanding request limit reached")]
    TooManyRequests,
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}
