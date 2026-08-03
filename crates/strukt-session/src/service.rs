use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use strukt_terminal::{
    DrainBudget, RuntimeError, RuntimePaneState, RuntimeStartJob, RuntimeTerminateCompletion,
    RuntimeTerminateJob, SpawnRequest, TerminalPaneId, TerminalRuntime, TerminalSize,
    TerminalTransport,
};
use thiserror::Error;

use crate::{
    AttentionState, CatalogError, PaneHistorySnapshot, PaneId, PaneLifecycle, PaneScreenSnapshot,
    PersistedCatalog, ServiceInstanceId, SessionCatalog, SessionId, SnapshotError,
};

pub struct SessionService {
    service_instance: ServiceInstanceId,
    catalog: SessionCatalog,
    runtime: TerminalRuntime,
    terminal_panes: BTreeMap<PaneId, TerminalPaneId>,
    session_panes: BTreeMap<TerminalPaneId, PaneId>,
    output_revisions: BTreeMap<PaneId, u64>,
    terminal_revisions: BTreeMap<PaneId, u64>,
    unread_counts: BTreeMap<PaneId, u64>,
    attention: BTreeMap<PaneId, AttentionState>,
    historical: BTreeMap<PaneId, PaneScreenSnapshot>,
    attached_clients: usize,
    detached_since: Instant,
    idle_timeout: Duration,
    dirty: bool,
}

impl SessionService {
    /// Creates a new service instance from stopped definitions without spawning.
    ///
    /// # Errors
    ///
    /// Returns catalog validation errors.
    pub fn new(
        service_instance: ServiceInstanceId,
        catalog: &SessionCatalog,
        transport: Arc<dyn TerminalTransport>,
        scrollback_limit: usize,
        idle_timeout: Duration,
    ) -> Result<Self, ServiceError> {
        Self::build(
            service_instance,
            catalog,
            &[],
            transport,
            scrollback_limit,
            idle_timeout,
        )
    }

    /// Restores stopped definitions and bounded historical screen snapshots.
    ///
    /// # Errors
    ///
    /// Returns catalog or history validation errors. No process is started.
    pub fn restore(
        service_instance: ServiceInstanceId,
        persisted: &PersistedCatalog,
        transport: Arc<dyn TerminalTransport>,
        scrollback_limit: usize,
        idle_timeout: Duration,
    ) -> Result<Self, ServiceError> {
        Self::build(
            service_instance,
            persisted.catalog(),
            persisted.histories(),
            transport,
            scrollback_limit,
            idle_timeout,
        )
    }

    fn build(
        service_instance: ServiceInstanceId,
        catalog: &SessionCatalog,
        histories: &[PaneHistorySnapshot],
        transport: Arc<dyn TerminalTransport>,
        scrollback_limit: usize,
        idle_timeout: Duration,
    ) -> Result<Self, ServiceError> {
        let catalog = catalog.stopped_clone()?;
        let historical = histories
            .iter()
            .map(|history| (history.pane(), history.screen().clone()))
            .collect();
        Ok(Self {
            service_instance,
            catalog,
            runtime: TerminalRuntime::new(transport, scrollback_limit),
            terminal_panes: BTreeMap::new(),
            session_panes: BTreeMap::new(),
            output_revisions: BTreeMap::new(),
            terminal_revisions: BTreeMap::new(),
            unread_counts: BTreeMap::new(),
            attention: BTreeMap::new(),
            historical,
            attached_clients: 0,
            detached_since: Instant::now(),
            idle_timeout,
            dirty: false,
        })
    }

    #[must_use]
    pub const fn service_instance(&self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn catalog(&self) -> &SessionCatalog {
        &self.catalog
    }

    #[must_use]
    pub const fn attached_clients(&self) -> usize {
        self.attached_clients
    }

    #[must_use]
    pub fn running_panes(&self) -> usize {
        self.runtime.running_processes()
    }

    #[must_use]
    pub fn pane_generation(&self, pane: PaneId) -> Option<u64> {
        self.catalog
            .pane(pane)
            .map(|(_, _, pane)| pane.generation())
    }

    #[must_use]
    pub fn pane_lifecycle(&self, pane: PaneId) -> Option<PaneLifecycle> {
        self.catalog
            .pane(pane)
            .map(|(_, _, pane)| pane.lifecycle().clone())
    }

    /// Attaches the single M3 controlling client.
    ///
    /// # Errors
    ///
    /// Returns stale-instance or conflicting-writer errors.
    pub fn attach(&mut self, service_instance: ServiceInstanceId) -> Result<(), ServiceError> {
        self.expect_instance(service_instance)?;
        if self.attached_clients > 0 {
            return Err(ServiceError::WriterAlreadyAttached);
        }
        self.attached_clients = 1;
        Ok(())
    }

    /// Detaches the client without terminating any pane.
    ///
    /// # Errors
    ///
    /// Returns stale-instance or missing-client errors.
    pub fn detach(&mut self, service_instance: ServiceInstanceId) -> Result<(), ServiceError> {
        self.expect_instance(service_instance)?;
        if self.attached_clients == 0 {
            return Err(ServiceError::NoAttachedClient);
        }
        self.attached_clients = 0;
        self.detached_since = Instant::now();
        Ok(())
    }

    /// Creates a stopped session definition.
    ///
    /// # Errors
    ///
    /// Returns stale-instance, catalog revision, validation, or capacity errors.
    pub fn create_session(
        &mut self,
        service_instance: ServiceInstanceId,
        expected_revision: u64,
        name: impl Into<String>,
        working_directory: impl AsRef<Path>,
    ) -> Result<SessionId, ServiceError> {
        self.expect_instance(service_instance)?;
        let session = self
            .catalog
            .create_session(expected_revision, name, working_directory)?;
        self.dirty = true;
        Ok(session)
    }

    /// Imports one complete stopped migration catalog only into an empty service.
    ///
    /// # Errors
    ///
    /// Returns stale-instance/revision, non-empty target, or catalog validation errors.
    pub fn import_stopped_catalog(
        &mut self,
        service_instance: ServiceInstanceId,
        expected_revision: u64,
        catalog: &SessionCatalog,
    ) -> Result<(), ServiceError> {
        self.expect_instance(service_instance)?;
        if self.catalog.revision() != expected_revision {
            return Err(CatalogError::StaleRevision {
                expected: expected_revision,
                actual: self.catalog.revision(),
            }
            .into());
        }
        if self.catalog.sessions().next().is_some() {
            return Err(ServiceError::ImportTargetNotEmpty);
        }
        self.catalog = catalog.stopped_clone()?;
        self.dirty = true;
        Ok(())
    }

    /// Applies one validated catalog-only mutation for the current service.
    ///
    /// Blocking process work is intentionally excluded from this API.
    ///
    /// # Errors
    ///
    /// Returns stale-instance or catalog validation errors.
    pub fn apply_catalog_mutation<T>(
        &mut self,
        service_instance: ServiceInstanceId,
        mutation: impl FnOnce(&mut SessionCatalog) -> Result<T, CatalogError>,
    ) -> Result<T, ServiceError> {
        self.expect_instance(service_instance)?;
        let result = mutation(&mut self.catalog)?;
        let removed = self
            .terminal_panes
            .keys()
            .copied()
            .filter(|pane| !self.catalog.contains_pane(*pane))
            .collect::<Vec<_>>();
        for pane in removed {
            if let Some(terminal) = self.terminal_panes.remove(&pane) {
                self.session_panes.remove(&terminal);
                self.runtime.discard(terminal);
            }
            self.output_revisions.remove(&pane);
            self.terminal_revisions.remove(&pane);
            self.unread_counts.remove(&pane);
            self.attention.remove(&pane);
            self.historical.remove(&pane);
        }
        self.dirty = true;
        Ok(result)
    }

    /// Begins an explicit pane start while leaving process spawn in the returned job.
    ///
    /// # Errors
    ///
    /// Returns stale-instance, revision, hierarchy, directory, or runtime errors.
    pub fn begin_start(
        &mut self,
        service_instance: ServiceInstanceId,
        expected_revision: u64,
        session: SessionId,
        pane: PaneId,
        request: SpawnRequest,
    ) -> Result<ServiceStartJob, ServiceError> {
        self.expect_instance(service_instance)?;
        let (actual_session, _, definition) =
            self.catalog.pane(pane).ok_or(CatalogError::PaneNotFound)?;
        if actual_session != session || definition.working_directory() != request.working_directory
        {
            return Err(ServiceError::InvalidStartRequest);
        }
        if self.catalog.revision() != expected_revision {
            return Err(CatalogError::StaleRevision {
                expected: expected_revision,
                actual: self.catalog.revision(),
            }
            .into());
        }
        let terminal_pane = self.ensure_runtime_pane(pane);
        self.historical.remove(&pane);
        let runtime_job = self.runtime.begin_restart(terminal_pane, request)?;
        let generation = self
            .catalog
            .begin_pane_generation(expected_revision, session, pane)?;
        if runtime_job.generation() != generation {
            self.runtime.discard(terminal_pane);
            return Err(ServiceError::GenerationDiverged);
        }
        self.dirty = true;
        Ok(ServiceStartJob {
            service_instance,
            session,
            pane,
            generation,
            runtime_job,
        })
    }

    /// Applies a start completion only to its originating service and generation.
    ///
    /// # Errors
    ///
    /// Returns stale-instance, hierarchy, or pane-local runtime errors.
    pub fn finish_start(&mut self, completion: ServiceStartCompletion) -> Result<(), ServiceError> {
        self.expect_instance(completion.service_instance)?;
        if !self.matches_generation(completion.session, completion.pane, completion.generation) {
            return Err(ServiceError::StaleGeneration);
        }
        let result = self.runtime.finish_restart(
            completion.runtime_pane,
            completion.generation,
            completion.result,
        );
        let lifecycle = self
            .runtime
            .state(completion.runtime_pane)
            .map(runtime_lifecycle)
            .ok_or(ServiceError::StaleGeneration)?;
        self.catalog.set_generation_lifecycle(
            completion.session,
            completion.pane,
            completion.generation,
            lifecycle,
        )?;
        self.dirty = true;
        result.map_err(ServiceError::Runtime)
    }

    /// Writes input only to the current service and pane generation.
    ///
    /// # Errors
    ///
    /// Returns stale identity/generation or runtime errors.
    pub fn write(
        &mut self,
        service_instance: ServiceInstanceId,
        pane: PaneId,
        generation: u64,
        bytes: &[u8],
    ) -> Result<(), ServiceError> {
        self.expect_instance(service_instance)?;
        let (session, _, _) = self.catalog.pane(pane).ok_or(CatalogError::PaneNotFound)?;
        if !self.matches_generation(session, pane, generation) {
            return Err(ServiceError::StaleGeneration);
        }
        let terminal = self.runtime_pane(pane)?;
        self.runtime.write(terminal, bytes)?;
        Ok(())
    }

    /// Resizes only the current service and pane generation.
    ///
    /// # Errors
    ///
    /// Returns stale identity/generation or runtime errors.
    pub fn resize(
        &mut self,
        service_instance: ServiceInstanceId,
        pane: PaneId,
        generation: u64,
        size: TerminalSize,
    ) -> Result<(), ServiceError> {
        self.expect_instance(service_instance)?;
        let (session, _, _) = self.catalog.pane(pane).ok_or(CatalogError::PaneNotFound)?;
        if !self.matches_generation(session, pane, generation) {
            return Err(ServiceError::StaleGeneration);
        }
        let terminal = self.runtime_pane(pane)?;
        self.runtime.resize(terminal, size)?;
        Ok(())
    }

    /// Moves bounded termination into an explicit background job.
    ///
    /// # Errors
    ///
    /// Returns stale identity/generation, hierarchy, or runtime errors.
    pub fn begin_terminate(
        &mut self,
        service_instance: ServiceInstanceId,
        session: SessionId,
        pane: PaneId,
        generation: u64,
        grace: Duration,
    ) -> Result<ServiceTerminateJob, ServiceError> {
        self.expect_instance(service_instance)?;
        if !self.matches_generation(session, pane, generation) {
            return Err(ServiceError::StaleGeneration);
        }
        let terminal_pane = self.runtime_pane(pane)?;
        let runtime_job = self.runtime.begin_terminate(terminal_pane, grace)?;
        Ok(ServiceTerminateJob {
            service_instance,
            session,
            pane,
            generation,
            runtime_job,
        })
    }

    /// Applies a bounded termination completion to its current generation.
    ///
    /// # Errors
    ///
    /// Returns stale identity/generation, hierarchy, or runtime errors.
    pub fn finish_terminate(
        &mut self,
        completion: ServiceTerminateCompletion,
    ) -> Result<(), ServiceError> {
        self.expect_instance(completion.service_instance)?;
        if !self.matches_generation(completion.session, completion.pane, completion.generation) {
            return Err(ServiceError::StaleGeneration);
        }
        let result = self.runtime.finish_terminate(completion.runtime_completion);
        let lifecycle = self
            .runtime
            .state(completion.runtime_pane)
            .map(runtime_lifecycle)
            .ok_or(ServiceError::StaleGeneration)?;
        self.catalog.set_generation_lifecycle(
            completion.session,
            completion.pane,
            completion.generation,
            lifecycle,
        )?;
        self.dirty = true;
        result.map_err(ServiceError::Runtime)
    }

    /// Drains each live pane fairly and coalesces changed pane notifications.
    #[must_use]
    pub fn tick(&mut self) -> ServiceBatch {
        let runtime_batch = self.runtime.drain(DrainBudget::default());
        let mut batch = ServiceBatch::default();
        for terminal in runtime_batch.changed_panes() {
            let Some(pane) = self.session_panes.get(terminal).copied() else {
                continue;
            };
            let Some(projection) = self.runtime.projection(*terminal) else {
                continue;
            };
            let Some((session, _, definition)) = self.catalog.pane(pane) else {
                continue;
            };
            if definition.generation() != projection.generation() {
                continue;
            }
            let terminal_revision = projection.snapshot().revision();
            let prior_revision = self.terminal_revisions.get(&pane).copied().unwrap_or(0);
            if terminal_revision > prior_revision {
                self.terminal_revisions.insert(pane, terminal_revision);
                let output_revision = self.output_revisions.entry(pane).or_default();
                *output_revision = output_revision.saturating_add(1);
                if !self.is_active_visible_pane(session, pane) {
                    let unread = self.unread_counts.entry(pane).or_default();
                    *unread = unread.saturating_add(1);
                    let attention = self.attention.entry(pane).or_default();
                    *attention = attention.on_output(false);
                }
            }
            let lifecycle = runtime_lifecycle(projection.state());
            let _ = self.catalog.set_generation_lifecycle(
                session,
                pane,
                projection.generation(),
                lifecycle,
            );
            batch.changed.insert(pane);
        }
        if !batch.changed.is_empty() {
            self.dirty = true;
        }
        batch
    }

    /// Returns the newest immutable bounded screen projection.
    ///
    /// # Errors
    ///
    /// Returns hierarchy or snapshot-bound errors.
    pub fn snapshot(&self, pane: PaneId) -> Result<PaneScreenSnapshot, ServiceError> {
        let (_, _, definition) = self.catalog.pane(pane).ok_or(CatalogError::PaneNotFound)?;
        let Some(terminal) = self.terminal_panes.get(&pane).copied() else {
            return self
                .historical
                .get(&pane)
                .cloned()
                .ok_or(ServiceError::StaleGeneration);
        };
        let projection = self
            .runtime
            .projection(terminal)
            .ok_or(ServiceError::StaleGeneration)?;
        Ok(PaneScreenSnapshot::from_terminal(
            projection.snapshot(),
            self.output_revisions.get(&pane).copied().unwrap_or(0),
            definition.generation(),
            definition.lifecycle().clone(),
            self.unread_counts.get(&pane).copied().unwrap_or(0),
            self.attention.get(&pane).copied().unwrap_or_default(),
        )?)
    }

    #[must_use]
    pub fn should_exit(&self) -> bool {
        self.attached_clients == 0
            && self.runtime.running_processes() == 0
            && self.detached_since.elapsed() >= self.idle_timeout
    }

    /// Takes a stopped-only persistence projection after service changes.
    ///
    /// # Errors
    ///
    /// Returns catalog or snapshot validation errors.
    pub fn take_persistence(&mut self) -> Result<Option<PersistedCatalog>, ServiceError> {
        if !self.dirty {
            return Ok(None);
        }
        let mut histories = self
            .historical
            .iter()
            .map(|(pane, screen)| (*pane, PaneHistorySnapshot::new(*pane, screen.clone())))
            .collect::<BTreeMap<_, _>>();
        for (pane, terminal) in &self.terminal_panes {
            let Some(projection) = self.runtime.projection(*terminal) else {
                continue;
            };
            let screen = PaneScreenSnapshot::from_terminal(
                projection.snapshot(),
                self.output_revisions.get(pane).copied().unwrap_or(0),
                0,
                PaneLifecycle::Stopped,
                self.unread_counts.get(pane).copied().unwrap_or(0),
                self.attention.get(pane).copied().unwrap_or_default(),
            )?;
            histories.insert(*pane, PaneHistorySnapshot::new(*pane, screen));
        }
        let record = PersistedCatalog::new(&self.catalog, histories.into_values().collect())?;
        self.dirty = false;
        Ok(Some(record))
    }

    fn ensure_runtime_pane(&mut self, pane: PaneId) -> TerminalPaneId {
        if let Some(terminal) = self.terminal_panes.get(&pane) {
            return *terminal;
        }
        let terminal = TerminalPaneId::new();
        self.terminal_panes.insert(pane, terminal);
        self.session_panes.insert(terminal, pane);
        terminal
    }

    fn runtime_pane(&self, pane: PaneId) -> Result<TerminalPaneId, ServiceError> {
        self.terminal_panes
            .get(&pane)
            .copied()
            .ok_or(ServiceError::StaleGeneration)
    }

    fn matches_generation(&self, session: SessionId, pane: PaneId, generation: u64) -> bool {
        self.catalog
            .pane(pane)
            .is_some_and(|(owner, _, definition)| {
                owner == session && definition.generation() == generation
            })
    }

    fn expect_instance(&self, service_instance: ServiceInstanceId) -> Result<(), ServiceError> {
        if service_instance == self.service_instance {
            Ok(())
        } else {
            Err(ServiceError::StaleServiceInstance)
        }
    }

    fn is_active_visible_pane(&self, session: SessionId, pane: PaneId) -> bool {
        self.attached_clients > 0
            && self.catalog.active_session_id() == Some(session)
            && self
                .catalog
                .session(session)
                .and_then(|session| session.active_window())
                .is_some_and(|window| window.focused_pane().id() == pane)
    }
}

pub struct ServiceStartJob {
    service_instance: ServiceInstanceId,
    session: SessionId,
    pane: PaneId,
    generation: u64,
    runtime_job: RuntimeStartJob,
}

impl ServiceStartJob {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn run(self) -> ServiceStartCompletion {
        let runtime_pane = self.runtime_job.pane();
        ServiceStartCompletion {
            service_instance: self.service_instance,
            session: self.session,
            pane: self.pane,
            generation: self.generation,
            runtime_pane,
            result: self.runtime_job.run(),
        }
    }
}

pub struct ServiceStartCompletion {
    service_instance: ServiceInstanceId,
    session: SessionId,
    pane: PaneId,
    generation: u64,
    runtime_pane: TerminalPaneId,
    result: Result<Box<dyn strukt_terminal::TerminalProcess>, String>,
}

pub struct ServiceTerminateJob {
    service_instance: ServiceInstanceId,
    session: SessionId,
    pane: PaneId,
    generation: u64,
    runtime_job: RuntimeTerminateJob,
}

impl ServiceTerminateJob {
    #[must_use]
    pub fn run(self) -> ServiceTerminateCompletion {
        let runtime_pane = self.runtime_job.pane();
        ServiceTerminateCompletion {
            service_instance: self.service_instance,
            session: self.session,
            pane: self.pane,
            generation: self.generation,
            runtime_pane,
            runtime_completion: self.runtime_job.run(),
        }
    }
}

pub struct ServiceTerminateCompletion {
    service_instance: ServiceInstanceId,
    session: SessionId,
    pane: PaneId,
    generation: u64,
    runtime_pane: TerminalPaneId,
    runtime_completion: RuntimeTerminateCompletion,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceBatch {
    changed: BTreeSet<PaneId>,
}

impl ServiceBatch {
    #[must_use]
    pub const fn changed_panes(&self) -> &BTreeSet<PaneId> {
        &self.changed
    }
}

fn runtime_lifecycle(state: &RuntimePaneState) -> PaneLifecycle {
    match state {
        RuntimePaneState::Stopped => PaneLifecycle::Stopped,
        RuntimePaneState::Starting => PaneLifecycle::Starting,
        RuntimePaneState::Running => PaneLifecycle::Running,
        RuntimePaneState::Exited { code } => PaneLifecycle::Exited { code: *code },
        RuntimePaneState::Failed { message } => PaneLifecycle::Failed {
            message: message.clone(),
        },
        RuntimePaneState::Backpressured => PaneLifecycle::Backpressured,
    }
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("session service instance is stale")]
    StaleServiceInstance,
    #[error("session pane generation is stale")]
    StaleGeneration,
    #[error("session and terminal generations diverged")]
    GenerationDiverged,
    #[error("session start request does not match its pane definition")]
    InvalidStartRequest,
    #[error("a controlling session client is already attached")]
    WriterAlreadyAttached,
    #[error("no controlling session client is attached")]
    NoAttachedClient,
    #[error("session wire request is invalid")]
    InvalidWireRequest,
    #[error("repository session fixture is unavailable")]
    FixtureUnavailable,
    #[error("running panes prevent service shutdown")]
    RunningPanesPreventShutdown,
    #[error("stopped catalog import requires an empty service")]
    ImportTargetNotEmpty,
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    TransportRequest(#[from] strukt_terminal::TransportError),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error(transparent)]
    Store(#[from] crate::SessionStoreError),
}
