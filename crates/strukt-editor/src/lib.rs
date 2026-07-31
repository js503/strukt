//! UI-independent editor domain.

mod buffer;
mod position;
mod transaction;

pub use buffer::{LineEnding, TextBuffer};
pub use position::CharRange;
pub use transaction::{
    AppliedTransaction, EditTransaction, Replacement, Revision, TransactionError,
};
