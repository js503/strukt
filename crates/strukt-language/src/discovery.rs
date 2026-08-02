use std::ffi::OsStr;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};
use cap_std::fs::OpenOptions;
use strukt_workspace::WorkspaceRoot;
use thiserror::Error;

use crate::{
    CommandApproval, DescriptorError, DescriptorRegistry, DescriptorSource, ExecutableCandidate,
    LanguageServerDescriptor, ResolvedCommand, registry_from_json,
};

const WORKSPACE_CONFIGURATION: &str = ".strukt-language.json";
const MAX_CONFIGURATION_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum ApprovalStatus<'a> {
    Unreviewed,
    Approved(&'a CommandApproval),
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredServer {
    descriptor_id: String,
    source: DescriptorSource,
    command: ResolvedCommand,
}

impl DiscoveredServer {
    #[must_use]
    pub fn descriptor_id(&self) -> &str {
        &self.descriptor_id
    }

    #[must_use]
    pub const fn source(&self) -> DescriptorSource {
        self.source
    }

    #[must_use]
    pub const fn command(&self) -> &ResolvedCommand {
        &self.command
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveryOutcome {
    Available(DiscoveredServer),
    ApprovalRequired(DiscoveredServer),
    Unavailable { guidance: Option<String> },
    Disabled,
}

/// Resolves a descriptor to a canonical executable without running any command.
///
/// # Errors
///
/// Returns a discovery error when filesystem inspection or command validation
/// fails unexpectedly.
pub fn discover(
    descriptor: &LanguageServerDescriptor,
    path_env: Option<&OsStr>,
    workspace: &WorkspaceRoot,
    approval: ApprovalStatus<'_>,
) -> Result<DiscoveryOutcome, DiscoveryError> {
    if !descriptor.enabled() || matches!(approval, ApprovalStatus::Denied) {
        return Ok(DiscoveryOutcome::Disabled);
    }

    let Some(executable) = resolve_executable(descriptor.candidates(), path_env)? else {
        return Ok(DiscoveryOutcome::Unavailable {
            guidance: descriptor.installation_guidance().map(ToOwned::to_owned),
        });
    };
    let command = ResolvedCommand::new(executable, descriptor.arguments().to_vec())?;
    let server = DiscoveredServer {
        descriptor_id: descriptor.id().to_owned(),
        source: descriptor.source(),
        command,
    };
    if descriptor.source() != DescriptorSource::Workspace {
        return Ok(DiscoveryOutcome::Available(server));
    }

    match approval {
        ApprovalStatus::Approved(record) if record.authorizes(workspace.id(), server.command()) => {
            Ok(DiscoveryOutcome::Available(server))
        }
        ApprovalStatus::Approved(_) | ApprovalStatus::Unreviewed => {
            Ok(DiscoveryOutcome::ApprovalRequired(server))
        }
        ApprovalStatus::Denied => Ok(DiscoveryOutcome::Disabled),
    }
}

/// Reads the optional root-level workspace descriptor registry without
/// following symbolic links.
///
/// # Errors
///
/// Returns a discovery error for a changed workspace, a symlink or non-regular
/// configuration, an oversized file, invalid UTF-8 JSON, or invalid descriptors.
pub fn load_workspace_registry(
    workspace: &WorkspaceRoot,
) -> Result<Option<DescriptorRegistry>, DiscoveryError> {
    workspace
        .validate_location()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;
    let directory = workspace
        .try_clone_capability()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;
    let metadata = match directory.symlink_metadata(WORKSPACE_CONFIGURATION) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(DiscoveryError::Io(error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(DiscoveryError::SymlinkConfiguration);
    }
    if !metadata.is_file() {
        return Err(DiscoveryError::NonRegularConfiguration);
    }
    if metadata.len() > MAX_CONFIGURATION_BYTES {
        return Err(DiscoveryError::ConfigurationTooLarge);
    }

    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(WORKSPACE_CONFIGURATION, &options)
        .map_err(DiscoveryError::Io)?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(MAX_CONFIGURATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(DiscoveryError::Io)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIGURATION_BYTES {
        return Err(DiscoveryError::ConfigurationTooLarge);
    }
    registry_from_json(&bytes, DescriptorSource::Workspace)
        .map(Some)
        .map_err(DiscoveryError::Descriptor)
}

/// Selects a descriptor deterministically for a language.
///
/// An explicit matching ID wins. Otherwise enabled descriptors are ranked by
/// the number of configured marker paths present in the workspace, preserving
/// registry order for ties.
///
/// # Errors
///
/// Returns a discovery error if the workspace capability cannot be validated or
/// marker metadata cannot be inspected.
pub fn select_descriptor<'a>(
    registry: &'a DescriptorRegistry,
    language_id: &str,
    workspace: &WorkspaceRoot,
    explicit_id: Option<&str>,
) -> Result<Option<&'a LanguageServerDescriptor>, DiscoveryError> {
    let matching = registry
        .iter()
        .filter(|descriptor| descriptor.language_ids().iter().any(|id| id == language_id))
        .collect::<Vec<_>>();
    if let Some(explicit_id) = explicit_id {
        return Ok(matching
            .into_iter()
            .find(|descriptor| descriptor.id() == explicit_id));
    }

    workspace
        .validate_location()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;
    let directory = workspace
        .try_clone_capability()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;
    let mut selected = None;
    let mut selected_score = 0;
    for descriptor in matching
        .into_iter()
        .filter(|descriptor| descriptor.enabled())
    {
        let score = marker_score(&directory, descriptor.workspace_markers())?;
        if selected.is_none() || score > selected_score {
            selected = Some(descriptor);
            selected_score = score;
        }
    }
    Ok(selected)
}

fn marker_score(
    directory: &cap_std::fs::Dir,
    markers: &[PathBuf],
) -> Result<usize, DiscoveryError> {
    let mut score = 0;
    for marker in markers {
        match directory.symlink_metadata(marker) {
            Ok(_) => score += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(DiscoveryError::Io(error)),
        }
    }
    Ok(score)
}

fn resolve_executable(
    candidates: &[ExecutableCandidate],
    path_env: Option<&OsStr>,
) -> Result<Option<PathBuf>, DiscoveryError> {
    for candidate in candidates {
        match candidate {
            ExecutableCandidate::Absolute(path) => {
                if let Some(path) = canonical_executable(path)? {
                    return Ok(Some(path));
                }
            }
            ExecutableCandidate::PathName(name) => {
                let Some(path_env) = path_env else {
                    continue;
                };
                for directory in std::env::split_paths(path_env) {
                    for path in platform_candidates(&directory, name) {
                        if let Some(path) = canonical_executable(&path)? {
                            return Ok(Some(path));
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

#[cfg(not(windows))]
fn platform_candidates(directory: &Path, name: &OsStr) -> Vec<PathBuf> {
    vec![directory.join(name)]
}

#[cfg(windows)]
fn platform_candidates(directory: &Path, name: &OsStr) -> Vec<PathBuf> {
    let path = Path::new(name);
    if path.extension().is_some() {
        return vec![directory.join(path)];
    }
    ["exe", "cmd", "bat", "com"]
        .into_iter()
        .map(|extension| directory.join(path).with_extension(extension))
        .collect()
}

fn canonical_executable(path: &Path) -> Result<Option<PathBuf>, DiscoveryError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(DiscoveryError::Io(error)),
    };
    if !metadata.is_file() || !platform_is_executable(&metadata) {
        return Ok(None);
    }
    path.canonicalize().map(Some).map_err(DiscoveryError::Io)
}

#[cfg(unix)]
fn platform_is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn platform_is_executable(_metadata: &std::fs::Metadata) -> bool {
    true
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("workspace location changed during language-server discovery")]
    WorkspaceChanged,
    #[error("workspace language configuration may not be a symbolic link")]
    SymlinkConfiguration,
    #[error("workspace language configuration must be a regular file")]
    NonRegularConfiguration,
    #[error("workspace language configuration exceeds 256 KiB")]
    ConfigurationTooLarge,
    #[error(transparent)]
    Descriptor(#[from] DescriptorError),
    #[error("language-server discovery I/O failed: {0}")]
    Io(#[source] io::Error),
}
