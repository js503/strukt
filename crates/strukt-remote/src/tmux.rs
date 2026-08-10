use std::ffi::OsString;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{PersistentProvider, SessionPayload, read_frame, write_frame};

const MAX_DISCOVERY_BYTES: usize = 1024 * 1024;
const MAX_CONTROL_RECORD_BYTES: usize = 64 * 1024;
const MAX_RECORDS: usize = 1024;
const MAX_NAME_BYTES: usize = 256;
const MAX_INPUT_BYTES: usize = 256 * 1024;

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
        if pane.kind != TargetKind::Pane || bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
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
}

impl TmuxManager {
    #[must_use]
    pub fn new(provider: TmuxProvider) -> Self {
        Self {
            provider,
            attached: Mutex::new(None),
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
                execute_bounded(
                    &self.provider.input_command(&target, &bytes)?,
                    MAX_CONTROL_RECORD_BYTES,
                )?;
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
    let output = Command::new(command.program())
        .args(command.arguments().iter().map(OsString::from))
        .output()
        .map_err(TmuxError::Io)?;
    if output.stdout.len() > maximum || output.stderr.len() > MAX_CONTROL_RECORD_BYTES {
        return Err(TmuxError::OutputTooLarge);
    }
    if !output.status.success() {
        let mut detail = String::from_utf8_lossy(&output.stderr).into_owned();
        detail.truncate(detail.floor_char_boundary(1024));
        return Err(TmuxError::ProcessFailed(detail));
    }
    Ok(output.stdout)
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
    #[error("tmux process IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("tmux framing failed: {0}")]
    Framing(#[from] crate::FramingError),
}
