#![forbid(unsafe_code)]

mod workspace_store;

pub use workspace_store::{RecentWorkspaces, StoreError, WorkspaceSnapshot, WorkspaceStore};
