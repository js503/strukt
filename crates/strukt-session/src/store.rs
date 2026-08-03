use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{CatalogError, PaneId, PaneScreenSnapshot, SessionCatalog};

const CURRENT_SCHEMA: u16 = 1;
const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;
const MAX_HISTORIES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PaneHistorySnapshot {
    pane: PaneId,
    screen: PaneScreenSnapshot,
}

impl PaneHistorySnapshot {
    #[must_use]
    pub const fn new(pane: PaneId, screen: PaneScreenSnapshot) -> Self {
        Self { pane, screen }
    }

    #[must_use]
    pub const fn pane(&self) -> PaneId {
        self.pane
    }

    #[must_use]
    pub const fn screen(&self) -> &PaneScreenSnapshot {
        &self.screen
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersistedCatalog {
    schema_version: u16,
    catalog: SessionCatalog,
    histories: Vec<PaneHistorySnapshot>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl PersistedCatalog {
    /// Creates a validated stopped-only persistence record.
    ///
    /// # Errors
    ///
    /// Returns catalog or history validation errors.
    pub fn new(
        catalog: &SessionCatalog,
        histories: Vec<PaneHistorySnapshot>,
    ) -> Result<Self, SessionStoreError> {
        let record = Self {
            schema_version: CURRENT_SCHEMA,
            catalog: catalog.stopped_clone()?,
            histories,
            extra: BTreeMap::new(),
        };
        record.validate()?;
        Ok(record)
    }

    #[must_use]
    pub const fn catalog(&self) -> &SessionCatalog {
        &self.catalog
    }

    #[must_use]
    pub fn histories(&self) -> &[PaneHistorySnapshot] {
        &self.histories
    }

    fn normalized(&self) -> Result<Self, SessionStoreError> {
        let record = Self {
            schema_version: CURRENT_SCHEMA,
            catalog: self.catalog.stopped_clone()?,
            histories: self.histories.clone(),
            extra: self.extra.clone(),
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<(), SessionStoreError> {
        if self.schema_version != CURRENT_SCHEMA {
            return Err(SessionStoreError::UnsupportedSchema(self.schema_version));
        }
        self.catalog.validate()?;
        if self.histories.len() > MAX_HISTORIES {
            return Err(SessionStoreError::InvalidHistory);
        }
        let mut panes = BTreeSet::new();
        if self
            .histories
            .iter()
            .any(|history| !self.catalog.contains_pane(history.pane) || !panes.insert(history.pane))
        {
            return Err(SessionStoreError::InvalidHistory);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn current_path(&self) -> PathBuf {
        self.root.join("session-catalog.json")
    }

    #[must_use]
    pub fn backup_path(&self) -> PathBuf {
        self.root.join("session-catalog.last-valid.json")
    }

    /// Atomically saves a stopped-only record and retains the last valid value.
    ///
    /// # Errors
    ///
    /// Returns validation, serialization, size, or IO errors.
    pub fn save(&self, record: &PersistedCatalog) -> Result<(), SessionStoreError> {
        let record = record.normalized()?;
        let current = self.current_path();
        let backup = self.backup_path();
        if let Ok(bytes) = read_bounded(&current)
            && decode_record(&bytes).is_ok()
        {
            write_bytes(&backup, &bytes)?;
        }
        let bytes = serde_json::to_vec_pretty(&record).map_err(SessionStoreError::Json)?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(SessionStoreError::TooLarge);
        }
        write_bytes(&current, &bytes)
    }

    /// Loads the current valid record or its last-valid fallback.
    ///
    /// # Errors
    ///
    /// Returns the most relevant validation, size, serialization, or IO error when
    /// candidates exist but neither is valid.
    pub fn load(&self) -> Result<Option<PersistedCatalog>, SessionStoreError> {
        let paths = [self.current_path(), self.backup_path()];
        if paths.iter().all(|path| !path.exists()) {
            return Ok(None);
        }
        let mut last_error = None;
        for path in paths {
            match read_bounded(&path) {
                Ok(bytes) => match decode_record(&bytes) {
                    Ok(record) => return Ok(Some(record.normalized()?)),
                    Err(error) => last_error = Some(error),
                },
                Err(SessionStoreError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(SessionStoreError::InvalidCatalog))
    }
}

fn decode_record(bytes: &[u8]) -> Result<PersistedCatalog, SessionStoreError> {
    let value: Value = serde_json::from_slice(bytes).map_err(SessionStoreError::Json)?;
    let schema = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|schema| u16::try_from(schema).ok())
        .ok_or(SessionStoreError::InvalidCatalog)?;
    if schema != CURRENT_SCHEMA {
        return Err(SessionStoreError::UnsupportedSchema(schema));
    }
    let record: PersistedCatalog =
        serde_json::from_value(value).map_err(SessionStoreError::Json)?;
    record.validate()?;
    Ok(record)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, SessionStoreError> {
    let metadata = fs::metadata(path).map_err(SessionStoreError::Io)?;
    if metadata.len() > MAX_STORE_BYTES as u64 {
        return Err(SessionStoreError::TooLarge);
    }
    let bytes = fs::read(path).map_err(SessionStoreError::Io)?;
    if bytes.len() > MAX_STORE_BYTES {
        return Err(SessionStoreError::TooLarge);
    }
    Ok(bytes)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), SessionStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(SessionStoreError::Io)?;
    }
    let mut file = AtomicWriteFile::open(path).map_err(SessionStoreError::Io)?;
    file.write_all(bytes).map_err(SessionStoreError::Io)?;
    file.commit().map_err(SessionStoreError::Io)
}

#[derive(Debug, Error)]
pub enum SessionStoreError {
    #[error("unsupported session catalog schema {0}")]
    UnsupportedSchema(u16),
    #[error("session catalog is invalid")]
    InvalidCatalog,
    #[error("session history is invalid")]
    InvalidHistory,
    #[error("session catalog exceeds the aggregate byte limit")]
    TooLarge,
    #[error("session catalog IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("session catalog serialization failed: {0}")]
    Json(#[source] serde_json::Error),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}
