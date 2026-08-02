#![forbid(unsafe_code)]

mod editor_store;
mod language_store;
mod terminal_store;
mod workspace_store;

pub use editor_store::{
    EditorRecoveryStore, EditorSessionSnapshot, EditorSessionStore, EditorTabSnapshot,
    RecoveryEnvelope, RecoveryKey, RecoveryKeyError, RecoveryKeyProvider, RecoveryMetadata,
    RecoveryPayload, RecoveryStoreError,
};
pub use language_store::{
    ApprovalSnapshot, LANGUAGE_CONTRIBUTION_ID, LanguageSelectionSnapshot, LanguageSessionSnapshot,
    LanguageStoreError, RestoredLanguageSession, language_contribution, set_language_contribution,
};
pub use terminal_store::{
    TERMINAL_CONTRIBUTION_ID, TerminalSessionSnapshot, TerminalStoreError,
    set_terminal_contribution, terminal_contribution,
};

pub use workspace_store::{RecentWorkspaces, StoreError, WorkspaceSnapshot, WorkspaceStore};
