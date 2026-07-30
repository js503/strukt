#![forbid(unsafe_code)]

mod workspace_store;

pub use workspace_store::{StoreError, WorkspaceSnapshot, WorkspaceStore};
