use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use strukt_terminal::{
    TerminalTabSnapshot, TerminalWorkspace, TerminalWorkspaceError, TerminalWorkspaceSnapshot,
};
use strukt_workspace::WorkspaceState;
use thiserror::Error;

pub const TERMINAL_CONTRIBUTION_ID: &str = "strukt.terminal";
const TERMINAL_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalSessionSnapshot {
    pub schema_version: u32,
    #[serde(default)]
    pub tabs: Vec<TerminalTabSnapshot>,
    #[serde(default)]
    pub active_tab: Option<strukt_terminal::TerminalTabId>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl TerminalSessionSnapshot {
    #[must_use]
    pub fn from_workspace(workspace: &TerminalWorkspace) -> Self {
        let snapshot = workspace.snapshot();
        Self {
            schema_version: TERMINAL_SCHEMA_VERSION,
            tabs: snapshot.tabs,
            active_tab: snapshot.active_tab,
            extensions: BTreeMap::new(),
        }
    }

    /// Restores validated terminal presentation state with every pane stopped.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalStoreError::UnsupportedSchema`] for a version other
    /// than 1, or [`TerminalStoreError::InvalidLayout`] for malformed state.
    pub fn restore(&self) -> Result<TerminalWorkspace, TerminalStoreError> {
        if self.schema_version != TERMINAL_SCHEMA_VERSION {
            return Err(TerminalStoreError::UnsupportedSchema(self.schema_version));
        }
        TerminalWorkspace::restore(TerminalWorkspaceSnapshot {
            tabs: self.tabs.clone(),
            active_tab: self.active_tab,
        })
        .map_err(TerminalStoreError::InvalidLayout)
    }
}

/// Stores the terminal contribution without altering opaque sibling payloads.
///
/// # Errors
///
/// Returns [`TerminalStoreError::Serialization`] if the snapshot cannot be
/// represented as JSON.
pub fn set_terminal_contribution(
    state: &mut WorkspaceState,
    snapshot: &TerminalSessionSnapshot,
) -> Result<(), TerminalStoreError> {
    state
        .set_contribution(TERMINAL_CONTRIBUTION_ID, snapshot)
        .map_err(|error| TerminalStoreError::Serialization(error.to_string()))
}

/// Decodes and validates the optional terminal workspace contribution.
///
/// # Errors
///
/// Returns [`TerminalStoreError::MalformedContribution`] for invalid JSON and
/// the same validation errors as [`TerminalSessionSnapshot::restore`].
pub fn terminal_contribution(
    state: &WorkspaceState,
) -> Result<Option<TerminalSessionSnapshot>, TerminalStoreError> {
    let Some(value) = state.contributions.get(TERMINAL_CONTRIBUTION_ID) else {
        return Ok(None);
    };
    let snapshot = serde_json::from_value::<TerminalSessionSnapshot>(value.clone())
        .map_err(|error| TerminalStoreError::MalformedContribution(error.to_string()))?;
    snapshot.restore()?;
    Ok(Some(snapshot))
}

pub(crate) fn contribution_is_valid(state: &WorkspaceState) -> bool {
    terminal_contribution(state).is_ok()
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TerminalStoreError {
    #[error("unsupported terminal schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid terminal layout: {0}")]
    InvalidLayout(#[source] TerminalWorkspaceError),
    #[error("terminal contribution serialization failed: {0}")]
    Serialization(String),
    #[error("terminal contribution is malformed: {0}")]
    MalformedContribution(String),
}
