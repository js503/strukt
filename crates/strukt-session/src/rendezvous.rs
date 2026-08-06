use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{EndpointError, EndpointIdentity, PROTOCOL_VERSION, ServiceInstanceId};

const RENDEZVOUS_SCHEMA: u16 = 1;
const MAX_RENDEZVOUS_BYTES: usize = 16 * 1024;
const MAX_SECRET_REFERENCE_BYTES: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RendezvousRecord {
    schema_version: u16,
    protocol_version: u16,
    service_instance: ServiceInstanceId,
    endpoint_identity: String,
    secret_reference: String,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl RendezvousRecord {
    /// Creates a validated rendezvous record without embedding secret material.
    ///
    /// # Errors
    ///
    /// Returns an error for mismatched identities or unsafe secret references.
    pub fn new(
        endpoint: &EndpointIdentity,
        service_instance: ServiceInstanceId,
        secret_reference: impl Into<String>,
    ) -> Result<Self, RendezvousError> {
        if endpoint.service_instance() != service_instance {
            return Err(RendezvousError::InvalidRecord);
        }
        let record = Self {
            schema_version: RENDEZVOUS_SCHEMA,
            protocol_version: PROTOCOL_VERSION,
            service_instance,
            endpoint_identity: endpoint.identity().to_owned(),
            secret_reference: secret_reference.into(),
            extra: BTreeMap::new(),
        };
        validate_secret_reference(&record.secret_reference)?;
        Ok(record)
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
    pub fn secret_reference(&self) -> &str {
        &self.secret_reference
    }

    fn validate(&self, application_root: &Path) -> Result<(), RendezvousError> {
        if self.schema_version != RENDEZVOUS_SCHEMA || self.protocol_version != PROTOCOL_VERSION {
            return Err(RendezvousError::UnsupportedVersion);
        }
        EndpointIdentity::from_record(
            application_root,
            self.service_instance,
            &self.endpoint_identity,
        )?;
        validate_secret_reference(&self.secret_reference)
    }
}

fn validate_secret_reference(reference: &str) -> Result<(), RendezvousError> {
    let path = Path::new(reference);
    let mut components = path.components();
    let one_normal =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if reference.is_empty()
        || reference.len() > MAX_SECRET_REFERENCE_BYTES
        || reference.contains('\0')
        || !one_normal
    {
        return Err(RendezvousError::InvalidSecretReference);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RendezvousStatus {
    Missing,
    Live(RendezvousRecord),
    StaleRemoved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RendezvousStore {
    root: PathBuf,
}

impl RendezvousStore {
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn record_path(&self) -> PathBuf {
        self.root.join("session-rendezvous.json")
    }

    /// Atomically publishes a validated owner-only record after listener startup.
    ///
    /// # Errors
    ///
    /// Returns validation, serialization, or IO errors.
    pub fn publish(&self, record: &RendezvousRecord) -> Result<(), RendezvousError> {
        record.validate(&self.root)?;
        ensure_private_root(&self.root)?;
        let bytes = serde_json::to_vec_pretty(record)?;
        if bytes.len() > MAX_RENDEZVOUS_BYTES {
            return Err(RendezvousError::TooLarge);
        }
        let path = self.record_path();
        let mut file = AtomicWriteFile::open(&path)?;
        file.write_all(&bytes)?;
        file.commit()?;
        #[cfg(unix)]
        set_owner_only(&path)?;
        Ok(())
    }

    /// Loads and validates the current rendezvous record.
    ///
    /// # Errors
    ///
    /// Returns validation, size, serialization, or IO errors.
    pub fn load(&self) -> Result<Option<RendezvousRecord>, RendezvousError> {
        let path = self.record_path();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.len() > MAX_RENDEZVOUS_BYTES as u64 {
            return Err(RendezvousError::TooLarge);
        }
        let bytes = fs::read(path)?;
        if bytes.len() > MAX_RENDEZVOUS_BYTES {
            return Err(RendezvousError::TooLarge);
        }
        let record: RendezvousRecord = serde_json::from_slice(&bytes)?;
        record.validate(&self.root)?;
        Ok(Some(record))
    }

    /// Authenticates a discovered owner or removes a crash-stale record only when
    /// the OS service lock is available.
    ///
    /// # Errors
    ///
    /// Returns [`RendezvousError::OwnerUnverified`] when another process still
    /// owns the lock but does not pass the supplied authenticated probe.
    pub fn discover(
        &self,
        authenticated_probe: impl FnOnce(&RendezvousRecord) -> bool,
    ) -> Result<RendezvousStatus, RendezvousError> {
        let Some(record) = self.load()? else {
            return Ok(RendezvousStatus::Missing);
        };
        if authenticated_probe(&record) {
            return Ok(RendezvousStatus::Live(record));
        }
        match ServiceLock::acquire(&self.root) {
            Ok(_lock) => {
                self.clear_if_owner(record.service_instance)?;
                Ok(RendezvousStatus::StaleRemoved)
            }
            Err(RendezvousError::ServiceAlreadyRunning) => Err(RendezvousError::OwnerUnverified),
            Err(error) => Err(error),
        }
    }

    /// Removes the rendezvous file only if it still names the supplied owner.
    ///
    /// # Errors
    ///
    /// Returns validation or IO errors. Callers performing service cleanup must
    /// hold the service lock across this operation.
    pub fn clear_if_owner(
        &self,
        service_instance: ServiceInstanceId,
    ) -> Result<bool, RendezvousError> {
        let Some(record) = self.load()? else {
            return Ok(false);
        };
        if record.service_instance != service_instance {
            return Ok(false);
        }
        fs::remove_file(self.record_path())?;
        Ok(true)
    }
}

#[derive(Debug)]
pub struct ServiceLock {
    _file: File,
}

impl ServiceLock {
    /// Acquires the OS-released exclusive service lock without blocking.
    ///
    /// # Errors
    ///
    /// Returns [`RendezvousError::ServiceAlreadyRunning`] for a live owner.
    pub fn acquire(application_root: &Path) -> Result<Self, RendezvousError> {
        ensure_private_root(application_root)?;
        let path = application_root.join("session-service.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(RendezvousError::ServiceAlreadyRunning),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

fn ensure_private_root(root: &Path) -> Result<(), RendezvousError> {
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(RendezvousError::InvalidRoot);
    }
    fs::create_dir_all(root)?;
    #[cfg(unix)]
    set_owner_only_directory(root)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), RendezvousError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), RendezvousError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum RendezvousError {
    #[error("rendezvous application-data root is invalid")]
    InvalidRoot,
    #[error("rendezvous record is invalid")]
    InvalidRecord,
    #[error("rendezvous schema or protocol version is unsupported")]
    UnsupportedVersion,
    #[error("rendezvous secret reference is invalid")]
    InvalidSecretReference,
    #[error("rendezvous record exceeds its byte limit")]
    TooLarge,
    #[error("another session service owns the application-data directory")]
    ServiceAlreadyRunning,
    #[error("the locked session service owner failed authenticated discovery")]
    OwnerUnverified,
    #[error("rendezvous IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("rendezvous serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
}
