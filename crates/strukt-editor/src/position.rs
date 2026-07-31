use crate::TransactionError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharRange {
    pub start: usize,
    pub end: usize,
}

impl CharRange {
    /// Creates an ordered character range.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError::InvalidRange`] when `start` exceeds `end`.
    pub fn new(start: usize, end: usize) -> Result<Self, TransactionError> {
        if start > end {
            return Err(TransactionError::InvalidRange { start, end });
        }
        Ok(Self { start, end })
    }
}
