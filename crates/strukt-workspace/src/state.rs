use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::WorkspaceRoot;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExplorerState {
    pub visible: bool,
    pub show_hidden: bool,
    pub show_ignored: bool,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            visible: true,
            show_hidden: false,
            show_ignored: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceState {
    pub root: WorkspaceRoot,
    pub explorer: ExplorerState,
    pub stale_filesystem: bool,
    #[serde(default)]
    pub contributions: BTreeMap<String, serde_json::Value>,
}

impl WorkspaceState {
    #[must_use]
    pub fn new(root: WorkspaceRoot) -> Self {
        Self {
            root,
            explorer: ExplorerState::default(),
            stale_filesystem: false,
            contributions: BTreeMap::new(),
        }
    }

    /// Stores a versioned subsystem payload without coupling the workspace core
    /// to that subsystem's schema.
    ///
    /// # Errors
    ///
    /// Returns a serialization error when `value` cannot be represented as JSON.
    pub fn set_contribution<T: Serialize>(
        &mut self,
        id: impl Into<String>,
        value: &T,
    ) -> Result<(), serde_json::Error> {
        self.contributions
            .insert(id.into(), serde_json::to_value(value)?);
        Ok(())
    }

    /// Decodes a subsystem payload while preserving unknown payloads untouched.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error when the stored payload does not match `T`.
    pub fn contribution<T: for<'de> Deserialize<'de>>(
        &self,
        id: &str,
    ) -> Result<Option<T>, serde_json::Error> {
        self.contributions
            .get(id)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
    }
}
