use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorkspaceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let is_lowercase_hex = value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if value.len() != 64 || !is_lowercase_hex {
            return Err(D::Error::custom(
                "workspace ID must be 64 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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
    /// [`WorkspaceError::NotDirectory`] when the canonical path is not a directory,
    /// or [`WorkspaceError::NonUtf8Path`] when it is not valid UTF-8.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let requested = path.as_ref();
        let path = requested
            .canonicalize()
            .map_err(|source| WorkspaceError::Access {
                path: requested.to_path_buf(),
                source,
            })?;
        let metadata = std::fs::metadata(&path).map_err(|source| WorkspaceError::Access {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(WorkspaceError::NotDirectory(path));
        }
        let identity = path
            .to_str()
            .ok_or_else(|| WorkspaceError::NonUtf8Path(path.clone()))?;
        let id = WorkspaceId(blake3::hash(identity.as_bytes()).to_hex().to_string());
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

impl<'de> Deserialize<'de> for WorkspaceRoot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedWorkspaceRoot {
            id: WorkspaceId,
            path: PathBuf,
            display_name: String,
        }

        let serialized = SerializedWorkspaceRoot::deserialize(deserializer)?;
        let root = Self::open(&serialized.path).map_err(D::Error::custom)?;
        if root.id != serialized.id {
            return Err(D::Error::custom(
                "workspace ID does not match the canonical path",
            ));
        }
        if root.display_name != serialized.display_name {
            return Err(D::Error::custom(
                "workspace display name does not match the canonical path",
            ));
        }
        Ok(root)
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
    #[error("workspace root path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
}
