use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;

use atomic_write_file::AtomicWriteFile;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::{IdError, ServiceInstanceId};

const SECRET_BYTES: usize = 32;
const NONCE_BYTES: usize = 32;
const MAX_ENDPOINT_BYTES: usize = 256;

type HmacSha256 = Hmac<Sha256>;

pub struct ServiceSecret(Zeroizing<[u8; SECRET_BYTES]>);

impl ServiceSecret {
    /// Generates a secret from the operating-system random source.
    ///
    /// # Errors
    ///
    /// Returns [`AuthenticationError::RandomUnavailable`] when generation fails.
    pub fn generate() -> Result<Self, AuthenticationError> {
        let mut bytes = [0_u8; SECRET_BYTES];
        getrandom::fill(&mut bytes).map_err(|_| AuthenticationError::RandomUnavailable)?;
        Ok(Self(Zeroizing::new(bytes)))
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Generates a fresh per-service secret and atomically stores it outside the
    /// workspace with owner-only Unix permissions.
    ///
    /// # Errors
    ///
    /// Returns random-source or IO errors.
    pub fn generate_and_store(path: &Path) -> Result<Self, AuthenticationError> {
        let secret = Self::generate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = AtomicWriteFile::open(path)?;
        file.write_all(secret.0.as_ref())?;
        file.commit()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(secret)
    }

    /// Loads one exact-length service secret from its private reference.
    ///
    /// # Errors
    ///
    /// Returns IO or invalid-secret errors.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, AuthenticationError> {
        let bytes = fs::read(path)?;
        let bytes: [u8; SECRET_BYTES] = bytes
            .try_into()
            .map_err(|_| AuthenticationError::InvalidSecret)?;
        Ok(Self::from_bytes(bytes))
    }

    /// Produces an authentication proof for one challenge.
    ///
    /// # Errors
    ///
    /// Returns [`AuthenticationError::InvalidSecret`] if the MAC rejects the key.
    pub fn prove(
        &self,
        challenge: &HandshakeChallenge,
    ) -> Result<AuthenticationProof, AuthenticationError> {
        let mut mac = self.mac()?;
        update_challenge(&mut mac, challenge);
        Ok(AuthenticationProof(mac.finalize().into_bytes().into()))
    }

    #[must_use]
    pub fn verify(&self, challenge: &HandshakeChallenge, proof: &AuthenticationProof) -> bool {
        let Ok(mut mac) = self.mac() else {
            return false;
        };
        update_challenge(&mut mac, challenge);
        mac.verify_slice(&proof.0).is_ok()
    }

    fn mac(&self) -> Result<HmacSha256, AuthenticationError> {
        <HmacSha256 as Mac>::new_from_slice(self.0.as_ref())
            .map_err(|_| AuthenticationError::InvalidSecret)
    }
}

impl fmt::Debug for ServiceSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServiceSecret([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HandshakeChallenge {
    protocol_version: u16,
    service_instance: ServiceInstanceId,
    endpoint_identity: String,
    client_nonce: [u8; NONCE_BYTES],
}

impl HandshakeChallenge {
    /// Creates a validated authentication challenge.
    ///
    /// # Errors
    ///
    /// Returns [`AuthenticationError::InvalidEndpoint`] for an unsafe identity.
    pub fn new(
        protocol_version: u16,
        service_instance: ServiceInstanceId,
        endpoint_identity: impl Into<String>,
        client_nonce: [u8; NONCE_BYTES],
    ) -> Result<Self, AuthenticationError> {
        let endpoint_identity = endpoint_identity.into();
        if endpoint_identity.is_empty()
            || endpoint_identity.len() > MAX_ENDPOINT_BYTES
            || endpoint_identity.contains('\0')
        {
            return Err(AuthenticationError::InvalidEndpoint);
        }
        Ok(Self {
            protocol_version,
            service_instance,
            endpoint_identity,
            client_nonce,
        })
    }

    /// Generates a challenge with a fresh client nonce.
    ///
    /// # Errors
    ///
    /// Returns endpoint validation or OS-random errors.
    pub fn generate(
        protocol_version: u16,
        service_instance: ServiceInstanceId,
        endpoint_identity: impl Into<String>,
    ) -> Result<Self, AuthenticationError> {
        let mut client_nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut client_nonce).map_err(|_| AuthenticationError::RandomUnavailable)?;
        Self::new(
            protocol_version,
            service_instance,
            endpoint_identity,
            client_nonce,
        )
    }

    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    #[must_use]
    pub const fn service_instance(&self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub fn endpoint_identity(&self) -> &str {
        &self.endpoint_identity
    }

    #[must_use]
    pub const fn client_nonce(&self) -> [u8; NONCE_BYTES] {
        self.client_nonce
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuthenticationProof([u8; 32]);

fn update_challenge(mac: &mut HmacSha256, challenge: &HandshakeChallenge) {
    mac.update(b"strukt-session-auth-v1\0");
    mac.update(&challenge.protocol_version.to_be_bytes());
    mac.update(&challenge.service_instance.as_bytes());
    let endpoint_length = u32::try_from(challenge.endpoint_identity.len()).unwrap_or(u32::MAX);
    mac.update(&endpoint_length.to_be_bytes());
    mac.update(challenge.endpoint_identity.as_bytes());
    mac.update(&challenge.client_nonce);
}

#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("the operating system random source is unavailable")]
    RandomUnavailable,
    #[error("the endpoint identity is invalid")]
    InvalidEndpoint,
    #[error("the authentication secret is invalid")]
    InvalidSecret,
    #[error(transparent)]
    Id(#[from] IdError),
    #[error("authentication secret IO failed: {0}")]
    Io(#[from] std::io::Error),
}
