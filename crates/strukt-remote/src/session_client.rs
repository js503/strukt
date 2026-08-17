use std::sync::{Arc, Mutex};
use std::time::Duration;

use strukt_session::{
    ClientBackend, ClientError, FrameDecoder, ProviderConnection, ProviderError,
    RequestEnvelope as SessionRequestEnvelope, ResponseBody as SessionResponse,
    ResponseEnvelope as SessionResponseEnvelope, ServiceInstanceId, decode_cbor, encode_cbor,
};

use crate::{
    Capability, OpenSshClient, PersistentProvider, RequestBody, ResponseBody, SessionPayload,
};

const MAX_SESSION_FRAME_BYTES: usize = 1024 * 1024;

pub struct RemoteSessionBackend {
    client: Arc<Mutex<OpenSshClient>>,
    provider: PersistentProvider,
}

impl RemoteSessionBackend {
    #[must_use]
    pub fn new(client: Arc<Mutex<OpenSshClient>>, provider: PersistentProvider) -> Self {
        Self { client, provider }
    }
}

impl ClientBackend for RemoteSessionBackend {
    fn connect(&self) -> Result<Box<dyn ProviderConnection>, ClientError> {
        let capability = match self.provider {
            PersistentProvider::Native => Capability::Sessions,
            PersistentProvider::Tmux => Capability::Tmux,
        };
        let available = self
            .client
            .lock()
            .map_err(|_| ClientError::Unavailable)?
            .capabilities()
            .is_some_and(|capabilities| capabilities.contains(&capability));
        if !available {
            return Err(ClientError::Unavailable);
        }
        let instance = ServiceInstanceId::new()
            .map_err(|error| ClientError::ProtocolDetail(error.to_string()))?;
        Ok(Box::new(RemoteProviderConnection {
            client: Arc::clone(&self.client),
            provider: self.provider,
            instance,
        }))
    }

    fn start_service(&self) -> Result<(), ClientError> {
        // The authenticated remote helper applies explicit-start policy when it
        // receives Attach. No local process or second SSH channel is started here.
        Ok(())
    }

    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

struct RemoteProviderConnection {
    client: Arc<Mutex<OpenSshClient>>,
    provider: PersistentProvider,
    instance: ServiceInstanceId,
}

impl ProviderConnection for RemoteProviderConnection {
    fn service_instance(&self) -> ServiceInstanceId {
        self.instance
    }

    fn exchange(
        &mut self,
        request: SessionRequestEnvelope,
    ) -> Result<SessionResponseEnvelope, ClientError> {
        let bytes = encode_cbor(&request, MAX_SESSION_FRAME_BYTES)?;
        let payload = SessionPayload::new(self.provider, bytes)
            .map_err(|error| ClientError::ProtocolDetail(error.to_string()))?;
        let response = self
            .client
            .lock()
            .map_err(|_| ClientError::TransportLost)?
            .request(RequestBody::SessionExchange { payload })
            .map_err(|_| ClientError::TransportLost)?;
        let payload = match response {
            ResponseBody::SessionExchange { payload } if payload.provider() == self.provider => {
                payload
            }
            ResponseBody::Error(error) => {
                return Err(ClientError::Provider(ProviderError::internal(error.detail)));
            }
            _ => return Err(ClientError::Protocol),
        };
        let response = decode_response(payload.bytes())?;
        if let Ok(
            SessionResponse::Catalog(snapshot)
            | SessionResponse::Attached(snapshot)
            | SessionResponse::CatalogChanged(snapshot),
        ) = response.result()
        {
            self.instance = snapshot.service_instance();
        }
        Ok(response)
    }
}

fn decode_response(bytes: &[u8]) -> Result<SessionResponseEnvelope, ClientError> {
    let mut decoder = FrameDecoder::new(MAX_SESSION_FRAME_BYTES);
    let frames = decoder.push(bytes)?;
    if frames.len() != 1 || decoder.retained_bytes() != 0 {
        return Err(ClientError::Protocol);
    }
    decode_cbor(&frames[0]).map_err(Into::into)
}
