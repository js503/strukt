use crate::{AppliedTransaction, EditTransaction, Replacement, Revision, TransactionError};
use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    None,
    Lf,
    CrLf,
    Mixed,
}

pub struct TextBuffer {
    text: Rope,
    revision: Revision,
    line_ending: LineEnding,
}

impl TextBuffer {
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            text: Rope::from_str(text),
            revision: Revision::INITIAL,
            line_ending: detect_line_ending(text),
        }
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Applies all replacements atomically and returns their inverse.
    ///
    /// # Errors
    ///
    /// Returns an error for stale revisions, out-of-bounds ranges, or exhausted
    /// revision space. The buffer is unchanged on error.
    pub fn apply(
        &mut self,
        transaction: EditTransaction,
    ) -> Result<AppliedTransaction, TransactionError> {
        let EditTransaction {
            expected_revision,
            replacements,
        } = transaction;
        if expected_revision != self.revision {
            return Err(TransactionError::StaleRevision {
                expected: self.revision,
                actual: expected_revision,
            });
        }
        let char_len = self.text.len_chars();
        if let Some(end) = replacements
            .iter()
            .map(|replacement| replacement.range.end)
            .find(|end| *end > char_len)
        {
            return Err(TransactionError::RangeOutOfBounds { end, char_len });
        }
        let next_revision = self.revision.next()?;
        let mut inverse = Vec::with_capacity(replacements.len());
        let mut char_delta: isize = 0;
        let mut inserted_bytes = 0;
        let mut removed_bytes = 0;
        for replacement in &replacements {
            let removed = self
                .text
                .slice(replacement.range.start..replacement.range.end)
                .to_string();
            let inserted_chars = replacement.text.chars().count();
            let final_start = replacement.range.start.saturating_add_signed(char_delta);
            inverse.push(Replacement::new(
                crate::CharRange {
                    start: final_start,
                    end: final_start + inserted_chars,
                },
                removed.clone(),
            ));
            char_delta += inserted_chars.cast_signed()
                - (replacement.range.end - replacement.range.start).cast_signed();
            inserted_bytes += replacement.text.len();
            removed_bytes += removed.len();
        }
        for replacement in replacements.iter().rev() {
            self.text
                .remove(replacement.range.start..replacement.range.end);
            self.text.insert(replacement.range.start, &replacement.text);
        }
        self.revision = next_revision;
        self.line_ending = detect_line_ending(&self.text.to_string());
        Ok(AppliedTransaction {
            revision: next_revision,
            inverse: EditTransaction::new(next_revision, inverse)?,
            inserted_bytes,
            removed_bytes,
        })
    }
}

impl fmt::Display for TextBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.text.chunks() {
            formatter.write_str(chunk)?;
        }
        Ok(())
    }
}

fn detect_line_ending(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count().saturating_sub(crlf);
    match (crlf, lf) {
        (0, 0) => LineEnding::None,
        (0, _) => LineEnding::Lf,
        (_, 0) => LineEnding::CrLf,
        _ => LineEnding::Mixed,
    }
}
