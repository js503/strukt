use crate::{
    DiskRevision, Document, DocumentError, DocumentId, EditKind, EditTransaction,
    RelativeDocumentPath, Revision,
};
use strukt_workspace::WorkspaceId;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDisposition {
    Preview,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Save,
    Discard,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    Closed,
    NeedsDecision,
    SaveRequired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTabView {
    pub id: DocumentId,
    pub path: RelativeDocumentPath,
    pub pinned: bool,
    pub active: bool,
    pub recoverable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorViewState {
    pub workspace_id: WorkspaceId,
    pub tabs: Vec<EditorTabView>,
    pub active: Option<DocumentId>,
}

pub struct EditorWorkspace {
    workspace_id: WorkspaceId,
    documents: Vec<Document>,
    active: Option<DocumentId>,
    preview: Option<DocumentId>,
}

impl EditorWorkspace {
    #[must_use]
    pub const fn new(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            documents: Vec::new(),
            active: None,
            preview: None,
        }
    }

    /// Opens or focuses a document.
    ///
    /// # Errors
    ///
    /// This operation currently has no fallible state transition, but returns the
    /// shared workspace error type for command-handler consistency.
    pub fn open(
        &mut self,
        path: RelativeDocumentPath,
        text: &str,
        disk_revision: DiskRevision,
        read_only: bool,
        disposition: OpenDisposition,
    ) -> Result<DocumentId, EditorWorkspaceError> {
        if let Some(existing) = self
            .documents
            .iter()
            .find(|document| document.path() == &path)
            .map(Document::id)
        {
            if !read_only {
                self.document_mut(existing)?
                    .upgrade_read_only(text, disk_revision);
            }
            self.active = Some(existing);
            if disposition == OpenDisposition::Pinned {
                self.pin(existing)?;
            }
            return Ok(existing);
        }
        if disposition == OpenDisposition::Preview {
            self.clear_replaceable_preview();
        }
        let document = Document::new(path, text, disk_revision, read_only);
        let id = document.id();
        self.documents.push(document);
        self.active = Some(id);
        self.preview = (disposition == OpenDisposition::Preview).then_some(id);
        Ok(id)
    }

    /// Applies an edit and pins the document.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is absent or rejects the edit.
    pub fn edit(
        &mut self,
        id: DocumentId,
        transaction: EditTransaction,
        kind: EditKind,
        cursor_before: usize,
        cursor_after: usize,
    ) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?
            .edit(transaction, kind, cursor_before, cursor_after)?;
        self.pin(id)
    }

    /// Undoes the active document's latest edit and pins it.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is absent or has no undo entry.
    pub fn undo(&mut self, id: DocumentId) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?.undo()?;
        self.pin(id)
    }

    /// Redoes the active document's latest undone edit and pins it.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is absent or has no redo entry.
    pub fn redo(&mut self, id: DocumentId) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?.redo()?;
        self.pin(id)
    }

    /// Applies a revision-guarded successful save result.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent document or stale completion.
    pub fn complete_save(
        &mut self,
        id: DocumentId,
        expected: Revision,
        disk_revision: DiskRevision,
    ) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?
            .complete_save(expected, disk_revision)?;
        Ok(())
    }

    /// Focuses an already-open document.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is absent.
    pub fn select(&mut self, id: DocumentId) -> Result<(), EditorWorkspaceError> {
        self.document(id)
            .ok_or(EditorWorkspaceError::DocumentNotFound(id))?;
        self.active = Some(id);
        Ok(())
    }

    /// Promotes a preview document to a pinned tab.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is absent.
    pub fn pin_document(&mut self, id: DocumentId) -> Result<(), EditorWorkspaceError> {
        self.pin(id)
    }

    /// Marks a document as restored from recovery and pins it.
    ///
    /// # Errors
    ///
    /// Returns [`EditorWorkspaceError::DocumentNotFound`] when absent.
    pub fn mark_recovered(&mut self, id: DocumentId) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?.mark_recovered();
        self.pin(id)
    }

    /// Applies an external change to a document.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent document or rejected event.
    pub fn observe_disk_change(
        &mut self,
        id: DocumentId,
        expected: Revision,
        disk_revision: DiskRevision,
        text: &str,
    ) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?
            .observe_disk_change(expected, disk_revision, text)?;
        if self.document(id).is_some_and(Document::is_recoverable) {
            self.pin(id)?;
        }
        Ok(())
    }

    /// Marks a document's path missing.
    ///
    /// # Errors
    ///
    /// Returns an error for an absent document or stale event.
    pub fn observe_missing(
        &mut self,
        id: DocumentId,
        expected: Revision,
    ) -> Result<(), EditorWorkspaceError> {
        self.document_mut(id)?.observe_missing(expected)?;
        self.pin(id)
    }

    /// Requests a close without discarding recoverable content.
    ///
    /// # Errors
    ///
    /// Returns [`EditorWorkspaceError::DocumentNotFound`] when absent.
    pub fn request_close(&mut self, id: DocumentId) -> Result<CloseOutcome, EditorWorkspaceError> {
        if self
            .document(id)
            .ok_or(EditorWorkspaceError::DocumentNotFound(id))?
            .is_recoverable()
        {
            return Ok(CloseOutcome::NeedsDecision);
        }
        self.remove(id);
        Ok(CloseOutcome::Closed)
    }

    /// Resolves a close decision.
    ///
    /// # Errors
    ///
    /// Returns [`EditorWorkspaceError::DocumentNotFound`] when absent.
    pub fn resolve_close(
        &mut self,
        id: DocumentId,
        decision: CloseDecision,
    ) -> Result<CloseOutcome, EditorWorkspaceError> {
        self.document(id)
            .ok_or(EditorWorkspaceError::DocumentNotFound(id))?;
        Ok(match decision {
            CloseDecision::Cancel => CloseOutcome::Cancelled,
            CloseDecision::Save => CloseOutcome::SaveRequired,
            CloseDecision::Discard => {
                self.remove(id);
                CloseOutcome::Closed
            }
        })
    }

    #[must_use]
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.documents.iter().find(|document| document.id() == id)
    }

    #[must_use]
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    #[must_use]
    pub const fn active_document_id(&self) -> Option<DocumentId> {
        self.active
    }

    #[must_use]
    pub fn view_state(&self) -> EditorViewState {
        EditorViewState {
            workspace_id: self.workspace_id.clone(),
            tabs: self
                .documents
                .iter()
                .map(|document| EditorTabView {
                    id: document.id(),
                    path: document.path().clone(),
                    pinned: self.preview != Some(document.id()),
                    active: self.active == Some(document.id()),
                    recoverable: document.is_recoverable(),
                })
                .collect(),
            active: self.active,
        }
    }

    fn document_mut(&mut self, id: DocumentId) -> Result<&mut Document, EditorWorkspaceError> {
        self.documents
            .iter_mut()
            .find(|document| document.id() == id)
            .ok_or(EditorWorkspaceError::DocumentNotFound(id))
    }

    fn pin(&mut self, id: DocumentId) -> Result<(), EditorWorkspaceError> {
        self.document(id)
            .ok_or(EditorWorkspaceError::DocumentNotFound(id))?;
        if self.preview == Some(id) {
            self.preview = None;
        }
        Ok(())
    }

    fn clear_replaceable_preview(&mut self) {
        let Some(id) = self.preview.take() else {
            return;
        };
        if self
            .document(id)
            .is_some_and(Document::is_preview_replaceable)
        {
            self.remove(id);
        }
    }

    fn remove(&mut self, id: DocumentId) {
        let Some(index) = self
            .documents
            .iter()
            .position(|document| document.id() == id)
        else {
            return;
        };
        self.documents.remove(index);
        if self.preview == Some(id) {
            self.preview = None;
        }
        if self.active == Some(id) {
            self.active = self
                .documents
                .get(index.saturating_sub(1))
                .or_else(|| self.documents.get(index))
                .map(Document::id);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EditorWorkspaceError {
    #[error("document {0:?} is not open")]
    DocumentNotFound(DocumentId),
    #[error(transparent)]
    Document(#[from] DocumentError),
}
