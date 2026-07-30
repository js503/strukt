use serde::{Deserialize, Serialize};

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
}

impl WorkspaceState {
    #[must_use]
    pub fn new(root: WorkspaceRoot) -> Self {
        Self {
            root,
            explorer: ExplorerState::default(),
            stale_filesystem: false,
        }
    }
}
