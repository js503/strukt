use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_ERROR_DETAIL_BYTES: usize = 1_024;
const HARD_MAX_FRAME_BYTES: usize = 16 * 1_024 * 1_024;
const HARD_MAX_IN_FLIGHT: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Capability {
    Files,
    Search,
    Git,
    Processes,
    Language,
    Watches,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteBuildTarget {
    LinuxX86_64,
    LinuxAarch64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProtocolLimits {
    pub max_frame_bytes: usize,
    pub max_in_flight: usize,
    pub max_stream_chunk_bytes: usize,
    pub initial_stream_credit_bytes: usize,
}

impl ProtocolLimits {
    /// Creates validated protocol limits.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::InvalidLimits`] for zero, internally inconsistent,
    /// or hard-limit-exceeding values.
    pub const fn new(
        max_frame_bytes: usize,
        max_in_flight: usize,
        max_stream_chunk_bytes: usize,
        initial_stream_credit_bytes: usize,
    ) -> Result<Self, ProtocolError> {
        let limits = Self {
            max_frame_bytes,
            max_in_flight,
            max_stream_chunk_bytes,
            initial_stream_credit_bytes,
        };
        if limits.is_valid() {
            Ok(limits)
        } else {
            Err(ProtocolError::InvalidLimits)
        }
    }

    const fn is_valid(self) -> bool {
        self.max_frame_bytes > 0
            && self.max_frame_bytes <= HARD_MAX_FRAME_BYTES
            && self.max_in_flight > 0
            && self.max_in_flight <= HARD_MAX_IN_FLIGHT
            && self.max_stream_chunk_bytes > 0
            && self.max_stream_chunk_bytes <= self.max_frame_bytes
            && self.initial_stream_credit_bytes > 0
            && self.initial_stream_credit_bytes <= HARD_MAX_FRAME_BYTES
    }

    const fn intersection(self, other: Self) -> Self {
        Self {
            max_frame_bytes: min_usize(self.max_frame_bytes, other.max_frame_bytes),
            max_in_flight: min_usize(self.max_in_flight, other.max_in_flight),
            max_stream_chunk_bytes: min_usize(
                self.max_stream_chunk_bytes,
                other.max_stream_chunk_bytes,
            ),
            initial_stream_credit_bytes: min_usize(
                self.initial_stream_credit_bytes,
                other.initial_stream_credit_bytes,
            ),
        }
    }
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 1024 * 1024,
            max_in_flight: 64,
            max_stream_chunk_bytes: 32 * 1024,
            initial_stream_credit_bytes: 256 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientHello {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub nonce: [u8; 32],
    pub workspace_root: String,
    pub limits: ProtocolLimits,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerHello {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub nonce: [u8; 32],
    pub helper_version: String,
    pub build_target: RemoteBuildTarget,
    pub workspace_root: String,
    pub limits: ProtocolLimits,
    pub capabilities: BTreeSet<Capability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiatedProtocol {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub limits: ProtocolLimits,
    pub capabilities: BTreeSet<Capability>,
}

/// Negotiates a compatible bounded helper protocol.
///
/// # Errors
///
/// Returns a protocol error for major, nonce, or limit mismatch.
pub fn negotiate(
    client: &ClientHello,
    server: &ServerHello,
    client_capabilities: &BTreeSet<Capability>,
) -> Result<NegotiatedProtocol, ProtocolError> {
    if client.protocol_major != server.protocol_major {
        return Err(ProtocolError::IncompatibleMajor);
    }
    if client.nonce != server.nonce {
        return Err(ProtocolError::NonceMismatch);
    }
    if !client.limits.is_valid() || !server.limits.is_valid() {
        return Err(ProtocolError::InvalidLimits);
    }
    Ok(NegotiatedProtocol {
        protocol_major: client.protocol_major,
        protocol_minor: client.protocol_minor.min(server.protocol_minor),
        limits: client.limits.intersection(server.limits),
        capabilities: client_capabilities
            .intersection(&server.capabilities)
            .copied()
            .collect(),
    })
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(u64);

impl RequestId {
    /// Creates a nonzero request identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::InvalidRequestId`] for zero.
    pub const fn new(value: u64) -> Result<Self, ProtocolError> {
        if value == 0 {
            Err(ProtocolError::InvalidRequestId)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub request_id: RequestId,
    pub generation: u64,
    pub body: RequestBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RequestBody {
    Stat {
        path: String,
    },
    ListDirectory {
        path: String,
        cursor: Option<String>,
        limit: u32,
    },
    ReadFile {
        path: String,
        offset: u64,
        length: u32,
    },
    WriteFile {
        path: String,
        expected_revision: String,
        bytes: Vec<u8>,
    },
    EnumerateFiles {
        include_ignored: bool,
    },
    Search {
        query: String,
        include_ignored: bool,
        limit: u32,
    },
    GitSummary,
    Spawn {
        executable: String,
        args: Vec<String>,
        cwd: String,
        shell: bool,
    },
    ProcessInput {
        process_id: u64,
        bytes: Vec<u8>,
    },
    Resize {
        process_id: u64,
        rows: u16,
        columns: u16,
    },
    LanguageInput {
        process_id: u64,
        bytes: Vec<u8>,
    },
    Watch {
        path: String,
    },
    Cancel {
        request_id: RequestId,
    },
    GrantCredit {
        request_id: RequestId,
        bytes: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub request_id: RequestId,
    pub generation: u64,
    pub body: ResponseBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ResponseBody {
    Acknowledged,
    Metadata {
        revision: String,
        kind: String,
        size: u64,
    },
    DirectoryPage {
        entries: Vec<String>,
        next_cursor: Option<String>,
    },
    Stream(StreamChunk),
    ProcessStarted {
        process_id: u64,
    },
    Completed {
        exit_code: Option<i32>,
    },
    Error(RemoteError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StreamChunk {
    pub request_id: RequestId,
    pub sequence: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteErrorKind {
    InvalidRequest,
    Unsupported,
    NotFound,
    PermissionDenied,
    Conflict,
    Cancelled,
    CapacityReached,
    ProcessFailed,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteError {
    pub kind: RemoteErrorKind,
    pub detail: String,
}

impl RemoteError {
    #[must_use]
    pub fn new(kind: RemoteErrorKind, detail: impl AsRef<str>) -> Self {
        Self {
            kind,
            detail: bounded_detail(detail.as_ref()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OperationState {
    next_sequence: u64,
    credit: usize,
    cancelled: bool,
}

#[derive(Debug)]
pub struct OperationTracker {
    limits: ProtocolLimits,
    operations: HashMap<RequestId, OperationState>,
}

impl OperationTracker {
    /// Creates an operation tracker with validated negotiated limits.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::InvalidLimits`] for invalid limits.
    pub fn new(limits: ProtocolLimits) -> Result<Self, ProtocolError> {
        if !limits.is_valid() {
            return Err(ProtocolError::InvalidLimits);
        }
        Ok(Self {
            limits,
            operations: HashMap::new(),
        })
    }

    /// Registers a unique in-flight request.
    ///
    /// # Errors
    ///
    /// Returns duplicate or capacity errors without mutating existing state.
    pub fn register(&mut self, request_id: RequestId) -> Result<(), ProtocolError> {
        if self.operations.contains_key(&request_id) {
            return Err(ProtocolError::DuplicateRequest);
        }
        if self.operations.len() >= self.limits.max_in_flight {
            return Err(ProtocolError::TooManyInFlight);
        }
        self.operations.insert(
            request_id,
            OperationState {
                next_sequence: 0,
                credit: self.limits.initial_stream_credit_bytes,
                cancelled: false,
            },
        );
        Ok(())
    }

    /// Accepts a correctly sequenced chunk within size and credit bounds.
    ///
    /// # Errors
    ///
    /// Returns a typed request, cancellation, sequence, chunk, or credit error.
    pub fn accept_chunk(&mut self, chunk: &StreamChunk) -> Result<(), ProtocolError> {
        let state = self
            .operations
            .get_mut(&chunk.request_id)
            .ok_or(ProtocolError::UnknownRequest)?;
        if state.cancelled {
            return Err(ProtocolError::RequestCancelled);
        }
        if chunk.sequence != state.next_sequence {
            return Err(ProtocolError::InvalidSequence);
        }
        if chunk.bytes.len() > self.limits.max_stream_chunk_bytes {
            return Err(ProtocolError::ChunkTooLarge);
        }
        if chunk.bytes.len() > state.credit {
            return Err(ProtocolError::CreditExceeded);
        }
        state.credit -= chunk.bytes.len();
        state.next_sequence = state.next_sequence.saturating_add(1);
        Ok(())
    }

    /// Adds stream credit to an active non-cancelled request.
    ///
    /// # Errors
    ///
    /// Returns an error for unknown, cancelled, or overflowing requests.
    pub fn grant_credit(
        &mut self,
        request_id: RequestId,
        bytes: usize,
    ) -> Result<(), ProtocolError> {
        let state = self
            .operations
            .get_mut(&request_id)
            .ok_or(ProtocolError::UnknownRequest)?;
        if state.cancelled {
            return Err(ProtocolError::RequestCancelled);
        }
        state.credit = state
            .credit
            .checked_add(bytes)
            .ok_or(ProtocolError::CreditExceeded)?;
        Ok(())
    }

    /// Completes and removes an active request.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::UnknownRequest`] if no request exists.
    pub fn complete(&mut self, request_id: RequestId) -> Result<(), ProtocolError> {
        self.operations
            .remove(&request_id)
            .map(|_| ())
            .ok_or(ProtocolError::UnknownRequest)
    }

    /// Marks an active request cancelled while retaining a tombstone that rejects
    /// post-cancellation stream data.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::UnknownRequest`] if no request exists.
    pub fn cancel(&mut self, request_id: RequestId) -> Result<(), ProtocolError> {
        let state = self
            .operations
            .get_mut(&request_id)
            .ok_or(ProtocolError::UnknownRequest)?;
        state.cancelled = true;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum ProtocolError {
    #[error("remote helper protocol major version is incompatible")]
    IncompatibleMajor,
    #[error("remote helper nonce does not match")]
    NonceMismatch,
    #[error("remote helper protocol limits are invalid")]
    InvalidLimits,
    #[error("remote helper request ID must be nonzero")]
    InvalidRequestId,
    #[error("remote helper request ID is already active")]
    DuplicateRequest,
    #[error("remote helper in-flight request capacity was reached")]
    TooManyInFlight,
    #[error("remote helper request is unknown")]
    UnknownRequest,
    #[error("remote helper request was cancelled")]
    RequestCancelled,
    #[error("remote helper stream sequence is invalid")]
    InvalidSequence,
    #[error("remote helper stream chunk exceeds the negotiated bound")]
    ChunkTooLarge,
    #[error("remote helper stream exceeded granted credit")]
    CreditExceeded,
}

const fn min_usize(left: usize, right: usize) -> usize {
    if left < right { left } else { right }
}

fn bounded_detail(detail: &str) -> String {
    let sanitized = detail.replace('\0', "�");
    if sanitized.len() <= MAX_ERROR_DETAIL_BYTES {
        return sanitized;
    }
    let mut end = MAX_ERROR_DETAIL_BYTES;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_owned()
}
