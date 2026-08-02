use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use strukt_editor::{DocumentId, GrammarRegistry};
use strukt_language::{
    ApprovalStatus, ClientTimeout, DiscoveredServer, DiscoveryOutcome, FeatureRequest,
    FeatureRequestKind, FrameDecoder, FrameLimits, IncomingMessage, LanguageClient,
    LanguageProcess, LanguageTransport, PositionEncoding, RequestId, ResponseDisposition,
    ServerCapabilities, ServerRequestDisposition, SpawnRequest, StdioTransport,
    SynchronizationKind, built_in_descriptors, discover, encode_frame, load_workspace_registry,
    parse_message, sanitize_hover_markdown,
};
use strukt_persistence::{
    ApprovalSnapshot, LanguageSelectionSnapshot, LanguageSessionSnapshot, RestoredLanguageSession,
};
use strukt_workspace::WorkspaceRoot;
use url::Url;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum LanguageState {
    #[default]
    Stopped,
    Discovering,
    Unavailable,
    ApprovalRequired,
    Disabled,
    Starting,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LanguageEffect {
    Discover {
        workspace_id: String,
        language_id: String,
        descriptor_id: Option<String>,
        generation: u64,
    },
    Open {
        language_id: String,
        generation: u64,
        document_id: DocumentId,
        path: PathBuf,
        revision: u64,
        text: String,
    },
    Change {
        language_id: String,
        generation: u64,
        document_id: DocumentId,
        path: PathBuf,
        revision: u64,
        text: String,
    },
    Save {
        language_id: String,
        generation: u64,
        document_id: DocumentId,
        path: PathBuf,
    },
    Close {
        language_id: String,
        generation: u64,
        document_id: DocumentId,
        path: PathBuf,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LanguageDiscoveryCompletion {
    Available(DiscoveredServer),
    ApprovalRequired(DiscoveredServer),
    Unavailable,
    Disabled,
}

type LanguageSpawnResult = Result<Box<dyn LanguageProcess>, String>;

#[derive(Clone)]
pub(crate) struct LanguageSpawnCompletion(Arc<Mutex<Option<LanguageSpawnResult>>>);

impl std::fmt::Debug for LanguageSpawnCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("LanguageSpawnCompletion").finish()
    }
}

impl LanguageSpawnCompletion {
    pub(crate) fn new(result: LanguageSpawnResult) -> Self {
        Self(Arc::new(Mutex::new(Some(result))))
    }

    fn take(&self) -> Option<LanguageSpawnResult> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

pub(crate) fn spawn_discovered_server(
    server: &DiscoveredServer,
    workspace_root: &Path,
) -> LanguageSpawnCompletion {
    let request = SpawnRequest::new(server.command().clone(), workspace_root.to_path_buf());
    let result = request
        .map_err(|error| error.to_string())
        .and_then(|request| {
            StdioTransport
                .spawn(request)
                .map_err(|error| error.to_string())
        });
    LanguageSpawnCompletion::new(result)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LanguageRuntimeEvent {
    Ready {
        language_id: String,
        generation: u64,
        position_encoding: PositionEncoding,
    },
    Failed {
        language_id: String,
        generation: u64,
        message: String,
    },
    Notification {
        language_id: String,
        generation: u64,
        method: String,
        params: Option<serde_json::Value>,
    },
    FeatureResponse {
        language_id: String,
        generation: u64,
        request: FeatureRequest,
        kind: FeatureRequestKind,
        result: serde_json::Value,
    },
    RequestTimedOut {
        request: FeatureRequest,
    },
    Stopped {
        language_id: String,
        generation: u64,
    },
}

struct RunningLanguage {
    generation: u64,
    root: PathBuf,
    process: Box<dyn LanguageProcess>,
    decoder: FrameDecoder,
    client: LanguageClient,
    initialize_id: strukt_language::RequestId,
    feature_requests: HashMap<RequestId, FeatureRequest>,
    shutdown_id: Option<RequestId>,
}

pub(crate) struct LanguageRuntime {
    processes: HashMap<String, RunningLanguage>,
    clock: Instant,
}

impl Default for LanguageRuntime {
    fn default() -> Self {
        Self {
            processes: HashMap::new(),
            clock: Instant::now(),
        }
    }
}

impl LanguageRuntime {
    pub(crate) fn finish_start(
        &mut self,
        workspace_id: &str,
        language_id: &str,
        generation: u64,
        root: PathBuf,
        completion: &LanguageSpawnCompletion,
    ) -> Result<(), String> {
        let Some(result) = completion.take() else {
            return Ok(());
        };
        let mut process = result?;
        let mut client = LanguageClient::new(workspace_id, language_id);
        let initialize = client
            .start(self.now())
            .map_err(|error| error.to_string())?;
        let initialize_id = initialize.id().ok_or("initialize request had no ID")?;
        write_outbound(process.as_mut(), &initialize)?;
        self.processes.insert(
            language_id.to_owned(),
            RunningLanguage {
                generation,
                root,
                process,
                decoder: FrameDecoder::new(FrameLimits::default()),
                client,
                initialize_id,
                feature_requests: HashMap::new(),
                shutdown_id: None,
            },
        );
        Ok(())
    }

    pub(crate) fn apply_effects(&mut self, effects: Vec<LanguageEffect>) -> Result<(), String> {
        let now = self.now();
        for effect in effects {
            let (language_id, generation) = match &effect {
                LanguageEffect::Discover { .. } => continue,
                LanguageEffect::Open {
                    language_id,
                    generation,
                    ..
                }
                | LanguageEffect::Change {
                    language_id,
                    generation,
                    ..
                }
                | LanguageEffect::Save {
                    language_id,
                    generation,
                    ..
                }
                | LanguageEffect::Close {
                    language_id,
                    generation,
                    ..
                } => (language_id, *generation),
            };
            let Some(running) = self.processes.get_mut(language_id) else {
                continue;
            };
            if running.generation != generation {
                continue;
            }
            match effect {
                LanguageEffect::Open {
                    path,
                    revision,
                    text,
                    ..
                } => {
                    let uri = file_uri(&running.root, &path)?;
                    running
                        .client
                        .did_open(uri, revision, &text)
                        .map_err(|error| error.to_string())?;
                }
                LanguageEffect::Change {
                    path,
                    revision,
                    text,
                    ..
                } => {
                    let uri = file_uri(&running.root, &path)?;
                    running
                        .client
                        .did_change(&uri, revision, text, now)
                        .map_err(|error| error.to_string())?;
                }
                LanguageEffect::Save { path, .. } => {
                    let uri = file_uri(&running.root, &path)?;
                    running
                        .client
                        .did_save(&uri)
                        .map_err(|error| error.to_string())?;
                }
                LanguageEffect::Close { path, .. } => {
                    let uri = file_uri(&running.root, &path)?;
                    running
                        .client
                        .did_close(&uri, now)
                        .map_err(|error| error.to_string())?;
                }
                LanguageEffect::Discover { .. } => {}
            }
            flush_client(running)?;
        }
        Ok(())
    }

    pub(crate) fn request_feature(
        &mut self,
        language_id: &str,
        generation: u64,
        path: &Path,
        revision: u64,
        kind: FeatureRequestKind,
        position: strukt_language::LspPosition,
    ) -> Result<FeatureRequest, String> {
        let now = self.now();
        let running = self
            .processes
            .get_mut(language_id)
            .ok_or("language server is not running")?;
        if running.generation != generation {
            return Err("language server generation is stale".to_owned());
        }
        let uri = file_uri(&running.root, path)?;
        let request = running
            .client
            .request_feature(kind, &uri, revision, position, now)
            .map_err(|error| error.to_string())?;
        running
            .feature_requests
            .insert(request.id(), request.clone());
        flush_client(running)?;
        Ok(request)
    }

    pub(crate) fn begin_shutdown(
        &mut self,
        language_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        let now = self.now();
        let running = self
            .processes
            .get_mut(language_id)
            .ok_or("language server is not running")?;
        if running.generation != generation {
            return Err("language server generation is stale".to_owned());
        }
        let shutdown = running
            .client
            .begin_shutdown(now)
            .map_err(|error| error.to_string())?;
        running.shutdown_id = shutdown.id();
        write_outbound(running.process.as_mut(), &shutdown)
    }

    pub(crate) fn poll(&mut self) -> Vec<LanguageRuntimeEvent> {
        let now = self.now();
        self.poll_at(now)
    }

    fn poll_at(&mut self, now: Duration) -> Vec<LanguageRuntimeEvent> {
        let languages = self.processes.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
        let mut failed = Vec::new();
        let mut stopped = Vec::new();
        for language_id in languages {
            let Some(running) = self.processes.get_mut(&language_id) else {
                continue;
            };
            let mut terminal_timeout = None;
            for timeout in running.client.poll_timeouts(now) {
                match timeout {
                    ClientTimeout::Initialize => {
                        terminal_timeout = Some("language server initialization timed out");
                    }
                    ClientTimeout::Request(id) => {
                        if let Some(request) = running.feature_requests.remove(&id) {
                            events.push(LanguageRuntimeEvent::RequestTimedOut { request });
                        }
                    }
                    ClientTimeout::Shutdown => terminal_timeout = Some(""),
                    ClientTimeout::IdleShutdown => {
                        running.shutdown_id = running.client.shutdown_request_id();
                    }
                }
            }
            if let Some(message) = terminal_timeout {
                let _ = running.process.terminate(Duration::ZERO);
                if message.is_empty() {
                    stopped.push((language_id.clone(), running.generation));
                } else {
                    failed.push((language_id.clone(), running.generation, message.to_owned()));
                }
                continue;
            }
            running.client.flush_changes(now);
            if let Err(error) = flush_client(running) {
                failed.push((language_id, running.generation, error));
                continue;
            }
            for _ in 0..64 {
                let bytes = match running.process.try_read() {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => break,
                    Err(error) => {
                        failed.push((language_id.clone(), running.generation, error.to_string()));
                        break;
                    }
                };
                let frames = match running.decoder.push(&bytes) {
                    Ok(frames) => frames,
                    Err(error) => {
                        failed.push((language_id.clone(), running.generation, error.to_string()));
                        break;
                    }
                };
                for frame in frames {
                    match handle_frame(running, &language_id, frame.body()) {
                        Ok(Some(event)) => events.push(event),
                        Ok(None) => {}
                        Err(error) => {
                            failed.push((language_id.clone(), running.generation, error));
                            break;
                        }
                    }
                }
            }
            if let Ok(Some(exit)) = running.process.try_wait() {
                if running.shutdown_id.is_none()
                    && running.client.state() == strukt_language::LanguageServerState::Stopping
                {
                    running.client.finish_shutdown();
                    stopped.push((language_id.clone(), running.generation));
                } else {
                    failed.push((
                        language_id.clone(),
                        running.generation,
                        format!("language server exited with {:?}", exit.code()),
                    ));
                }
            }
        }
        for (language_id, generation) in stopped {
            self.processes.remove(&language_id);
            events.push(LanguageRuntimeEvent::Stopped {
                language_id,
                generation,
            });
        }
        for (language_id, generation, message) in failed {
            self.processes.remove(&language_id);
            events.push(LanguageRuntimeEvent::Failed {
                language_id,
                generation,
                message,
            });
        }
        events
    }

    fn now(&self) -> Duration {
        self.clock.elapsed()
    }
}

fn handle_frame(
    running: &mut RunningLanguage,
    language_id: &str,
    body: &[u8],
) -> Result<Option<LanguageRuntimeEvent>, String> {
    match parse_message(body).map_err(|error| error.to_string())? {
        IncomingMessage::Response(response) if response.id() == running.initialize_id => {
            let result = response
                .result()
                .ok_or("initialize response contained an error")?;
            let capabilities = capabilities_from_initialize(result);
            running
                .client
                .accept_initialize(response.id(), capabilities)
                .map_err(|error| error.to_string())?;
            flush_client(running)?;
            Ok(Some(LanguageRuntimeEvent::Ready {
                language_id: language_id.to_owned(),
                generation: running.generation,
                position_encoding: capabilities.position_encoding(),
            }))
        }
        IncomingMessage::Notification(notification) => {
            Ok(Some(LanguageRuntimeEvent::Notification {
                language_id: language_id.to_owned(),
                generation: running.generation,
                method: notification.method().to_owned(),
                params: notification.params().cloned(),
            }))
        }
        IncomingMessage::Response(response) => {
            if running.shutdown_id == Some(response.id()) {
                running
                    .client
                    .accept_shutdown(response.id())
                    .map_err(|error| error.to_string())?;
                running.shutdown_id = None;
                flush_client(running)?;
                return Ok(None);
            }
            let Some(request) = running.feature_requests.remove(&response.id()) else {
                return Ok(None);
            };
            if running.client.accept_feature_response(&request) == ResponseDisposition::Stale {
                return Ok(None);
            }
            Ok(Some(LanguageRuntimeEvent::FeatureResponse {
                language_id: language_id.to_owned(),
                generation: running.generation,
                kind: request.kind(),
                request,
                result: response
                    .result()
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }))
        }
        IncomingMessage::Request(request) => {
            let response = match running.client.handle_server_request(request.method()) {
                ServerRequestDisposition::Configuration(values) => serde_json::json!({
                    "jsonrpc":"2.0",
                    "id":request.id().get(),
                    "result":values,
                }),
                ServerRequestDisposition::MethodNotFound => serde_json::json!({
                    "jsonrpc":"2.0",
                    "id":request.id().get(),
                    "error":{"code":-32601,"message":"Method not found"},
                }),
            };
            write_json_value(running.process.as_mut(), &response)?;
            Ok(None)
        }
    }
}

fn capabilities_from_initialize(result: &serde_json::Value) -> ServerCapabilities {
    let capabilities = &result["capabilities"];
    let encoding = match capabilities["positionEncoding"].as_str() {
        Some("utf-8") => PositionEncoding::Utf8,
        _ => PositionEncoding::Utf16,
    };
    let mut features = Vec::new();
    if !capabilities["completionProvider"].is_null() {
        features.push(FeatureRequestKind::Completion);
    }
    if capabilities["hoverProvider"].as_bool().unwrap_or(false) {
        features.push(FeatureRequestKind::Hover);
    }
    if capabilities["definitionProvider"]
        .as_bool()
        .unwrap_or(false)
    {
        features.push(FeatureRequestKind::Definition);
    }
    let text_document_sync = &capabilities["textDocumentSync"];
    let synchronization = if text_document_sync.is_null() {
        SynchronizationKind::None
    } else {
        SynchronizationKind::Full
    };
    let save_notifications = text_document_sync
        .as_object()
        .and_then(|options| options.get("save"))
        .is_some_and(|save| save.as_bool().unwrap_or_else(|| save.is_object()));
    ServerCapabilities::new(synchronization, features, encoding)
        .with_save_notifications(save_notifications)
}

fn flush_client(running: &mut RunningLanguage) -> Result<(), String> {
    while let Some(message) = running.client.take_outbound() {
        write_outbound(running.process.as_mut(), &message)?;
    }
    Ok(())
}

fn write_outbound(
    process: &mut dyn LanguageProcess,
    message: &strukt_language::OutboundMessage,
) -> Result<(), String> {
    let body = serde_json::to_vec(&message.json_rpc()).map_err(|error| error.to_string())?;
    let frame = encode_frame(&body, FrameLimits::default()).map_err(|error| error.to_string())?;
    process.write(&frame).map_err(|error| error.to_string())
}

fn write_json_value(
    process: &mut dyn LanguageProcess,
    message: &serde_json::Value,
) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    let frame = encode_frame(&body, FrameLimits::default()).map_err(|error| error.to_string())?;
    process.write(&frame).map_err(|error| error.to_string())
}

fn file_uri(root: &Path, relative: &Path) -> Result<String, String> {
    Url::from_file_path(root.join(relative))
        .map(Into::into)
        .map_err(|()| "document path could not be represented as a file URI".to_owned())
}

pub(crate) fn parse_publish_diagnostics(
    params: Option<&serde_json::Value>,
    workspace_root: &Path,
) -> Option<(PathBuf, Option<u64>, Vec<PublishedDiagnostic>)> {
    let params = params?;
    let uri = params.get("uri")?.as_str()?;
    let absolute = Url::parse(uri).ok()?.to_file_path().ok()?;
    let relative = absolute.strip_prefix(workspace_root).ok()?.to_path_buf();
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return None;
    }
    let version = params.get("version").and_then(serde_json::Value::as_u64);
    let diagnostics = params
        .get("diagnostics")?
        .as_array()?
        .iter()
        .take(2_000)
        .filter_map(|diagnostic| {
            let start = diagnostic.get("range")?.get("start")?;
            let line = u32::try_from(start.get("line")?.as_u64()?).ok()?;
            let character = u32::try_from(start.get("character")?.as_u64()?).ok()?;
            let message = diagnostic.get("message")?.as_str()?;
            let severity = match diagnostic
                .get("severity")
                .and_then(serde_json::Value::as_u64)
            {
                Some(1) => DiagnosticSeverity::Error,
                Some(2) => DiagnosticSeverity::Warning,
                Some(4) => DiagnosticSeverity::Hint,
                _ => DiagnosticSeverity::Information,
            };
            Some(PublishedDiagnostic::new(
                line,
                character,
                severity,
                message,
                diagnostic.get("source").and_then(serde_json::Value::as_str),
            ))
        })
        .collect();
    Some((relative, version, diagnostics))
}

pub(crate) fn discover_workspace_language(
    workspace: &WorkspaceRoot,
    language_id: &str,
    descriptor_id: Option<&str>,
    approvals: &[ApprovalSnapshot],
) -> Result<LanguageDiscoveryCompletion, String> {
    let builtins = built_in_descriptors().map_err(|error| error.to_string())?;
    let workspace_registry =
        load_workspace_registry(workspace).map_err(|error| error.to_string())?;
    let descriptor = descriptor_id
        .and_then(|id| {
            workspace_registry
                .as_ref()
                .and_then(|registry| registry.iter().find(|descriptor| descriptor.id() == id))
                .or_else(|| builtins.iter().find(|descriptor| descriptor.id() == id))
        })
        .or_else(|| builtins.for_language(language_id));
    let Some(descriptor) = descriptor else {
        return Ok(LanguageDiscoveryCompletion::Unavailable);
    };
    let outcome = discover(
        descriptor,
        std::env::var_os("PATH").as_deref(),
        workspace,
        ApprovalStatus::Unreviewed,
    )
    .map_err(|error| error.to_string())?;
    Ok(match outcome {
        DiscoveryOutcome::Available(server) => LanguageDiscoveryCompletion::Available(server),
        DiscoveryOutcome::ApprovalRequired(server) => {
            if approvals.iter().any(|approval| {
                approval.language_id() == language_id && approval.matches(server.command())
            }) {
                LanguageDiscoveryCompletion::Available(server)
            } else {
                LanguageDiscoveryCompletion::ApprovalRequired(server)
            }
        }
        DiscoveryOutcome::Unavailable { .. } => LanguageDiscoveryCompletion::Unavailable,
        DiscoveryOutcome::Disabled => LanguageDiscoveryCompletion::Disabled,
    })
}

#[derive(Clone, Debug)]
struct DocumentBinding {
    path: PathBuf,
    language_override: Option<String>,
    language_id: String,
    revision: u64,
    text: String,
    eligible: bool,
}

#[derive(Clone, Debug, Default)]
struct ServerBinding {
    state: LanguageState,
    generation: u64,
    position_encoding: PositionEncoding,
    pending_approval: Option<DiscoveredServer>,
    last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedDiagnostic {
    line: u32,
    character: u32,
    severity: DiagnosticSeverity,
    message: String,
    source: Option<String>,
}

impl PublishedDiagnostic {
    #[must_use]
    pub(crate) fn new(
        line: u32,
        character: u32,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
        source: Option<&str>,
    ) -> Self {
        Self {
            line,
            character,
            severity,
            message: message.into(),
            source: source.map(ToOwned::to_owned),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Problem {
    document_id: DocumentId,
    path: PathBuf,
    line: u32,
    character: u32,
    severity: DiagnosticSeverity,
    message: String,
    source: Option<String>,
}

impl Problem {
    #[must_use]
    pub(crate) const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub(crate) const fn line(&self) -> u32 {
        self.line
    }

    #[must_use]
    pub(crate) const fn character(&self) -> u32 {
        self.character
    }

    #[must_use]
    pub(crate) const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub(crate) fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ProblemCounts {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) information: usize,
    pub(crate) hints: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompletionCandidate {
    label: String,
    insertion: String,
    range: Option<strukt_language::LanguageRange>,
}

impl CompletionCandidate {
    #[must_use]
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub(crate) fn insertion(&self) -> &str {
        &self.insertion
    }

    #[must_use]
    pub(crate) const fn range(&self) -> Option<strukt_language::LanguageRange> {
        self.range
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DefinitionLocation {
    path: Option<PathBuf>,
    line: u32,
    character: u32,
    external: bool,
    uri: String,
}

impl DefinitionLocation {
    #[must_use]
    pub(crate) fn label(&self) -> String {
        self.path.as_ref().map_or_else(
            || self.uri.clone(),
            |path| {
                format!(
                    "{}:{}:{}",
                    path.display(),
                    self.line + 1,
                    self.character + 1
                )
            },
        )
    }

    #[must_use]
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    #[must_use]
    pub(crate) const fn line(&self) -> u32 {
        self.line
    }

    #[must_use]
    pub(crate) const fn character(&self) -> u32 {
        self.character
    }

    #[must_use]
    pub(crate) const fn external(&self) -> bool {
        self.external
    }
}

#[derive(Clone, Debug)]
struct FeatureGuard {
    document_id: DocumentId,
    revision: u64,
    generation: u64,
    kind: FeatureRequestKind,
    position: strukt_language::LspPosition,
}

#[derive(Clone, Debug)]
pub(crate) struct LanguageRequestContext {
    pub(crate) language_id: String,
    pub(crate) generation: u64,
    pub(crate) path: PathBuf,
    pub(crate) revision: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ProblemFilter {
    #[default]
    All,
    Errors,
    Warnings,
}

#[derive(Clone, Debug)]
struct DiagnosticSet {
    language_id: String,
    generation: u64,
    problems: Vec<Problem>,
}

#[derive(Clone, Debug)]
pub(crate) struct LanguageCoordinator {
    workspace_id: String,
    selections: HashMap<String, LanguageSelectionSnapshot>,
    approvals: Vec<ApprovalSnapshot>,
    problems_visible: bool,
    documents: HashMap<DocumentId, DocumentBinding>,
    servers: HashMap<String, ServerBinding>,
    diagnostics: HashMap<DocumentId, DiagnosticSet>,
    problem_filter: ProblemFilter,
    feature_guards: HashMap<RequestId, FeatureGuard>,
    completion_document: Option<(DocumentId, u64)>,
    completion_items: Vec<CompletionCandidate>,
    hover_text: Option<String>,
    definition_document: Option<DocumentId>,
    definition_locations: Vec<DefinitionLocation>,
    persistence_dirty: bool,
}

impl Default for LanguageCoordinator {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl LanguageCoordinator {
    #[must_use]
    pub(crate) fn new(workspace_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            selections: HashMap::new(),
            approvals: Vec::new(),
            problems_visible: true,
            documents: HashMap::new(),
            servers: HashMap::new(),
            diagnostics: HashMap::new(),
            problem_filter: ProblemFilter::All,
            feature_guards: HashMap::new(),
            completion_document: None,
            completion_items: Vec::new(),
            hover_text: None,
            definition_document: None,
            definition_locations: Vec::new(),
            persistence_dirty: false,
        }
    }

    #[must_use]
    pub(crate) fn restore(
        workspace_id: impl Into<String>,
        restored: &RestoredLanguageSession,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            selections: restored
                .selections()
                .iter()
                .cloned()
                .map(|selection| (selection.language_id().to_owned(), selection))
                .collect(),
            approvals: restored.approvals().to_vec(),
            problems_visible: restored.problems_visible(),
            documents: HashMap::new(),
            servers: HashMap::new(),
            diagnostics: HashMap::new(),
            problem_filter: ProblemFilter::All,
            feature_guards: HashMap::new(),
            completion_document: None,
            completion_items: Vec::new(),
            hover_text: None,
            definition_document: None,
            definition_locations: Vec::new(),
            persistence_dirty: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn replace_workspace(&mut self, workspace_id: impl Into<String>) {
        self.workspace_id = workspace_id.into();
        self.documents.clear();
        self.servers.clear();
        self.diagnostics.clear();
        self.clear_transient_features();
        self.persistence_dirty = false;
    }

    #[must_use]
    pub(crate) fn running_servers(&self) -> usize {
        self.servers
            .values()
            .filter(|server| matches!(server.state, LanguageState::Starting | LanguageState::Ready))
            .count()
    }

    #[must_use]
    pub(crate) fn server_states(&self) -> Vec<(String, LanguageState)> {
        let mut states = self
            .servers
            .iter()
            .map(|(language, server)| (language.clone(), server.state))
            .collect::<Vec<_>>();
        states.sort_by(|left, right| left.0.cmp(&right.0));
        states
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn state(&self, language_id: &str) -> LanguageState {
        self.servers
            .get(language_id)
            .map_or(LanguageState::Stopped, |server| server.state)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn document_language(&self, id: DocumentId) -> Option<&str> {
        self.documents
            .get(&id)
            .map(|binding| binding.language_id.as_str())
    }

    pub(crate) fn open_document(
        &mut self,
        id: DocumentId,
        path: &Path,
        language_override: Option<&str>,
        revision: u64,
        text: Option<&str>,
    ) -> Vec<LanguageEffect> {
        let language_id = detect_language(path, language_override);
        let eligible = text.is_some() && language_id != "plain-text";
        self.documents.insert(
            id,
            DocumentBinding {
                path: path.to_path_buf(),
                language_override: language_override.map(ToOwned::to_owned),
                language_id: language_id.clone(),
                revision,
                text: text.unwrap_or_default().to_owned(),
                eligible,
            },
        );
        if !eligible || !self.selection_enabled(&language_id) {
            return Vec::new();
        }
        if let Some(server) = self.servers.get(&language_id)
            && server.state == LanguageState::Ready
        {
            return vec![LanguageEffect::Open {
                language_id,
                generation: server.generation,
                document_id: id,
                path: path.to_path_buf(),
                revision,
                text: text.unwrap_or_default().to_owned(),
            }];
        }
        self.ensure_language_started(&language_id)
    }

    pub(crate) fn edit_document(
        &mut self,
        id: DocumentId,
        revision: u64,
        text: &str,
    ) -> Vec<LanguageEffect> {
        self.clear_features_for_document(id);
        let Some(binding) = self.documents.get_mut(&id) else {
            return Vec::new();
        };
        binding.revision = revision;
        text.clone_into(&mut binding.text);
        let language_id = binding.language_id.clone();
        let Some(server) = self.servers.get(&language_id) else {
            return Vec::new();
        };
        (binding.eligible && server.state == LanguageState::Ready)
            .then(|| LanguageEffect::Change {
                language_id,
                generation: server.generation,
                document_id: id,
                path: binding.path.clone(),
                revision,
                text: text.to_owned(),
            })
            .into_iter()
            .collect()
    }

    pub(crate) fn save_document(&self, id: DocumentId) -> Vec<LanguageEffect> {
        let Some(binding) = self.documents.get(&id) else {
            return Vec::new();
        };
        let Some(server) = self.servers.get(&binding.language_id) else {
            return Vec::new();
        };
        (binding.eligible && server.state == LanguageState::Ready)
            .then(|| LanguageEffect::Save {
                language_id: binding.language_id.clone(),
                generation: server.generation,
                document_id: id,
                path: binding.path.clone(),
            })
            .into_iter()
            .collect()
    }

    pub(crate) fn close_document(&mut self, id: DocumentId) -> Vec<LanguageEffect> {
        self.diagnostics.remove(&id);
        self.clear_features_for_document(id);
        let Some(binding) = self.documents.remove(&id) else {
            return Vec::new();
        };
        let Some(server) = self.servers.get(&binding.language_id) else {
            return Vec::new();
        };
        (binding.eligible
            && matches!(
                server.state,
                LanguageState::Discovering | LanguageState::Starting | LanguageState::Ready
            ))
        .then_some(LanguageEffect::Close {
            language_id: binding.language_id,
            generation: server.generation,
            document_id: id,
            path: binding.path,
        })
        .into_iter()
        .collect()
    }

    pub(crate) fn set_override(
        &mut self,
        id: DocumentId,
        language_override: Option<&str>,
    ) -> Vec<LanguageEffect> {
        let Some(previous) = self.documents.get(&id).cloned() else {
            return Vec::new();
        };
        let mut effects = self.close_document(id);
        effects.extend(self.open_document(
            id,
            &previous.path,
            language_override,
            previous.revision,
            previous.eligible.then_some(previous.text.as_str()),
        ));
        if let Some(binding) = self.documents.get_mut(&id) {
            binding.language_override = language_override.map(ToOwned::to_owned);
        }
        effects
    }

    pub(crate) fn discovery_available(
        &mut self,
        workspace_id: &str,
        language_id: &str,
        generation: u64,
    ) -> bool {
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if self.workspace_id != workspace_id
            || server.generation != generation
            || server.state != LanguageState::Discovering
        {
            return false;
        }
        server.state = LanguageState::Starting;
        true
    }

    pub(crate) fn discovery_finished(
        &mut self,
        workspace_id: &str,
        language_id: &str,
        generation: u64,
        state: LanguageState,
    ) -> bool {
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if self.workspace_id != workspace_id
            || server.generation != generation
            || server.state != LanguageState::Discovering
        {
            return false;
        }
        server.state = state;
        server.pending_approval = None;
        true
    }

    pub(crate) fn discovery_requires_approval(
        &mut self,
        workspace_id: &str,
        language_id: &str,
        generation: u64,
        discovered: DiscoveredServer,
    ) -> bool {
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if self.workspace_id != workspace_id
            || server.generation != generation
            || server.state != LanguageState::Discovering
        {
            return false;
        }
        server.state = LanguageState::ApprovalRequired;
        server.pending_approval = Some(discovered);
        true
    }

    pub(crate) fn approve(&mut self, language_id: &str) -> Option<(u64, DiscoveredServer)> {
        let server = self.servers.get_mut(language_id)?;
        if server.state != LanguageState::ApprovalRequired {
            return None;
        }
        let discovered = server.pending_approval.take()?;
        let approval =
            ApprovalSnapshot::new(language_id, discovered.command().fingerprint()).ok()?;
        self.approvals
            .retain(|approval| approval.language_id() != language_id);
        self.approvals.push(approval);
        self.persistence_dirty = true;
        server.state = LanguageState::Starting;
        Some((server.generation, discovered))
    }

    pub(crate) fn deny(&mut self, language_id: &str) -> bool {
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if server.state != LanguageState::ApprovalRequired {
            return false;
        }
        server.pending_approval = None;
        server.state = LanguageState::Disabled;
        self.approvals
            .retain(|approval| approval.language_id() != language_id);
        self.persistence_dirty = true;
        true
    }

    pub(crate) fn retry(&mut self, language_id: &str) -> Vec<LanguageEffect> {
        let Some(server) = self.servers.get_mut(language_id) else {
            return Vec::new();
        };
        if !matches!(
            server.state,
            LanguageState::Unavailable | LanguageState::Disabled | LanguageState::Failed
        ) {
            return Vec::new();
        }
        server.generation = server.generation.wrapping_add(1);
        server.state = LanguageState::Discovering;
        server.pending_approval = None;
        let descriptor_id = self
            .selections
            .get(language_id)
            .map(|selection| selection.descriptor_id().to_owned());
        vec![LanguageEffect::Discover {
            workspace_id: self.workspace_id.clone(),
            language_id: language_id.to_owned(),
            descriptor_id,
            generation: server.generation,
        }]
    }

    #[must_use]
    pub(crate) fn approval_command(&self, language_id: &str) -> Option<String> {
        let command = self
            .servers
            .get(language_id)?
            .pending_approval
            .as_ref()?
            .command();
        let mut display = command.executable().display().to_string();
        for argument in command.arguments() {
            display.push(' ');
            display.push_str(&argument.to_string_lossy());
        }
        Some(display.chars().take(512).collect())
    }

    #[must_use]
    pub(crate) fn approvals(&self) -> Vec<ApprovalSnapshot> {
        self.approvals.clone()
    }

    pub(crate) fn mark_ready(
        &mut self,
        language_id: &str,
        generation: u64,
        position_encoding: PositionEncoding,
    ) -> Vec<LanguageEffect> {
        let Some(server) = self.servers.get_mut(language_id) else {
            return Vec::new();
        };
        if server.generation != generation || server.state != LanguageState::Starting {
            return Vec::new();
        }
        server.state = LanguageState::Ready;
        server.position_encoding = position_encoding;
        self.documents
            .iter()
            .filter(|(_, binding)| binding.eligible && binding.language_id == language_id)
            .map(|(id, binding)| LanguageEffect::Open {
                language_id: language_id.to_owned(),
                generation,
                document_id: *id,
                path: binding.path.clone(),
                revision: binding.revision,
                text: binding.text.clone(),
            })
            .collect()
    }

    pub(crate) fn fail(&mut self, language_id: &str, generation: u64) -> bool {
        self.fail_with_message(language_id, generation, "language server failed")
    }

    pub(crate) fn fail_with_message(
        &mut self,
        language_id: &str,
        generation: u64,
        message: &str,
    ) -> bool {
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if server.generation != generation {
            return false;
        }
        server.state = LanguageState::Failed;
        server.last_error = Some(message.chars().take(2_048).collect());
        self.diagnostics.retain(|_, diagnostics| {
            diagnostics.language_id != language_id || diagnostics.generation != generation
        });
        self.clear_transient_features();
        true
    }

    #[must_use]
    pub(crate) fn failure_details(&self, language_id: &str) -> Option<&str> {
        self.servers.get(language_id)?.last_error.as_deref()
    }

    pub(crate) fn mark_stopped(&mut self, language_id: &str, generation: u64) -> bool {
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if server.generation != generation {
            return false;
        }
        server.state = LanguageState::Stopped;
        server.last_error = None;
        self.diagnostics.retain(|_, diagnostics| {
            diagnostics.language_id != language_id || diagnostics.generation != generation
        });
        self.clear_transient_features();
        true
    }

    #[must_use]
    pub(crate) fn request_context(&self, id: DocumentId) -> Option<LanguageRequestContext> {
        let binding = self.documents.get(&id)?;
        let server = self.servers.get(&binding.language_id)?;
        (binding.eligible && server.state == LanguageState::Ready).then(|| LanguageRequestContext {
            language_id: binding.language_id.clone(),
            generation: server.generation,
            path: binding.path.clone(),
            revision: binding.revision,
        })
    }

    pub(crate) fn begin_feature(
        &mut self,
        document_id: DocumentId,
        server_generation: u64,
        request: &FeatureRequest,
    ) {
        self.feature_guards
            .retain(|_, guard| guard.document_id != document_id || guard.kind != request.kind());
        self.feature_guards.insert(
            request.id(),
            FeatureGuard {
                document_id,
                revision: request.revision(),
                generation: server_generation,
                kind: request.kind(),
                position: request.position(),
            },
        );
    }

    pub(crate) fn accept_feature_response(
        &mut self,
        generation: u64,
        request: &FeatureRequest,
        result: &serde_json::Value,
        workspace_root: &Path,
    ) -> bool {
        let Some(guard) = self.feature_guards.remove(&request.id()) else {
            return false;
        };
        let Some(binding) = self.documents.get(&guard.document_id) else {
            return false;
        };
        if guard.generation != generation
            || guard.revision != binding.revision
            || guard.kind != request.kind()
            || guard.position != request.position()
        {
            return false;
        }
        match guard.kind {
            FeatureRequestKind::Completion => {
                self.completion_document = Some((guard.document_id, guard.revision));
                self.completion_items = parse_completion_items(result);
            }
            FeatureRequestKind::Hover => {
                self.hover_text = parse_hover(result);
            }
            FeatureRequestKind::Definition => {
                self.definition_document = Some(guard.document_id);
                self.definition_locations = parse_definitions(result, workspace_root);
            }
        }
        true
    }

    pub(crate) fn expire_feature(&mut self, request: &FeatureRequest) {
        self.feature_guards.remove(&request.id());
    }

    #[must_use]
    pub(crate) fn completion(&self) -> Option<(DocumentId, u64, &[CompletionCandidate])> {
        let (id, revision) = self.completion_document?;
        Some((id, revision, &self.completion_items))
    }

    #[must_use]
    pub(crate) fn hover_text(&self) -> Option<&str> {
        self.hover_text.as_deref()
    }

    #[must_use]
    pub(crate) fn definitions(&self) -> &[DefinitionLocation] {
        &self.definition_locations
    }

    pub(crate) fn dismiss_features(&mut self) {
        self.clear_transient_features();
    }

    #[must_use]
    pub(crate) fn has_transient_features(&self) -> bool {
        self.completion_document.is_some()
            || self.hover_text.is_some()
            || !self.definition_locations.is_empty()
    }

    fn clear_features_for_document(&mut self, id: DocumentId) {
        self.feature_guards
            .retain(|_, guard| guard.document_id != id);
        if self
            .completion_document
            .is_some_and(|(document, _)| document == id)
        {
            self.completion_document = None;
            self.completion_items.clear();
        }
        if self.definition_document == Some(id) {
            self.definition_document = None;
            self.definition_locations.clear();
        }
        self.hover_text = None;
    }

    fn clear_transient_features(&mut self) {
        self.feature_guards.clear();
        self.completion_document = None;
        self.completion_items.clear();
        self.hover_text = None;
        self.definition_document = None;
        self.definition_locations.clear();
    }

    pub(crate) fn publish_diagnostics(
        &mut self,
        language_id: &str,
        generation: u64,
        path: &Path,
        version: Option<u64>,
        diagnostics: Vec<PublishedDiagnostic>,
    ) -> bool {
        let Some(server) = self.servers.get(language_id) else {
            return false;
        };
        if server.generation != generation || server.state != LanguageState::Ready {
            return false;
        }
        let Some((&document_id, binding)) = self.documents.iter().find(|(_, binding)| {
            binding.eligible && binding.language_id == language_id && binding.path == path
        }) else {
            return false;
        };
        if version.is_some_and(|version| version != binding.revision) {
            return false;
        }
        let problems = diagnostics
            .into_iter()
            .take(2_000)
            .map(|diagnostic| Problem {
                document_id,
                path: path.to_path_buf(),
                line: diagnostic.line,
                character: diagnostic.character,
                severity: diagnostic.severity,
                message: diagnostic.message.chars().take(4_096).collect(),
                source: diagnostic
                    .source
                    .map(|source| source.chars().take(128).collect()),
            })
            .collect::<Vec<_>>();
        if problems.is_empty() {
            self.diagnostics.remove(&document_id);
        } else {
            self.diagnostics.insert(
                document_id,
                DiagnosticSet {
                    language_id: language_id.to_owned(),
                    generation,
                    problems,
                },
            );
        }
        true
    }

    #[must_use]
    pub(crate) fn problems(&self) -> Vec<&Problem> {
        let mut problems = self
            .diagnostics
            .values()
            .flat_map(|set| set.problems.iter())
            .collect::<Vec<_>>();
        problems.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.line.cmp(&right.line))
                .then(left.character.cmp(&right.character))
        });
        problems
    }

    #[must_use]
    pub(crate) fn visible_problems(&self) -> Vec<&Problem> {
        self.problems()
            .into_iter()
            .filter(|problem| match self.problem_filter {
                ProblemFilter::All => true,
                ProblemFilter::Errors => problem.severity == DiagnosticSeverity::Error,
                ProblemFilter::Warnings => problem.severity == DiagnosticSeverity::Warning,
            })
            .collect()
    }

    pub(crate) const fn set_problem_filter(&mut self, filter: ProblemFilter) {
        self.problem_filter = filter;
    }

    #[must_use]
    pub(crate) fn problem_counts(&self) -> ProblemCounts {
        self.problems()
            .into_iter()
            .fold(ProblemCounts::default(), |mut counts, problem| {
                match problem.severity {
                    DiagnosticSeverity::Error => counts.errors += 1,
                    DiagnosticSeverity::Warning => counts.warnings += 1,
                    DiagnosticSeverity::Information => counts.information += 1,
                    DiagnosticSeverity::Hint => counts.hints += 1,
                }
                counts
            })
    }

    #[must_use]
    pub(crate) fn position_encoding(&self, language_id: &str) -> PositionEncoding {
        self.servers
            .get(language_id)
            .map_or(PositionEncoding::Utf16, |server| server.position_encoding)
    }

    #[must_use]
    pub(crate) fn document_position_encoding(&self, id: DocumentId) -> PositionEncoding {
        self.documents
            .get(&id)
            .map_or(PositionEncoding::Utf16, |binding| {
                self.position_encoding(&binding.language_id)
            })
    }

    #[must_use]
    pub(crate) const fn problems_visible(&self) -> bool {
        self.problems_visible
    }

    pub(crate) fn toggle_problems(&mut self) {
        self.problems_visible = !self.problems_visible;
        self.persistence_dirty = true;
    }

    pub(crate) fn snapshot(&mut self) -> Option<LanguageSessionSnapshot> {
        self.persistence_dirty = false;
        LanguageSessionSnapshot::new(
            self.selections.values().cloned().collect(),
            self.approvals.clone(),
            self.problems_visible,
        )
        .ok()
    }

    fn selection_enabled(&self, language_id: &str) -> bool {
        self.selections
            .get(language_id)
            .is_none_or(LanguageSelectionSnapshot::enabled_state)
    }

    fn ensure_language_started(&mut self, language_id: &str) -> Vec<LanguageEffect> {
        let descriptor_id = self
            .selections
            .get(language_id)
            .map(|selection| selection.descriptor_id().to_owned());
        let server = self.servers.entry(language_id.to_owned()).or_default();
        if !matches!(
            server.state,
            LanguageState::Stopped | LanguageState::Unavailable | LanguageState::Failed
        ) {
            return Vec::new();
        }
        server.generation = server.generation.wrapping_add(1);
        server.state = LanguageState::Discovering;
        vec![LanguageEffect::Discover {
            workspace_id: self.workspace_id.clone(),
            language_id: language_id.to_owned(),
            descriptor_id,
            generation: server.generation,
        }]
    }
}

fn parse_completion_items(result: &serde_json::Value) -> Vec<CompletionCandidate> {
    let items = result
        .as_array()
        .or_else(|| result.get("items").and_then(serde_json::Value::as_array));
    items
        .into_iter()
        .flatten()
        .take(200)
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.trim();
            if label.is_empty() {
                return None;
            }
            let raw = item
                .get("textEdit")
                .and_then(|edit| edit.get("newText"))
                .or_else(|| item.get("insertText"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(label);
            let insertion = if item
                .get("insertTextFormat")
                .and_then(serde_json::Value::as_u64)
                == Some(2)
            {
                flatten_snippet(raw)
            } else {
                raw.to_owned()
            };
            Some(CompletionCandidate {
                label: label.chars().take(512).collect(),
                insertion: insertion.chars().take(64 * 1024).collect(),
                range: item
                    .get("textEdit")
                    .and_then(|edit| edit.get("range"))
                    .and_then(|range| serde_json::from_value(range.clone()).ok()),
            })
        })
        .collect()
}

fn flatten_snippet(snippet: &str) -> String {
    let mut output = String::with_capacity(snippet.len());
    let characters = snippet.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < characters.len() {
        if characters[index] != '$' {
            output.push(characters[index]);
            index += 1;
            continue;
        }
        if characters.get(index + 1) == Some(&'{') {
            let Some(end) = characters[index + 2..]
                .iter()
                .position(|value| *value == '}')
            else {
                index += 1;
                continue;
            };
            let inner = &characters[index + 2..index + 2 + end];
            if let Some(colon) = inner.iter().position(|value| *value == ':') {
                output.extend(inner[colon + 1..].iter());
            }
            index += end + 3;
        } else {
            index += 1;
            while characters.get(index).is_some_and(char::is_ascii_digit) {
                index += 1;
            }
        }
    }
    output
}

fn parse_hover(result: &serde_json::Value) -> Option<String> {
    let contents = result.get("contents").unwrap_or(result);
    let value = contents
        .as_str()
        .or_else(|| contents.get("value").and_then(serde_json::Value::as_str))?;
    let sanitized = sanitize_hover_markdown(value);
    (!sanitized.value().trim().is_empty()).then(|| sanitized.value().to_owned())
}

fn parse_definitions(result: &serde_json::Value, workspace_root: &Path) -> Vec<DefinitionLocation> {
    let locations = result
        .as_array()
        .map_or_else(|| vec![result], |locations| locations.iter().collect());
    locations
        .into_iter()
        .take(100)
        .filter_map(|location| {
            let uri = location
                .get("uri")
                .or_else(|| location.get("targetUri"))?
                .as_str()?;
            let range = location
                .get("range")
                .or_else(|| location.get("targetSelectionRange"))?;
            let start = range.get("start")?;
            let line = u32::try_from(start.get("line")?.as_u64()?).ok()?;
            let character = u32::try_from(start.get("character")?.as_u64()?).ok()?;
            let absolute = Url::parse(uri).ok().and_then(|uri| uri.to_file_path().ok());
            let path = absolute
                .as_ref()
                .and_then(|path| path.strip_prefix(workspace_root).ok())
                .map(Path::to_path_buf);
            Some(DefinitionLocation {
                external: absolute.is_some() && path.is_none(),
                path,
                line,
                character,
                uri: uri.chars().take(2_048).collect(),
            })
        })
        .collect()
}

fn detect_language(path: &Path, language_override: Option<&str>) -> String {
    if let Some(language_override) = language_override {
        return language_override.to_owned();
    }
    GrammarRegistry::detect(path, None).id.to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use strukt_editor::DocumentId;
    use strukt_language::{
        FeatureRequestKind, FrameDecoder, FrameLimits, LanguageProcess, PositionEncoding,
        ProcessExit, TransportError, encode_frame,
    };
    use strukt_persistence::{LanguageSelectionSnapshot, LanguageSessionSnapshot};
    use url::Url;

    use super::{
        DiagnosticSeverity, LanguageCoordinator, LanguageEffect, LanguageRuntime,
        LanguageRuntimeEvent, LanguageSpawnCompletion, LanguageState, PublishedDiagnostic,
        flatten_snippet, parse_completion_items, parse_definitions, parse_hover,
        parse_publish_diagnostics,
    };

    #[test]
    fn language_restore_stays_stopped_until_a_matching_document_opens() {
        let restored = LanguageSessionSnapshot::new(
            vec![LanguageSelectionSnapshot::enabled("rust", "rust-analyzer").unwrap()],
            Vec::new(),
            true,
        )
        .unwrap()
        .restore()
        .unwrap();
        let mut coordinator = LanguageCoordinator::restore("workspace-a", &restored);

        assert_eq!(coordinator.running_servers(), 0);
        let effects = coordinator.open_document(
            document_id(7),
            Path::new("src/main.rs"),
            None,
            1,
            Some("fn main() {}"),
        );

        assert!(
            matches!(effects.as_slice(), [LanguageEffect::Discover { language_id, .. }] if language_id == "rust")
        );
        assert_eq!(coordinator.state("rust"), LanguageState::Discovering);
    }

    #[test]
    fn language_workspace_replacement_invalidates_old_discovery_completions() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let effects = coordinator.open_document(
            document_id(1),
            Path::new("main.rs"),
            None,
            1,
            Some("fn main() {}"),
        );
        let LanguageEffect::Discover { generation, .. } = effects[0] else {
            panic!("expected discovery");
        };
        coordinator.replace_workspace("workspace-b");

        assert!(!coordinator.discovery_available("workspace-a", "rust", generation));
        assert_eq!(coordinator.running_servers(), 0);
    }

    #[test]
    fn language_overrides_close_old_pairing_and_open_the_new_language() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let id = document_id(2);
        coordinator.open_document(id, Path::new("main.rs"), None, 1, Some("fn main() {}"));
        let effects = coordinator.set_override(id, Some("python"));

        assert!(effects.iter().any(|effect| matches!(effect, LanguageEffect::Close { language_id, .. } if language_id == "rust")));
        assert!(effects.iter().any(|effect| matches!(effect, LanguageEffect::Discover { language_id, .. } if language_id == "python")));
        assert_eq!(coordinator.document_language(id), Some("python"));
    }

    #[test]
    fn language_metadata_only_documents_never_schedule_language_work() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        assert!(
            coordinator
                .open_document(document_id(3), Path::new("large.rs"), None, 1, None,)
                .is_empty()
        );
        assert_eq!(coordinator.running_servers(), 0);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one native contract covers initialization, synchronization, features, and server requests"
    )]
    fn language_runtime_initializes_and_opens_documents_after_readiness() {
        let root = tempfile::tempdir().unwrap();
        let id = document_id(9);
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let effects =
            coordinator.open_document(id, Path::new("src/main.rs"), None, 1, Some("fn main() {}"));
        let LanguageEffect::Discover { generation, .. } = effects[0] else {
            panic!("expected discovery");
        };
        assert!(coordinator.discovery_available("workspace-a", "rust", generation));

        let shared = Arc::new(Mutex::new(FakeProcessState::default()));
        let completion = LanguageSpawnCompletion::new(Ok(Box::new(FakeProcess {
            shared: Arc::clone(&shared),
        })));
        let mut runtime = LanguageRuntime::default();
        runtime
            .finish_start(
                "workspace-a",
                "rust",
                generation,
                root.path().to_path_buf(),
                &completion,
            )
            .unwrap();
        assert_eq!(written_methods(&shared), vec!["initialize"]);

        let response = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0",
            "id":1,
            "result":{"capabilities":{
                "positionEncoding":"utf-16",
                "textDocumentSync":1,
                "completionProvider":{},
                "hoverProvider":true,
                "definitionProvider":true
            }}
        }))
        .unwrap();
        shared
            .lock()
            .unwrap()
            .reads
            .push_back(encode_frame(&response, FrameLimits::default()).unwrap());
        assert!(matches!(
            runtime.poll().as_slice(),
            [LanguageRuntimeEvent::Ready { language_id, generation: ready_generation, .. }]
                if language_id == "rust" && *ready_generation == generation
        ));
        let opens = coordinator.mark_ready("rust", generation, PositionEncoding::Utf16);
        runtime.apply_effects(opens).unwrap();

        assert_eq!(
            written_methods(&shared),
            vec!["initialize", "initialized", "textDocument/didOpen"]
        );

        let request = runtime
            .request_feature(
                "rust",
                generation,
                Path::new("src/main.rs"),
                1,
                FeatureRequestKind::Completion,
                strukt_language::LspPosition::new(0, 2),
            )
            .unwrap();
        let response = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0",
            "id":request.id().get(),
            "result":{"items":[{"label":"main","insertText":"main"}]}
        }))
        .unwrap();
        shared
            .lock()
            .unwrap()
            .reads
            .push_back(encode_frame(&response, FrameLimits::default()).unwrap());
        assert!(matches!(
            runtime.poll().as_slice(),
            [LanguageRuntimeEvent::FeatureResponse {
                kind: FeatureRequestKind::Completion,
                ..
            }]
        ));

        let server_request = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0",
            "id":77,
            "method":"workspace/configuration",
            "params":{"items":[]}
        }))
        .unwrap();
        shared
            .lock()
            .unwrap()
            .reads
            .push_back(encode_frame(&server_request, FrameLimits::default()).unwrap());
        assert!(runtime.poll().is_empty());
        let writes = shared.lock().unwrap().writes.clone();
        let mut decoder = FrameDecoder::new(FrameLimits::default());
        let frames = decoder.push(writes.last().unwrap()).unwrap();
        let response: serde_json::Value = serde_json::from_slice(frames[0].body()).unwrap();
        assert_eq!(
            response,
            serde_json::json!({"jsonrpc":"2.0","id":77,"result":[]})
        );
    }

    #[test]
    fn language_runtime_enforces_initialize_timeout() {
        let root = tempfile::tempdir().unwrap();
        let shared = Arc::new(Mutex::new(FakeProcessState::default()));
        let completion = LanguageSpawnCompletion::new(Ok(Box::new(FakeProcess {
            shared: Arc::clone(&shared),
        })));
        let mut runtime = LanguageRuntime::default();
        runtime
            .finish_start(
                "workspace-a",
                "rust",
                3,
                root.path().to_path_buf(),
                &completion,
            )
            .unwrap();

        assert!(matches!(
            runtime.poll_at(Duration::from_secs(11)).as_slice(),
            [LanguageRuntimeEvent::Failed {
                language_id,
                generation: 3,
                ..
            }] if language_id == "rust"
        ));
        assert!(runtime.processes.is_empty());
    }

    #[test]
    fn language_runtime_starts_idle_shutdown_after_last_document_closes() {
        let root = tempfile::tempdir().unwrap();
        let shared = Arc::new(Mutex::new(FakeProcessState::default()));
        let completion = LanguageSpawnCompletion::new(Ok(Box::new(FakeProcess {
            shared: Arc::clone(&shared),
        })));
        let mut runtime = LanguageRuntime::default();
        runtime
            .finish_start(
                "workspace-a",
                "rust",
                4,
                root.path().to_path_buf(),
                &completion,
            )
            .unwrap();
        let initialize = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0",
            "id":1,
            "result":{"capabilities":{"textDocumentSync":1}}
        }))
        .unwrap();
        shared
            .lock()
            .unwrap()
            .reads
            .push_back(encode_frame(&initialize, FrameLimits::default()).unwrap());
        assert!(matches!(
            runtime.poll_at(Duration::ZERO).as_slice(),
            [LanguageRuntimeEvent::Ready { .. }]
        ));
        runtime
            .apply_effects(vec![LanguageEffect::Open {
                language_id: "rust".to_owned(),
                generation: 4,
                document_id: document_id(21),
                path: Path::new("main.rs").to_path_buf(),
                revision: 1,
                text: "fn main() {}".to_owned(),
            }])
            .unwrap();
        runtime
            .apply_effects(vec![LanguageEffect::Close {
                language_id: "rust".to_owned(),
                generation: 4,
                document_id: document_id(21),
                path: Path::new("main.rs").to_path_buf(),
            }])
            .unwrap();

        assert!(runtime.poll_at(Duration::from_secs(31)).is_empty());
        assert_eq!(
            written_methods(&shared).last().map(String::as_str),
            Some("shutdown")
        );
        assert!(runtime.processes["rust"].shutdown_id.is_some());
    }

    #[test]
    fn stopped_runtime_state_rediscovers_when_a_document_reopens() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let id = document_id(22);
        let effects =
            coordinator.open_document(id, Path::new("main.rs"), None, 1, Some("fn main() {}"));
        let LanguageEffect::Discover { generation, .. } = effects[0] else {
            panic!("expected discovery");
        };
        assert!(coordinator.discovery_available("workspace-a", "rust", generation));
        let _ = coordinator.mark_ready("rust", generation, PositionEncoding::Utf16);
        assert!(matches!(
            coordinator.close_document(id).as_slice(),
            [LanguageEffect::Close { .. }]
        ));

        assert!(coordinator.mark_stopped("rust", generation));
        assert_eq!(coordinator.state("rust"), LanguageState::Stopped);
        assert!(matches!(
            coordinator
                .open_document(
                    id,
                    Path::new("main.rs"),
                    None,
                    1,
                    Some("fn main() {}"),
                )
                .as_slice(),
            [LanguageEffect::Discover {
                generation: next_generation,
                ..
            }] if *next_generation != generation
        ));
    }

    #[test]
    fn language_failures_and_capability_states_are_isolated() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let rust =
            coordinator.open_document(document_id(10), Path::new("main.rs"), None, 1, Some("rust"));
        let python = coordinator.open_document(
            document_id(11),
            Path::new("main.py"),
            None,
            1,
            Some("python"),
        );
        let LanguageEffect::Discover {
            generation: rust_generation,
            ..
        } = rust[0]
        else {
            panic!("expected rust discovery");
        };
        let LanguageEffect::Discover {
            generation: python_generation,
            ..
        } = python[0]
        else {
            panic!("expected python discovery");
        };
        assert!(coordinator.discovery_finished(
            "workspace-a",
            "rust",
            rust_generation,
            LanguageState::Unavailable,
        ));
        assert!(coordinator.discovery_finished(
            "workspace-a",
            "python",
            python_generation,
            LanguageState::ApprovalRequired,
        ));

        assert_eq!(coordinator.state("rust"), LanguageState::Unavailable);
        assert_eq!(coordinator.state("python"), LanguageState::ApprovalRequired);

        let failed_spawn = LanguageSpawnCompletion::new(Err("spawn failed".to_owned()));
        let mut runtime = LanguageRuntime::default();
        assert!(
            runtime
                .finish_start(
                    "workspace-a",
                    "rust",
                    rust_generation,
                    Path::new("/workspace").to_path_buf(),
                    &failed_spawn,
                )
                .is_err()
        );
        assert_eq!(coordinator.state("python"), LanguageState::ApprovalRequired);
    }

    #[test]
    fn language_diagnostics_replace_current_revision_and_reject_stale_publications() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let id = document_id(12);
        let effects =
            coordinator.open_document(id, Path::new("src/main.rs"), None, 4, Some("fn main() {}"));
        let LanguageEffect::Discover { generation, .. } = effects[0] else {
            panic!("expected discovery");
        };
        assert!(coordinator.discovery_available("workspace-a", "rust", generation));
        let _ = coordinator.mark_ready("rust", generation, PositionEncoding::Utf16);

        assert!(coordinator.publish_diagnostics(
            "rust",
            generation,
            Path::new("src/main.rs"),
            Some(4),
            vec![PublishedDiagnostic::new(
                1,
                2,
                DiagnosticSeverity::Error,
                "expected semicolon",
                Some("rustc"),
            )],
        ));
        assert_eq!(coordinator.problems().len(), 1);
        assert_eq!(coordinator.problem_counts().errors, 1);

        assert!(!coordinator.publish_diagnostics(
            "rust",
            generation,
            Path::new("src/main.rs"),
            Some(3),
            vec![PublishedDiagnostic::new(
                0,
                0,
                DiagnosticSeverity::Warning,
                "stale",
                None,
            )],
        ));
        assert_eq!(coordinator.problems()[0].message(), "expected semicolon");

        assert!(coordinator.publish_diagnostics(
            "rust",
            generation,
            Path::new("src/main.rs"),
            Some(4),
            Vec::new(),
        ));
        assert!(coordinator.problems().is_empty());
    }

    #[test]
    fn language_diagnostics_clear_when_document_closes_or_server_fails() {
        let mut coordinator = LanguageCoordinator::new("workspace-a");
        let id = document_id(13);
        let effects =
            coordinator.open_document(id, Path::new("main.py"), None, 1, Some("print('hello')"));
        let LanguageEffect::Discover { generation, .. } = effects[0] else {
            panic!("expected discovery");
        };
        assert!(coordinator.discovery_available("workspace-a", "python", generation));
        let _ = coordinator.mark_ready("python", generation, PositionEncoding::Utf16);
        assert!(coordinator.publish_diagnostics(
            "python",
            generation,
            Path::new("main.py"),
            Some(1),
            vec![PublishedDiagnostic::new(
                0,
                0,
                DiagnosticSeverity::Hint,
                "consider a module docstring",
                Some("ruff"),
            )],
        ));

        let _ = coordinator.close_document(id);
        assert!(coordinator.problems().is_empty());

        let effects =
            coordinator.open_document(id, Path::new("main.py"), None, 1, Some("print('hello')"));
        let LanguageEffect::Open { .. } = effects[0] else {
            panic!("expected reopen");
        };
        assert!(coordinator.publish_diagnostics(
            "python",
            generation,
            Path::new("main.py"),
            Some(1),
            vec![PublishedDiagnostic::new(
                0,
                0,
                DiagnosticSeverity::Warning,
                "warning",
                None,
            )],
        ));
        assert!(coordinator.fail("python", generation));
        assert!(coordinator.problems().is_empty());
    }

    #[test]
    fn diagnostic_publication_parser_confines_file_uris_to_the_workspace() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("src/main.rs");
        let params = serde_json::json!({
            "uri": Url::from_file_path(&file).unwrap().to_string(),
            "version": 7,
            "diagnostics": [{
                "range": {"start": {"line": 2, "character": 3}, "end": {"line": 2, "character": 4}},
                "severity": 2,
                "message": "warning",
                "source": "fixture"
            }]
        });
        let (path, version, diagnostics) =
            parse_publish_diagnostics(Some(&params), root.path()).unwrap();
        assert_eq!(path, Path::new("src/main.rs"));
        assert_eq!(version, Some(7));
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);

        let outside = serde_json::json!({
            "uri": Url::from_file_path(root.path().parent().unwrap().join("outside.rs"))
                .unwrap()
                .to_string(),
            "diagnostics": []
        });
        assert!(parse_publish_diagnostics(Some(&outside), root.path()).is_none());
    }

    #[test]
    fn completion_hover_and_definition_results_are_bounded_and_safe() {
        let completion = serde_json::json!({"items": [
            {"label":"call", "insertText":"call(${1:value})$0", "insertTextFormat":2},
            {"label":"plain", "insertText":"plain"}
        ]});
        let items = parse_completion_items(&completion);
        assert_eq!(items[0].insertion(), "call(value)");
        assert_eq!(flatten_snippet("${1:first} + $2"), "first + ");

        let hover = serde_json::json!({"contents": {"kind":"markdown", "value":"<script>x</script>[safe](https://example.com)"}});
        assert_eq!(parse_hover(&hover).as_deref(), Some("xsafe"));

        let root = tempfile::tempdir().unwrap();
        let inside = Url::from_file_path(root.path().join("src/lib.rs"))
            .unwrap()
            .to_string();
        let outside = Url::from_file_path(root.path().parent().unwrap().join("elsewhere.rs"))
            .unwrap()
            .to_string();
        let definitions = parse_definitions(
            &serde_json::json!([
                {"uri": inside, "range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
                {"uri": outside, "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}}
            ]),
            root.path(),
        );
        assert_eq!(definitions[0].path(), Some(Path::new("src/lib.rs")));
        assert!(!definitions[0].external());
        assert!(definitions[1].external());
    }

    #[derive(Default)]
    struct FakeProcessState {
        reads: VecDeque<Vec<u8>>,
        writes: Vec<Vec<u8>>,
    }

    struct FakeProcess {
        shared: Arc<Mutex<FakeProcessState>>,
    }

    impl LanguageProcess for FakeProcess {
        fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            self.shared.lock().unwrap().writes.push(frame.to_vec());
            Ok(())
        }

        fn try_read(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
            Ok(self.shared.lock().unwrap().reads.pop_front())
        }

        fn try_read_stderr(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
            Ok(None)
        }

        fn try_wait(&mut self) -> Result<Option<ProcessExit>, TransportError> {
            Ok(None)
        }

        fn terminate(&mut self, _grace: Duration) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn written_methods(shared: &Arc<Mutex<FakeProcessState>>) -> Vec<String> {
        shared
            .lock()
            .unwrap()
            .writes
            .iter()
            .map(|frame| {
                let mut decoder = FrameDecoder::new(FrameLimits::default());
                let frames = decoder.push(frame).unwrap();
                let message: serde_json::Value = serde_json::from_slice(frames[0].body()).unwrap();
                message["method"].as_str().unwrap().to_owned()
            })
            .collect()
    }

    fn document_id(value: u64) -> DocumentId {
        serde_json::from_value(serde_json::json!(value)).unwrap()
    }
}
