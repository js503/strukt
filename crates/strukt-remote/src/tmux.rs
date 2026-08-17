use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use strukt_session::{
    AttentionState, PaneLifecycle, PaneScreenSnapshot, ProviderCapabilities,
    ProviderCatalogSnapshot, ProviderError, ProviderKind, RequestBody as SessionRequest,
    RequestEnvelope as SessionRequestEnvelope, ResponseBody as SessionResponse,
    ResponseEnvelope as SessionResponseEnvelope, ServiceInstanceId, SessionCatalog,
};
use strukt_terminal::{GridSize, SplitAxis, TerminalModel};
use thiserror::Error;

use crate::{PersistentProvider, SessionPayload, read_frame, write_frame};

const MAX_DISCOVERY_BYTES: usize = 1024 * 1024;
const MAX_CONTROL_RECORD_BYTES: usize = 64 * 1024;
const MAX_RECORDS: usize = 1024;
const MAX_NAME_BYTES: usize = 256;
const MAX_INPUT_BYTES: usize = 256 * 1024;
const MAX_INPUT_CHUNK_BYTES: usize = 4 * 1024;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmuxCommand {
    program: PathBuf,
    arguments: Vec<String>,
}

impl TmuxCommand {
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TmuxTarget {
    raw: String,
    kind: TargetKind,
}

impl TmuxTarget {
    /// Creates a validated tmux session target.
    ///
    /// # Errors
    ///
    /// Returns an error unless the target is `$` followed by decimal digits.
    pub fn session(raw: impl Into<String>) -> Result<Self, TmuxError> {
        Self::new(raw.into(), TargetKind::Session)
    }

    /// Creates a validated tmux pane target.
    ///
    /// # Errors
    ///
    /// Returns an error unless the target is `%` followed by decimal digits.
    pub fn pane(raw: impl Into<String>) -> Result<Self, TmuxError> {
        Self::new(raw.into(), TargetKind::Pane)
    }

    fn window(raw: impl Into<String>) -> Result<Self, TmuxError> {
        Self::new(raw.into(), TargetKind::Window)
    }

    fn new(raw: String, kind: TargetKind) -> Result<Self, TmuxError> {
        let prefix = match kind {
            TargetKind::Session => '$',
            TargetKind::Window => '@',
            TargetKind::Pane => '%',
        };
        if raw.len() < 2
            || raw.len() > 32
            || !raw.starts_with(prefix)
            || !raw[1..].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(TmuxError::InvalidTarget);
        }
        Ok(Self { raw, kind })
    }

    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum TargetKind {
    Session,
    Window,
    Pane,
}

#[derive(Clone, Debug)]
pub struct TmuxProvider {
    executable: PathBuf,
    server_name: Option<String>,
}

impl TmuxProvider {
    /// Creates a provider for an exact executable path.
    ///
    /// # Errors
    ///
    /// Returns an error for a relative or traversing path.
    pub fn new(executable: PathBuf) -> Result<Self, TmuxError> {
        if !executable.is_absolute()
            || executable
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(TmuxError::InvalidExecutable);
        }
        Ok(Self {
            executable,
            server_name: None,
        })
    }

    /// Selects an isolated tmux server name without shell interpolation.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty, oversized, or non-ASCII-safe name.
    pub fn with_server_name(mut self, server_name: impl Into<String>) -> Result<Self, TmuxError> {
        let server_name = server_name.into();
        if server_name.is_empty()
            || server_name.len() > 64
            || !server_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(TmuxError::InvalidServerName);
        }
        self.server_name = Some(server_name);
        Ok(self)
    }

    #[must_use]
    pub fn discover_executable() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|directory| directory.join("tmux"))
            .find(|candidate| candidate.is_file())
    }

    /// Executes bounded discovery with the exact configured program and argv.
    ///
    /// # Errors
    ///
    /// Returns process, exit, or output validation errors.
    pub fn discover(&self) -> Result<TmuxCatalog, TmuxError> {
        let output = execute_bounded(&self.discovery_command(), MAX_DISCOVERY_BYTES)?;
        self.parse_discovery(&output)
    }

    /// Sends literal bytes to one validated tmux pane.
    ///
    /// # Errors
    ///
    /// Returns validation, process, or bounded-output errors.
    pub fn send_input(&self, pane: &TmuxTarget, bytes: &[u8]) -> Result<(), TmuxError> {
        if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
            return Err(TmuxError::InvalidInput);
        }
        for chunk in bytes.chunks(MAX_INPUT_CHUNK_BYTES) {
            execute_bounded(&self.input_command(pane, chunk)?, MAX_CONTROL_RECORD_BYTES)?;
        }
        Ok(())
    }

    /// Resizes one validated tmux pane.
    ///
    /// # Errors
    ///
    /// Returns validation, process, or bounded-output errors.
    pub fn resize(&self, pane: &TmuxTarget, rows: u16, columns: u16) -> Result<(), TmuxError> {
        execute_bounded(
            &self.resize_command(pane, rows, columns)?,
            MAX_CONTROL_RECORD_BYTES,
        )?;
        Ok(())
    }

    /// Captures bounded visible/history text for one validated tmux pane.
    ///
    /// # Errors
    ///
    /// Returns process or bounded-output errors.
    pub fn capture(&self, pane: &TmuxTarget) -> Result<Vec<u8>, TmuxError> {
        execute_bounded(&self.capture_command(pane), MAX_DISCOVERY_BYTES)
    }

    #[must_use]
    pub fn discovery_command(&self) -> TmuxCommand {
        self.command([
            "list-panes",
            "-a",
            "-F",
            "#{session_id}\x1f#{session_name}\x1f#{window_id}\x1f#{window_name}\x1f#{pane_id}\x1f#{pane_active}\x1f#{pane_width}\x1f#{pane_height}",
        ])
    }

    /// Parses bounded machine-formatted discovery records.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid IDs, fields, dimensions, or bounds.
    pub fn parse_discovery(&self, output: &[u8]) -> Result<TmuxCatalog, TmuxError> {
        if output.len() > MAX_DISCOVERY_BYTES {
            return Err(TmuxError::OutputTooLarge);
        }
        let text = std::str::from_utf8(output).map_err(|_| TmuxError::MalformedOutput)?;
        let mut catalog = TmuxCatalog::default();
        for (index, line) in text.lines().enumerate() {
            if index >= MAX_RECORDS || line.len() > MAX_CONTROL_RECORD_BYTES {
                return Err(TmuxError::OutputTooLarge);
            }
            if line.is_empty() {
                continue;
            }
            // tmux renders the unit separator in format output as its escaped
            // octal form, keeping record framing printable over stdio.
            let fields: Vec<_> = line.split("\\037").collect();
            let [
                session_id,
                session_name,
                window_id,
                window_name,
                pane_id,
                active,
                width,
                height,
            ] = fields.as_slice()
            else {
                return Err(TmuxError::MalformedOutput);
            };
            let session_target = TmuxTarget::session((*session_id).to_owned())?;
            let window_target = TmuxTarget::window((*window_id).to_owned())?;
            let pane_target = TmuxTarget::pane((*pane_id).to_owned())?;
            let session_name = bounded_name(session_name)?;
            let window_name = bounded_name(window_name)?;
            let active = match *active {
                "0" => false,
                "1" => true,
                _ => return Err(TmuxError::MalformedOutput),
            };
            let width = parse_dimension(width)?;
            let height = parse_dimension(height)?;
            let session = find_or_insert_session(&mut catalog, session_target, session_name);
            let window = find_or_insert_window(session, window_target, window_name);
            if window
                .panes
                .iter()
                .any(|pane| pane.target.raw == pane_target.raw)
            {
                return Err(TmuxError::DuplicateTarget);
            }
            window.panes.push(TmuxPane {
                target: pane_target,
                active,
                width,
                height,
            });
        }
        catalog.sort();
        Ok(catalog)
    }

    #[must_use]
    pub fn attach_command(&self, session: &TmuxTarget) -> TmuxCommand {
        debug_assert_eq!(session.kind, TargetKind::Session);
        self.command(["-C", "attach-session", "-t", session.raw()])
    }

    /// Builds a literal byte-input command using tmux hexadecimal key mode.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong target or an empty/oversized input.
    pub fn input_command(&self, pane: &TmuxTarget, bytes: &[u8]) -> Result<TmuxCommand, TmuxError> {
        if pane.kind != TargetKind::Pane || bytes.is_empty() || bytes.len() > MAX_INPUT_CHUNK_BYTES
        {
            return Err(TmuxError::InvalidInput);
        }
        let mut arguments = vec![
            "send-keys".to_owned(),
            "-t".to_owned(),
            pane.raw.clone(),
            "-H".to_owned(),
        ];
        arguments.extend(bytes.iter().map(|byte| format!("{byte:02x}")));
        Ok(self.command(arguments))
    }

    /// Builds a validated exact pane resize command.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong target or zero dimensions.
    pub fn resize_command(
        &self,
        pane: &TmuxTarget,
        rows: u16,
        columns: u16,
    ) -> Result<TmuxCommand, TmuxError> {
        if pane.kind != TargetKind::Pane || rows == 0 || columns == 0 {
            return Err(TmuxError::InvalidDimensions);
        }
        Ok(self.command([
            "resize-pane".to_owned(),
            "-t".to_owned(),
            pane.raw.clone(),
            "-x".to_owned(),
            columns.to_string(),
            "-y".to_owned(),
            rows.to_string(),
        ]))
    }

    #[must_use]
    pub fn capture_command(&self, pane: &TmuxTarget) -> TmuxCommand {
        debug_assert_eq!(pane.kind, TargetKind::Pane);
        self.command(["capture-pane", "-p", "-e", "-S", "-1000", "-t", pane.raw()])
    }

    fn command<I, S>(&self, arguments: I) -> TmuxCommand
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut fixed = Vec::new();
        if let Some(server_name) = &self.server_name {
            fixed.extend(["-L".to_owned(), server_name.clone()]);
        }
        fixed.extend(arguments.into_iter().map(Into::into));
        TmuxCommand {
            program: self.executable.clone(),
            arguments: fixed,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TmuxCatalog {
    sessions: Vec<TmuxSession>,
}

impl TmuxCatalog {
    #[must_use]
    pub fn sessions(&self) -> &[TmuxSession] {
        &self.sessions
    }

    fn sort(&mut self) {
        self.sessions
            .sort_by(|left, right| left.raw_id().cmp(right.raw_id()));
        for session in &mut self.sessions {
            session
                .windows
                .sort_by(|left, right| left.raw_id().cmp(right.raw_id()));
            for window in &mut session.windows {
                window
                    .panes
                    .sort_by(|left, right| left.raw_id().cmp(right.raw_id()));
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TmuxSession {
    target: TmuxTarget,
    name: String,
    windows: Vec<TmuxWindow>,
}

impl TmuxSession {
    #[must_use]
    pub fn raw_id(&self) -> &str {
        self.target.raw()
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn windows(&self) -> &[TmuxWindow] {
        &self.windows
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TmuxWindow {
    target: TmuxTarget,
    name: String,
    panes: Vec<TmuxPane>,
}

impl TmuxWindow {
    #[must_use]
    pub fn raw_id(&self) -> &str {
        self.target.raw()
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn panes(&self) -> &[TmuxPane] {
        &self.panes
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TmuxPane {
    target: TmuxTarget,
    active: bool,
    width: u16,
    height: u16,
}

impl TmuxPane {
    #[must_use]
    pub fn raw_id(&self) -> &str {
        self.target.raw()
    }

    #[must_use]
    pub const fn active(&self) -> bool {
        self.active
    }

    #[must_use]
    pub const fn dimensions(&self) -> (u16, u16) {
        (self.height, self.width)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TmuxControlEvent {
    Output { pane: String, bytes: Vec<u8> },
    LayoutChanged { window: String },
    SessionClosed { session: String },
    Exit,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TmuxRequest {
    Catalog,
    Attach {
        session: String,
    },
    Input {
        pane: String,
        bytes: Vec<u8>,
    },
    Resize {
        pane: String,
        rows: u16,
        columns: u16,
    },
    Snapshot {
        pane: String,
    },
    Detach,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TmuxResponse {
    Catalog(TmuxCatalog),
    Attached { session: String },
    Snapshot { pane: String, bytes: Vec<u8> },
    Acknowledged,
    Detached,
}

pub struct TmuxManager {
    provider: TmuxProvider,
    attached: Mutex<Option<TmuxTarget>>,
    session_bridge: Mutex<TmuxSessionBridge>,
}

impl TmuxManager {
    #[must_use]
    pub fn new(provider: TmuxProvider) -> Self {
        Self {
            provider,
            attached: Mutex::new(None),
            session_bridge: Mutex::new(TmuxSessionBridge::new()),
        }
    }

    /// Exchanges one bounded tmux-provider request.
    ///
    /// # Errors
    ///
    /// Returns framing, validation, process, attachment, or state errors.
    pub fn exchange(&self, payload: &SessionPayload) -> Result<SessionPayload, TmuxError> {
        if payload.provider() != PersistentProvider::Tmux {
            return Err(TmuxError::WrongProvider);
        }
        let request: TmuxRequest =
            read_frame(&mut Cursor::new(payload.bytes()), MAX_DISCOVERY_BYTES)?;
        let response = match request {
            TmuxRequest::Catalog => TmuxResponse::Catalog(self.provider.discover()?),
            TmuxRequest::Attach { session } => {
                let target = TmuxTarget::session(session)?;
                let exists = self
                    .provider
                    .discover()?
                    .sessions()
                    .iter()
                    .any(|item| item.raw_id() == target.raw());
                if !exists {
                    return Err(TmuxError::SessionNotFound);
                }
                *self
                    .attached
                    .lock()
                    .map_err(|_| TmuxError::StateUnavailable)? = Some(target.clone());
                TmuxResponse::Attached {
                    session: target.raw,
                }
            }
            TmuxRequest::Input { pane, bytes } => {
                self.require_attached()?;
                let target = TmuxTarget::pane(pane)?;
                self.provider.send_input(&target, &bytes)?;
                TmuxResponse::Acknowledged
            }
            TmuxRequest::Resize {
                pane,
                rows,
                columns,
            } => {
                self.require_attached()?;
                let target = TmuxTarget::pane(pane)?;
                execute_bounded(
                    &self.provider.resize_command(&target, rows, columns)?,
                    MAX_CONTROL_RECORD_BYTES,
                )?;
                TmuxResponse::Acknowledged
            }
            TmuxRequest::Snapshot { pane } => {
                self.require_attached()?;
                let target = TmuxTarget::pane(pane)?;
                let bytes =
                    execute_bounded(&self.provider.capture_command(&target), MAX_DISCOVERY_BYTES)?;
                TmuxResponse::Snapshot {
                    pane: target.raw,
                    bytes,
                }
            }
            TmuxRequest::Detach => {
                *self
                    .attached
                    .lock()
                    .map_err(|_| TmuxError::StateUnavailable)? = None;
                TmuxResponse::Detached
            }
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &response, MAX_DISCOVERY_BYTES)?;
        SessionPayload::new(PersistentProvider::Tmux, bytes).map_err(|_| TmuxError::OutputTooLarge)
    }

    /// Translates the shared M3 session protocol into capability-limited tmux
    /// discovery, input, resize, snapshot, and detach operations.
    ///
    /// # Errors
    ///
    /// Returns framing, catalog, process, snapshot, or state errors.
    pub fn exchange_session(&self, payload: &SessionPayload) -> Result<SessionPayload, TmuxError> {
        if payload.provider() != PersistentProvider::Tmux {
            return Err(TmuxError::WrongProvider);
        }
        let mut cursor = Cursor::new(payload.bytes());
        let request: SessionRequestEnvelope = read_frame(&mut cursor, MAX_DISCOVERY_BYTES)?;
        if cursor.position() != payload.bytes().len() as u64 || request.validate().is_err() {
            return Err(TmuxError::MalformedSessionRequest);
        }
        let mut bridge = self
            .session_bridge
            .lock()
            .map_err(|_| TmuxError::StateUnavailable)?;
        let body = bridge.handle(&self.provider, request.body().clone());
        let response = match body {
            Ok(body) => SessionResponseEnvelope::ok(request.request_id(), body),
            Err(error) => SessionResponseEnvelope::error(request.request_id(), error),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &response, MAX_DISCOVERY_BYTES)?;
        SessionPayload::new(PersistentProvider::Tmux, bytes).map_err(|_| TmuxError::OutputTooLarge)
    }

    fn require_attached(&self) -> Result<(), TmuxError> {
        if self
            .attached
            .lock()
            .map_err(|_| TmuxError::StateUnavailable)?
            .is_none()
        {
            return Err(TmuxError::NotAttached);
        }
        Ok(())
    }
}

struct TmuxSessionBridge {
    instance: Option<ServiceInstanceId>,
    attached: bool,
    discovered: Option<TmuxCatalog>,
    snapshot: Option<ProviderCatalogSnapshot>,
    panes: HashMap<strukt_session::PaneId, TmuxBridgePane>,
    output_revisions: HashMap<strukt_session::PaneId, u64>,
}

#[derive(Clone)]
struct TmuxBridgePane {
    target: TmuxTarget,
    rows: u16,
    columns: u16,
}

impl TmuxSessionBridge {
    fn new() -> Self {
        Self {
            instance: None,
            attached: false,
            discovered: None,
            snapshot: None,
            panes: HashMap::new(),
            output_revisions: HashMap::new(),
        }
    }

    fn handle(
        &mut self,
        provider: &TmuxProvider,
        request: SessionRequest,
    ) -> Result<SessionResponse, ProviderError> {
        match request {
            SessionRequest::Catalog => {
                self.refresh(provider)?;
                Ok(SessionResponse::Catalog(self.snapshot()?))
            }
            SessionRequest::Attach | SessionRequest::Reconnect { .. } => {
                self.refresh(provider)?;
                self.attached = true;
                Ok(SessionResponse::Attached(self.snapshot()?))
            }
            SessionRequest::Detach => {
                self.attached = false;
                Ok(SessionResponse::Detached)
            }
            SessionRequest::WritePane { pane, bytes, .. } => {
                self.require_attached()?;
                let target = self.pane(pane)?.target.clone();
                provider
                    .send_input(&target, &bytes)
                    .map_err(provider_process_error)?;
                Ok(SessionResponse::PaneWritten)
            }
            SessionRequest::ResizePane {
                pane,
                rows,
                columns,
                ..
            } => {
                self.require_attached()?;
                let target = self.pane(pane)?.target.clone();
                provider
                    .resize(&target, rows, columns)
                    .map_err(provider_process_error)?;
                if let Some(projection) = self.panes.get_mut(&pane) {
                    projection.rows = rows;
                    projection.columns = columns;
                }
                Ok(SessionResponse::PaneResized)
            }
            SessionRequest::Snapshot { pane } => {
                self.require_attached()?;
                let projection = self.pane(pane)?.clone();
                let bytes = provider
                    .capture(&projection.target)
                    .map_err(provider_process_error)?;
                let size = GridSize::new(
                    usize::from(projection.rows),
                    usize::from(projection.columns),
                )
                .map_err(|error| ProviderError::internal(error.to_string()))?;
                let mut terminal = TerminalModel::new(size, 1_000);
                terminal.advance(&bytes);
                let revision = self
                    .output_revisions
                    .entry(pane)
                    .and_modify(|revision| *revision = revision.saturating_add(1))
                    .or_insert(1);
                let snapshot = PaneScreenSnapshot::from_terminal(
                    &terminal.snapshot(0),
                    *revision,
                    1,
                    PaneLifecycle::Running,
                    0,
                    AttentionState::None,
                )
                .map_err(|error| ProviderError::internal(error.to_string()))?;
                Ok(SessionResponse::PaneSnapshot(snapshot))
            }
            _ => Err(ProviderError::InvalidAction),
        }
    }

    fn refresh(&mut self, provider: &TmuxProvider) -> Result<(), ProviderError> {
        let discovered = provider.discover().map_err(provider_process_error)?;
        if self.discovered.as_ref() == Some(&discovered) {
            return Ok(());
        }
        let mut catalog = SessionCatalog::new();
        let mut panes = HashMap::new();
        for (session_index, discovered_session) in discovered.sessions().iter().enumerate() {
            if session_index >= strukt_session::MAX_SESSIONS {
                break;
            }
            let session_name = catalog_name(discovered_session.name(), "tmux session");
            let session = catalog
                .create_session(catalog.revision(), session_name, Path::new("/"))
                .map_err(|error| ProviderError::internal(error.to_string()))?;
            for (window_index, discovered_window) in discovered_session.windows().iter().enumerate()
            {
                let window = if window_index == 0 {
                    catalog
                        .session(session)
                        .and_then(strukt_session::Session::active_window)
                        .map(strukt_session::SessionWindow::id)
                        .ok_or(ProviderError::NotFound)?
                } else {
                    catalog
                        .create_window(
                            catalog.revision(),
                            session,
                            catalog_name(discovered_window.name(), "tmux window"),
                            Path::new("/"),
                        )
                        .map_err(|error| ProviderError::internal(error.to_string()))?
                };
                if window_index == 0 {
                    catalog
                        .rename_window(
                            catalog.revision(),
                            session,
                            window,
                            catalog_name(discovered_window.name(), "tmux window"),
                        )
                        .map_err(|error| ProviderError::internal(error.to_string()))?;
                }
                for (pane_index, discovered_pane) in discovered_window.panes().iter().enumerate() {
                    let pane = if pane_index == 0 {
                        catalog
                            .session(session)
                            .and_then(|item| item.windows().iter().find(|item| item.id() == window))
                            .map(strukt_session::SessionWindow::focused_pane)
                            .map(strukt_session::SessionPane::id)
                            .ok_or(ProviderError::NotFound)?
                    } else {
                        catalog
                            .activate_window(catalog.revision(), session, window)
                            .map_err(|error| ProviderError::internal(error.to_string()))?;
                        catalog
                            .split_focused(catalog.revision(), session, SplitAxis::Vertical)
                            .map_err(|error| ProviderError::internal(error.to_string()))?
                    };
                    let (rows, columns) = discovered_pane.dimensions();
                    let generation = catalog
                        .begin_pane_generation(catalog.revision(), session, pane, rows, columns)
                        .map_err(|error| ProviderError::internal(error.to_string()))?;
                    catalog
                        .set_generation_lifecycle(session, pane, generation, PaneLifecycle::Running)
                        .map_err(|error| ProviderError::internal(error.to_string()))?;
                    panes.insert(
                        pane,
                        TmuxBridgePane {
                            target: TmuxTarget::pane(discovered_pane.raw_id().to_owned())
                                .map_err(provider_process_error)?,
                            rows,
                            columns,
                        },
                    );
                }
            }
        }
        let instance = if let Some(instance) = self.instance {
            instance
        } else {
            let instance = ServiceInstanceId::new()
                .map_err(|error| ProviderError::internal(error.to_string()))?;
            self.instance = Some(instance);
            instance
        };
        self.discovered = Some(discovered);
        self.panes = panes;
        self.snapshot = Some(ProviderCatalogSnapshot::new(
            instance,
            ProviderKind::Tmux,
            ProviderCapabilities::tmux_interop(),
            catalog,
        ));
        Ok(())
    }

    fn snapshot(&self) -> Result<ProviderCatalogSnapshot, ProviderError> {
        self.snapshot.clone().ok_or(ProviderError::Unavailable)
    }

    fn pane(&self, pane: strukt_session::PaneId) -> Result<&TmuxBridgePane, ProviderError> {
        self.panes.get(&pane).ok_or(ProviderError::NotFound)
    }

    fn require_attached(&self) -> Result<(), ProviderError> {
        self.attached
            .then_some(())
            .ok_or(ProviderError::Unavailable)
    }
}

fn catalog_name(value: &str, fallback: &str) -> String {
    let value = if value.trim().is_empty() {
        fallback
    } else {
        value
    };
    value.chars().take(80).collect()
}

fn provider_process_error(error: TmuxError) -> ProviderError {
    match error {
        TmuxError::SessionNotFound | TmuxError::InvalidTarget => ProviderError::NotFound,
        TmuxError::InvalidInput | TmuxError::InvalidDimensions => ProviderError::InvalidAction,
        other => ProviderError::process_failed(other.to_string()),
    }
}

impl TmuxControlEvent {
    /// Parses one bounded tmux control-mode record.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid required records, IDs, escapes, or bounds.
    pub fn parse(record: &[u8]) -> Result<Self, TmuxError> {
        if record.is_empty() || record.len() > MAX_CONTROL_RECORD_BYTES {
            return Err(TmuxError::ControlRecordTooLarge);
        }
        if let Some(rest) = record.strip_prefix(b"%output ") {
            let separator = rest
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or(TmuxError::MalformedControlRecord)?;
            let pane = std::str::from_utf8(&rest[..separator])
                .map_err(|_| TmuxError::MalformedControlRecord)?;
            TmuxTarget::pane(pane.to_owned())?;
            return Ok(Self::Output {
                pane: pane.to_owned(),
                bytes: decode_control_bytes(&rest[separator + 1..])?,
            });
        }
        if let Some(rest) = record.strip_prefix(b"%layout-change ") {
            let window = first_token(rest)?;
            TmuxTarget::window(window.clone())?;
            return Ok(Self::LayoutChanged { window });
        }
        if let Some(rest) = record.strip_prefix(b"%session-closed ") {
            let session = first_token(rest)?;
            TmuxTarget::session(session.clone())?;
            return Ok(Self::SessionClosed { session });
        }
        if record == b"%exit" || record.starts_with(b"%exit ") {
            return Ok(Self::Exit);
        }
        Ok(Self::Unknown)
    }
}

fn find_or_insert_session(
    catalog: &mut TmuxCatalog,
    target: TmuxTarget,
    name: String,
) -> &mut TmuxSession {
    if let Some(index) = catalog
        .sessions
        .iter()
        .position(|session| session.target.raw == target.raw)
    {
        return &mut catalog.sessions[index];
    }
    catalog.sessions.push(TmuxSession {
        target,
        name,
        windows: Vec::new(),
    });
    catalog.sessions.last_mut().expect("session was inserted")
}

fn find_or_insert_window(
    session: &mut TmuxSession,
    target: TmuxTarget,
    name: String,
) -> &mut TmuxWindow {
    if let Some(index) = session
        .windows
        .iter()
        .position(|window| window.target.raw == target.raw)
    {
        return &mut session.windows[index];
    }
    session.windows.push(TmuxWindow {
        target,
        name,
        panes: Vec::new(),
    });
    session.windows.last_mut().expect("window was inserted")
}

fn bounded_name(value: &str) -> Result<String, TmuxError> {
    if value.len() > MAX_NAME_BYTES || value.contains('\0') {
        return Err(TmuxError::MalformedOutput);
    }
    Ok(value.to_owned())
}

fn parse_dimension(value: &str) -> Result<u16, TmuxError> {
    value
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(TmuxError::InvalidDimensions)
}

fn first_token(bytes: &[u8]) -> Result<String, TmuxError> {
    let end = bytes
        .iter()
        .position(|byte| *byte == b' ')
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end])
        .map(str::to_owned)
        .map_err(|_| TmuxError::MalformedControlRecord)
}

fn decode_control_bytes(encoded: &[u8]) -> Result<Vec<u8>, TmuxError> {
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        if encoded[index] != b'\\' {
            decoded.push(encoded[index]);
            index += 1;
            continue;
        }
        if encoded.get(index + 1) == Some(&b'\\') {
            decoded.push(b'\\');
            index += 2;
            continue;
        }
        let octal = encoded
            .get(index + 1..index + 4)
            .ok_or(TmuxError::MalformedControlRecord)?;
        if !octal.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
            return Err(TmuxError::MalformedControlRecord);
        }
        decoded.push((octal[0] - b'0') * 64 + (octal[1] - b'0') * 8 + (octal[2] - b'0'));
        index += 4;
    }
    Ok(decoded)
}

fn execute_bounded(command: &TmuxCommand, maximum: usize) -> Result<Vec<u8>, TmuxError> {
    let mut child = Command::new(command.program())
        .args(command.arguments().iter().map(OsString::from))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(TmuxError::Io)?;
    let stdout = child.stdout.take().ok_or(TmuxError::MissingChildPipe)?;
    let stderr = child.stderr.take().ok_or(TmuxError::MissingChildPipe)?;
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout_thread = drain_bounded(stdout, maximum, Arc::clone(&overflow));
    let stderr_thread = drain_bounded(stderr, MAX_CONTROL_RECORD_BYTES, Arc::clone(&overflow));
    let started = Instant::now();
    let mut exit_status = None;
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TmuxError::OutputTooLarge);
        }
        if started.elapsed() >= PROCESS_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TmuxError::ProcessTimedOut);
        }
        if exit_status.is_none() {
            match child.try_wait() {
                Ok(status) => exit_status = status,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TmuxError::Io(error));
                }
            }
        }
        if let Some(status) = exit_status
            && stdout_thread.is_finished()
            && stderr_thread.is_finished()
        {
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = join_output(stdout_thread)?;
    let stderr = join_output(stderr_thread)?;
    if overflow.load(Ordering::Acquire) {
        return Err(TmuxError::OutputTooLarge);
    }
    if !status.success() {
        let mut detail = String::from_utf8_lossy(&stderr).into_owned();
        detail.truncate(detail.floor_char_boundary(1024));
        return Err(TmuxError::ProcessFailed(detail));
    }
    Ok(stdout)
}

fn drain_bounded(
    mut reader: impl Read + Send + 'static,
    maximum: usize,
    overflow: Arc<AtomicBool>,
) -> JoinHandle<Result<Vec<u8>, std::io::Error>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let remaining = maximum.saturating_sub(output.len());
            output.extend_from_slice(&buffer[..read.min(remaining)]);
            if read > remaining {
                overflow.store(true, Ordering::Release);
            }
        }
        Ok(output)
    })
}

fn join_output(thread: JoinHandle<Result<Vec<u8>, std::io::Error>>) -> Result<Vec<u8>, TmuxError> {
    thread
        .join()
        .map_err(|_| TmuxError::OutputReaderFailed)?
        .map_err(TmuxError::Io)
}

#[derive(Debug, Error)]
pub enum TmuxError {
    #[error("tmux executable path is invalid")]
    InvalidExecutable,
    #[error("tmux server name is invalid")]
    InvalidServerName,
    #[error("tmux target is invalid")]
    InvalidTarget,
    #[error("tmux discovery output is malformed")]
    MalformedOutput,
    #[error("tmux output exceeded its bound")]
    OutputTooLarge,
    #[error("tmux discovery contains a duplicate target")]
    DuplicateTarget,
    #[error("tmux pane dimensions are invalid")]
    InvalidDimensions,
    #[error("tmux input is empty, oversized, or targets a non-pane")]
    InvalidInput,
    #[error("tmux control record exceeded its bound")]
    ControlRecordTooLarge,
    #[error("tmux control record is malformed")]
    MalformedControlRecord,
    #[error("tmux shared session request is malformed")]
    MalformedSessionRequest,
    #[error("tmux provider payload names the wrong provider")]
    WrongProvider,
    #[error("tmux provider is not attached")]
    NotAttached,
    #[error("tmux session was not found")]
    SessionNotFound,
    #[error("tmux provider state is unavailable")]
    StateUnavailable,
    #[error("tmux process failed: {0}")]
    ProcessFailed(String),
    #[error("tmux process exceeded its deadline")]
    ProcessTimedOut,
    #[error("tmux process did not expose a required stdio pipe")]
    MissingChildPipe,
    #[error("tmux output reader failed")]
    OutputReaderFailed,
    #[error("tmux process IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("tmux framing failed: {0}")]
    Framing(#[from] crate::FramingError),
}
