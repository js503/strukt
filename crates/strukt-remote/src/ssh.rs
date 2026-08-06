use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;

use crate::SshAlias;

const MAX_EFFECTIVE_CONFIG_BYTES: usize = 128 * 1_024;
const MAX_DIAGNOSTIC_BYTES: usize = 2_048;
const MAX_STDOUT_BYTES: usize = 256 * 1_024;
const MAX_STDERR_BYTES: usize = 64 * 1_024;

#[derive(Clone, Debug, Default)]
pub struct SshCancellation(Arc<AtomicBool>);

impl SshCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl SshOutput {
    /// Constructs bounded, separated process output.
    ///
    /// # Errors
    ///
    /// Returns [`OpenSshError::OutputTooLarge`] when either stream exceeds its
    /// independent capture limit.
    pub fn new(success: bool, stdout: Vec<u8>, stderr: Vec<u8>) -> Result<Self, OpenSshError> {
        if stdout.len() > MAX_STDOUT_BYTES || stderr.len() > MAX_STDERR_BYTES {
            return Err(OpenSshError::OutputTooLarge);
        }
        Ok(Self {
            success,
            stdout,
            stderr,
        })
    }
}

pub trait SshExecutor: Send + Sync {
    /// Executes one typed OpenSSH command using the supplied cancellation signal.
    ///
    /// # Errors
    ///
    /// Returns a structured OpenSSH error for spawn, deadline, cancellation, or
    /// output-limit failure.
    fn execute(
        &self,
        spec: &SshCommandSpec,
        cancellation: &SshCancellation,
    ) -> Result<SshOutput, OpenSshError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshExecutable {
    path: PathBuf,
}

impl SshExecutable {
    /// Creates a validated executable reference without consulting the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`OpenSshError::InvalidExecutable`] for an empty path.
    pub fn from_path(path: PathBuf) -> Result<Self, OpenSshError> {
        if path.as_os_str().is_empty() {
            return Err(OpenSshError::InvalidExecutable);
        }
        Ok(Self { path })
    }

    /// Finds OpenSSH from an explicit file or injected process-search directories.
    ///
    /// # Errors
    ///
    /// Returns [`OpenSshError::ExecutableNotFound`] when no candidate is a file.
    pub fn discover(
        explicit: Option<&Path>,
        search_directories: &[PathBuf],
    ) -> Result<Self, OpenSshError> {
        if let Some(path) = explicit {
            return path
                .is_file()
                .then(|| Self {
                    path: path.to_path_buf(),
                })
                .ok_or(OpenSshError::ExecutableNotFound);
        }
        let filename = if cfg!(windows) { "ssh.exe" } else { "ssh" };
        search_directories
            .iter()
            .map(|directory| directory.join(filename))
            .find(|candidate| candidate.is_file())
            .map(|path| Self { path })
            .ok_or(OpenSshError::ExecutableNotFound)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SshCommandKind {
    Version,
    ResolveConfig,
    Probe,
    Terminal,
    Helper,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshCommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub kind: SshCommandKind,
    pub deadline: Duration,
    pub interactive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenSsh {
    executable: SshExecutable,
}

impl OpenSsh {
    #[must_use]
    pub const fn new(executable: SshExecutable) -> Self {
        Self { executable }
    }

    #[must_use]
    pub const fn executable(&self) -> &SshExecutable {
        &self.executable
    }

    #[must_use]
    pub fn version(&self) -> SshCommandSpec {
        self.spec(
            SshCommandKind::Version,
            ["-V"],
            Duration::from_secs(5),
            false,
        )
    }

    #[must_use]
    pub fn resolve_config(&self, alias: &SshAlias) -> SshCommandSpec {
        self.spec(
            SshCommandKind::ResolveConfig,
            ["-G", "--", alias.as_str()],
            Duration::from_secs(5),
            false,
        )
    }

    #[must_use]
    pub fn probe(&self, alias: &SshAlias) -> SshCommandSpec {
        self.spec(
            SshCommandKind::Probe,
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "--",
                alias.as_str(),
                "true",
            ],
            Duration::from_secs(15),
            false,
        )
    }

    #[must_use]
    pub fn open_terminal(&self, alias: &SshAlias) -> SshCommandSpec {
        self.spec(
            SshCommandKind::Terminal,
            ["-tt", "--", alias.as_str()],
            Duration::ZERO,
            true,
        )
    }

    /// Builds a non-interactive helper command for a validated semantic version.
    ///
    /// # Errors
    ///
    /// Returns [`OpenSshError::InvalidHelperVersion`] for any version containing
    /// tokens that could alter the fixed remote command.
    pub fn open_helper(
        &self,
        alias: &SshAlias,
        version: &str,
    ) -> Result<SshCommandSpec, OpenSshError> {
        validate_version(version)?;
        let remote_path = format!("~/.local/share/strukt/bin/{version}/strukt-remote");
        Ok(self.spec(
            SshCommandKind::Helper,
            [
                OsString::from("-T"),
                OsString::from("-o"),
                OsString::from("BatchMode=yes"),
                OsString::from("--"),
                OsString::from(alias.as_str()),
                OsString::from(remote_path),
                OsString::from("--stdio"),
            ],
            Duration::ZERO,
            false,
        ))
    }

    #[must_use]
    pub fn sanitize_diagnostic(&self, value: impl AsRef<str>) -> String {
        bounded_text(value.as_ref(), MAX_DIAGNOSTIC_BYTES)
    }

    /// Resolves and parses effective OpenSSH configuration through an injectable
    /// process executor.
    ///
    /// # Errors
    ///
    /// Returns a structured cancellation, execution, bound, or parse error.
    pub fn read_effective_config(
        &self,
        alias: &SshAlias,
        executor: &impl SshExecutor,
        cancellation: &SshCancellation,
    ) -> Result<EffectiveConfig, OpenSshError> {
        if cancellation.is_cancelled() {
            return Err(OpenSshError::Cancelled);
        }
        let output = executor.execute(&self.resolve_config(alias), cancellation)?;
        if cancellation.is_cancelled() {
            return Err(OpenSshError::Cancelled);
        }
        if !output.success {
            return Err(OpenSshError::CommandFailed);
        }
        parse_effective_config(&output.stdout)
    }

    fn spec<I, S>(
        &self,
        kind: SshCommandKind,
        args: I,
        deadline: Duration,
        interactive: bool,
    ) -> SshCommandSpec
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        SshCommandSpec {
            program: self.executable.path.clone(),
            args: args.into_iter().map(Into::into).collect(),
            kind,
            deadline,
            interactive,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectiveConfig {
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub proxy_jump: Option<String>,
    pub identity_files: Vec<PathBuf>,
}

/// Parses the bounded machine-readable output of `ssh -G`.
///
/// # Errors
///
/// Returns an error for oversized, NUL-containing, non-UTF-8, or typed-invalid
/// effective configuration.
pub fn parse_effective_config(output: &[u8]) -> Result<EffectiveConfig, OpenSshError> {
    if output.len() > MAX_EFFECTIVE_CONFIG_BYTES {
        return Err(OpenSshError::EffectiveConfigTooLarge);
    }
    if output.contains(&0) {
        return Err(OpenSshError::InvalidEffectiveConfig);
    }
    let text = std::str::from_utf8(output).map_err(|_| OpenSshError::InvalidEffectiveConfig)?;
    let mut config = EffectiveConfig::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "hostname" => config.hostname = Some(value.to_owned()),
            "user" => config.user = Some(value.to_owned()),
            "port" => {
                config.port = Some(
                    value
                        .parse()
                        .map_err(|_| OpenSshError::InvalidEffectiveConfig)?,
                );
            }
            "proxyjump" if value != "none" => config.proxy_jump = Some(value.to_owned()),
            "identityfile" => config.identity_files.push(PathBuf::from(value)),
            _ => {}
        }
    }
    Ok(config)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum OpenSshError {
    #[error("OpenSSH executable was not found")]
    ExecutableNotFound,
    #[error("OpenSSH executable path is invalid")]
    InvalidExecutable,
    #[error("remote helper version is invalid")]
    InvalidHelperVersion,
    #[error("effective OpenSSH configuration exceeded the output bound")]
    EffectiveConfigTooLarge,
    #[error("effective OpenSSH configuration is invalid")]
    InvalidEffectiveConfig,
    #[error("OpenSSH operation was cancelled")]
    Cancelled,
    #[error("OpenSSH output exceeded its capture bound")]
    OutputTooLarge,
    #[error("OpenSSH command failed")]
    CommandFailed,
}

impl fmt::Display for SshCommandKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Version => "version",
            Self::ResolveConfig => "resolve-config",
            Self::Probe => "probe",
            Self::Terminal => "terminal",
            Self::Helper => "helper",
        })
    }
}

fn validate_version(version: &str) -> Result<(), OpenSshError> {
    let valid = !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        && version.bytes().any(|byte| byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(OpenSshError::InvalidHelperVersion)
    }
}

fn bounded_text(value: &str, maximum: usize) -> String {
    let sanitized = value.replace('\0', "�");
    if sanitized.len() <= maximum {
        return sanitized;
    }
    let mut end = maximum;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_owned()
}
