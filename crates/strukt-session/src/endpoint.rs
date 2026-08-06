use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use interprocess::local_socket::{GenericFilePath, Listener, ListenerOptions, Stream, prelude::*};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AuthenticationError, AuthenticationProof, FrameError, HandshakeChallenge, PROTOCOL_VERSION,
    ServiceInstanceId, ServiceSecret, decode_cbor, encode_cbor,
};

const MAX_AUTH_FRAME_BYTES: usize = 4 * 1024;
// macOS has the narrowest supported sockaddr_un.sun_path (104 bytes including NUL).
#[cfg(unix)]
const MAX_UNIX_ENDPOINT_BYTES: usize = 103;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointTransport {
    UnixDomainSocket,
    WindowsNamedPipe,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointIdentity {
    application_root: PathBuf,
    service_instance: ServiceInstanceId,
    identity: String,
}

impl EndpointIdentity {
    /// Generates an endpoint identity from a trusted per-user application-data root.
    ///
    /// # Errors
    ///
    /// Returns an error for relative, traversing, or excessively long roots.
    pub fn for_service(
        application_root: impl AsRef<Path>,
        service_instance: ServiceInstanceId,
    ) -> Result<Self, EndpointError> {
        let application_root = validate_root(application_root.as_ref())?;
        #[cfg(unix)]
        let identity = format!("s-{service_instance}");
        #[cfg(windows)]
        let identity = {
            let mut digest = Sha256::new();
            digest.update(application_root.as_os_str().as_encoded_bytes());
            let digest = digest.finalize();
            format!("strukt-{}-{service_instance}", hex_prefix(&digest[..8]))
        };
        let endpoint = Self {
            application_root,
            service_instance,
            identity,
        };
        #[cfg(unix)]
        endpoint.validate_native_name()?;
        Ok(endpoint)
    }

    /// Reconstructs only the exact endpoint generated for this root and instance.
    ///
    /// # Errors
    ///
    /// Returns an error when a rendezvous record attempts to redirect outside the
    /// application namespace.
    pub fn from_record(
        application_root: impl AsRef<Path>,
        service_instance: ServiceInstanceId,
        identity: &str,
    ) -> Result<Self, EndpointError> {
        let expected = Self::for_service(application_root, service_instance)?;
        if expected.identity != identity {
            return Err(EndpointError::InvalidIdentity);
        }
        Ok(expected)
    }

    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    #[must_use]
    pub const fn service_instance(&self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn transport(&self) -> EndpointTransport {
        #[cfg(unix)]
        return EndpointTransport::UnixDomainSocket;
        #[cfg(windows)]
        return EndpointTransport::WindowsNamedPipe;
    }

    #[must_use]
    pub fn belongs_to(&self, application_root: &Path, service_instance: ServiceInstanceId) -> bool {
        Self::for_service(application_root, service_instance)
            .is_ok_and(|expected| expected == *self)
    }

    #[must_use]
    pub fn native_path(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        return Some(self.application_root.join(&self.identity));
        #[cfg(windows)]
        return None;
    }

    #[cfg(windows)]
    fn native_pipe_path(&self) -> PathBuf {
        PathBuf::from(format!(r"\\.\pipe\{}", self.identity))
    }

    #[cfg(unix)]
    fn validate_native_name(&self) -> Result<(), EndpointError> {
        if self
            .native_path()
            .expect("Unix endpoint has a path")
            .as_os_str()
            .as_encoded_bytes()
            .len()
            > MAX_UNIX_ENDPOINT_BYTES
        {
            return Err(EndpointError::EndpointTooLong);
        }
        Ok(())
    }
}

fn validate_root(root: &Path) -> Result<PathBuf, EndpointError> {
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(EndpointError::InvalidApplicationRoot);
    }
    Ok(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()))
}

#[cfg(windows)]
fn hex_prefix(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub struct LocalEndpoint;

impl LocalEndpoint {
    /// Connects to an endpoint and completes the service-secret handshake.
    ///
    /// # Errors
    ///
    /// Returns endpoint IO, framing, identity, or authentication errors.
    pub fn connect_authenticated(
        identity: &EndpointIdentity,
        service_instance: ServiceInstanceId,
        secret: &ServiceSecret,
    ) -> Result<LocalStream, EndpointError> {
        if identity.service_instance != service_instance {
            return Err(EndpointError::InvalidIdentity);
        }
        let name = socket_name(identity)?;
        let mut stream = LocalStream(Stream::connect(name)?);
        let challenge = HandshakeChallenge::generate(
            PROTOCOL_VERSION,
            service_instance,
            identity.identity.clone(),
        )?;
        let request = AuthenticationRequest {
            proof: secret.prove(&challenge)?,
            challenge,
        };
        write_message(&mut stream, &request)?;
        let response: AuthenticationResponse = read_message(&mut stream)?;
        if !response.accepted {
            return Err(EndpointError::AuthenticationRejected);
        }
        Ok(stream)
    }
}

pub struct AuthenticatedListener {
    listener: Listener,
    identity: EndpointIdentity,
    secret: Arc<ServiceSecret>,
}

impl AuthenticatedListener {
    /// Binds the generated local endpoint with owner-only Unix permissions.
    ///
    /// # Errors
    ///
    /// Returns directory, name conversion, or listener creation errors.
    pub fn bind(
        identity: EndpointIdentity,
        secret: Arc<ServiceSecret>,
    ) -> Result<Self, EndpointError> {
        #[cfg(unix)]
        prepare_endpoint_parent(&identity)?;
        let name = socket_name(&identity)?;
        let options = ListenerOptions::new().name(name);
        #[cfg(windows)]
        let options = windows_owner_only(options)?;
        let listener = options.create_sync()?;
        #[cfg(unix)]
        set_endpoint_permissions(&identity)?;
        Ok(Self {
            listener,
            identity,
            secret,
        })
    }

    /// Accepts one connection and authenticates it before returning the stream.
    ///
    /// A rejected or disconnected client affects only that connection; callers may
    /// immediately accept the next client on the same listener.
    ///
    /// # Errors
    ///
    /// Returns endpoint IO, framing, or authentication rejection errors.
    pub fn accept(&self) -> Result<LocalStream, EndpointError> {
        let mut stream = LocalStream(self.listener.accept()?);
        let request: AuthenticationRequest = read_message(&mut stream)?;
        let challenge = &request.challenge;
        let accepted = challenge.protocol_version() == PROTOCOL_VERSION
            && challenge.service_instance() == self.identity.service_instance
            && challenge.endpoint_identity() == self.identity.identity
            && self.secret.verify(challenge, &request.proof);
        write_message(&mut stream, &AuthenticationResponse { accepted })?;
        if !accepted {
            return Err(EndpointError::AuthenticationRejected);
        }
        Ok(stream)
    }
}

#[cfg(unix)]
fn set_endpoint_permissions(identity: &EndpointIdentity) -> Result<(), EndpointError> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let path = identity.native_path().expect("Unix endpoint has a path");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn prepare_endpoint_parent(identity: &EndpointIdentity) -> Result<(), EndpointError> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let path = identity.native_path().expect("Unix endpoint has a path");
    let parent = path.parent().ok_or(EndpointError::InvalidIdentity)?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn socket_name(
    identity: &EndpointIdentity,
) -> Result<interprocess::local_socket::Name<'_>, EndpointError> {
    #[cfg(unix)]
    let path = identity.native_path().expect("Unix endpoint has a path");
    #[cfg(windows)]
    let path = identity.native_pipe_path();
    Ok(path.to_fs_name::<GenericFilePath>()?)
}

#[cfg(windows)]
fn windows_owner_only(options: ListenerOptions<'_>) -> Result<ListenerOptions<'_>, EndpointError> {
    use interprocess::os::windows::{
        local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
    };
    use widestring::U16CString;

    let sddl = U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;OW)")
        .map_err(|_| EndpointError::InvalidIdentity)?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    Ok(options.security_descriptor(descriptor))
}

pub struct LocalStream(Stream);

impl Read for LocalStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[derive(Debug)]
pub struct EndpointQueue {
    queued: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    max_bytes: usize,
    max_events: usize,
}

impl EndpointQueue {
    #[must_use]
    pub const fn new(max_bytes: usize, max_events: usize) -> Self {
        Self {
            queued: VecDeque::new(),
            queued_bytes: 0,
            max_bytes,
            max_events,
        }
    }

    /// Queues one encoded event without exceeding either configured bound.
    ///
    /// # Errors
    ///
    /// Returns [`EndpointError::QueueFull`] without changing the queue.
    pub fn push(&mut self, event: Vec<u8>) -> Result<(), EndpointError> {
        let new_bytes = self
            .queued_bytes
            .checked_add(event.len())
            .ok_or(EndpointError::QueueFull)?;
        if self.queued.len() >= self.max_events || new_bytes > self.max_bytes {
            return Err(EndpointError::QueueFull);
        }
        self.queued_bytes = new_bytes;
        self.queued.push_back(event);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<Vec<u8>> {
        let event = self.queued.pop_front()?;
        self.queued_bytes -= event.len();
        Some(event)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.queued.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    #[must_use]
    pub const fn queued_bytes(&self) -> usize {
        self.queued_bytes
    }
}

#[derive(Serialize, Deserialize)]
struct AuthenticationRequest {
    challenge: HandshakeChallenge,
    proof: AuthenticationProof,
}

#[derive(Serialize, Deserialize)]
struct AuthenticationResponse {
    accepted: bool,
}

fn write_message<T: Serialize>(stream: &mut LocalStream, value: &T) -> Result<(), EndpointError> {
    let frame = encode_cbor(value, MAX_AUTH_FRAME_BYTES)?;
    stream.write_all(&frame)?;
    stream.flush()?;
    Ok(())
}

fn read_message<T: serde::de::DeserializeOwned>(
    stream: &mut LocalStream,
) -> Result<T, EndpointError> {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length > MAX_AUTH_FRAME_BYTES {
        return Err(EndpointError::Frame(FrameError::FrameTooLarge));
    }
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload)?;
    Ok(decode_cbor(&payload)?)
}

#[derive(Debug, Error)]
pub enum EndpointError {
    #[error("application-data endpoint root must be an absolute non-traversing path")]
    InvalidApplicationRoot,
    #[error("endpoint identity is outside the generated application namespace")]
    InvalidIdentity,
    #[error("local endpoint path exceeds the portable platform limit")]
    EndpointTooLong,
    #[error("local endpoint authentication was rejected")]
    AuthenticationRejected,
    #[error("local endpoint outbound queue is full")]
    QueueFull,
    #[error("local endpoint IO failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Authentication(#[from] AuthenticationError),
    #[error(transparent)]
    Frame(#[from] FrameError),
}
