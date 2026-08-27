use std::collections::BTreeSet;

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId(pub String);

impl CommandId {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
            && self.0.len() <= 128
            && self.0.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
            })
    }
}

impl std::fmt::Display for CommandId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionBoundary {
    Local,
    Remote,
    Interface,
}

impl ExecutionBoundary {
    #[must_use]
    pub fn label(self, remote_alias: Option<&str>) -> &str {
        match self {
            Self::Local => "LOCAL",
            Self::Remote => remote_alias
                .filter(|alias| !alias.is_empty())
                .unwrap_or("REMOTE"),
            Self::Interface => "INTERFACE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandContribution {
    pub id: CommandId,
    pub title: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub shortcut: Option<String>,
    pub boundary: ExecutionBoundary,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CommandMatch<'a> {
    pub command: &'a CommandContribution,
    pub score: u16,
}

#[derive(Default)]
pub struct CommandCatalog {
    commands: Vec<CommandContribution>,
    ids: BTreeSet<CommandId>,
}

impl CommandCatalog {
    /// Registers a command while preserving insertion order.
    ///
    /// # Errors
    ///
    /// Returns an error when the identifier or display metadata is invalid, or
    /// when the identifier was already registered.
    pub fn register(&mut self, command: CommandContribution) -> Result<(), CommandError> {
        if !command.id.is_valid()
            || command.title.trim().is_empty()
            || command.category.trim().is_empty()
        {
            return Err(CommandError::Invalid(command.id));
        }
        if !self.ids.insert(command.id.clone()) {
            return Err(CommandError::DuplicateId(command.id));
        }
        self.commands.push(command);
        Ok(())
    }

    #[must_use]
    pub fn search(&self, query: &str) -> Vec<CommandMatch<'_>> {
        self.search_in_category(query, None)
    }

    #[must_use]
    pub fn search_in_category(&self, query: &str, category: Option<&str>) -> Vec<CommandMatch<'_>> {
        let tokens = query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let category = category.map(str::to_lowercase);
        let mut matches = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                category
                    .as_ref()
                    .is_none_or(|category| command.category.to_lowercase() == *category)
            })
            .filter_map(|(index, command)| {
                command_score(command, &tokens).map(|score| (index, command, score))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(index, _, score)| (std::cmp::Reverse(*score), *index));
        matches
            .into_iter()
            .map(|(_, command, score)| CommandMatch { command, score })
            .collect()
    }

    #[must_use]
    pub fn select(&self, index: usize, matches: &[CommandMatch<'_>]) -> Option<CommandId> {
        matches.get(index).map(|entry| entry.command.id.clone())
    }
}

fn command_score(command: &CommandContribution, tokens: &[String]) -> Option<u16> {
    if tokens.is_empty() {
        return Some(0);
    }
    let title = command.title.to_lowercase();
    let category = command.category.to_lowercase();
    let id = command.id.0.to_lowercase();
    let keywords = command
        .keywords
        .iter()
        .map(|keyword| keyword.to_lowercase())
        .collect::<Vec<_>>();
    let mut score = 0_u16;
    for token in tokens {
        if title.starts_with(token) {
            score = score.saturating_add(30);
        } else if title.contains(token) {
            score = score.saturating_add(20);
        } else if keywords.iter().any(|keyword| keyword.contains(token)) {
            score = score.saturating_add(12);
        } else if category.contains(token) || id.contains(token) {
            score = score.saturating_add(8);
        } else {
            return None;
        }
    }
    Some(score)
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error("command `{0}` is invalid")]
    Invalid(CommandId),
    #[error("command `{0}` is already registered")]
    DuplicateId(CommandId),
}
