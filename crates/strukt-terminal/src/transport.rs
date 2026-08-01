use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    rows: u16,
    columns: u16,
    pixel_width: u16,
    pixel_height: u16,
}

impl TerminalSize {
    /// Creates a nonempty character-cell terminal size.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidSize`] if rows or columns is zero.
    pub fn new(rows: u16, columns: u16) -> Result<Self, TransportError> {
        if rows == 0 || columns == 0 {
            return Err(TransportError::InvalidSize);
        }
        Ok(Self {
            rows,
            columns,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    #[must_use]
    pub const fn rows(self) -> u16 {
        self.rows
    }

    #[must_use]
    pub const fn columns(self) -> u16 {
        self.columns
    }

    #[must_use]
    pub const fn pixel_width(self) -> u16 {
        self.pixel_width
    }

    #[must_use]
    pub const fn pixel_height(self) -> u16 {
        self.pixel_height
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnRequest {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub working_directory: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
    pub size: TerminalSize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputChunk {
    sequence: u64,
    bytes: Vec<u8>,
}

impl OutputChunk {
    #[must_use]
    pub fn new(sequence: u64, bytes: Vec<u8>) -> Self {
        Self { sequence, bytes }
    }

    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitStatus {
    code: Option<i32>,
    signal: Option<String>,
    terminated: bool,
}

impl ExitStatus {
    #[must_use]
    pub fn new(code: Option<i32>, signal: Option<String>, terminated: bool) -> Self {
        Self {
            code,
            signal,
            terminated,
        }
    }

    #[must_use]
    pub const fn code(&self) -> Option<i32> {
        self.code
    }

    #[must_use]
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }

    #[must_use]
    pub const fn was_terminated(&self) -> bool {
        self.terminated
    }
}

pub trait TerminalTransport: Send + Sync {
    /// Spawns a child attached to a new native PTY or `ConPTY`.
    ///
    /// # Errors
    ///
    /// Returns a validation or platform adapter error if the request cannot be
    /// safely started.
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn TerminalProcess>, TransportError>;
}

pub trait TerminalProcess: Send {
    /// Writes bytes to the child terminal input.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Io`] if the PTY writer fails.
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError>;

    /// Resizes the native terminal.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Adapter`] if the platform resize fails.
    fn resize(&mut self, size: TerminalSize) -> Result<(), TransportError>;

    /// Takes the next sequence-tagged output chunk without blocking.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Io`] if the dedicated reader failed.
    fn try_read(&mut self) -> Result<Option<OutputChunk>, TransportError>;

    /// Polls the child exit status without blocking.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Io`] if the child status cannot be queried.
    fn try_wait(&mut self) -> Result<Option<ExitStatus>, TransportError>;

    /// Waits up to `timeout` for the child to exit.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::WaitTimeout`] on expiry or an IO error when
    /// status polling fails.
    fn wait(&mut self, timeout: Duration) -> Result<ExitStatus, TransportError>;

    /// Requests termination and waits up to `grace` for exit observation.
    ///
    /// # Errors
    ///
    /// Returns an IO or timeout error when termination cannot be completed.
    fn terminate(&mut self, grace: Duration) -> Result<(), TransportError>;
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TransportError {
    #[error("terminal rows and columns must be nonzero")]
    InvalidSize,
    #[error("terminal executable must not be empty")]
    InvalidExecutable,
    #[error("terminal working directory must be an existing absolute directory")]
    InvalidWorkingDirectory,
    #[error("terminal environment key is invalid")]
    InvalidEnvironmentKey,
    #[error("terminal platform adapter failed: {0}")]
    Adapter(String),
    #[error("terminal IO failed: {0}")]
    Io(String),
    #[error("terminal wait timed out")]
    WaitTimeout,
    #[error("terminal termination timed out")]
    TerminationTimeout,
}
