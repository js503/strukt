//! UI-independent editor domain.

mod buffer;
mod document;
mod find;
mod grammar;
mod history;
mod position;
mod tabs;
mod transaction;

pub use buffer::{LineEnding, TextBuffer};
pub use document::{
    DiskRevision, Document, DocumentError, DocumentId, DocumentStatus, RelativeDocumentPath,
};
pub use find::{FindError, FindMatch, FindOptions, FindQuery, FindResult};
pub use grammar::{GrammarDescriptor, GrammarRegistry, PLAIN_TEXT_GRAMMAR};
pub use history::{EditKind, History, HistoryBudget, HistoryEntry, HistoryError};
pub use position::CharRange;
pub use tabs::{
    CloseDecision, CloseOutcome, EditorTabView, EditorViewState, EditorWorkspace,
    EditorWorkspaceError, OpenDisposition,
};
pub use transaction::{
    AppliedTransaction, EditTransaction, Replacement, Revision, TransactionError,
};
