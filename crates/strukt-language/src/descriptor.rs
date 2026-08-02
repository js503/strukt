use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use strukt_workspace::WorkspaceId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorSource {
    BuiltIn,
    User,
    Workspace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableCandidate {
    PathName(OsString),
    Absolute(PathBuf),
}

impl ExecutableCandidate {
    /// Creates an executable candidate resolved through the inherited `PATH`.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidExecutable`] unless `name` is one
    /// nonempty path component without NUL bytes.
    pub fn path_name(name: impl Into<OsString>) -> Result<Self, DescriptorError> {
        let name = name.into();
        let path = Path::new(&name);
        let mut components = path.components();
        let one_normal_component =
            matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
        if !one_normal_component || has_nul(&name) {
            return Err(DescriptorError::InvalidExecutable);
        }
        Ok(Self::PathName(name))
    }

    /// Creates an absolute executable candidate.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidExecutable`] for a relative path or
    /// one containing a NUL byte.
    pub fn absolute(path: PathBuf) -> Result<Self, DescriptorError> {
        if !path.is_absolute() || has_nul(path.as_os_str()) {
            return Err(DescriptorError::InvalidExecutable);
        }
        Ok(Self::Absolute(path))
    }
}

#[derive(Clone, Debug)]
pub struct LanguageServerDescriptor {
    id: String,
    display_name: String,
    language_ids: Vec<String>,
    candidates: Vec<ExecutableCandidate>,
    arguments: Vec<OsString>,
    source: DescriptorSource,
    enabled: bool,
    installation_guidance: Option<String>,
    workspace_markers: Vec<PathBuf>,
    unknown_fields: BTreeMap<String, Value>,
}

impl LanguageServerDescriptor {
    /// Creates a validated language-server descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidDescriptor`] when identifiers,
    /// candidates, arguments, or required labels violate descriptor bounds.
    pub fn new<L, C, A>(
        id: impl Into<String>,
        display_name: impl Into<String>,
        language_ids: L,
        candidates: C,
        arguments: A,
        source: DescriptorSource,
    ) -> Result<Self, DescriptorError>
    where
        L: IntoIterator,
        L::Item: Into<String>,
        C: IntoIterator<Item = ExecutableCandidate>,
        A: IntoIterator<Item = OsString>,
    {
        let id = id.into();
        let display_name = display_name.into();
        let language_ids = language_ids.into_iter().map(Into::into).collect::<Vec<_>>();
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        if !valid_identifier(&id)
            || display_name.trim().is_empty()
            || language_ids.is_empty()
            || language_ids
                .iter()
                .any(|language| !valid_identifier(language))
            || candidates.is_empty()
            || arguments.iter().any(|argument| has_nul(argument))
        {
            return Err(DescriptorError::InvalidDescriptor);
        }
        let unique_languages = language_ids.iter().collect::<HashSet<_>>();
        if unique_languages.len() != language_ids.len() {
            return Err(DescriptorError::InvalidDescriptor);
        }
        Ok(Self {
            id,
            display_name,
            language_ids,
            candidates,
            arguments,
            source,
            enabled: true,
            installation_guidance: None,
            workspace_markers: Vec::new(),
            unknown_fields: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub fn language_ids(&self) -> &[String] {
        &self.language_ids
    }

    #[must_use]
    pub fn candidates(&self) -> &[ExecutableCandidate] {
        &self.candidates
    }

    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    #[must_use]
    pub const fn source(&self) -> DescriptorSource {
        self.source
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Adds bounded human-readable installation guidance.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidDescriptor`] when the guidance is
    /// empty or longer than 4 KiB.
    pub fn with_installation_guidance(
        mut self,
        guidance: Option<String>,
    ) -> Result<Self, DescriptorError> {
        if guidance
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > 4 * 1024)
        {
            return Err(DescriptorError::InvalidDescriptor);
        }
        self.installation_guidance = guidance;
        Ok(self)
    }

    #[must_use]
    pub fn installation_guidance(&self) -> Option<&str> {
        self.installation_guidance.as_deref()
    }

    /// Adds bounded, relative workspace marker paths used only for ranking.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidDescriptor`] for absolute, escaping,
    /// empty, duplicated, or excessively numerous marker paths.
    pub fn with_workspace_markers(
        mut self,
        markers: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, DescriptorError> {
        let markers = markers.into_iter().collect::<Vec<_>>();
        let unique = markers.iter().collect::<HashSet<_>>();
        if markers.len() > 64
            || markers.len() != unique.len()
            || markers.iter().any(|marker| !valid_marker(marker))
        {
            return Err(DescriptorError::InvalidDescriptor);
        }
        self.workspace_markers = markers;
        Ok(self)
    }

    #[must_use]
    pub fn workspace_markers(&self) -> &[PathBuf] {
        &self.workspace_markers
    }

    #[must_use]
    pub const fn unknown_fields(&self) -> &BTreeMap<String, Value> {
        &self.unknown_fields
    }
}

#[derive(Clone, Debug)]
pub struct DescriptorRegistry {
    descriptors: Vec<LanguageServerDescriptor>,
}

impl DescriptorRegistry {
    /// Creates a registry with unique descriptor IDs.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::DuplicateDescriptor`] when an ID repeats.
    pub fn new(descriptors: Vec<LanguageServerDescriptor>) -> Result<Self, DescriptorError> {
        let mut ids = HashSet::with_capacity(descriptors.len());
        if descriptors
            .iter()
            .any(|descriptor| !ids.insert(&descriptor.id))
        {
            return Err(DescriptorError::DuplicateDescriptor);
        }
        Ok(Self { descriptors })
    }

    #[must_use]
    pub fn for_language(&self, language_id: &str) -> Option<&LanguageServerDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.language_ids.iter().any(|id| id == language_id))
    }

    pub fn iter(&self) -> impl Iterator<Item = &LanguageServerDescriptor> {
        self.descriptors.iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCommand {
    executable: PathBuf,
    arguments: Vec<OsString>,
}

impl ResolvedCommand {
    /// Creates the exact no-shell command used for approval and process launch.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidExecutable`] for a relative executable
    /// or any NUL-containing command component.
    pub fn new(executable: PathBuf, arguments: Vec<OsString>) -> Result<Self, DescriptorError> {
        if !executable.is_absolute()
            || has_nul(executable.as_os_str())
            || arguments.iter().any(|argument| has_nul(argument))
        {
            return Err(DescriptorError::InvalidExecutable);
        }
        Ok(Self {
            executable,
            arguments,
        })
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        update_os_str(&mut hasher, self.executable.as_os_str());
        for argument in &self.arguments {
            update_os_str(&mut hasher, argument);
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandApproval {
    workspace: WorkspaceId,
    command_fingerprint: [u8; 32],
}

impl CommandApproval {
    #[must_use]
    pub fn grant(workspace: WorkspaceId, command: &ResolvedCommand) -> Self {
        Self {
            workspace,
            command_fingerprint: command.fingerprint(),
        }
    }

    #[must_use]
    pub fn authorizes(&self, workspace: &WorkspaceId, command: &ResolvedCommand) -> bool {
        self.workspace == *workspace && self.command_fingerprint == command.fingerprint()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum DescriptorError {
    #[error("language server descriptor is invalid")]
    InvalidDescriptor,
    #[error("language server executable must be a bare PATH name or absolute path")]
    InvalidExecutable,
    #[error("language server descriptor IDs must be unique")]
    DuplicateDescriptor,
    #[error("language server configuration is invalid: {0}")]
    InvalidConfiguration(String),
}

const MAX_CONFIGURATION_BYTES: usize = 256 * 1024;

#[derive(Deserialize)]
struct RegistryConfiguration {
    schema_version: u32,
    descriptors: Vec<DescriptorConfiguration>,
}

#[derive(Deserialize)]
struct DescriptorConfiguration {
    id: String,
    display_name: String,
    language_ids: Vec<String>,
    executable: String,
    #[serde(default)]
    arguments: Vec<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    installation_guidance: Option<String>,
    #[serde(default)]
    workspace_markers: Vec<PathBuf>,
    #[serde(flatten)]
    unknown_fields: BTreeMap<String, Value>,
}

/// Parses a bounded schema-version-1 descriptor registry.
///
/// # Errors
///
/// Returns [`DescriptorError::InvalidConfiguration`] for an oversized,
/// malformed, or unsupported configuration, or another descriptor error for
/// invalid entries.
pub fn registry_from_json(
    bytes: &[u8],
    source: DescriptorSource,
) -> Result<DescriptorRegistry, DescriptorError> {
    if bytes.len() > MAX_CONFIGURATION_BYTES {
        return Err(DescriptorError::InvalidConfiguration(
            "configuration exceeds 256 KiB".into(),
        ));
    }
    let configuration: RegistryConfiguration = serde_json::from_slice(bytes)
        .map_err(|error| DescriptorError::InvalidConfiguration(error.to_string()))?;
    if configuration.schema_version != 1 {
        return Err(DescriptorError::InvalidConfiguration(
            "unsupported schema version".into(),
        ));
    }
    let descriptors = configuration
        .descriptors
        .into_iter()
        .map(|raw| descriptor_from_configuration(raw, source))
        .collect::<Result<Vec<_>, _>>()?;
    DescriptorRegistry::new(descriptors)
}

/// Returns the public-alpha built-in descriptor registry.
///
/// # Errors
///
/// Returns a descriptor error if the compiled registry violates its own
/// validation contract.
pub fn built_in_descriptors() -> Result<DescriptorRegistry, DescriptorError> {
    DescriptorRegistry::new(vec![
        built_in(
            "rust-analyzer",
            "Rust Analyzer",
            &["rust"],
            "rust-analyzer",
            &[],
        )?,
        built_in(
            "typescript-language-server",
            "TypeScript Language Server",
            &["javascript", "typescript"],
            "typescript-language-server",
            &["--stdio"],
        )?,
        built_in(
            "pyright",
            "Pyright",
            &["python"],
            "pyright-langserver",
            &["--stdio"],
        )?,
        built_in(
            "vscode-json",
            "JSON Language Server",
            &["json"],
            "vscode-json-language-server",
            &["--stdio"],
        )?,
        built_in("taplo", "Taplo", &["toml"], "taplo", &["lsp", "stdio"])?,
        built_in(
            "marksman",
            "Marksman",
            &["markdown"],
            "marksman",
            &["server"],
        )?,
        built_in(
            "bash-language-server",
            "Bash Language Server",
            &["shell"],
            "bash-language-server",
            &["start"],
        )?,
        built_in(
            "yaml-language-server",
            "YAML Language Server",
            &["yaml"],
            "yaml-language-server",
            &["--stdio"],
        )?,
        built_in(
            "vscode-html",
            "HTML Language Server",
            &["html"],
            "vscode-html-language-server",
            &["--stdio"],
        )?,
        built_in(
            "vscode-css",
            "CSS Language Server",
            &["css"],
            "vscode-css-language-server",
            &["--stdio"],
        )?,
    ])
}

fn descriptor_from_configuration(
    raw: DescriptorConfiguration,
    source: DescriptorSource,
) -> Result<LanguageServerDescriptor, DescriptorError> {
    let executable_path = PathBuf::from(&raw.executable);
    let executable = if executable_path.is_absolute() {
        ExecutableCandidate::absolute(executable_path)?
    } else {
        ExecutableCandidate::path_name(raw.executable)?
    };
    let mut descriptor = LanguageServerDescriptor::new(
        raw.id,
        raw.display_name,
        raw.language_ids,
        [executable],
        raw.arguments.into_iter().map(OsString::from),
        source,
    )?
    .with_enabled(raw.enabled)
    .with_installation_guidance(raw.installation_guidance)?
    .with_workspace_markers(raw.workspace_markers)?;
    descriptor.unknown_fields = raw.unknown_fields;
    Ok(descriptor)
}

const fn default_enabled() -> bool {
    true
}

fn built_in(
    id: &str,
    display_name: &str,
    language_ids: &[&str],
    executable: &str,
    arguments: &[&str],
) -> Result<LanguageServerDescriptor, DescriptorError> {
    LanguageServerDescriptor::new(
        id,
        display_name,
        language_ids.iter().copied(),
        [ExecutableCandidate::path_name(executable)?],
        arguments.iter().map(OsString::from),
        DescriptorSource::BuiltIn,
    )
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_marker(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.as_os_str().to_string_lossy().len() <= 512
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn has_nul(value: &OsStr) -> bool {
    value.to_string_lossy().contains('\0')
}

#[cfg(unix)]
fn update_os_str(hasher: &mut blake3::Hasher, value: &OsStr) {
    use std::os::unix::ffi::OsStrExt as _;

    let bytes = value.as_bytes();
    hasher.update(&bytes.len().to_le_bytes());
    hasher.update(bytes);
}

#[cfg(windows)]
fn update_os_str(hasher: &mut blake3::Hasher, value: &OsStr) {
    use std::os::windows::ffi::OsStrExt as _;

    let words = value.encode_wide().collect::<Vec<_>>();
    hasher.update(&words.len().to_le_bytes());
    for word in words {
        hasher.update(&word.to_le_bytes());
    }
}
