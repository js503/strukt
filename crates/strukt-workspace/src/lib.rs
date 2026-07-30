#![forbid(unsafe_code)]

mod identity;
mod state;

pub use identity::{WorkspaceError, WorkspaceId, WorkspaceRoot};
pub use state::{ExplorerState, WorkspaceState};
