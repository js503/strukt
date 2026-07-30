use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceRoot {
    id: WorkspaceId,
    path: PathBuf,
    display_name: String,
}

impl WorkspaceRoot {
    /// Opens a canonical workspace root and derives its stable identity.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError::Access`] when the path cannot be canonicalized,
    /// or [`WorkspaceError::NotDirectory`] when the canonical path is not a directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let requested = path.as_ref();
        let path = requested
            .canonicalize()
            .map_err(|source| WorkspaceError::Access {
                path: requested.to_path_buf(),
                source,
            })?;
        if !path.is_dir() {
            return Err(WorkspaceError::NotDirectory(path));
        }
        let identity_bytes = path.to_string_lossy();
        let id = WorkspaceId(blake3::hash(identity_bytes.as_bytes()).to_hex().to_string());
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map_or_else(|| path.display().to_string(), ToOwned::to_owned);
        Ok(Self {
            id,
            path,
            display_name,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &WorkspaceId {
        &self.id
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("cannot access workspace path {path}: {source}")]
    Access {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("workspace root is not a directory: {0}")]
    NotDirectory(PathBuf),
}
