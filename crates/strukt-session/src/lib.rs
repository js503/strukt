//! Provider-independent persistent session domain.

mod auth;
mod catalog;
mod client;
mod endpoint;
mod framing;
mod id;
mod protocol;
mod provider;
mod rendezvous;
mod service;
mod snapshot;
mod store;

pub use auth::{AuthenticationError, AuthenticationProof, HandshakeChallenge, ServiceSecret};
pub use catalog::{
    CatalogError, MAX_PANES_PER_WINDOW, MAX_SESSIONS, MAX_TOTAL_PANES, MAX_WINDOWS_PER_SESSION,
    PaneLifecycle, Session, SessionCatalog, SessionLayoutNode, SessionPane, SessionWindow,
    StoppedWindowDefinition,
};
pub use client::{
    ClientBackend, ClientConnectCompletion, ClientConnectIntent, ClientConnectJob, ClientError,
    ClientHealth, ClientRequestCompletion, ClientRequestJob, ProviderConnection, SessionClient,
};
pub use endpoint::{
    AuthenticatedListener, EndpointError, EndpointIdentity, EndpointQueue, EndpointTransport,
    LocalEndpoint, LocalStream,
};
pub use framing::{FrameDecoder, FrameError, decode_cbor, encode_cbor};
pub use id::{IdError, PaneId, ServiceInstanceId, SessionId, WindowId};
pub use protocol::{
    EventEnvelope, EventGuard, FixtureMode, PROTOCOL_VERSION, RequestBody, RequestEnvelope,
    RequestIdGenerator, ResponseBody, ResponseEnvelope, WireError,
};
pub use provider::{
    ProviderAction, ProviderCapabilities, ProviderError, ProviderHealth, ProviderKind,
};
pub use rendezvous::{
    RendezvousError, RendezvousRecord, RendezvousStatus, RendezvousStore, ServiceLock,
};
pub use service::{
    ServiceBatch, ServiceError, ServiceStartCompletion, ServiceStartJob,
    ServiceTerminateCompletion, ServiceTerminateJob, SessionService,
};
pub use snapshot::{
    AttentionState, CursorSnapshot, ModesSnapshot, PaneScreenSnapshot, ProviderCatalogSnapshot,
    SnapshotError,
};
pub use store::{PaneHistorySnapshot, PersistedCatalog, SessionStore, SessionStoreError};
