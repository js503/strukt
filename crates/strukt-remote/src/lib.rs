#![forbid(unsafe_code)]

pub mod config;
pub mod filesystem;
pub mod framing;
pub mod helper;
pub mod path;
pub mod protocol;
pub mod ssh;
pub mod state;
pub mod target;

pub use config::{ConfigDiscovery, ConfigDiscoveryLimits, discover_aliases};
pub use filesystem::{
    DirectoryEntry, DirectoryPage, RemoteDocument, RemoteDocumentKind, RemoteEntryKind,
    RemoteEnumeration, RemoteFilesystem, RemoteFilesystemError, RemoteSaveOutcome,
    RemoteSearchMatch, RemoteSearchResult, RemoteWatchBatch, RemoteWatchEvent, RemoteWatchInput,
    RemoteWatchSequencer,
};
pub use framing::{
    DEFAULT_FRAME_LIMIT, FramingError, read_frame, read_preface, write_frame, write_preface,
};
pub use helper::{HelperError, HelperServer, run_helper_stdio};
pub use path::{RemotePath, RemotePathError};
pub use protocol::{
    Capability, ClientHello, NegotiatedProtocol, OperationTracker, ProtocolError, ProtocolLimits,
    RemoteBuildTarget, RemoteError, RemoteErrorKind, RequestBody, RequestEnvelope, RequestId,
    ResponseBody, ResponseEnvelope, ServerHello, StreamChunk, negotiate,
};
pub use ssh::{
    EffectiveConfig, OpenSsh, OpenSshError, SshCancellation, SshCommandKind, SshCommandSpec,
    SshExecutable, SshExecutor, SshOutput, parse_effective_config,
};
pub use state::{
    ConnectionCapabilities, ConnectionMachine, ConnectionPhase, ConnectionProjection,
    RecoveryAction, RetryPolicy, StateError,
};
pub use target::{ConnectionId, RemoteRoot, RemoteTargetError, RemoteWorkspaceId, SshAlias};
