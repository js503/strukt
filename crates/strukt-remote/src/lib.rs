#![forbid(unsafe_code)]

pub mod state;
pub mod target;

pub use state::{
    ConnectionCapabilities, ConnectionMachine, ConnectionPhase, ConnectionProjection,
    RecoveryAction, RetryPolicy, StateError,
};
pub use target::{ConnectionId, RemoteRoot, RemoteTargetError, RemoteWorkspaceId, SshAlias};
