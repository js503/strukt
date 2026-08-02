use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{
    FocusEvent, GridSize, MouseEvent, OutputChunk, PasteDecision, Selection, SelectionError,
    SpawnRequest, TerminalKey, TerminalLink, TerminalModel, TerminalPaneId, TerminalProcess,
    TerminalSize, TerminalSnapshot, TerminalTransport, TransportError,
};

const MAX_PENDING_BYTES: usize = 4 * 1024 * 1024;
const MAX_PENDING_CHUNKS: usize = 1024;
const DEFAULT_PANE_BUDGET: usize = 256 * 1024;
const DEFAULT_AGGREGATE_BUDGET: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainBudget {
    per_pane_bytes: usize,
    aggregate_bytes: usize,
}

impl DrainBudget {
    #[must_use]
    pub const fn new(per_pane_bytes: usize, aggregate_bytes: usize) -> Self {
        Self {
            per_pane_bytes,
            aggregate_bytes,
        }
    }
}

impl Default for DrainBudget {
    fn default() -> Self {
        Self::new(DEFAULT_PANE_BUDGET, DEFAULT_AGGREGATE_BUDGET)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePaneState {
    Stopped,
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { message: String },
    Backpressured,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimePaneHealth {
    pub backpressured: bool,
    pub sustained_output: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeBatch {
    bytes: BTreeMap<TerminalPaneId, usize>,
    changed: BTreeSet<TerminalPaneId>,
}

impl RuntimeBatch {
    #[must_use]
    pub fn bytes_for(&self, pane: TerminalPaneId) -> usize {
        self.bytes.get(&pane).copied().unwrap_or(0)
    }

    #[must_use]
    pub const fn changed_panes(&self) -> &BTreeSet<TerminalPaneId> {
        &self.changed
    }
}

struct PendingOutput {
    bytes: Vec<u8>,
    offset: usize,
}

struct PaneRuntime {
    generation: u64,
    process: Option<Box<dyn TerminalProcess>>,
    model: TerminalModel,
    state: RuntimePaneState,
    pending: VecDeque<PendingOutput>,
    pending_bytes: usize,
    last_sequence: Option<u64>,
    pending_since: Option<Instant>,
    output_since: Option<Instant>,
    last_output_at: Option<Instant>,
    transport_pressure_since: Option<Instant>,
    last_error: Option<String>,
}

pub struct TerminalRuntime {
    transport: Arc<dyn TerminalTransport>,
    panes: BTreeMap<TerminalPaneId, PaneRuntime>,
    scrollback_limit: usize,
    round_robin_cursor: usize,
}

pub struct RuntimeStartJob {
    pane: TerminalPaneId,
    generation: u64,
    transport: Arc<dyn TerminalTransport>,
    request: SpawnRequest,
    prior_process: Option<Box<dyn TerminalProcess>>,
}

impl RuntimeStartJob {
    #[must_use]
    pub const fn pane(&self) -> TerminalPaneId {
        self.pane
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Terminates the prior generation and spawns the requested process.
    ///
    /// # Errors
    ///
    /// Returns the platform termination or spawn failure as a pane-local message.
    pub fn run(mut self) -> Result<Box<dyn TerminalProcess>, String> {
        if let Some(process) = &mut self.prior_process {
            process
                .terminate(Duration::from_millis(500))
                .map_err(|error| error.to_string())?;
        }
        self.transport
            .spawn(self.request)
            .map_err(|error| error.to_string())
    }
}

impl TerminalRuntime {
    #[must_use]
    pub fn new(transport: Arc<dyn TerminalTransport>, scrollback_limit: usize) -> Self {
        Self {
            transport,
            panes: BTreeMap::new(),
            scrollback_limit,
            round_robin_cursor: 0,
        }
    }

    pub fn prepare(&mut self, pane: TerminalPaneId, size: GridSize) -> u64 {
        let generation = self
            .panes
            .get(&pane)
            .map_or(1, |runtime| runtime.generation.saturating_add(1));
        self.panes.insert(
            pane,
            PaneRuntime::new(generation, TerminalModel::new(size, self.scrollback_limit)),
        );
        generation
    }

    /// Starts a fresh generation for a pane and discards prior runtime output.
    ///
    /// # Errors
    ///
    /// Returns a transport or size error. A failed pane remains independently
    /// inspectable and does not affect other pane runtimes.
    pub fn restart(
        &mut self,
        pane: TerminalPaneId,
        request: SpawnRequest,
    ) -> Result<u64, RuntimeError> {
        let job = self.begin_restart(pane, request)?;
        let generation = job.generation();
        let result = job.run();
        self.finish_restart(pane, generation, result)?;
        Ok(generation)
    }

    /// Moves all potentially blocking process work into an executable start job.
    ///
    /// # Errors
    ///
    /// Returns a size error before mutating the pane runtime.
    pub fn begin_restart(
        &mut self,
        pane: TerminalPaneId,
        request: SpawnRequest,
    ) -> Result<RuntimeStartJob, RuntimeError> {
        let size = grid_size(request.size)?;
        let generation = self
            .panes
            .get(&pane)
            .map_or(1, |runtime| runtime.generation.saturating_add(1));
        let prior_process = self
            .panes
            .get_mut(&pane)
            .and_then(|runtime| runtime.process.take());
        let mut runtime =
            PaneRuntime::new(generation, TerminalModel::new(size, self.scrollback_limit));
        runtime.state = RuntimePaneState::Starting;
        self.panes.insert(pane, runtime);
        Ok(RuntimeStartJob {
            pane,
            generation,
            transport: Arc::clone(&self.transport),
            request,
            prior_process,
        })
    }

    /// Installs a completed start only when its pane generation is still current.
    ///
    /// # Errors
    ///
    /// Returns a pane-local start error. Stale completions are discarded safely.
    pub fn finish_restart(
        &mut self,
        pane: TerminalPaneId,
        generation: u64,
        result: Result<Box<dyn TerminalProcess>, String>,
    ) -> Result<(), RuntimeError> {
        let current = self
            .panes
            .get(&pane)
            .is_some_and(|runtime| runtime.generation == generation);
        if !current {
            drop(result);
            return Ok(());
        }
        let runtime = self
            .panes
            .get_mut(&pane)
            .ok_or(RuntimeError::PaneNotFound)?;
        match result {
            Ok(process) => {
                runtime.process = Some(process);
                runtime.state = RuntimePaneState::Running;
                runtime.last_error = None;
                Ok(())
            }
            Err(message) => {
                runtime.state = RuntimePaneState::Failed {
                    message: message.clone(),
                };
                runtime.last_error = Some(message.clone());
                Err(RuntimeError::StartFailed(message))
            }
        }
    }

    /// Queues a generation-scoped output chunk for bounded parsing.
    ///
    /// Stale generations are deliberately ignored and return success.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PaneNotFound`], [`RuntimeError::OutOfOrder`], or
    /// [`RuntimeError::QueueFull`] for invalid current-generation events.
    pub fn apply_output(
        &mut self,
        pane: TerminalPaneId,
        generation: u64,
        chunk: OutputChunk,
    ) -> Result<(), RuntimeError> {
        let runtime = self
            .panes
            .get_mut(&pane)
            .ok_or(RuntimeError::PaneNotFound)?;
        if runtime.generation != generation {
            return Ok(());
        }
        runtime.enqueue(chunk)
    }

    #[must_use]
    pub fn drain(&mut self, budget: DrainBudget) -> RuntimeBatch {
        let pane_ids = self.panes.keys().copied().collect::<Vec<_>>();
        let mut batch = RuntimeBatch::default();
        if pane_ids.is_empty() || budget.aggregate_bytes == 0 || budget.per_pane_bytes == 0 {
            return batch;
        }
        let start = self.round_robin_cursor % pane_ids.len();
        let mut aggregate_remaining = budget.aggregate_bytes;

        for offset in 0..pane_ids.len() {
            if aggregate_remaining == 0 {
                break;
            }
            let pane = pane_ids[(start + offset) % pane_ids.len()];
            let Some(runtime) = self.panes.get_mut(&pane) else {
                continue;
            };
            let state_before = runtime.state.clone();
            runtime.poll_one_output();
            let consumed = runtime.drain_bytes(budget.per_pane_bytes.min(aggregate_remaining));
            aggregate_remaining = aggregate_remaining.saturating_sub(consumed);
            if consumed > 0 {
                batch.bytes.insert(pane, consumed);
                batch.changed.insert(pane);
            }
            if runtime.poll_exit() {
                batch.changed.insert(pane);
            }
            runtime.update_pressure_state();
            if runtime.state != state_before {
                batch.changed.insert(pane);
            }
        }
        self.round_robin_cursor = (start + 1) % pane_ids.len();
        batch
    }

    #[must_use]
    pub fn generation(&self, pane: TerminalPaneId) -> Option<u64> {
        self.panes.get(&pane).map(|runtime| runtime.generation)
    }

    #[must_use]
    pub fn running_processes(&self) -> usize {
        self.panes
            .values()
            .filter(|runtime| runtime.process.is_some())
            .count()
    }

    #[must_use]
    pub fn snapshot(&self, pane: TerminalPaneId) -> Option<TerminalSnapshot> {
        self.snapshot_at(pane, 0)
    }

    #[must_use]
    pub fn snapshot_at(
        &self,
        pane: TerminalPaneId,
        viewport_offset: usize,
    ) -> Option<TerminalSnapshot> {
        self.panes
            .get(&pane)
            .map(|runtime| runtime.model.snapshot(viewport_offset))
    }

    /// Copies selected visible text through the pane's bounded model.
    ///
    /// # Errors
    ///
    /// Returns a missing-pane or selection-bounds error.
    pub fn copy_text(
        &self,
        pane: TerminalPaneId,
        selection: &Selection,
    ) -> Result<String, RuntimeError> {
        self.copy_text_at(pane, 0, selection)
    }

    /// Copies selected text from a specific bounded pane viewport.
    ///
    /// # Errors
    ///
    /// Returns a missing-pane or selection-bounds error.
    pub fn copy_text_at(
        &self,
        pane: TerminalPaneId,
        viewport_offset: usize,
        selection: &Selection,
    ) -> Result<String, RuntimeError> {
        self.panes
            .get(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .model
            .copy_text_at(viewport_offset, selection)
            .map_err(RuntimeError::Selection)
    }

    /// Encodes a semantic key using the pane's current terminal modes.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PaneNotFound`] for an unknown pane.
    pub fn encode_key(
        &self,
        pane: TerminalPaneId,
        key: TerminalKey,
    ) -> Result<Vec<u8>, RuntimeError> {
        Ok(self
            .panes
            .get(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .model
            .encode_key(key))
    }

    /// Encodes a focus transition using the pane's current reporting mode.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PaneNotFound`] for an unknown pane.
    pub fn encode_focus(
        &self,
        pane: TerminalPaneId,
        event: FocusEvent,
    ) -> Result<Option<Vec<u8>>, RuntimeError> {
        Ok(self
            .panes
            .get(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .model
            .encode_focus(event))
    }

    /// Encodes a pointer transition using the pane's current reporting mode.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PaneNotFound`] for an unknown pane.
    pub fn encode_mouse(
        &self,
        pane: TerminalPaneId,
        event: MouseEvent,
    ) -> Result<Option<Vec<u8>>, RuntimeError> {
        Ok(self
            .panes
            .get(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .model
            .encode_mouse(event))
    }

    /// Applies paste sanitization, size confirmation, and bracketed-paste policy.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PaneNotFound`] for an unknown pane.
    pub fn prepare_paste(
        &self,
        pane: TerminalPaneId,
        text: &str,
        confirmed: bool,
    ) -> Result<PasteDecision, RuntimeError> {
        Ok(self
            .panes
            .get(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .model
            .prepare_paste(text, confirmed))
    }

    /// Discovers links in the pane snapshot without opening them.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PaneNotFound`] for an unknown pane.
    pub fn links(&self, pane: TerminalPaneId) -> Result<Vec<TerminalLink>, RuntimeError> {
        Ok(self
            .panes
            .get(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .model
            .links()
            .collect())
    }

    #[must_use]
    pub fn state(&self, pane: TerminalPaneId) -> Option<&RuntimePaneState> {
        self.panes.get(&pane).map(|runtime| &runtime.state)
    }

    #[must_use]
    pub fn health(&self, pane: TerminalPaneId) -> Option<RuntimePaneHealth> {
        self.panes.get(&pane).map(PaneRuntime::health)
    }

    #[must_use]
    pub fn last_error(&self, pane: TerminalPaneId) -> Option<&str> {
        self.panes
            .get(&pane)
            .and_then(|runtime| runtime.last_error.as_deref())
    }

    /// Writes input to a running pane.
    ///
    /// # Errors
    ///
    /// Returns a missing-process or transport error local to the pane.
    pub fn write(&mut self, pane: TerminalPaneId, bytes: &[u8]) -> Result<(), RuntimeError> {
        let result = self.process_mut(pane)?.write(bytes);
        self.record_transport_result(pane, result)
    }

    /// Resizes a running pane and its model after the native adapter succeeds.
    ///
    /// # Errors
    ///
    /// Returns a missing-process, validation, or transport error local to the
    /// pane. The prior model remains usable on failure.
    pub fn resize(&mut self, pane: TerminalPaneId, size: TerminalSize) -> Result<(), RuntimeError> {
        let grid_size = grid_size(size)?;
        let result = self.process_mut(pane)?.resize(size);
        self.record_transport_result(pane, result)?;
        if let Some(runtime) = self.panes.get_mut(&pane) {
            runtime.model.resize(grid_size);
        }
        Ok(())
    }

    /// Terminates a running pane.
    ///
    /// # Errors
    ///
    /// Returns a missing-process or transport error local to the pane.
    pub fn terminate(&mut self, pane: TerminalPaneId, grace: Duration) -> Result<(), RuntimeError> {
        let result = self.process_mut(pane)?.terminate(grace);
        self.record_transport_result(pane, result)
    }

    /// Removes a stopped or terminated pane runtime from memory.
    pub fn discard(&mut self, pane: TerminalPaneId) {
        self.panes.remove(&pane);
    }

    /// Removes a pane runtime and returns its live process for background cleanup.
    pub fn take_process_and_discard(
        &mut self,
        pane: TerminalPaneId,
    ) -> Option<Box<dyn TerminalProcess>> {
        self.panes.remove(&pane).and_then(|runtime| runtime.process)
    }

    fn process_mut(
        &mut self,
        pane: TerminalPaneId,
    ) -> Result<&mut Box<dyn TerminalProcess>, RuntimeError> {
        self.panes
            .get_mut(&pane)
            .ok_or(RuntimeError::PaneNotFound)?
            .process
            .as_mut()
            .ok_or(RuntimeError::ProcessNotRunning)
    }

    fn record_transport_result(
        &mut self,
        pane: TerminalPaneId,
        result: Result<(), TransportError>,
    ) -> Result<(), RuntimeError> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                if let Some(runtime) = self.panes.get_mut(&pane) {
                    runtime.last_error = Some(error.to_string());
                }
                Err(RuntimeError::Transport(error))
            }
        }
    }
}

impl PaneRuntime {
    fn new(generation: u64, model: TerminalModel) -> Self {
        Self {
            generation,
            process: None,
            model,
            state: RuntimePaneState::Stopped,
            pending: VecDeque::new(),
            pending_bytes: 0,
            last_sequence: None,
            pending_since: None,
            output_since: None,
            last_output_at: None,
            transport_pressure_since: None,
            last_error: None,
        }
    }

    fn enqueue(&mut self, chunk: OutputChunk) -> Result<(), RuntimeError> {
        if self
            .last_sequence
            .is_some_and(|sequence| chunk.sequence() <= sequence)
        {
            return Err(RuntimeError::OutOfOrder);
        }
        if self.pending.len() >= MAX_PENDING_CHUNKS
            || self.pending_bytes.saturating_add(chunk.bytes().len()) > MAX_PENDING_BYTES
        {
            return Err(RuntimeError::QueueFull);
        }
        self.last_sequence = Some(chunk.sequence());
        self.pending_bytes += chunk.bytes().len();
        self.pending.push_back(PendingOutput {
            bytes: chunk.into_bytes(),
            offset: 0,
        });
        let now = Instant::now();
        self.pending_since.get_or_insert(now);
        if self
            .last_output_at
            .is_none_or(|last| now.duration_since(last) > Duration::from_millis(100))
        {
            self.output_since = Some(now);
        }
        self.last_output_at = Some(now);
        Ok(())
    }

    fn poll_one_output(&mut self) {
        let result = self
            .process
            .as_mut()
            .map_or(Ok(None), |process| process.try_read());
        match result {
            Ok(Some(chunk)) => {
                if let Err(error) = self.enqueue(chunk) {
                    self.fail(error.to_string());
                }
            }
            Ok(None) => {}
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn drain_bytes(&mut self, limit: usize) -> usize {
        let mut consumed = 0;
        while consumed < limit {
            if self.pending.is_empty() {
                self.poll_one_output();
            }
            let Some(front) = self.pending.front_mut() else {
                break;
            };
            let available = front.bytes.len() - front.offset;
            let count = available.min(limit - consumed);
            self.model
                .advance(&front.bytes[front.offset..front.offset + count]);
            front.offset += count;
            consumed += count;
            self.pending_bytes = self.pending_bytes.saturating_sub(count);
            if front.offset == front.bytes.len() {
                self.pending.pop_front();
            }
        }
        if self.pending.is_empty() {
            self.pending_since = None;
            if self.state == RuntimePaneState::Backpressured {
                self.state = RuntimePaneState::Running;
            }
        }
        consumed
    }

    fn poll_exit(&mut self) -> bool {
        let result = self
            .process
            .as_mut()
            .map_or(Ok(None), |process| process.try_wait());
        match result {
            Ok(Some(status)) => {
                self.state = RuntimePaneState::Exited {
                    code: status.code(),
                };
                self.process = None;
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.fail(error.to_string());
                true
            }
        }
    }

    fn update_pressure_state(&mut self) {
        if self
            .last_output_at
            .is_some_and(|last| last.elapsed() > Duration::from_millis(100))
        {
            self.output_since = None;
            self.last_output_at = None;
        }
        let transport_blocked = self
            .process
            .as_ref()
            .is_some_and(|process| process.output_backpressured());
        if transport_blocked {
            self.transport_pressure_since
                .get_or_insert_with(Instant::now);
        } else {
            self.transport_pressure_since = None;
        }
        if self
            .pending_since
            .is_some_and(|since| since.elapsed() >= Duration::from_millis(250))
            || self
                .transport_pressure_since
                .is_some_and(|since| since.elapsed() >= Duration::from_millis(250))
        {
            self.state = RuntimePaneState::Backpressured;
        } else if self.state == RuntimePaneState::Backpressured {
            self.state = RuntimePaneState::Running;
        }
    }

    fn health(&self) -> RuntimePaneHealth {
        RuntimePaneHealth {
            backpressured: self.state == RuntimePaneState::Backpressured,
            sustained_output: self
                .output_since
                .is_some_and(|since| since.elapsed() >= Duration::from_secs(2)),
        }
    }

    fn fail(&mut self, message: String) {
        self.last_error = Some(message.clone());
        self.state = RuntimePaneState::Failed { message };
        self.process = None;
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum RuntimeError {
    #[error("terminal pane runtime was not found")]
    PaneNotFound,
    #[error("terminal pane process is not running")]
    ProcessNotRunning,
    #[error("terminal output sequence is stale or out of order")]
    OutOfOrder,
    #[error("terminal pane output queue is full")]
    QueueFull,
    #[error("terminal process failed to start: {0}")]
    StartFailed(String),
    #[error(transparent)]
    Selection(#[from] SelectionError),
    #[error(transparent)]
    Transport(#[from] TransportError),
}

fn grid_size(size: TerminalSize) -> Result<GridSize, RuntimeError> {
    GridSize::new(usize::from(size.rows()), usize::from(size.columns()))
        .map_err(|_| RuntimeError::Transport(TransportError::InvalidSize))
}
