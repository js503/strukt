use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use strukt_language::{
    LanguageProcess, LanguageTransport, ResolvedCommand, SpawnRequest, StdioTransport,
    TransportError,
};
use strukt_workspace::{WorkspaceError, WorkspaceRoot};
use thiserror::Error;

use crate::RemotePath;

const MAX_LANGUAGE_PROCESSES: usize = 16;

pub struct RemoteLanguageManager {
    root: WorkspaceRoot,
    next_id: u64,
    processes: HashMap<u64, Box<dyn LanguageProcess>>,
}

impl RemoteLanguageManager {
    /// Retains a remote workspace root without starting a language server.
    ///
    /// # Errors
    ///
    /// Returns a typed root error when the workspace cannot be opened.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, RemoteLanguageError> {
        Ok(Self {
            root: WorkspaceRoot::open(root).map_err(RemoteLanguageError::OpenRoot)?,
            next_id: 1,
            processes: HashMap::new(),
        })
    }

    #[must_use]
    pub fn running(&self) -> usize {
        self.processes.len()
    }

    /// Explicitly starts one exact no-shell language-server process.
    ///
    /// # Errors
    ///
    /// Returns a request, cwd, capacity, root, descriptor, or transport error.
    pub fn spawn(
        &mut self,
        executable: PathBuf,
        arguments: Vec<OsString>,
        cwd: &RemotePath,
    ) -> Result<u64, RemoteLanguageError> {
        self.root
            .validate_location()
            .map_err(|_| RemoteLanguageError::WorkspaceChanged)?;
        if self.processes.len() >= MAX_LANGUAGE_PROCESSES {
            return Err(RemoteLanguageError::CapacityReached);
        }
        let cwd = self.root.path().join(cwd.as_path());
        if !cwd.is_dir() {
            return Err(RemoteLanguageError::InvalidWorkingDirectory);
        }
        let command = ResolvedCommand::new(executable, arguments)
            .map_err(|_| RemoteLanguageError::InvalidCommand)?;
        let request = SpawnRequest::new(command, cwd)
            .map_err(|_| RemoteLanguageError::InvalidWorkingDirectory)?;
        let process = StdioTransport
            .spawn(request)
            .map_err(RemoteLanguageError::Transport)?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(1).max(1);
        self.processes.insert(id, process);
        Ok(id)
    }

    /// Writes exact language-protocol bytes.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn write(&mut self, id: u64, bytes: &[u8]) -> Result<(), RemoteLanguageError> {
        self.process_mut(id)?
            .write(bytes)
            .map_err(RemoteLanguageError::Transport)
    }

    /// Reads the next bounded language-protocol chunk without blocking.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn try_read(&mut self, id: u64) -> Result<Option<Vec<u8>>, RemoteLanguageError> {
        self.process_mut(id)?
            .try_read()
            .map_err(RemoteLanguageError::Transport)
    }

    /// Terminates and removes one language process.
    ///
    /// # Errors
    ///
    /// Returns a not-found or transport error.
    pub fn terminate(&mut self, id: u64, grace: Duration) -> Result<(), RemoteLanguageError> {
        let mut process = self
            .processes
            .remove(&id)
            .ok_or(RemoteLanguageError::NotFound)?;
        process
            .terminate(grace)
            .map_err(RemoteLanguageError::Transport)
    }

    fn process_mut(&mut self, id: u64) -> Result<&mut dyn LanguageProcess, RemoteLanguageError> {
        let process = self
            .processes
            .get_mut(&id)
            .ok_or(RemoteLanguageError::NotFound)?;
        Ok(process.as_mut())
    }
}

#[derive(Debug, Error)]
pub enum RemoteLanguageError {
    #[error("remote language command is invalid")]
    InvalidCommand,
    #[error("remote language working directory is invalid")]
    InvalidWorkingDirectory,
    #[error("remote language process capacity was reached")]
    CapacityReached,
    #[error("remote language process was not found")]
    NotFound,
    #[error("remote workspace root could not be opened: {0}")]
    OpenRoot(WorkspaceError),
    #[error("remote workspace root changed")]
    WorkspaceChanged,
    #[error(transparent)]
    Transport(TransportError),
}
