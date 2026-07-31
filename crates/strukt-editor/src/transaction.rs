use crate::CharRange;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Self = Self(0);

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn next(self) -> Result<Self, TransactionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(TransactionError::RevisionExhausted)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub range: CharRange,
    pub text: String,
}

impl Replacement {
    pub fn new(range: CharRange, text: impl Into<String>) -> Self {
        Self {
            range,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditTransaction {
    pub expected_revision: Revision,
    pub replacements: Vec<Replacement>,
}

impl EditTransaction {
    /// Creates a transaction with ordered, non-overlapping replacements.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError::OverlappingRanges`] when any replacements
    /// overlap.
    pub fn new(
        expected_revision: Revision,
        mut replacements: Vec<Replacement>,
    ) -> Result<Self, TransactionError> {
        replacements.sort_by_key(|replacement| (replacement.range.start, replacement.range.end));
        if replacements
            .windows(2)
            .any(|pair| pair[1].range.start < pair[0].range.end)
        {
            return Err(TransactionError::OverlappingRanges);
        }
        Ok(Self {
            expected_revision,
            replacements,
        })
    }

    pub fn insert(expected_revision: Revision, at: usize, text: impl Into<String>) -> Self {
        Self {
            expected_revision,
            replacements: vec![Replacement::new(CharRange { start: at, end: at }, text)],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedTransaction {
    pub revision: Revision,
    pub inverse: EditTransaction,
    pub inserted_bytes: usize,
    pub removed_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransactionError {
    #[error("range start {start} exceeds end {end}")]
    InvalidRange { start: usize, end: usize },
    #[error("replacement ranges overlap")]
    OverlappingRanges,
    #[error("range end {end} exceeds buffer character length {char_len}")]
    RangeOutOfBounds { end: usize, char_len: usize },
    #[error("stale revision: expected {expected:?}, received {actual:?}")]
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    #[error("document revision space is exhausted")]
    RevisionExhausted,
}
