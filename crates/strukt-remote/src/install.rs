use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{RemoteBuildTarget, SshCancellation, SshCommandSpec, SshOutput};

const MAX_INSTALL_STDOUT_BYTES: usize = 256 * 1_024;
const MAX_INSTALL_STDERR_BYTES: usize = 64 * 1_024;

const INSTALL_BOOTSTRAP: &str = r#"set -eu
umask 077
version="$1"
expected="$2"
base="$HOME/.local/share/strukt/bin/$version"
target="$base/strukt-remote"
mkdir -p "$base"
test ! -e "$target" || exit 73
tmp=$(mktemp "$base/.strukt-remote.XXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
cat > "$tmp"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$tmp" | awk '{print $1}')
fi
test "$actual" = "$expected"
chmod 700 "$tmp"
mv "$tmp" "$target"
trap - EXIT HUP INT TERM
"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelperArtifact {
    path: PathBuf,
    bytes: Vec<u8>,
    version: String,
    checksum: String,
    target: RemoteBuildTarget,
}

impl HelperArtifact {
    /// Selects and verifies one packaged Linux helper artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when the version or checksum is invalid, the artifact is
    /// missing, cannot be read, or does not match its declared SHA-256 digest.
    pub fn select(
        target: RemoteBuildTarget,
        version: &str,
        directory: &Path,
        expected_checksum: &str,
    ) -> Result<Self, HelperInstallError> {
        validate_version(version)?;
        validate_checksum(expected_checksum)?;
        let filename = match target {
            RemoteBuildTarget::LinuxX86_64 => "strukt-remote-linux-x86_64",
            RemoteBuildTarget::LinuxAarch64 => "strukt-remote-linux-aarch64",
        };
        let path = directory.join(filename);
        if !path.is_file() {
            return Err(HelperInstallError::ArtifactMissing);
        }
        let bytes = fs::read(&path).map_err(HelperInstallError::ReadArtifact)?;
        let checksum = format!("{:x}", Sha256::digest(&bytes));
        if checksum != expected_checksum {
            return Err(HelperInstallError::ChecksumMismatch);
        }
        Ok(Self {
            path,
            bytes,
            version: version.to_owned(),
            checksum,
            target,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn checksum(&self) -> &str {
        &self.checksum
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn install_path(&self) -> String {
        format!(".local/share/strukt/bin/{}/strukt-remote", self.version)
    }

    #[must_use]
    pub fn consent_summary(&self) -> String {
        let target = match self.target {
            RemoteBuildTarget::LinuxX86_64 => "Linux x86_64",
            RemoteBuildTarget::LinuxAarch64 => "Linux aarch64",
        };
        format!(
            "Install strukt remote helper {} for {target} at ~/.{} (SHA-256 {})",
            self.version,
            self.install_path(),
            self.checksum
        )
    }
}

#[must_use]
pub const fn helper_install_bootstrap() -> &'static str {
    INSTALL_BOOTSTRAP
}

/// Streams a verified helper artifact to a typed OpenSSH install command.
///
/// # Errors
///
/// Returns a spawn, pipe, I/O, cancellation, deadline, or bounded-output error.
pub fn execute_helper_install(
    spec: &SshCommandSpec,
    artifact: &HelperArtifact,
    cancellation: &SshCancellation,
) -> Result<SshOutput, HelperInstallError> {
    if cancellation.is_cancelled() {
        return Err(HelperInstallError::Cancelled);
    }
    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(HelperInstallError::Execute)?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or(HelperInstallError::MissingChildPipe)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(HelperInstallError::MissingChildPipe)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(HelperInstallError::MissingChildPipe)?;
    let stdout_thread = read_bounded(stdout, MAX_INSTALL_STDOUT_BYTES);
    let stderr_thread = read_bounded(stderr, MAX_INSTALL_STDERR_BYTES);
    if let Err(error) = stdin.write_all(artifact.bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(HelperInstallError::Execute(error));
    }
    drop(stdin);
    let started = Instant::now();
    let status = loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HelperInstallError::Cancelled);
        }
        if !spec.deadline.is_zero() && started.elapsed() >= spec.deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HelperInstallError::DeadlineExceeded);
        }
        match child.try_wait().map_err(HelperInstallError::Execute)? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = join_reader(stdout_thread)?;
    let stderr = join_reader(stderr_thread)?;
    SshOutput::new(status.success(), stdout, stderr).map_err(HelperInstallError::OpenSsh)
}

fn read_bounded(mut reader: impl Read + Send + 'static, maximum: usize) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 8_192];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let remaining = maximum.saturating_sub(output.len());
            output.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        output
    })
}

fn join_reader(thread: JoinHandle<Vec<u8>>) -> Result<Vec<u8>, HelperInstallError> {
    thread
        .join()
        .map_err(|_| HelperInstallError::OutputReaderFailed)
}

#[derive(Debug, Error)]
pub enum HelperInstallError {
    #[error("remote helper version is invalid")]
    InvalidVersion,
    #[error("remote helper checksum is invalid")]
    InvalidChecksum,
    #[error("remote helper artifact is missing")]
    ArtifactMissing,
    #[error("remote helper artifact could not be read: {0}")]
    ReadArtifact(std::io::Error),
    #[error("remote helper artifact checksum does not match")]
    ChecksumMismatch,
    #[error("remote helper installation was cancelled")]
    Cancelled,
    #[error("remote helper installation exceeded its deadline")]
    DeadlineExceeded,
    #[error("remote helper installer child did not expose its stdio pipe")]
    MissingChildPipe,
    #[error("remote helper installer output reader failed")]
    OutputReaderFailed,
    #[error("remote helper installation I/O failed: {0}")]
    Execute(std::io::Error),
    #[error(transparent)]
    OpenSsh(crate::OpenSshError),
}

fn validate_version(version: &str) -> Result<(), HelperInstallError> {
    let valid = !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        && version.bytes().any(|byte| byte.is_ascii_digit());
    valid
        .then_some(())
        .ok_or(HelperInstallError::InvalidVersion)
}

fn validate_checksum(checksum: &str) -> Result<(), HelperInstallError> {
    (checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(())
        .ok_or(HelperInstallError::InvalidChecksum)
}
