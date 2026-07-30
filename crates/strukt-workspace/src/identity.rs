use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::MetadataExt as _;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as _;
use thiserror::Error;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

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

#[derive(Clone)]
pub struct WorkspaceRoot {
    id: WorkspaceId,
    path: PathBuf,
    display_name: String,
    capability: Arc<Dir>,
    legacy_path_id: WorkspaceId,
    device: u64,
    inode: u64,
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
        let path_type =
            std::fs::symlink_metadata(&path).map_err(|source| WorkspaceError::Access {
                path: path.clone(),
                source,
            })?;
        if is_link_like(&path_type) {
            return Err(WorkspaceError::LocationChanged(path));
        }
        let capability = Dir::open_ambient_dir(&path, ambient_authority()).map_err(|source| {
            WorkspaceError::Access {
                path: path.clone(),
                source,
            }
        })?;
        let metadata = capability
            .dir_metadata()
            .map_err(|source| WorkspaceError::Access {
                path: path.clone(),
                source,
            })?;
        if !metadata.is_dir() {
            return Err(WorkspaceError::NotDirectory(path));
        }
        let identity = path
            .to_str()
            .ok_or_else(|| WorkspaceError::NonUtf8Path(path.clone()))?;
        let device = metadata.dev();
        let inode = metadata.ino();
        let id = workspace_id_for(identity, Some(device));
        let legacy_path_id = workspace_id_for(identity, None);
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map_or_else(|| path.display().to_string(), ToOwned::to_owned);
        Ok(Self {
            id,
            path,
            display_name,
            capability: Arc::new(capability),
            legacy_path_id,
            device,
            inode,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &WorkspaceId {
        &self.id
    }

    /// Returns the path-only ID used by workspace snapshots written before
    /// volume identity became part of workspace identity.
    #[must_use]
    pub const fn legacy_path_id(&self) -> &WorkspaceId {
        &self.legacy_path_id
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Confirms that the canonical display path still names the directory
    /// whose capability was retained when the workspace was opened.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError::LocationChanged`] when the path no longer
    /// names the retained directory, or [`WorkspaceError::Access`] when the
    /// location cannot be inspected.
    pub fn validate_location(&self) -> Result<(), WorkspaceError> {
        let path_type =
            std::fs::symlink_metadata(&self.path).map_err(|source| WorkspaceError::Access {
                path: self.path.clone(),
                source,
            })?;
        if is_link_like(&path_type) || !path_type.is_dir() {
            return Err(WorkspaceError::LocationChanged(self.path.clone()));
        }
        let current = Dir::open_ambient_dir(&self.path, ambient_authority()).map_err(|source| {
            WorkspaceError::Access {
                path: self.path.clone(),
                source,
            }
        })?;
        let metadata = current
            .dir_metadata()
            .map_err(|source| WorkspaceError::Access {
                path: self.path.clone(),
                source,
            })?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(WorkspaceError::LocationChanged(self.path.clone()));
        }
        Ok(())
    }

    /// Clones the already-open directory capability without consulting the
    /// ambient workspace path again.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError::Access`] when the retained OS handle cannot
    /// be duplicated.
    pub fn try_clone_capability(&self) -> Result<Dir, WorkspaceError> {
        self.capability
            .try_clone()
            .map_err(|source| WorkspaceError::Access {
                path: self.path.clone(),
                source,
            })
    }
}

fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return true;
    }
    false
}

impl fmt::Debug for WorkspaceRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceRoot")
            .field("id", &self.id)
            .field("path", &self.path)
            .field("display_name", &self.display_name)
            .field("legacy_path_id", &self.legacy_path_id)
            .field("device", &self.device)
            .field("inode", &self.inode)
            .finish_non_exhaustive()
    }
}

impl PartialEq for WorkspaceRoot {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.path == other.path
            && self.display_name == other.display_name
            && self.legacy_path_id == other.legacy_path_id
            && self.device == other.device
            && self.inode == other.inode
    }
}

impl Eq for WorkspaceRoot {}

impl Serialize for WorkspaceRoot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct SerializedWorkspaceRoot<'a> {
            id: &'a WorkspaceId,
            path: &'a Path,
            display_name: &'a str,
        }

        SerializedWorkspaceRoot {
            id: &self.id,
            path: &self.path,
            display_name: &self.display_name,
        }
        .serialize(serializer)
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
        let serialized_path = root.path.to_str().ok_or_else(|| {
            D::Error::custom("canonical workspace path is unexpectedly not valid UTF-8")
        })?;
        let legacy_id = workspace_id_for(serialized_path, None);
        if root.id != serialized.id && legacy_id != serialized.id {
            return Err(D::Error::custom(
                "workspace ID does not match the canonical path and volume",
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

fn workspace_id_for(path: &str, device: Option<u64>) -> WorkspaceId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.as_bytes());
    if let Some(device) = device {
        hasher.update(b"\0device\0");
        hasher.update(&device.to_le_bytes());
    }
    WorkspaceId(hasher.finalize().to_hex().to_string())
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
    #[error("workspace root location no longer names the opened directory: {0}")]
    LocationChanged(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::workspace_id_for;

    #[test]
    fn stable_id_includes_the_volume_identity() {
        assert_ne!(
            workspace_id_for("/workspace", Some(7)),
            workspace_id_for("/workspace", Some(8))
        );
    }

    #[test]
    fn stable_id_has_a_deterministic_path_only_fallback() {
        assert_eq!(
            workspace_id_for("/workspace", None),
            workspace_id_for("/workspace", None)
        );
    }
}
