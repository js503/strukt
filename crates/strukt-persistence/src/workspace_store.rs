use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use strukt_workspace::{WorkspaceId, WorkspaceState};
use thiserror::Error;

const CURRENT_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    pub schema_version: u32,
    pub state: WorkspaceState,
}

#[derive(Clone, Debug)]
pub struct WorkspaceStore {
    root: PathBuf,
}

impl WorkspaceStore {
    /// Creates a store under the platform-specific local application-data directory.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::ApplicationDataUnavailable`] when the platform does not
    /// provide an application-data directory.
    pub fn platform_default() -> Result<Self, StoreError> {
        let dirs = ProjectDirs::from("dev", "strukt", "strukt")
            .ok_or(StoreError::ApplicationDataUnavailable)?;
        Ok(Self::at(dirs.data_local_dir().join("workspaces")))
    }

    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn current_path(&self, id: &WorkspaceId) -> PathBuf {
        self.root.join(format!("{}.json", id.as_str()))
    }

    fn backup_path(&self, id: &WorkspaceId) -> PathBuf {
        self.root.join(format!("{}.last-valid.json", id.as_str()))
    }

    /// Saves the workspace state using atomic file replacement.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when the store directory or snapshot files cannot
    /// be written, and [`StoreError::Json`] when the state cannot be serialized.
    pub fn save(&self, state: &WorkspaceState) -> Result<(), StoreError> {
        fs::create_dir_all(&self.root).map_err(StoreError::Io)?;
        let id = state.root.id();
        let current = self.current_path(id);
        let bytes = serde_json::to_vec_pretty(&WorkspaceSnapshot {
            schema_version: CURRENT_SCHEMA,
            state: state.clone(),
        })
        .map_err(StoreError::Json)?;

        if let Some(previous) = Self::read_valid(&current, id)? {
            let previous_bytes = serde_json::to_vec_pretty(&previous).map_err(StoreError::Json)?;
            Self::replace_atomically(&self.backup_path(id), &previous_bytes)?;
        }

        Self::replace_atomically(&current, &bytes)
    }

    /// Loads the first valid snapshot from the current or last-valid file.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when a candidate cannot be read for a reason other
    /// than not existing. Invalid persisted JSON is treated as recoverable.
    pub fn load(&self, id: &WorkspaceId) -> Result<Option<WorkspaceSnapshot>, StoreError> {
        for path in [self.current_path(id), self.backup_path(id)] {
            if let Some(snapshot) = Self::read_valid(&path, id)? {
                return Ok(Some(snapshot));
            }
        }
        Ok(None)
    }

    fn read_valid(path: &Path, id: &WorkspaceId) -> Result<Option<WorkspaceSnapshot>, StoreError> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice::<WorkspaceSnapshot>(&bytes)
                .ok()
                .filter(|snapshot| {
                    snapshot.schema_version == CURRENT_SCHEMA && snapshot.state.root.id() == id
                })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    fn replace_atomically(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        let mut file = AtomicWriteFile::open(path).map_err(StoreError::Io)?;
        file.write_all(bytes).map_err(StoreError::Io)?;
        file.commit().map_err(StoreError::Io)
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("platform application-data directory is unavailable")]
    ApplicationDataUnavailable,
    #[error("workspace state IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("workspace state serialization failed: {0}")]
    Json(#[source] serde_json::Error),
}
