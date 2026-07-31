//! UI-independent editor domain.

mod buffer;
mod find;
mod history;
mod position;
mod transaction;

pub use buffer::{LineEnding, TextBuffer};
pub use find::{FindError, FindMatch, FindOptions, FindQuery, FindResult};
pub use history::{EditKind, History, HistoryBudget, HistoryEntry, HistoryError};
pub use position::CharRange;
pub use transaction::{
    AppliedTransaction, EditTransaction, Replacement, Revision, TransactionError,
};
