use crate::{CharRange, EditTransaction, Replacement, Revision, TransactionError};
use regex::{Regex, RegexBuilder};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl FindOptions {
    #[must_use]
    pub const fn regex() -> Self {
        Self {
            case_sensitive: true,
            whole_word: false,
            regex: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FindQuery {
    regex: Regex,
    whole_word: bool,
}

impl FindQuery {
    /// Compiles a literal or regular-expression query.
    ///
    /// # Errors
    ///
    /// Returns [`FindError::InvalidRegex`] when the expression is invalid.
    pub fn new(pattern: &str, options: FindOptions) -> Result<Self, FindError> {
        let source = if options.regex {
            pattern.to_owned()
        } else {
            regex::escape(pattern)
        };
        let regex = RegexBuilder::new(&source)
            .case_insensitive(!options.case_sensitive)
            .build()
            .map_err(|error| FindError::InvalidRegex(error.to_string()))?;
        Ok(Self {
            regex,
            whole_word: options.whole_word,
        })
    }

    #[must_use]
    pub fn find_all(&self, text: &str) -> FindResult {
        let matches = self
            .regex
            .find_iter(text)
            .filter(|found| {
                !self.whole_word || has_word_boundaries(text, found.start(), found.end())
            })
            .map(|found| FindMatch {
                range: CharRange {
                    start: byte_to_char(text, found.start()),
                    end: byte_to_char(text, found.end()),
                },
            })
            .collect();
        FindResult { matches }
    }

    /// Creates one transaction replacing every match.
    ///
    /// # Errors
    ///
    /// Returns an error when no matches exist or the resulting ranges are invalid.
    pub fn replace_all(
        &self,
        revision: Revision,
        text: &str,
        replacement: &str,
    ) -> Result<EditTransaction, FindError> {
        let mut replacements = Vec::new();
        for captures in self.regex.captures_iter(text) {
            let Some(found) = captures.get(0) else {
                continue;
            };
            if self.whole_word && !has_word_boundaries(text, found.start(), found.end()) {
                continue;
            }
            let mut expanded = String::new();
            captures.expand(replacement, &mut expanded);
            replacements.push(Replacement::new(
                CharRange {
                    start: byte_to_char(text, found.start()),
                    end: byte_to_char(text, found.end()),
                },
                expanded,
            ));
        }
        if replacements.is_empty() {
            return Err(FindError::NoMatches);
        }
        EditTransaction::new(revision, replacements).map_err(FindError::Transaction)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindMatch {
    pub range: CharRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindResult {
    matches: Vec<FindMatch>,
}

impl FindResult {
    #[must_use]
    pub fn matches(&self) -> &[FindMatch] {
        &self.matches
    }

    #[must_use]
    pub fn next_after(&self, position: usize) -> Option<FindMatch> {
        self.matches
            .iter()
            .copied()
            .find(|found| found.range.start > position)
            .or_else(|| self.matches.first().copied())
    }

    #[must_use]
    pub fn previous_before(&self, position: usize) -> Option<FindMatch> {
        self.matches
            .iter()
            .rev()
            .copied()
            .find(|found| found.range.end <= position)
            .or_else(|| self.matches.last().copied())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FindError {
    #[error("invalid regular expression: {0}")]
    InvalidRegex(String),
    #[error("the query has no matches")]
    NoMatches,
    #[error(transparent)]
    Transaction(#[from] TransactionError),
}

fn byte_to_char(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

fn has_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word) && !after.is_some_and(is_word)
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}
