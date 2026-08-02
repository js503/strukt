#![forbid(unsafe_code)]

mod editor_store;
mod terminal_store;
mod workspace_store;

pub use editor_store::{
    EditorRecoveryStore, EditorSessionSnapshot, EditorSessionStore, EditorTabSnapshot,
    RecoveryEnvelope, RecoveryKey, RecoveryKeyError, RecoveryKeyProvider, RecoveryMetadata,
    RecoveryPayload, RecoveryStoreError,
};
pub use terminal_store::{
    TERMINAL_CONTRIBUTION_ID, TerminalSessionSnapshot, TerminalStoreError,
    set_terminal_contribution, terminal_contribution,
};

pub use workspace_store::{RecentWorkspaces, StoreError, WorkspaceSnapshot, WorkspaceStore};
