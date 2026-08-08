use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CURRENT_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteHelperMetadata {
    pub version: String,
    pub checksum_sha256: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl RemoteHelperMetadata {
    /// Creates validated, presentation-only helper metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for a hostile version or non-SHA-256 checksum.
    pub fn new(
        version: impl Into<String>,
        checksum_sha256: impl Into<String>,
    ) -> Result<Self, RemoteStoreError> {
        let value = Self {
            version: version.into(),
            checksum_sha256: checksum_sha256.into(),
            extensions: BTreeMap::new(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), RemoteStoreError> {
        if !valid_version(&self.version)
            || self.checksum_sha256.len() != 64
            || !self
                .checksum_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(RemoteStoreError::InvalidRecord);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteConnectionRecord {
    pub connection_id: String,
    pub alias: String,
    pub display_name: Option<String>,
    pub recent_roots: Vec<String>,
    pub helper: Option<RemoteHelperMetadata>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl RemoteConnectionRecord {
    /// Creates one secret-free remote connection record.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed IDs, aliases, labels, roots, or helper
    /// metadata.
    pub fn new(
        connection_id: impl Into<String>,
        alias: impl Into<String>,
        display_name: Option<String>,
        recent_roots: Vec<String>,
        helper: Option<RemoteHelperMetadata>,
    ) -> Result<Self, RemoteStoreError> {
        let value = Self {
            connection_id: connection_id.into(),
            alias: alias.into(),
            display_name,
            recent_roots,
            helper,
            extensions: BTreeMap::new(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), RemoteStoreError> {
        let id_valid = self.connection_id.len() == 32
            && self
                .connection_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
        let alias_valid = !self.alias.is_empty()
            && self.alias.len() <= 255
            && !self.alias.starts_with('-')
            && !self.alias.chars().any(char::is_control);
        let label_valid = self.display_name.as_ref().is_none_or(|label| {
            !label.is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
        });
        let roots_valid = self.recent_roots.len() <= 20
            && self.recent_roots.iter().all(|root| {
                (root.starts_with('/') || root == "~" || root.starts_with("~/"))
                    && !root.split('/').any(|segment| segment == "..")
                    && !root.chars().any(char::is_control)
            });
        if !id_valid || !alias_valid || !label_valid || !roots_valid {
            return Err(RemoteStoreError::InvalidRecord);
        }
        if let Some(helper) = &self.helper {
            helper.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RemoteStore {
    root: PathBuf,
}

impl RemoteStore {
    /// Creates the secret-free remote store in platform application data.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not expose an application-data
    /// location.
    pub fn platform_default() -> Result<Self, RemoteStoreError> {
        let directories = ProjectDirs::from("dev", "strukt", "strukt")
            .ok_or(RemoteStoreError::ApplicationDataUnavailable)?;
        Ok(Self::at(directories.data_local_dir().join("remote")))
    }

    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn current_path(&self) -> PathBuf {
        self.root.join("remote-connections.json")
    }

    fn backup_path(&self) -> PathBuf {
        self.root.join("remote-connections.last-valid.json")
    }

    /// Loads the current or last-valid set without making a connection.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for unreadable non-missing files. Corrupt or invalid
    /// documents are skipped.
    pub fn load(&self) -> Result<Vec<RemoteConnectionRecord>, RemoteStoreError> {
        for path in [self.current_path(), self.backup_path()] {
            match read_document(&path) {
                Ok(document) if document.validate() => return Ok(document.connections),
                Ok(_) | Err(RemoteStoreError::Json(_) | RemoteStoreError::InvalidRecord) => {}
                Err(RemoteStoreError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Vec::new())
    }

    /// Adds or replaces a connection and records deterministic ordering.
    ///
    /// # Errors
    ///
    /// Returns a validation, serialization, or atomic I/O error.
    pub fn upsert(&self, record: RemoteConnectionRecord) -> Result<(), RemoteStoreError> {
        record.validate()?;
        let mut connections = self.load()?;
        connections.retain(|existing| existing.connection_id != record.connection_id);
        connections.push(record);
        sort_records(&mut connections);
        self.write(&connections)
    }

    /// Forgets a connection record only; no remote host state is changed.
    ///
    /// # Errors
    ///
    /// Returns a serialization or atomic I/O error.
    pub fn forget(&self, connection_id: &str) -> Result<bool, RemoteStoreError> {
        let mut connections = self.load()?;
        let original = connections.len();
        connections.retain(|record| record.connection_id != connection_id);
        if connections.len() == original {
            return Ok(false);
        }
        self.write(&connections)?;
        Ok(true)
    }

    fn write(&self, connections: &[RemoteConnectionRecord]) -> Result<(), RemoteStoreError> {
        let current = self.current_path();
        if let Some(parent) = current.parent() {
            fs::create_dir_all(parent).map_err(RemoteStoreError::Io)?;
        }
        if let Ok(previous) = read_document(&current)
            && previous.validate()
        {
            write_document(&self.backup_path(), &previous)?;
        }
        write_document(
            &current,
            &RemoteDocument {
                schema_version: CURRENT_SCHEMA,
                connections: connections.to_vec(),
                extensions: BTreeMap::new(),
            },
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct RemoteDocument {
    schema_version: u32,
    connections: Vec<RemoteConnectionRecord>,
    #[serde(flatten)]
    extensions: BTreeMap<String, serde_json::Value>,
}

impl RemoteDocument {
    fn validate(&self) -> bool {
        self.schema_version == CURRENT_SCHEMA
            && self.connections.len() <= 256
            && self
                .connections
                .iter()
                .all(|record| record.validate().is_ok())
    }
}

fn sort_records(records: &mut [RemoteConnectionRecord]) {
    records.sort_by(|left, right| {
        left.alias
            .to_ascii_lowercase()
            .cmp(&right.alias.to_ascii_lowercase())
            .then_with(|| left.connection_id.cmp(&right.connection_id))
    });
}

fn read_document(path: &Path) -> Result<RemoteDocument, RemoteStoreError> {
    let bytes = fs::read(path).map_err(RemoteStoreError::Io)?;
    serde_json::from_slice(&bytes).map_err(RemoteStoreError::Json)
}

fn write_document(path: &Path, document: &RemoteDocument) -> Result<(), RemoteStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(RemoteStoreError::Io)?;
    }
    let bytes = serde_json::to_vec_pretty(document).map_err(RemoteStoreError::Json)?;
    let mut file = AtomicWriteFile::open(path).map_err(RemoteStoreError::Io)?;
    file.write_all(&bytes).map_err(RemoteStoreError::Io)?;
    file.commit().map_err(RemoteStoreError::Io)?;
    set_private_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), RemoteStoreError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(RemoteStoreError::Io)
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), RemoteStoreError> {
    Ok(())
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        && version.bytes().any(|byte| byte.is_ascii_digit())
}

#[derive(Debug, Error)]
pub enum RemoteStoreError {
    #[error("platform application-data directory is unavailable")]
    ApplicationDataUnavailable,
    #[error("remote connection record is invalid")]
    InvalidRecord,
    #[error("remote connection store I/O failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("remote connection store serialization failed: {0}")]
    Json(#[source] serde_json::Error),
}
