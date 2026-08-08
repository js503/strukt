use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use iced::widget::text_editor;
use strukt_language::{FrameDecoder, FrameLimits, IncomingMessage, encode_frame, parse_message};
use strukt_persistence::RemoteConnectionRecord;
use strukt_remote::{
    Capability as RemoteCapability, HelperArtifact, OpenSsh, OpenSshClient, RemoteBuildTarget,
    RemoteRoot, RequestBody, ResponseBody, SshAlias, SshCancellation, SshCommandSpec,
    SshExecutable, execute_helper_install,
};
use strukt_terminal::{SpawnRequest, TerminalSize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RemoteStatus {
    #[default]
    Disconnected,
    Connecting,
    Ready,
    TerminalOnly,
    Stale,
}

impl RemoteStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Ready => "Ready",
            Self::TerminalOnly => "Terminal only",
            Self::Stale => "Stale — reconnect required",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteSurfaces {
    pub alias_input: String,
    pub root_input: String,
    pub status: RemoteStatus,
    pub host_label: Option<String>,
    pub root_label: Option<String>,
    pub files: Vec<String>,
    pub selected_path: Option<String>,
    pub document_content: text_editor::Content,
    pub document_revision: Option<String>,
    pub document_dirty: bool,
    pub capabilities: BTreeSet<RemoteCapability>,
    pub search_input: String,
    pub search_results: Vec<String>,
    pub git_summary: Option<String>,
    pub task_executable: String,
    pub task_arguments_json: String,
    pub task_consent: Option<String>,
    pub task_output: String,
    pub language_executable: String,
    pub language_arguments_json: String,
    pub language_status: Option<String>,
    pub error: Option<String>,
    pub install_consent: Option<String>,
    pub records: Vec<RemoteConnectionRecord>,
    pending_artifact: Option<HelperArtifact>,
    generation: u64,
    operation_in_flight: bool,
}

impl Default for RemoteSurfaces {
    fn default() -> Self {
        Self {
            alias_input: String::new(),
            root_input: "~/".into(),
            status: RemoteStatus::Disconnected,
            host_label: None,
            root_label: None,
            files: Vec::new(),
            selected_path: None,
            document_content: text_editor::Content::new(),
            document_revision: None,
            document_dirty: false,
            capabilities: BTreeSet::new(),
            search_input: String::new(),
            search_results: Vec::new(),
            git_summary: None,
            task_executable: String::new(),
            task_arguments_json: "[]".into(),
            task_consent: None,
            task_output: String::new(),
            language_executable: String::new(),
            language_arguments_json: "[]".into(),
            language_status: None,
            error: None,
            install_consent: None,
            records: Vec::new(),
            pending_artifact: None,
            generation: 0,
            operation_in_flight: false,
        }
    }
}

impl RemoteSurfaces {
    /// Validates user input and creates a side-effect-free connection job.
    ///
    /// # Errors
    ///
    /// Returns a validation or executable-discovery error before state changes to
    /// connected.
    pub fn begin_connect(&mut self) -> Result<RemoteConnectJob, String> {
        let alias = SshAlias::new(self.alias_input.clone()).map_err(|error| error.to_string())?;
        let root = RemoteRoot::new(self.root_input.clone()).map_err(|error| error.to_string())?;
        let executable = discover_ssh()?;
        self.generation = self.generation.saturating_add(1).max(1);
        self.status = RemoteStatus::Connecting;
        self.error = None;
        self.files.clear();
        self.selected_path = None;
        self.document_content = text_editor::Content::new();
        self.document_revision = None;
        self.document_dirty = false;
        Ok(RemoteConnectJob {
            executable,
            alias,
            root,
            generation: self.generation,
        })
    }

    /// Builds a direct interactive OpenSSH terminal independently of the helper.
    ///
    /// # Errors
    ///
    /// Returns alias, executable, current-directory, or terminal-size errors.
    pub fn terminal_request(&self) -> Result<SpawnRequest, String> {
        let alias = SshAlias::new(self.alias_input.clone()).map_err(|error| error.to_string())?;
        let spec = OpenSsh::new(discover_ssh()?).open_terminal(&alias);
        Ok(SpawnRequest {
            executable: spec.program,
            arguments: spec.args,
            working_directory: std::env::current_dir().map_err(|error| error.to_string())?,
            environment: vec![
                (OsString::from("TERM"), OsString::from("xterm-256color")),
                (OsString::from("COLORTERM"), OsString::from("truecolor")),
            ],
            size: TerminalSize::new(24, 80).map_err(|error| error.to_string())?,
        })
    }

    /// Verifies a packaged helper and exposes its exact installation consent.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown filename, missing sidecar checksum, or
    /// checksum mismatch.
    pub fn prepare_install(&mut self, path: &Path) -> Result<(), String> {
        let target = match path.file_name().and_then(|name| name.to_str()) {
            Some("strukt-remote-linux-x86_64") => RemoteBuildTarget::LinuxX86_64,
            Some("strukt-remote-linux-aarch64") => RemoteBuildTarget::LinuxAarch64,
            _ => return Err("select a packaged Linux strukt-remote artifact".into()),
        };
        let parent = path
            .parent()
            .ok_or_else(|| "helper artifact has no parent directory".to_owned())?;
        let mut sidecar_name = path
            .file_name()
            .ok_or_else(|| "helper artifact has no filename".to_owned())?
            .to_os_string();
        sidecar_name.push(".sha256");
        let checksum = std::fs::read_to_string(parent.join(sidecar_name))
            .map_err(|error| format!("helper checksum sidecar could not be read: {error}"))?
            .split_whitespace()
            .next()
            .ok_or_else(|| "helper checksum sidecar is empty".to_owned())?
            .to_owned();
        let artifact = HelperArtifact::select(target, env!("CARGO_PKG_VERSION"), parent, &checksum)
            .map_err(|error| error.to_string())?;
        self.install_consent = Some(artifact.consent_summary());
        self.pending_artifact = Some(artifact);
        self.error = None;
        Ok(())
    }

    /// Consumes the approved artifact into a bounded install job.
    ///
    /// # Errors
    ///
    /// Returns an error when no verified artifact is pending or the alias is
    /// invalid.
    pub fn begin_install(&mut self) -> Result<RemoteInstallJob, String> {
        let artifact = self
            .pending_artifact
            .take()
            .ok_or_else(|| "choose and verify a helper artifact first".to_owned())?;
        let alias = SshAlias::new(self.alias_input.clone()).map_err(|error| error.to_string())?;
        let spec = OpenSsh::new(discover_ssh()?)
            .install_helper(&alias, artifact.version(), artifact.checksum())
            .map_err(|error| error.to_string())?;
        self.install_consent = None;
        self.operation_in_flight = true;
        Ok(RemoteInstallJob { spec, artifact })
    }

    pub fn cancel_install(&mut self) {
        self.pending_artifact = None;
        self.install_consent = None;
    }

    pub fn finish_install(&mut self, completion: &RemoteInstallCompletion) {
        self.operation_in_flight = false;
        match &completion.result {
            Ok(()) => {
                self.status = RemoteStatus::Disconnected;
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn finish_connect(&mut self, completion: &RemoteConnectCompletion) -> bool {
        if completion.generation != self.generation {
            return false;
        }
        match &completion.result {
            Ok(runtime) => {
                self.status = RemoteStatus::Ready;
                self.capabilities.clone_from(runtime.capabilities());
                self.host_label = Some(runtime.alias().to_owned());
                self.root_label = Some(runtime.root().to_owned());
                self.error = None;
            }
            Err(error) => {
                self.status = RemoteStatus::TerminalOnly;
                self.host_label = Some(self.alias_input.clone());
                self.root_label = Some(self.root_input.clone());
                self.error = Some(format!(
                    "The SSH host is available, but the strukt helper is unavailable: {error}"
                ));
            }
        }
        true
    }

    pub fn restore_records(&mut self, records: Vec<RemoteConnectionRecord>) {
        self.records = records;
        if let Some(record) = self.records.first() {
            self.alias_input.clone_from(&record.alias);
            if let Some(root) = record.recent_roots.first() {
                self.root_input.clone_from(root);
            }
        }
    }

    pub fn remember_record(&mut self, record: RemoteConnectionRecord) {
        self.records
            .retain(|existing| existing.connection_id != record.connection_id);
        self.records.push(record);
        self.records.sort_by(|left, right| {
            left.alias
                .to_ascii_lowercase()
                .cmp(&right.alias.to_ascii_lowercase())
                .then_with(|| left.connection_id.cmp(&right.connection_id))
        });
    }

    pub fn select_record(&mut self, index: usize) {
        if let Some(record) = self.records.get(index) {
            self.alias_input.clone_from(&record.alias);
            if let Some(root) = record.recent_roots.first() {
                self.root_input.clone_from(root);
            }
            self.error = None;
        }
    }

    pub fn forget_record(&mut self, connection_id: &str) {
        self.records
            .retain(|record| record.connection_id != connection_id);
    }

    /// Creates the current secret-free persistence record without connecting.
    ///
    /// # Errors
    ///
    /// Returns an identity or record validation error.
    pub fn current_record(&self) -> Result<RemoteConnectionRecord, String> {
        let connection_id = self
            .records
            .iter()
            .find(|record| record.alias == self.alias_input)
            .map_or_else(
                || {
                    strukt_remote::ConnectionId::new()
                        .map(|id| id.to_string())
                        .map_err(|error| error.to_string())
                },
                |record| Ok(record.connection_id.clone()),
            )?;
        let mut roots = self
            .records
            .iter()
            .find(|record| record.connection_id == connection_id)
            .map_or_else(Vec::new, |record| record.recent_roots.clone());
        roots.retain(|root| root != &self.root_input);
        roots.insert(0, self.root_input.clone());
        roots.truncate(20);
        RemoteConnectionRecord::new(connection_id, self.alias_input.clone(), None, roots, None)
            .map_err(|error| error.to_string())
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn operation_in_flight(&self) -> bool {
        self.operation_in_flight
    }

    pub fn begin_operation(&mut self) -> bool {
        if self.status != RemoteStatus::Ready || self.operation_in_flight {
            return false;
        }
        self.operation_in_flight = true;
        true
    }

    pub fn finish_files(&mut self, completion: &RemoteFilesCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        match &completion.result {
            Ok(files) => {
                self.files.clone_from(files);
                self.error = None;
            }
            Err(error) => {
                self.status = RemoteStatus::Stale;
                self.error = Some(error.clone());
            }
        }
    }

    pub fn finish_document(&mut self, completion: &RemoteDocumentCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        match &completion.result {
            Ok(document) => {
                self.selected_path = Some(document.path.clone());
                self.document_content = text_editor::Content::with_text(&document.text);
                self.document_revision = Some(document.revision.clone());
                self.document_dirty = false;
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn edit_document(&mut self, action: text_editor::Action) {
        if self.selected_path.is_some() {
            self.document_content.perform(action);
            self.document_dirty = true;
        }
    }

    pub fn prepare_task(&mut self) -> Result<(), String> {
        if self.task_executable.trim().is_empty() {
            return Err("enter an exact remote task executable".into());
        }
        let arguments: Vec<String> = serde_json::from_str(&self.task_arguments_json)
            .map_err(|_| "task arguments must be a JSON string array".to_owned())?;
        self.task_consent = Some(format!(
            "Run on {}: {} {}",
            self.host_label.as_deref().unwrap_or("remote host"),
            self.task_executable,
            serde_json::to_string(&arguments).map_err(|error| error.to_string())?
        ));
        Ok(())
    }

    pub fn approved_task_command(&self) -> Result<(String, Vec<String>), String> {
        if self.task_consent.is_none() {
            return Err("review the exact remote task before running it".into());
        }
        let arguments = serde_json::from_str(&self.task_arguments_json)
            .map_err(|_| "task arguments must be a JSON string array".to_owned())?;
        Ok((self.task_executable.clone(), arguments))
    }

    pub fn parsed_language_command(&self) -> Result<(String, Vec<String>), String> {
        if self.language_executable.trim().is_empty() {
            return Err("enter an exact language-server executable".into());
        }
        let arguments = serde_json::from_str(&self.language_arguments_json)
            .map_err(|_| "language arguments must be a JSON string array".to_owned())?;
        Ok((self.language_executable.clone(), arguments))
    }

    pub fn finish_save(&mut self, completion: &RemoteSaveCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        match &completion.result {
            Ok(revision) => {
                self.document_revision = Some(revision.clone());
                self.document_dirty = false;
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn finish_search(&mut self, completion: &RemoteTextCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        match &completion.result {
            Ok(results) => {
                self.search_results.clone_from(results);
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn finish_git(&mut self, completion: &RemoteTextCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        match &completion.result {
            Ok(lines) => {
                self.git_summary = lines.first().cloned();
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn finish_task(&mut self, completion: &RemoteTaskCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        self.task_consent = None;
        match &completion.result {
            Ok(output) => {
                self.task_output.clone_from(output);
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn finish_language(&mut self, completion: &RemoteLanguageCompletion) {
        if completion.generation != self.generation {
            return;
        }
        self.operation_in_flight = false;
        match &completion.result {
            Ok(status) => {
                self.language_status = Some(status.clone());
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn disconnected(&mut self) {
        self.generation = self.generation.saturating_add(1).max(1);
        self.status = RemoteStatus::Disconnected;
        self.host_label = None;
        self.root_label = None;
        self.files.clear();
        self.selected_path = None;
        self.document_content = text_editor::Content::new();
        self.document_revision = None;
        self.document_dirty = false;
        self.capabilities.clear();
        self.search_results.clear();
        self.git_summary = None;
        self.task_consent = None;
        self.task_output.clear();
        self.language_status = None;
        self.operation_in_flight = false;
        self.error = None;
    }
}

#[derive(Clone, Debug)]
pub struct RemoteConnectJob {
    executable: SshExecutable,
    alias: SshAlias,
    root: RemoteRoot,
    generation: u64,
}

impl RemoteConnectJob {
    pub fn run(self) -> RemoteConnectCompletion {
        let generation = self.generation;
        let result =
            RemoteRuntime::connect(self.executable, &self.alias, self.root.as_str(), generation);
        RemoteConnectCompletion { generation, result }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteConnectCompletion {
    pub generation: u64,
    pub result: Result<RemoteRuntime, String>,
}

#[derive(Clone)]
pub struct RemoteRuntime {
    client: Arc<Mutex<OpenSshClient>>,
    alias: String,
    root: String,
    capabilities: BTreeSet<RemoteCapability>,
}

impl RemoteRuntime {
    pub(crate) fn connect(
        executable: SshExecutable,
        alias: &SshAlias,
        root: &str,
        generation: u64,
    ) -> Result<Self, String> {
        let alias_label = alias.as_str().to_owned();
        let openssh = OpenSsh::new(executable);
        let client =
            OpenSshClient::connect(&openssh, alias, env!("CARGO_PKG_VERSION"), root, generation)
                .map_err(|error| error.to_string())?;
        let capabilities = client.capabilities().cloned().unwrap_or_default();
        let canonical_root = client.workspace_root().unwrap_or(root).to_owned();
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            alias: alias_label,
            root: canonical_root,
            capabilities,
        })
    }

    #[must_use]
    pub fn capabilities(&self) -> &BTreeSet<RemoteCapability> {
        &self.capabilities
    }

    #[must_use]
    pub fn alias(&self) -> &str {
        &self.alias
    }

    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn list_root(&self, generation: u64) -> RemoteFilesCompletion {
        let result = self
            .client
            .lock()
            .map_err(|_| "remote client state is unavailable".to_owned())
            .and_then(|mut client| {
                client
                    .request(RequestBody::EnumerateFiles {
                        include_ignored: false,
                    })
                    .map_err(|error| error.to_string())
            })
            .and_then(|response| match response {
                ResponseBody::DirectoryPage { entries, .. } => Ok(entries),
                ResponseBody::Error(error) => Err(error.detail),
                _ => Err("remote helper returned an unexpected file discovery response".into()),
            });
        RemoteFilesCompletion { generation, result }
    }

    pub fn read_document(&self, generation: u64, path: String) -> RemoteDocumentCompletion {
        let result = self
            .client
            .lock()
            .map_err(|_| "remote client state is unavailable".to_owned())
            .and_then(|mut client| {
                let metadata = client
                    .request(RequestBody::Stat { path: path.clone() })
                    .map_err(|error| error.to_string())?;
                let revision = match metadata {
                    ResponseBody::Metadata { revision, kind, .. }
                        if kind.contains("utf8") || kind == "file" =>
                    {
                        revision
                    }
                    ResponseBody::Metadata { kind, .. } => {
                        return Err(format!("remote document is not editable text ({kind})"));
                    }
                    ResponseBody::Error(error) => return Err(error.detail),
                    _ => return Err("remote helper returned invalid metadata".into()),
                };
                let body = client
                    .request(RequestBody::ReadFile {
                        path: path.clone(),
                        offset: 0,
                        length: 1024 * 1024,
                    })
                    .map_err(|error| error.to_string())?;
                let bytes = match body {
                    ResponseBody::Stream(chunk) => chunk.bytes,
                    ResponseBody::Error(error) => return Err(error.detail),
                    _ => return Err("remote helper returned invalid document bytes".into()),
                };
                let text = String::from_utf8(bytes)
                    .map_err(|_| "remote document is not valid UTF-8".to_owned())?;
                Ok(RemoteDocument {
                    path,
                    revision,
                    text,
                })
            });
        RemoteDocumentCompletion { generation, result }
    }

    pub fn save_document(
        &self,
        generation: u64,
        path: String,
        expected_revision: String,
        text: String,
    ) -> RemoteSaveCompletion {
        let result = self
            .client
            .lock()
            .map_err(|_| "remote client state is unavailable".to_owned())
            .and_then(|mut client| {
                client
                    .request(RequestBody::WriteFile {
                        path,
                        expected_revision,
                        bytes: text.into_bytes(),
                    })
                    .map_err(|error| error.to_string())
            })
            .and_then(|response| match response {
                ResponseBody::Metadata { revision, .. } => Ok(revision),
                ResponseBody::Error(error) => Err(error.detail),
                _ => Err("remote helper returned an unexpected save response".into()),
            });
        RemoteSaveCompletion { generation, result }
    }

    pub fn search(&self, generation: u64, query: String) -> RemoteTextCompletion {
        let result = self.request(RequestBody::Search {
            query,
            include_ignored: false,
            limit: 500,
        });
        RemoteTextCompletion {
            generation,
            result: result.and_then(directory_entries),
        }
    }

    pub fn git_summary(&self, generation: u64) -> RemoteTextCompletion {
        let result = self
            .request(RequestBody::GitSummary)
            .and_then(|response| match response {
                ResponseBody::GitSummary {
                    branch,
                    detached,
                    staged,
                    modified,
                    untracked,
                } => Ok(vec![format!(
                    "branch: {} · staged {staged} · modified {modified} · untracked {untracked}",
                    branch.unwrap_or_else(|| if detached {
                        "detached".into()
                    } else {
                        "none".into()
                    })
                )]),
                ResponseBody::Error(error) => Err(error.detail),
                _ => Err("remote helper returned an unexpected Git response".into()),
            });
        RemoteTextCompletion { generation, result }
    }

    pub fn run_task(
        &self,
        generation: u64,
        executable: String,
        args: Vec<String>,
    ) -> RemoteTaskCompletion {
        let result = self.run_task_inner(executable, args);
        RemoteTaskCompletion { generation, result }
    }

    fn run_task_inner(&self, executable: String, args: Vec<String>) -> Result<String, String> {
        let process_id = match self.request(RequestBody::Spawn {
            executable,
            args,
            cwd: String::new(),
            shell: false,
        })? {
            ResponseBody::ProcessStarted { process_id } => process_id,
            ResponseBody::Error(error) => return Err(error.detail),
            _ => return Err("remote helper returned an unexpected task response".into()),
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut output = Vec::new();
        loop {
            match self.request(RequestBody::DrainProcess {
                process_id,
                max_bytes: 32 * 1024,
            })? {
                ResponseBody::Stream(chunk) => output.extend_from_slice(&chunk.bytes),
                ResponseBody::Error(error) => return Err(error.detail),
                _ => {}
            }
            if output.len() > 1024 * 1024 {
                let _ = self.request(RequestBody::TerminateProcess { process_id });
                return Err("remote task output exceeded 1 MiB".into());
            }
            match self.request(RequestBody::PollProcess { process_id })? {
                ResponseBody::Completed { exit_code } => {
                    return Ok(format!(
                        "exit {}\n{}",
                        exit_code.map_or_else(|| "unknown".into(), |code| code.to_string()),
                        String::from_utf8_lossy(&output)
                    ));
                }
                ResponseBody::Error(error) => return Err(error.detail),
                _ => {}
            }
            if std::time::Instant::now() >= deadline {
                let _ = self.request(RequestBody::TerminateProcess { process_id });
                return Err("remote task exceeded 30 seconds".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    pub fn run_language_diagnostics(
        &self,
        generation: u64,
        executable: String,
        args: Vec<String>,
        path: &str,
        text: &str,
    ) -> RemoteLanguageCompletion {
        let result = self.run_language_diagnostics_inner(executable, args, path, text);
        RemoteLanguageCompletion { generation, result }
    }

    fn run_language_diagnostics_inner(
        &self,
        executable: String,
        args: Vec<String>,
        path: &str,
        text: &str,
    ) -> Result<String, String> {
        let process_id = match self.request(RequestBody::SpawnLanguage {
            executable,
            args,
            cwd: String::new(),
        })? {
            ResponseBody::ProcessStarted { process_id } => process_id,
            ResponseBody::Error(error) => return Err(error.detail),
            _ => return Err("remote helper returned an unexpected language response".into()),
        };
        let result = self.exchange_language_diagnostics(process_id, path, text);
        let _ = self.request(RequestBody::TerminateLanguage { process_id });
        result
    }

    fn exchange_language_diagnostics(
        &self,
        process_id: u64,
        path: &str,
        text: &str,
    ) -> Result<String, String> {
        let root_uri = url::Url::from_directory_path(&self.root)
            .map_err(|()| "remote canonical root is not a valid file URI".to_owned())?;
        let document_uri = url::Url::from_file_path(Path::new(&self.root).join(path))
            .map_err(|()| "remote document path is not a valid file URI".to_owned())?;
        let initialize = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"processId": null, "rootUri": root_uri, "capabilities": {}}
        });
        self.write_language_json(process_id, &initialize)?;
        let mut decoder = FrameDecoder::new(FrameLimits::default());
        let initialize_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            for message in self.read_language_messages(process_id, &mut decoder)? {
                if matches!(message, IncomingMessage::Response(_)) {
                    let initialized = serde_json::json!({
                        "jsonrpc": "2.0", "method": "initialized", "params": {}
                    });
                    self.write_language_json(process_id, &initialized)?;
                    let did_open = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didOpen",
                        "params": {"textDocument": {
                            "uri": document_uri,
                            "languageId": "plaintext",
                            "version": 1,
                            "text": text
                        }}
                    });
                    self.write_language_json(process_id, &did_open)?;
                    return self.wait_for_diagnostics(process_id, &mut decoder);
                }
            }
            if std::time::Instant::now() >= initialize_deadline {
                return Err("remote language server initialization timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn wait_for_diagnostics(
        &self,
        process_id: u64,
        decoder: &mut FrameDecoder,
    ) -> Result<String, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            for message in self.read_language_messages(process_id, decoder)? {
                if let IncomingMessage::Notification(notification) = message
                    && notification.method() == "textDocument/publishDiagnostics"
                {
                    let diagnostics = notification
                        .params()
                        .and_then(|params| params.get("diagnostics"))
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    if diagnostics.is_empty() {
                        return Ok("Remote diagnostics: no problems".into());
                    }
                    let messages = diagnostics
                        .iter()
                        .filter_map(|diagnostic| diagnostic.get("message"))
                        .filter_map(serde_json::Value::as_str)
                        .take(100)
                        .collect::<Vec<_>>();
                    return Ok(format!(
                        "Remote diagnostics ({}): {}",
                        diagnostics.len(),
                        messages.join(" · ")
                    ));
                }
            }
            if std::time::Instant::now() >= deadline {
                return Ok("Remote language server initialized; no diagnostics published".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn write_language_json(
        &self,
        process_id: u64,
        message: &serde_json::Value,
    ) -> Result<(), String> {
        let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
        let frame =
            encode_frame(&body, FrameLimits::default()).map_err(|error| error.to_string())?;
        match self.request(RequestBody::LanguageInput {
            process_id,
            bytes: frame,
        })? {
            ResponseBody::Acknowledged => Ok(()),
            ResponseBody::Error(error) => Err(error.detail),
            _ => Err("remote helper returned an unexpected language write response".into()),
        }
    }

    fn read_language_messages(
        &self,
        process_id: u64,
        decoder: &mut FrameDecoder,
    ) -> Result<Vec<IncomingMessage>, String> {
        let bytes = match self.request(RequestBody::ReadLanguage { process_id })? {
            ResponseBody::Acknowledged => return Ok(Vec::new()),
            ResponseBody::Stream(chunk) => chunk.bytes,
            ResponseBody::Error(error) => return Err(error.detail),
            _ => return Err("remote helper returned an unexpected language read response".into()),
        };
        decoder
            .push(&bytes)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|frame| parse_message(frame.body()).map_err(|error| error.to_string()))
            .collect()
    }

    fn request(&self, body: RequestBody) -> Result<ResponseBody, String> {
        self.client
            .lock()
            .map_err(|_| "remote client state is unavailable".to_owned())?
            .request(body)
            .map_err(|error| error.to_string())
    }

    pub fn disconnect(&self) {
        if let Ok(mut client) = self.client.lock() {
            client.disconnect();
        }
    }
}

fn directory_entries(response: ResponseBody) -> Result<Vec<String>, String> {
    match response {
        ResponseBody::DirectoryPage { entries, .. } => Ok(entries),
        ResponseBody::Error(error) => Err(error.detail),
        _ => Err("remote helper returned an unexpected list response".into()),
    }
}

impl fmt::Debug for RemoteRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteRuntime")
            .field("alias", &self.alias)
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct RemoteFilesCompletion {
    pub generation: u64,
    pub result: Result<Vec<String>, String>,
}

#[derive(Clone, Debug)]
pub struct RemoteInstallJob {
    spec: SshCommandSpec,
    artifact: HelperArtifact,
}

impl RemoteInstallJob {
    pub fn run(self) -> RemoteInstallCompletion {
        let result = execute_helper_install(&self.spec, &self.artifact, &SshCancellation::new())
            .map_err(|error| error.to_string())
            .and_then(|output| {
                output.success.then_some(()).ok_or_else(|| {
                    format!(
                        "remote helper installer failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )
                })
            });
        RemoteInstallCompletion { result }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteInstallCompletion {
    pub result: Result<(), String>,
}

#[derive(Clone, Debug)]
pub struct RemoteDocument {
    pub path: String,
    pub revision: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct RemoteDocumentCompletion {
    pub generation: u64,
    pub result: Result<RemoteDocument, String>,
}

#[derive(Clone, Debug)]
pub struct RemoteSaveCompletion {
    pub generation: u64,
    pub result: Result<String, String>,
}

#[derive(Clone, Debug)]
pub struct RemoteTextCompletion {
    pub generation: u64,
    pub result: Result<Vec<String>, String>,
}

#[derive(Clone, Debug)]
pub struct RemoteTaskCompletion {
    pub generation: u64,
    pub result: Result<String, String>,
}

#[derive(Clone, Debug)]
pub struct RemoteLanguageCompletion {
    pub generation: u64,
    pub result: Result<String, String>,
}

fn discover_ssh() -> Result<SshExecutable, String> {
    let mut directories = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect::<Vec<PathBuf>>())
        .unwrap_or_default();
    if cfg!(windows)
        && let Some(system_root) = std::env::var_os("SystemRoot")
    {
        directories.push(PathBuf::from(system_root).join("System32/OpenSSH"));
    }
    SshExecutable::discover(None, &directories).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{RemoteStatus, RemoteSurfaces};

    #[test]
    fn status_labels_never_rely_on_color_alone() {
        assert_eq!(RemoteStatus::Disconnected.label(), "Disconnected");
        assert_eq!(RemoteStatus::Connecting.label(), "Connecting");
        assert_eq!(RemoteStatus::Ready.label(), "Ready");
        assert_eq!(RemoteStatus::TerminalOnly.label(), "Terminal only");
        assert_eq!(RemoteStatus::Stale.label(), "Stale — reconnect required");
    }

    #[test]
    fn invalid_connection_input_has_no_connected_side_effect() {
        let mut surfaces = RemoteSurfaces {
            alias_input: "-oProxyCommand=bad".into(),
            root_input: "relative".into(),
            ..RemoteSurfaces::default()
        };

        assert!(surfaces.begin_connect().is_err());
        assert_eq!(surfaces.status, RemoteStatus::Disconnected);
        assert_eq!(surfaces.generation(), 0);
    }

    #[test]
    fn helper_consent_requires_a_matching_packaged_checksum_sidecar() {
        let directory = tempdir().unwrap();
        let artifact = directory.path().join("strukt-remote-linux-x86_64");
        fs::write(&artifact, b"hello").unwrap();
        let mut surfaces = RemoteSurfaces::default();

        assert!(surfaces.prepare_install(&artifact).is_err());
        assert!(surfaces.install_consent.is_none());

        fs::write(
            directory
                .path()
                .join("strukt-remote-linux-x86_64.sha256"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  strukt-remote-linux-x86_64\n",
        )
        .unwrap();
        surfaces.prepare_install(&artifact).unwrap();
        let consent = surfaces.install_consent.unwrap();
        assert!(consent.contains("Linux x86_64"));
        assert!(consent.contains("SHA-256 2cf24dba"));
    }

    #[test]
    fn remote_task_requires_exact_json_arguments_and_review() {
        let mut surfaces = RemoteSurfaces {
            host_label: Some("devbox".into()),
            task_executable: "/usr/bin/cargo".into(),
            task_arguments_json: "not-json".into(),
            ..RemoteSurfaces::default()
        };

        assert!(surfaces.prepare_task().is_err());
        assert!(surfaces.task_consent.is_none());

        surfaces.task_arguments_json = r#"["test","--workspace"]"#.into();
        surfaces.prepare_task().unwrap();
        assert_eq!(
            surfaces.task_consent.as_deref(),
            Some(r#"Run on devbox: /usr/bin/cargo ["test","--workspace"]"#)
        );
    }
}
