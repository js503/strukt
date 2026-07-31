use keyring::{Entry, Error};
use strukt_persistence::{RecoveryKey, RecoveryKeyError, RecoveryKeyProvider};

const SERVICE: &str = "dev.strukt.editor-recovery";
const ACCOUNT: &str = "default";

pub(crate) struct NativeRecoveryKeyProvider;

impl NativeRecoveryKeyProvider {
    fn entry() -> Result<Entry, RecoveryKeyError> {
        Entry::new(SERVICE, ACCOUNT).map_err(map_error)
    }
}

impl RecoveryKeyProvider for NativeRecoveryKeyProvider {
    fn load_or_create(&self) -> Result<RecoveryKey, RecoveryKeyError> {
        let entry = Self::entry()?;
        match entry.get_secret() {
            Ok(secret) => RecoveryKey::from_secret(secret),
            Err(Error::NoEntry) => {
                let mut secret = vec![0; 32];
                getrandom::fill(&mut secret).map_err(|error| {
                    RecoveryKeyError::Provider(format!(
                        "operating-system randomness failed: {error}"
                    ))
                })?;
                entry.set_secret(&secret).map_err(map_error)?;
                RecoveryKey::from_secret(secret)
            }
            Err(error) => Err(map_error(error)),
        }
    }

    fn delete(&self) -> Result<(), RecoveryKeyError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

fn map_error(error: Error) -> RecoveryKeyError {
    match error {
        Error::NoDefaultStore
        | Error::NoStorageAccess(_)
        | Error::NotSupportedByStore(_)
        | Error::PlatformFailure(_) => RecoveryKeyError::Unavailable(error.to_string()),
        other => RecoveryKeyError::Provider(other.to_string()),
    }
}
