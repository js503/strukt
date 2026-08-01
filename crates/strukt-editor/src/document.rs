use crate::{EditKind, EditTransaction, History, HistoryEntry, HistoryError, Revision, TextBuffer};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelativeDocumentPath(String);

impl RelativeDocumentPath {
    /// Normalizes a workspace-relative document path.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::InvalidPath`] for empty, absolute, drive-prefixed,
    /// or parent-traversing paths.
    pub fn new(path: &str) -> Result<Self, DocumentError> {
        let normalized = path.replace('\\', "/");
        let invalid = normalized.is_empty()
            || normalized.starts_with('/')
            || normalized.starts_with("//")
            || normalized.as_bytes().get(1) == Some(&b':')
            || normalized
                .split('/')
                .any(|component| component.is_empty() || component == "..");
        if invalid {
            return Err(DocumentError::InvalidPath(path.to_owned()));
        }
        let normalized = normalized
            .split('/')
            .filter(|component| *component != ".")
            .collect::<Vec<_>>()
            .join("/");
        if normalized.is_empty() {
            return Err(DocumentError::InvalidPath(path.to_owned()));
        }
        Ok(Self(normalized))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiskRevision(String);

impl DiskRevision {
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentStatus {
    Clean,
    Dirty,
    Conflict {
        disk_revision: DiskRevision,
        disk_text: String,
    },
    Missing,
}

pub struct Document {
    id: DocumentId,
    path: RelativeDocumentPath,
    buffer: TextBuffer,
    history: History,
    saved_text: String,
    disk_revision: DiskRevision,
    status: DocumentStatus,
    read_only: bool,
    recovered: bool,
}

impl Document {
    #[must_use]
    pub fn new(
        path: RelativeDocumentPath,
        text: &str,
        disk_revision: DiskRevision,
        read_only: bool,
    ) -> Self {
        Self {
            id: DocumentId(NEXT_DOCUMENT_ID.fetch_add(1, Ordering::Relaxed)),
            path,
            buffer: TextBuffer::new(text),
            history: History::default(),
            saved_text: text.to_owned(),
            disk_revision,
            status: DocumentStatus::Clean,
            read_only,
            recovered: false,
        }
    }

    #[must_use]
    pub const fn id(&self) -> DocumentId {
        self.id
    }

    #[must_use]
    pub const fn path(&self) -> &RelativeDocumentPath {
        &self.path
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.buffer.to_string()
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.buffer.revision()
    }

    #[must_use]
    pub const fn disk_revision(&self) -> &DiskRevision {
        &self.disk_revision
    }

    #[must_use]
    pub const fn status(&self) -> &DocumentStatus {
        &self.status
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn upgrade_read_only(&mut self, text: &str, disk_revision: DiskRevision) {
        if !self.read_only {
            return;
        }
        self.buffer = TextBuffer::new(text);
        self.history = History::default();
        text.clone_into(&mut self.saved_text);
        self.disk_revision = disk_revision;
        self.status = DocumentStatus::Clean;
        self.read_only = false;
    }

    /// Applies an edit and records it in document history.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::ReadOnly`] or a transaction error.
    pub fn edit(
        &mut self,
        transaction: EditTransaction,
        kind: EditKind,
        cursor_before: usize,
        cursor_after: usize,
    ) -> Result<(), DocumentError> {
        if self.read_only {
            return Err(DocumentError::ReadOnly);
        }
        let forward = transaction.clone();
        let applied = self.buffer.apply(transaction)?;
        self.history.record(HistoryEntry::from_applied(
            forward,
            applied,
            kind,
            cursor_before,
            cursor_after,
        ));
        self.refresh_dirty_status();
        Ok(())
    }

    /// Undoes the most recent document edit.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no edit to undo or the inverse cannot apply.
    pub fn undo(&mut self) -> Result<(), DocumentError> {
        let transaction = self.history.undo(self.buffer.revision())?;
        self.buffer.apply(transaction)?;
        self.refresh_dirty_status();
        Ok(())
    }

    /// Redoes the most recently undone document edit.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no edit to redo or the transaction cannot
    /// apply.
    pub fn redo(&mut self) -> Result<(), DocumentError> {
        let transaction = self.history.redo(self.buffer.revision())?;
        self.buffer.apply(transaction)?;
        self.refresh_dirty_status();
        Ok(())
    }

    /// Applies a successful save result for the expected document revision.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::StaleEvent`] when editing advanced meanwhile.
    pub fn complete_save(
        &mut self,
        expected: Revision,
        disk_revision: DiskRevision,
    ) -> Result<(), DocumentError> {
        self.ensure_revision(expected)?;
        self.saved_text = self.buffer.to_string();
        self.disk_revision = disk_revision;
        self.status = DocumentStatus::Clean;
        self.recovered = false;
        Ok(())
    }

    /// Applies a revision-guarded external file change.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::StaleEvent`] for obsolete background results or a
    /// transaction error when a clean reload cannot be applied.
    pub fn observe_disk_change(
        &mut self,
        expected: Revision,
        disk_revision: DiskRevision,
        disk_text: &str,
    ) -> Result<(), DocumentError> {
        self.ensure_revision(expected)?;
        if self.status == DocumentStatus::Clean {
            let transaction = EditTransaction::new(
                self.buffer.revision(),
                vec![crate::Replacement::new(
                    crate::CharRange {
                        start: 0,
                        end: self.buffer.to_string().chars().count(),
                    },
                    disk_text,
                )],
            )?;
            self.buffer.apply(transaction)?;
            self.history = History::default();
            disk_text.clone_into(&mut self.saved_text);
            self.disk_revision = disk_revision;
            self.status = DocumentStatus::Clean;
        } else {
            self.status = DocumentStatus::Conflict {
                disk_revision,
                disk_text: disk_text.to_owned(),
            };
        }
        Ok(())
    }

    /// Marks the disk path missing for the expected document revision.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::StaleEvent`] for an obsolete event.
    pub fn observe_missing(&mut self, expected: Revision) -> Result<(), DocumentError> {
        self.ensure_revision(expected)?;
        self.status = DocumentStatus::Missing;
        Ok(())
    }

    /// Replaces local conflict content with the observed disk version while
    /// retaining a single undo boundary back to the local text.
    ///
    /// # Errors
    ///
    /// Returns an error when no conflict exists or the replacement cannot apply.
    pub fn reload_from_disk(&mut self) -> Result<(), DocumentError> {
        let DocumentStatus::Conflict {
            disk_revision,
            disk_text,
        } = self.status.clone()
        else {
            return Err(DocumentError::NoDiskConflict);
        };
        let transaction = EditTransaction::new(
            self.buffer.revision(),
            vec![crate::Replacement::new(
                crate::CharRange {
                    start: 0,
                    end: self.buffer.to_string().chars().count(),
                },
                &disk_text,
            )],
        )?;
        let forward = transaction.clone();
        let applied = self.buffer.apply(transaction)?;
        self.history.record(HistoryEntry::from_applied(
            forward,
            applied,
            EditKind::Other,
            0,
            0,
        ));
        self.saved_text = disk_text;
        self.disk_revision = disk_revision;
        self.status = DocumentStatus::Clean;
        self.recovered = false;
        Ok(())
    }

    /// Acknowledges an external conflict while preserving local content and
    /// the original save baseline.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::NoDiskConflict`] when no conflict exists.
    pub fn keep_editing(&mut self) -> Result<(), DocumentError> {
        if !matches!(self.status, DocumentStatus::Conflict { .. }) {
            return Err(DocumentError::NoDiskConflict);
        }
        self.status = DocumentStatus::Dirty;
        Ok(())
    }

    /// Restores encrypted unsaved text into this document.
    ///
    /// # Errors
    ///
    /// Returns an error when the recovery replacement cannot apply.
    pub fn restore_recovery(&mut self, text: &str) -> Result<(), DocumentError> {
        if self.buffer.to_string() != text {
            let transaction = EditTransaction::new(
                self.buffer.revision(),
                vec![crate::Replacement::new(
                    crate::CharRange {
                        start: 0,
                        end: self.buffer.to_string().chars().count(),
                    },
                    text,
                )],
            )?;
            self.buffer.apply(transaction)?;
            self.history = History::default();
        }
        self.mark_recovered();
        Ok(())
    }

    pub fn mark_recovered(&mut self) {
        self.recovered = true;
        if self.status == DocumentStatus::Clean {
            self.status = DocumentStatus::Dirty;
        }
    }

    #[must_use]
    pub fn is_recovered(&self) -> bool {
        self.recovered
    }

    #[must_use]
    pub fn is_recoverable(&self) -> bool {
        self.recovered || self.status != DocumentStatus::Clean
    }

    #[must_use]
    pub fn is_preview_replaceable(&self) -> bool {
        !self.is_recoverable()
    }

    fn ensure_revision(&self, expected: Revision) -> Result<(), DocumentError> {
        let actual = self.buffer.revision();
        if expected != actual {
            return Err(DocumentError::StaleEvent {
                expected: actual,
                actual: expected,
            });
        }
        Ok(())
    }

    fn refresh_dirty_status(&mut self) {
        if matches!(self.status, DocumentStatus::Clean | DocumentStatus::Dirty) {
            self.status = if self.buffer.to_string() == self.saved_text {
                DocumentStatus::Clean
            } else {
                DocumentStatus::Dirty
            };
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DocumentError {
    #[error("invalid workspace-relative document path: {0}")]
    InvalidPath(String),
    #[error("document is read-only")]
    ReadOnly,
    #[error("document has no unresolved disk conflict")]
    NoDiskConflict,
    #[error("stale document event: expected {expected:?}, received {actual:?}")]
    StaleEvent {
        expected: Revision,
        actual: Revision,
    },
    #[error(transparent)]
    Transaction(#[from] crate::TransactionError),
    #[error(transparent)]
    History(#[from] HistoryError),
}
