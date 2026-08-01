#![forbid(unsafe_code)]

mod editor_store;
mod workspace_store;

pub use editor_store::{
    EditorRecoveryStore, EditorSessionSnapshot, EditorSessionStore, EditorTabSnapshot,
    RecoveryEnvelope, RecoveryKey, RecoveryKeyError, RecoveryKeyProvider, RecoveryMetadata,
    RecoveryPayload, RecoveryStoreError,
};

pub use workspace_store::{RecentWorkspaces, StoreError, WorkspaceSnapshot, WorkspaceStore};
