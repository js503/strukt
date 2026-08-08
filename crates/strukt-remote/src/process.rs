use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use strukt_terminal::{
    ExitStatus, PortableTransport, SpawnRequest, TerminalProcess, TerminalSize, TerminalTransport,
    TransportError,
};
use strukt_workspace::{WorkspaceError, WorkspaceRoot};
use thiserror::Error;

use crate::{RemotePath, path::resolve_confined_directory};

const MAX_PROCESSES: usize = 64;

#[derive(Clone, Debug)]
pub struct RemoteProcessRequest {
    executable: PathBuf,
    arguments: Vec<OsString>,
    cwd: RemotePath,
    environment: Vec<(OsString, OsString)>,
    size: TerminalSize,
}

impl RemoteProcessRequest {
    /// Creates an exact no-shell remote process request.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteProcessError::InvalidRequest`] for empty/NUL command
    /// components or invalid environment keys and values.
    pub fn new(
        executable: PathBuf,
        arguments: Vec<OsString>,
        cwd: RemotePath,
        environment: Vec<(OsString, OsString)>,
        size: TerminalSize,
    ) -> Result<Self, RemoteProcessError> {
        if executable.as_os_str().is_empty()
            || has_nul(executable.as_os_str())
            || arguments.iter().any(|argument| has_nul(argument))
            || environment.iter().any(|(key, value)| {
                key.is_empty()
                    || os_contains(key, '=')
                    || has_nul(key)
                    || has_nul(value)
                    || key.len() > 256
                    || value.len() > 16 * 1024
            })
        {
            return Err(RemoteProcessError::InvalidRequest);
        }
        Ok(Self {
            executable,
            arguments,
            cwd,
            environment,
            size,
        })
    }
}

pub struct RemoteProcessManager {
    root: WorkspaceRoot,
    next_id: u64,
    processes: HashMap<u64, ManagedProcess>,
}

struct ManagedProcess {
    process: Box<dyn TerminalProcess>,
    pending: VecDeque<Vec<u8>>,
}

impl RemoteProcessManager {
    /// Retains a workspace root for all subsequently spawned processes.
    ///
    /// # Errors
    ///
    /// Returns a typed root error when the workspace cannot be opened.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, RemoteProcessError> {
        Ok(Self {
            root: WorkspaceRoot::open(root).map_err(RemoteProcessError::OpenRoot)?,
            next_id: 1,
            processes: HashMap::new(),
        })
    }

    /// Spawns one PTY process in a confined existing workspace directory.
    ///
    /// # Errors
    ///
    /// Returns a root, cwd, capacity, spawn, or request error.
    pub fn spawn(&mut self, request: RemoteProcessRequest) -> Result<u64, RemoteProcessError> {
        self.validate_root()?;
        if self.processes.len() >= MAX_PROCESSES {
            return Err(RemoteProcessError::CapacityReached);
        }
        let cwd = resolve_confined_directory(&self.root, &request.cwd)
            .ok_or(RemoteProcessError::InvalidWorkingDirectory)?;
        let process = PortableTransport::new()
            .spawn(SpawnRequest {
                executable: request.executable,
                arguments: request.arguments,
                working_directory: cwd,
                environment: request.environment,
                size: request.size,
            })
            .map_err(RemoteProcessError::Transport)?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(1).max(1);
        self.processes.insert(
            id,
            ManagedProcess {
                process,
                pending: VecDeque::new(),
            },
        );
        Ok(id)
    }

    /// Writes exact bytes to one process.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn write(&mut self, id: u64, bytes: &[u8]) -> Result<(), RemoteProcessError> {
        self.process_mut(id)?
            .write(bytes)
            .map_err(RemoteProcessError::Transport)
    }

    /// Resizes one process PTY.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn resize(&mut self, id: u64, size: TerminalSize) -> Result<(), RemoteProcessError> {
        self.process_mut(id)?
            .resize(size)
            .map_err(RemoteProcessError::Transport)
    }

    /// Drains bounded output fairly from one process without blocking.
    ///
    /// # Errors
    ///
    /// Returns an invalid-limit, not-found, or transport error.
    pub fn drain(
        &mut self,
        id: u64,
        max_chunks: usize,
        max_bytes: usize,
    ) -> Result<RemoteProcessOutput, RemoteProcessError> {
        if max_chunks == 0 || max_bytes == 0 {
            return Err(RemoteProcessError::InvalidLimit);
        }
        let managed = self
            .processes
            .get_mut(&id)
            .ok_or(RemoteProcessError::NotFound)?;
        let mut bytes = Vec::new();
        for _ in 0..max_chunks {
            let chunk = if let Some(chunk) = managed.pending.pop_front() {
                Some(chunk)
            } else {
                managed
                    .process
                    .try_read()
                    .map_err(RemoteProcessError::Transport)?
                    .map(strukt_terminal::OutputChunk::into_bytes)
            };
            let Some(mut chunk) = chunk else {
                break;
            };
            let remaining = max_bytes.saturating_sub(bytes.len());
            if remaining == 0 {
                managed.pending.push_front(chunk);
                break;
            }
            if chunk.len() > remaining {
                let remainder = chunk.split_off(remaining);
                bytes.extend_from_slice(&chunk);
                managed.pending.push_front(remainder);
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(RemoteProcessOutput {
            bytes,
            backpressured: managed.process.output_backpressured() || !managed.pending.is_empty(),
        })
    }

    /// Checks process exit without blocking.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn try_wait(&mut self, id: u64) -> Result<Option<ExitStatus>, RemoteProcessError> {
        self.process_mut(id)?
            .try_wait()
            .map_err(RemoteProcessError::Transport)
    }

    /// Terminates and removes one process.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn terminate(&mut self, id: u64, grace: Duration) -> Result<(), RemoteProcessError> {
        let mut managed = self
            .processes
            .remove(&id)
            .ok_or(RemoteProcessError::NotFound)?;
        managed
            .process
            .terminate(grace)
            .map_err(RemoteProcessError::Transport)
    }

    fn process_mut(&mut self, id: u64) -> Result<&mut dyn TerminalProcess, RemoteProcessError> {
        let managed = self
            .processes
            .get_mut(&id)
            .ok_or(RemoteProcessError::NotFound)?;
        Ok(managed.process.as_mut())
    }

    fn validate_root(&self) -> Result<(), RemoteProcessError> {
        self.root
            .validate_location()
            .map_err(|_| RemoteProcessError::WorkspaceChanged)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteProcessOutput {
    pub bytes: Vec<u8>,
    pub backpressured: bool,
}

#[derive(Debug, Error)]
pub enum RemoteProcessError {
    #[error("remote process request is invalid")]
    InvalidRequest,
    #[error("remote process working directory is invalid")]
    InvalidWorkingDirectory,
    #[error("remote process capacity was reached")]
    CapacityReached,
    #[error("remote process was not found")]
    NotFound,
    #[error("remote process drain limit is invalid")]
    InvalidLimit,
    #[error("remote workspace root could not be opened: {0}")]
    OpenRoot(WorkspaceError),
    #[error("remote workspace root changed")]
    WorkspaceChanged,
    #[error(transparent)]
    Transport(TransportError),
}

#[cfg(unix)]
fn has_nul(value: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    value.as_bytes().contains(&0)
}

#[cfg(windows)]
fn has_nul(value: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt as _;
    value.encode_wide().any(|unit| unit == 0)
}

fn os_contains(value: &OsStr, needle: char) -> bool {
    value.to_string_lossy().contains(needle)
}
