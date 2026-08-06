use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_ERROR_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProviderKind {
    NativeLocal,
    NativeRemote,
    Tmux,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NativeLocal => "native-local",
            Self::NativeRemote => "native-remote",
            Self::Tmux => "tmux",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProviderAction {
    Catalog,
    Attach,
    Detach,
    CreateSession,
    RenameSession,
    DuplicateSession,
    TerminateSession,
    MutateWindows,
    MutatePanes,
    StructuredHistory,
    Input,
    Resize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProviderCapabilities {
    pub catalog: bool,
    pub attach: bool,
    pub detach: bool,
    pub create_session: bool,
    pub rename_session: bool,
    pub duplicate_session: bool,
    pub terminate_session: bool,
    pub mutate_windows: bool,
    pub mutate_panes: bool,
    pub structured_history: bool,
    pub input: bool,
    pub resize: bool,
}

impl ProviderCapabilities {
    #[must_use]
    pub const fn native_local() -> Self {
        Self {
            catalog: true,
            attach: true,
            detach: true,
            create_session: true,
            rename_session: true,
            duplicate_session: true,
            terminate_session: true,
            mutate_windows: true,
            mutate_panes: true,
            structured_history: true,
            input: true,
            resize: true,
        }
    }

    #[must_use]
    pub const fn read_only() -> Self {
        Self {
            catalog: true,
            attach: true,
            detach: true,
            create_session: false,
            rename_session: false,
            duplicate_session: false,
            terminate_session: false,
            mutate_windows: false,
            mutate_panes: false,
            structured_history: false,
            input: false,
            resize: false,
        }
    }

    #[must_use]
    pub const fn supports(self, action: ProviderAction) -> bool {
        match action {
            ProviderAction::Catalog => self.catalog,
            ProviderAction::Attach => self.attach,
            ProviderAction::Detach => self.detach,
            ProviderAction::CreateSession => self.create_session,
            ProviderAction::RenameSession => self.rename_session,
            ProviderAction::DuplicateSession => self.duplicate_session,
            ProviderAction::TerminateSession => self.terminate_session,
            ProviderAction::MutateWindows => self.mutate_windows,
            ProviderAction::MutatePanes => self.mutate_panes,
            ProviderAction::StructuredHistory => self.structured_history,
            ProviderAction::Input => self.input,
            ProviderAction::Resize => self.resize,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProviderHealth {
    Stopped,
    Connecting,
    Ready,
    Stale { detail: String },
    Failed { detail: String },
}

impl ProviderHealth {
    #[must_use]
    pub fn stale(detail: impl AsRef<str>) -> Self {
        Self::Stale {
            detail: bounded_detail(detail.as_ref()),
        }
    }

    #[must_use]
    pub fn failed(detail: impl AsRef<str>) -> Self {
        Self::Failed {
            detail: bounded_detail(detail.as_ref()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error, Serialize, Deserialize)]
pub enum ProviderError {
    #[error("session provider is unavailable")]
    Unavailable,
    #[error("session provider authentication failed")]
    AuthenticationFailed,
    #[error("session provider protocol version is incompatible")]
    VersionIncompatible,
    #[error("stale provider revision")]
    StaleRevision,
    #[error("session provider capacity reached")]
    CapacityReached,
    #[error("session provider action is unsupported")]
    InvalidAction,
    #[error("session provider target was not found")]
    NotFound,
    #[error("session provider transport was lost")]
    TransportLost,
    #[error("session process failed: {detail}")]
    ProcessFailed { detail: String },
    #[error("session provider failed: {detail}")]
    Internal { detail: String },
}

impl ProviderError {
    #[must_use]
    pub fn process_failed(detail: impl AsRef<str>) -> Self {
        Self::ProcessFailed {
            detail: bounded_detail(detail.as_ref()),
        }
    }

    #[must_use]
    pub fn internal(detail: impl AsRef<str>) -> Self {
        Self::Internal {
            detail: bounded_detail(detail.as_ref()),
        }
    }
}

fn bounded_detail(detail: &str) -> String {
    let sanitized = detail.replace('\0', "�");
    if sanitized.len() <= MAX_ERROR_BYTES {
        return sanitized;
    }
    let mut end = MAX_ERROR_BYTES;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_owned()
}
