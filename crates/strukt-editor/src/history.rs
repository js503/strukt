use crate::{AppliedTransaction, EditTransaction, Revision};
use std::collections::VecDeque;
use thiserror::Error;

const DEFAULT_MAX_ENTRIES: usize = 10_000;
const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryBudget {
    max_entries: usize,
    max_bytes: usize,
}

impl HistoryBudget {
    #[must_use]
    pub const fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }
}

impl Default for HistoryBudget {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES, DEFAULT_MAX_BYTES)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    forward: EditTransaction,
    inverse: EditTransaction,
    kind: EditKind,
    cursor_before: usize,
    cursor_after: usize,
    byte_cost: usize,
}

impl HistoryEntry {
    #[must_use]
    pub fn from_applied(
        forward: EditTransaction,
        applied: AppliedTransaction,
        kind: EditKind,
        cursor_before: usize,
        cursor_after: usize,
    ) -> Self {
        Self {
            forward,
            inverse: applied.inverse,
            kind,
            cursor_before,
            cursor_after,
            byte_cost: applied.inserted_bytes + applied.removed_bytes,
        }
    }
}

#[derive(Debug)]
pub struct History {
    budget: HistoryBudget,
    undo: VecDeque<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    retained_bytes: usize,
}

impl History {
    #[must_use]
    pub fn new(budget: HistoryBudget) -> Self {
        Self {
            budget,
            undo: VecDeque::new(),
            redo: Vec::new(),
            retained_bytes: 0,
        }
    }

    pub fn record(&mut self, entry: HistoryEntry) {
        self.redo.clear();
        if let Some(previous) = self.undo.back_mut()
            && coalesce_typing(previous, &entry)
        {
            self.retained_bytes = self.retained_bytes.saturating_add(entry.byte_cost);
        } else {
            self.retained_bytes = self.retained_bytes.saturating_add(entry.byte_cost);
            self.undo.push_back(entry);
        }
        self.enforce_budget();
    }

    /// Returns the next undo transaction retargeted to `current_revision`.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::NothingToUndo`] when the undo stack is empty.
    pub fn undo(&mut self, current_revision: Revision) -> Result<EditTransaction, HistoryError> {
        let entry = self.undo.pop_back().ok_or(HistoryError::NothingToUndo)?;
        self.retained_bytes = self.retained_bytes.saturating_sub(entry.byte_cost);
        let transaction = retarget(&entry.inverse, current_revision);
        self.redo.push(entry);
        Ok(transaction)
    }

    /// Returns the next redo transaction retargeted to `current_revision`.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::NothingToRedo`] when the redo stack is empty.
    pub fn redo(&mut self, current_revision: Revision) -> Result<EditTransaction, HistoryError> {
        let entry = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        let transaction = retarget(&entry.forward, current_revision);
        self.retained_bytes = self.retained_bytes.saturating_add(entry.byte_cost);
        self.undo.push_back(entry);
        self.enforce_budget();
        Ok(transaction)
    }

    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    fn enforce_budget(&mut self) {
        while self.undo.len() > self.budget.max_entries
            || self.retained_bytes > self.budget.max_bytes
        {
            let Some(entry) = self.undo.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(entry.byte_cost);
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(HistoryBudget::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HistoryError {
    #[error("there is no edit to undo")]
    NothingToUndo,
    #[error("there is no edit to redo")]
    NothingToRedo,
}

fn retarget(transaction: &EditTransaction, revision: Revision) -> EditTransaction {
    EditTransaction {
        expected_revision: revision,
        replacements: transaction.replacements.clone(),
    }
}

fn coalesce_typing(previous: &mut HistoryEntry, next: &HistoryEntry) -> bool {
    if previous.kind != EditKind::Typing
        || next.kind != EditKind::Typing
        || previous.cursor_after != next.cursor_before
        || previous.forward.replacements.len() != 1
        || next.forward.replacements.len() != 1
        || previous.inverse.replacements.len() != 1
        || next.inverse.replacements.len() != 1
    {
        return false;
    }
    let previous_forward = &previous.forward.replacements[0];
    let next_forward = &next.forward.replacements[0];
    if previous_forward.range.start != previous_forward.range.end
        || next_forward.range.start != next_forward.range.end
        || previous_forward.range.start + previous_forward.text.chars().count()
            != next_forward.range.start
        || !previous.inverse.replacements[0].text.is_empty()
        || !next.inverse.replacements[0].text.is_empty()
    {
        return false;
    }
    previous.forward.replacements[0]
        .text
        .push_str(&next_forward.text);
    previous.inverse.replacements[0].range.end = next.inverse.replacements[0].range.end;
    previous.inverse.expected_revision = next.inverse.expected_revision;
    previous.cursor_after = next.cursor_after;
    previous.byte_cost = previous.byte_cost.saturating_add(next.byte_cost);
    true
}
