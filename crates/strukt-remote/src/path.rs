use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use strukt_workspace::WorkspaceRoot;
use thiserror::Error;

const MAX_REMOTE_PATH_BYTES: usize = 4_096;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RemotePath(String);

impl RemotePath {
    /// Validates a relative normalized Linux protocol path.
    ///
    /// # Errors
    ///
    /// Returns [`RemotePathError::Invalid`] for empty, absolute, control-containing,
    /// escaping, non-normalized, Windows-prefixed, or oversized paths.
    pub fn new(value: impl Into<String>) -> Result<Self, RemotePathError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_REMOTE_PATH_BYTES
            || value.starts_with('/')
            || value.contains('\\')
            || value.chars().any(char::is_control)
        {
            return Err(RemotePathError::Invalid);
        }
        for (index, segment) in value.split('/').enumerate() {
            if segment.is_empty()
                || matches!(segment, "." | "..")
                || (index == 0 && segment.ends_with(':'))
            {
                return Err(RemotePathError::Invalid);
            }
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn root() -> Self {
        Self(String::new())
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        if self.is_root() {
            Path::new(".")
        } else {
            Path::new(&self.0)
        }
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|segment| !segment.is_empty())
    }
}

impl fmt::Display for RemotePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RemotePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum RemotePathError {
    #[error("remote path must be a relative normalized Linux path")]
    Invalid,
}

pub(crate) fn resolve_confined_directory(
    root: &WorkspaceRoot,
    relative: &RemotePath,
) -> Option<PathBuf> {
    let mut candidate = root.path().to_path_buf();
    for segment in relative.segments() {
        candidate.push(segment);
        let metadata = candidate.symlink_metadata().ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return None;
        }
    }
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(root.path()).then_some(canonical)
}
