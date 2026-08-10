#![forbid(unsafe_code)]

pub mod client;
pub mod config;
pub mod filesystem;
pub mod framing;
pub mod git;
pub mod helper;
pub mod install;
pub mod language;
pub mod native_session;
pub mod path;
pub mod process;
pub mod protocol;
pub mod session_client;
pub mod ssh;
pub mod state;
pub mod target;
pub mod tmux;

pub use client::{HelperClient, OpenSshClient, RemoteClientError};
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
pub use git::{GitError, RemoteGitSummary};
pub use helper::{HelperError, HelperServer, run_helper_stdio};
pub use install::{
    HelperArtifact, HelperInstallError, execute_helper_install, helper_install_bootstrap,
};
pub use language::{RemoteLanguageError, RemoteLanguageManager};
pub use native_session::{NativeSessionError, NativeSessionManager};
pub use path::{RemotePath, RemotePathError};
pub use process::{
    RemoteProcessError, RemoteProcessManager, RemoteProcessOutput, RemoteProcessRequest,
};
pub use protocol::{
    Capability, ClientHello, NegotiatedProtocol, OperationTracker, PersistentProvider,
    ProtocolError, ProtocolLimits, RemoteBuildTarget, RemoteError, RemoteErrorKind, RequestBody,
    RequestEnvelope, RequestId, ResponseBody, ResponseEnvelope, ServerHello, SessionPayload,
    StreamChunk, negotiate,
};
pub use session_client::RemoteSessionBackend;
pub use ssh::{
    EffectiveConfig, OpenSsh, OpenSshError, SshCancellation, SshCommandKind, SshCommandSpec,
    SshExecutable, SshExecutor, SshOutput, parse_effective_config,
};
pub use state::{
    ConnectionCapabilities, ConnectionMachine, ConnectionPhase, ConnectionProjection,
    RecoveryAction, RetryPolicy, StateError,
};
pub use target::{ConnectionId, RemoteRoot, RemoteTargetError, RemoteWorkspaceId, SshAlias};
pub use tmux::{
    TmuxCatalog, TmuxCommand, TmuxControlEvent, TmuxError, TmuxManager, TmuxPane, TmuxProvider,
    TmuxRequest, TmuxResponse, TmuxSession, TmuxTarget, TmuxWindow,
};
