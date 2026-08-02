use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use strukt_editor::{DocumentId, GrammarRegistry};
use strukt_language::{
    ApprovalStatus, DiscoveredServer, DiscoveryOutcome, FeatureRequestKind, FrameDecoder,
    FrameLimits, IncomingMessage, LanguageClient, LanguageProcess, LanguageTransport,
    PositionEncoding, ServerCapabilities, SpawnRequest, StdioTransport, SynchronizationKind,
    built_in_descriptors, discover, encode_frame, load_workspace_registry, parse_message,
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
}

struct RunningLanguage {
    generation: u64,
    root: PathBuf,
    process: Box<dyn LanguageProcess>,
    decoder: FrameDecoder,
    client: LanguageClient,
    initialize_id: strukt_language::RequestId,
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

    pub(crate) fn poll(&mut self) -> Vec<LanguageRuntimeEvent> {
        let now = self.now();
        let languages = self.processes.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
        let mut failed = Vec::new();
        for language_id in languages {
            let Some(running) = self.processes.get_mut(&language_id) else {
                continue;
            };
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
                failed.push((
                    language_id.clone(),
                    running.generation,
                    format!("language server exited with {:?}", exit.code()),
                ));
            }
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
        IncomingMessage::Response(_) | IncomingMessage::Request(_) => Ok(None),
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
    let synchronization = if capabilities["textDocumentSync"].is_null() {
        SynchronizationKind::None
    } else {
        SynchronizationKind::Full
    };
    ServerCapabilities::new(synchronization, features, encoding)
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

fn file_uri(root: &Path, relative: &Path) -> Result<String, String> {
    Url::from_file_path(root.join(relative))
        .map(Into::into)
        .map_err(|()| "document path could not be represented as a file URI".to_owned())
}

pub(crate) fn discover_workspace_language(
    workspace: &WorkspaceRoot,
    language_id: &str,
    descriptor_id: Option<&str>,
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
            LanguageDiscoveryCompletion::ApprovalRequired(server)
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
}

#[derive(Clone, Debug)]
pub(crate) struct LanguageCoordinator {
    workspace_id: String,
    selections: HashMap<String, LanguageSelectionSnapshot>,
    approvals: Vec<ApprovalSnapshot>,
    problems_visible: bool,
    documents: HashMap<DocumentId, DocumentBinding>,
    servers: HashMap<String, ServerBinding>,
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
            persistence_dirty: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn replace_workspace(&mut self, workspace_id: impl Into<String>) {
        self.workspace_id = workspace_id.into();
        self.documents.clear();
        self.servers.clear();
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
        self.ensure_language_started(&language_id)
    }

    pub(crate) fn edit_document(
        &mut self,
        id: DocumentId,
        revision: u64,
        text: &str,
    ) -> Vec<LanguageEffect> {
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
        true
    }

    pub(crate) fn mark_ready(&mut self, language_id: &str, generation: u64) -> Vec<LanguageEffect> {
        let Some(server) = self.servers.get_mut(language_id) else {
            return Vec::new();
        };
        if server.generation != generation || server.state != LanguageState::Starting {
            return Vec::new();
        }
        server.state = LanguageState::Ready;
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
        let Some(server) = self.servers.get_mut(language_id) else {
            return false;
        };
        if server.generation != generation {
            return false;
        }
        server.state = LanguageState::Failed;
        true
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
        FrameDecoder, FrameLimits, LanguageProcess, ProcessExit, TransportError, encode_frame,
    };
    use strukt_persistence::{LanguageSelectionSnapshot, LanguageSessionSnapshot};

    use super::{
        LanguageCoordinator, LanguageEffect, LanguageRuntime, LanguageRuntimeEvent,
        LanguageSpawnCompletion, LanguageState,
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
            [LanguageRuntimeEvent::Ready { language_id, generation: ready_generation }]
                if language_id == "rust" && *ready_generation == generation
        ));
        let opens = coordinator.mark_ready("rust", generation);
        runtime.apply_effects(opens).unwrap();

        assert_eq!(
            written_methods(&shared),
            vec!["initialize", "initialized", "textDocument/didOpen"]
        );
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
