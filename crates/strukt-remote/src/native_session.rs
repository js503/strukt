use std::path::PathBuf;
use std::sync::Mutex;

use strukt_session::{
    ClientConnectIntent, ClientError, FrameDecoder, ProviderCapabilities, ProviderError,
    ProviderKind, RequestBody as SessionRequest, RequestEnvelope as SessionRequestEnvelope,
    ResponseBody as SessionResponse, ResponseEnvelope as SessionResponseEnvelope, SessionClient,
    decode_cbor, encode_cbor,
};
use thiserror::Error;

use crate::{PersistentProvider, ProtocolError, SessionPayload};

const MAX_SESSION_FRAME_BYTES: usize = 1024 * 1024;

/// Serializes one remote helper's controlling access to the native session service.
pub struct NativeSessionManager {
    client: Mutex<SessionClient>,
}

impl NativeSessionManager {
    /// Creates a lazy client for the exact remote application-data and service paths.
    ///
    /// This performs no service start or IPC connection.
    ///
    /// # Errors
    ///
    /// Returns an error when either trusted path is invalid.
    pub fn new(
        application_data: impl Into<PathBuf>,
        service_executable: impl Into<PathBuf>,
    ) -> Result<Self, NativeSessionError> {
        Ok(Self::from_client(SessionClient::new(
            application_data,
            service_executable,
        )?))
    }

    #[must_use]
    pub fn from_client(client: SessionClient) -> Self {
        Self {
            client: Mutex::new(client),
        }
    }

    /// Exchanges one bounded framed M3 request with the private native service.
    ///
    /// Attach may start the service; reconnect never does. All other operations
    /// require a ready attached client.
    ///
    /// # Errors
    ///
    /// Returns a provider, framing, protocol, or poisoned-state error.
    pub fn exchange(&self, payload: &SessionPayload) -> Result<SessionPayload, NativeSessionError> {
        if payload.provider() != PersistentProvider::Native {
            return Err(NativeSessionError::WrongProvider);
        }
        let request = decode_request(payload.bytes())?;
        let request_id = request.request_id();
        let body = request.body().clone();
        let mut client = self
            .client
            .lock()
            .map_err(|_| NativeSessionError::StateUnavailable)?;
        let response = match body {
            SessionRequest::Attach | SessionRequest::Reconnect { .. } => {
                if matches!(body, SessionRequest::Attach)
                    && client.health() == strukt_session::ClientHealth::Ready
                {
                    let response = client.catalog().cloned().map_or_else(
                        || SessionResponseEnvelope::error(request_id, ProviderError::Unavailable),
                        |catalog| {
                            SessionResponseEnvelope::ok(
                                request_id,
                                SessionResponse::Attached(remote_catalog(catalog)),
                            )
                        },
                    );
                    let bytes = encode_cbor(&response, MAX_SESSION_FRAME_BYTES)?;
                    return SessionPayload::new(PersistentProvider::Native, bytes)
                        .map_err(Into::into);
                }
                let intent = if matches!(body, SessionRequest::Reconnect { .. }) {
                    ClientConnectIntent::Reconnect
                } else {
                    ClientConnectIntent::ExplicitAttach
                };
                match client
                    .begin_connect(intent)
                    .map(strukt_session::ClientConnectJob::run)
                {
                    Ok(completion) => match client.finish_connect(completion) {
                        Ok(()) => client.catalog().cloned().map_or_else(
                            || {
                                SessionResponseEnvelope::error(
                                    request_id,
                                    ProviderError::Unavailable,
                                )
                            },
                            |catalog| {
                                SessionResponseEnvelope::ok(
                                    request_id,
                                    SessionResponse::Attached(remote_catalog(catalog)),
                                )
                            },
                        ),
                        Err(error) => {
                            SessionResponseEnvelope::error(request_id, provider_error(&error))
                        }
                    },
                    Err(error) => {
                        SessionResponseEnvelope::error(request_id, provider_error(&error))
                    }
                }
            }
            request_body => match client
                .begin_request(request_body)
                .map(strukt_session::ClientRequestJob::run)
            {
                Ok(completion) => match client.finish_request(completion) {
                    Ok(body) => SessionResponseEnvelope::ok(request_id, remote_response(body)),
                    Err(error) => {
                        SessionResponseEnvelope::error(request_id, provider_error(&error))
                    }
                },
                Err(error) => SessionResponseEnvelope::error(request_id, provider_error(&error)),
            },
        };
        let bytes = encode_cbor(&response, MAX_SESSION_FRAME_BYTES)?;
        SessionPayload::new(PersistentProvider::Native, bytes).map_err(Into::into)
    }
}

fn remote_catalog(
    catalog: strukt_session::ProviderCatalogSnapshot,
) -> strukt_session::ProviderCatalogSnapshot {
    catalog.for_provider(
        ProviderKind::NativeRemote,
        ProviderCapabilities::native_remote(),
    )
}

fn remote_response(response: SessionResponse) -> SessionResponse {
    match response {
        SessionResponse::Catalog(catalog) => SessionResponse::Catalog(remote_catalog(catalog)),
        SessionResponse::Attached(catalog) => SessionResponse::Attached(remote_catalog(catalog)),
        SessionResponse::CatalogChanged(catalog) => {
            SessionResponse::CatalogChanged(remote_catalog(catalog))
        }
        other => other,
    }
}

fn decode_request(bytes: &[u8]) -> Result<SessionRequestEnvelope, NativeSessionError> {
    let mut decoder = FrameDecoder::new(MAX_SESSION_FRAME_BYTES);
    let frames = decoder.push(bytes)?;
    if frames.len() != 1 || decoder.retained_bytes() != 0 {
        return Err(NativeSessionError::InvalidFrame);
    }
    let request: SessionRequestEnvelope = decode_cbor(&frames[0])?;
    request
        .validate()
        .map_err(|_| NativeSessionError::InvalidFrame)?;
    Ok(request)
}

fn provider_error(error: &ClientError) -> ProviderError {
    match error {
        ClientError::Provider(error) => error.clone(),
        ClientError::Unavailable => ProviderError::Unavailable,
        ClientError::RequestInFlight => ProviderError::CapacityReached,
        ClientError::StaleService => ProviderError::StaleRevision,
        ClientError::Authentication(_) => ProviderError::AuthenticationFailed,
        ClientError::Protocol | ClientError::ProtocolDetail(_) => {
            ProviderError::VersionIncompatible
        }
        ClientError::TransportLost
        | ClientError::InvalidPath
        | ClientError::Io(_)
        | ClientError::Endpoint(_)
        | ClientError::Rendezvous(_)
        | ClientError::Frame(_) => ProviderError::TransportLost,
    }
}

#[derive(Debug, Error)]
pub enum NativeSessionError {
    #[error("native session payload names the wrong provider")]
    WrongProvider,
    #[error("native session frame is invalid")]
    InvalidFrame,
    #[error("native session state is unavailable")]
    StateUnavailable,
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Frame(#[from] strukt_session::FrameError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}
