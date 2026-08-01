use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use chacha20poly1305::aead::{Aead, Generate, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroize;

const CURRENT_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EditorTabSnapshot {
    pub path: String,
    pub cursor: usize,
    pub selection_anchor: usize,
    pub scroll_line: f32,
    pub find_query: String,
    pub language_override: Option<String>,
    pub read_only: bool,
    #[serde(default)]
    pub disk_revision: Option<String>,
}

impl EditorTabSnapshot {
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        cursor: usize,
        selection_anchor: usize,
        scroll_line: f32,
    ) -> Self {
        Self {
            path: path.into(),
            cursor,
            selection_anchor,
            scroll_line,
            find_query: String::new(),
            language_override: None,
            read_only: false,
            disk_revision: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EditorSessionSnapshot {
    schema_version: u32,
    pub tabs: Vec<EditorTabSnapshot>,
    pub active_path: Option<String>,
    pub preview_path: Option<String>,
}

impl EditorSessionSnapshot {
    #[must_use]
    pub fn new(
        tabs: Vec<EditorTabSnapshot>,
        active_path: Option<String>,
        preview_path: Option<String>,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA,
            tabs,
            active_path,
            preview_path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EditorSessionStore {
    root: PathBuf,
}

impl EditorSessionStore {
    /// Opens the platform-local editor-session directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform has no application-data directory.
    pub fn platform_default() -> Result<Self, RecoveryStoreError> {
        let dirs = ProjectDirs::from("dev", "strukt", "strukt")
            .ok_or(RecoveryStoreError::ApplicationDataUnavailable)?;
        Ok(Self::at(dirs.data_local_dir().join("editor/sessions")))
    }

    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn current_path(&self, workspace_id: &str) -> PathBuf {
        self.root.join(format!(
            "{}.json",
            blake3::hash(workspace_id.as_bytes()).to_hex()
        ))
    }

    fn backup_path(&self, workspace_id: &str) -> PathBuf {
        self.root.join(format!(
            "{}.last-valid.json",
            blake3::hash(workspace_id.as_bytes()).to_hex()
        ))
    }

    /// Atomically saves tab order and editor view state without document content.
    ///
    /// # Errors
    ///
    /// Returns serialization or IO errors.
    pub fn save(
        &self,
        workspace_id: &str,
        snapshot: &EditorSessionSnapshot,
    ) -> Result<(), RecoveryStoreError> {
        let current = self.current_path(workspace_id);
        if let Ok(bytes) = fs::read(&current)
            && serde_json::from_slice::<EditorSessionSnapshot>(&bytes)
                .is_ok_and(|value| value.schema_version == CURRENT_SCHEMA)
        {
            write_bytes(&self.backup_path(workspace_id), &bytes)?;
        }
        let bytes = serde_json::to_vec(snapshot).map_err(RecoveryStoreError::Json)?;
        write_bytes(&current, &bytes)
    }

    /// Loads current editor view state or its last-valid fallback.
    ///
    /// # Errors
    ///
    /// Returns non-missing IO errors. Invalid or future-schema records are skipped.
    pub fn load(
        &self,
        workspace_id: &str,
    ) -> Result<Option<EditorSessionSnapshot>, RecoveryStoreError> {
        for path in [
            self.current_path(workspace_id),
            self.backup_path(workspace_id),
        ] {
            match fs::read(path) {
                Ok(bytes) => {
                    if let Ok(snapshot) = serde_json::from_slice::<EditorSessionSnapshot>(&bytes)
                        && snapshot.schema_version == CURRENT_SCHEMA
                    {
                        return Ok(Some(snapshot));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(RecoveryStoreError::Io(error)),
            }
        }
        Ok(None)
    }
}

pub trait RecoveryKeyProvider: Send + Sync {
    /// Loads the existing per-user key or creates it in protected storage.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable or provider failure. Implementations must
    /// never fall back to plaintext storage.
    fn load_or_create(&self) -> Result<RecoveryKey, RecoveryKeyError>;

    /// Deletes the protected key.
    ///
    /// # Errors
    ///
    /// Returns a typed provider failure.
    fn delete(&self) -> Result<(), RecoveryKeyError>;
}

pub struct RecoveryKey {
    bytes: [u8; 32],
}

impl RecoveryKey {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Copies exactly 32 bytes into guarded key material and clears the source.
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryKeyError::Provider`] for any other length.
    pub fn from_secret(mut secret: Vec<u8>) -> Result<Self, RecoveryKeyError> {
        if secret.len() != 32 {
            secret.zeroize();
            return Err(RecoveryKeyError::Provider(
                "stored recovery key must contain exactly 32 bytes".into(),
            ));
        }
        let mut bytes = [0; 32];
        bytes.copy_from_slice(&secret);
        secret.zeroize();
        Ok(Self { bytes })
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl Drop for RecoveryKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryKey([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryMetadata {
    pub workspace_id: String,
    pub document_path: String,
    pub baseline: String,
}

impl RecoveryMetadata {
    #[must_use]
    pub fn new(
        workspace_id: impl Into<String>,
        document_path: impl Into<String>,
        baseline: impl Into<String>,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            document_path: document_path.into(),
            baseline: baseline.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryPayload {
    metadata: RecoveryMetadata,
    pub document_revision: u64,
    pub text: String,
}

impl RecoveryPayload {
    #[must_use]
    pub fn new(
        metadata: RecoveryMetadata,
        document_revision: u64,
        text: impl Into<String>,
    ) -> Self {
        Self {
            metadata,
            document_revision,
            text: text.into(),
        }
    }

    #[must_use]
    pub const fn metadata(&self) -> &RecoveryMetadata {
        &self.metadata
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryEnvelope {
    pub schema_version: u32,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

#[derive(Serialize)]
struct AuthenticatedMetadata<'a> {
    schema_version: u32,
    metadata: &'a RecoveryMetadata,
}

#[derive(Clone, Debug)]
pub struct EditorRecoveryStore {
    root: PathBuf,
}

impl EditorRecoveryStore {
    /// Opens the platform-local editor recovery directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform has no application-data directory.
    pub fn platform_default() -> Result<Self, RecoveryStoreError> {
        let dirs = ProjectDirs::from("dev", "strukt", "strukt")
            .ok_or(RecoveryStoreError::ApplicationDataUnavailable)?;
        Ok(Self::at(dirs.data_local_dir().join("editor/recovery")))
    }

    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn current_path(&self, metadata: &RecoveryMetadata) -> PathBuf {
        self.root.join(format!("{}.json", record_id(metadata)))
    }

    #[must_use]
    pub fn backup_path(&self, metadata: &RecoveryMetadata) -> PathBuf {
        self.root
            .join(format!("{}.last-valid.json", record_id(metadata)))
    }

    /// Encrypts and atomically persists a recovery payload.
    ///
    /// # Errors
    ///
    /// Returns key-provider, serialization, encryption, or IO errors. No file
    /// is written when protected key storage is unavailable.
    pub fn save(
        &self,
        provider: &dyn RecoveryKeyProvider,
        payload: &RecoveryPayload,
    ) -> Result<(), RecoveryStoreError> {
        let key = provider.load_or_create()?;
        let current = self.current_path(payload.metadata());
        let backup = self.backup_path(payload.metadata());
        if let Ok(bytes) = fs::read(&current)
            && decode(&bytes, payload.metadata(), &key).is_ok()
        {
            write_bytes(&backup, &bytes)?;
        }
        let envelope = encode(payload, &key)?;
        write_json(&current, &envelope)
    }

    /// Loads the current authenticated record or its last-valid fallback.
    ///
    /// # Errors
    ///
    /// Returns key-provider and IO errors directly. If candidates exist but no
    /// candidate authenticates, returns the most relevant validation error.
    pub fn load(
        &self,
        provider: &dyn RecoveryKeyProvider,
        metadata: &RecoveryMetadata,
    ) -> Result<Option<RecoveryPayload>, RecoveryStoreError> {
        let paths = [self.current_path(metadata), self.backup_path(metadata)];
        if paths.iter().all(|path| !path.exists()) {
            return Ok(None);
        }
        let key = provider.load_or_create()?;
        let mut last_error = None;
        for path in paths {
            match fs::read(path) {
                Ok(bytes) => match decode(&bytes, metadata, &key) {
                    Ok(payload) => return Ok(Some(payload)),
                    Err(error) => last_error = Some(error),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(RecoveryStoreError::Io(error)),
            }
        }
        Err(last_error.unwrap_or(RecoveryStoreError::Authentication))
    }

    /// Deletes both current and last-valid records after confirmed save/discard.
    ///
    /// # Errors
    ///
    /// Returns non-missing filesystem errors.
    pub fn delete(&self, metadata: &RecoveryMetadata) -> Result<(), RecoveryStoreError> {
        for path in [self.current_path(metadata), self.backup_path(metadata)] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(RecoveryStoreError::Io(error)),
            }
        }
        Ok(())
    }
}

fn encode(
    payload: &RecoveryPayload,
    key: &RecoveryKey,
) -> Result<RecoveryEnvelope, RecoveryStoreError> {
    let plaintext = serde_json::to_vec(payload).map_err(RecoveryStoreError::Json)?;
    let aad = authenticated_bytes(payload.metadata())?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| RecoveryStoreError::InvalidKeyLength)?;
    let nonce = XNonce::generate();
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| RecoveryStoreError::Encryption)?;
    Ok(RecoveryEnvelope {
        schema_version: CURRENT_SCHEMA,
        nonce: nonce.into(),
        ciphertext,
    })
}

fn decode(
    bytes: &[u8],
    metadata: &RecoveryMetadata,
    key: &RecoveryKey,
) -> Result<RecoveryPayload, RecoveryStoreError> {
    let envelope: RecoveryEnvelope =
        serde_json::from_slice(bytes).map_err(RecoveryStoreError::Json)?;
    if envelope.schema_version != CURRENT_SCHEMA {
        return Err(RecoveryStoreError::UnsupportedSchema(
            envelope.schema_version,
        ));
    }
    let aad = authenticated_bytes(metadata)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| RecoveryStoreError::InvalidKeyLength)?;
    let nonce = XNonce::try_from(envelope.nonce.as_slice())
        .map_err(|_| RecoveryStoreError::Authentication)?;
    let plaintext = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &envelope.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| RecoveryStoreError::Authentication)?;
    let payload: RecoveryPayload =
        serde_json::from_slice(&plaintext).map_err(RecoveryStoreError::Json)?;
    if payload.metadata != *metadata {
        return Err(RecoveryStoreError::Authentication);
    }
    Ok(payload)
}

fn authenticated_bytes(metadata: &RecoveryMetadata) -> Result<Vec<u8>, RecoveryStoreError> {
    serde_json::to_vec(&AuthenticatedMetadata {
        schema_version: CURRENT_SCHEMA,
        metadata,
    })
    .map_err(RecoveryStoreError::Json)
}

fn record_id(metadata: &RecoveryMetadata) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(metadata.workspace_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(metadata.document_path.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn write_json(path: &Path, envelope: &RecoveryEnvelope) -> Result<(), RecoveryStoreError> {
    let bytes = serde_json::to_vec(envelope).map_err(RecoveryStoreError::Json)?;
    write_bytes(path, &bytes)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), RecoveryStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(RecoveryStoreError::Io)?;
    }
    let mut file = AtomicWriteFile::open(path).map_err(RecoveryStoreError::Io)?;
    file.write_all(bytes).map_err(RecoveryStoreError::Io)?;
    file.commit().map_err(RecoveryStoreError::Io)
}

#[derive(Debug, Error)]
pub enum RecoveryKeyError {
    #[error("protected recovery key storage is unavailable: {0}")]
    Unavailable(String),
    #[error("protected recovery key provider failed: {0}")]
    Provider(String),
}

#[derive(Debug, Error)]
pub enum RecoveryStoreError {
    #[error("platform application-data directory is unavailable")]
    ApplicationDataUnavailable,
    #[error(transparent)]
    Key(#[from] RecoveryKeyError),
    #[error("recovery key has an invalid length")]
    InvalidKeyLength,
    #[error("recovery encryption failed")]
    Encryption,
    #[error("recovery authentication failed")]
    Authentication,
    #[error("unsupported recovery schema {0}")]
    UnsupportedSchema(u32),
    #[error("recovery IO failed: {0}")]
    Io(#[source] io::Error),
    #[error("recovery serialization failed: {0}")]
    Json(#[source] serde_json::Error),
}
