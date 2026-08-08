use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use strukt_remote::{
    OpenSsh, OpenSshClient, RemoteRoot, RequestBody, ResponseBody, SshAlias, SshExecutable,
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
    pub document_text: String,
    pub document_revision: Option<String>,
    pub document_dirty: bool,
    pub error: Option<String>,
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
            document_text: String::new(),
            document_revision: None,
            document_dirty: false,
            error: None,
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
        self.document_text.clear();
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

    pub fn finish_connect(&mut self, completion: &RemoteConnectCompletion) -> bool {
        if completion.generation != self.generation {
            return false;
        }
        match &completion.result {
            Ok(runtime) => {
                self.status = RemoteStatus::Ready;
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
                self.document_text.clone_from(&document.text);
                self.document_revision = Some(document.revision.clone());
                self.document_dirty = false;
                self.error = None;
            }
            Err(error) => self.error = Some(error.clone()),
        }
    }

    pub fn edit_document(&mut self, text: String) {
        if self.selected_path.is_some() {
            self.document_text = text;
            self.document_dirty = true;
        }
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

    pub fn disconnected(&mut self) {
        self.generation = self.generation.saturating_add(1).max(1);
        self.status = RemoteStatus::Disconnected;
        self.host_label = None;
        self.root_label = None;
        self.files.clear();
        self.selected_path = None;
        self.document_text.clear();
        self.document_revision = None;
        self.document_dirty = false;
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
        let alias_label = self.alias.as_str().to_owned();
        let root_label = self.root.as_str().to_owned();
        let openssh = OpenSsh::new(self.executable);
        let result = OpenSshClient::connect(
            &openssh,
            &self.alias,
            env!("CARGO_PKG_VERSION"),
            self.root.as_str(),
            generation,
        )
        .map(|client| RemoteRuntime {
            client: Arc::new(Mutex::new(client)),
            alias: alias_label,
            root: root_label,
        })
        .map_err(|error| error.to_string());
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
}

impl RemoteRuntime {
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
                    .request(RequestBody::ListDirectory {
                        path: String::new(),
                        cursor: None,
                        limit: 1_000,
                    })
                    .map_err(|error| error.to_string())
            })
            .and_then(|response| match response {
                ResponseBody::DirectoryPage { entries, .. } => Ok(entries),
                ResponseBody::Error(error) => Err(error.detail),
                _ => Err("remote helper returned an unexpected file response".into()),
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

    pub fn disconnect(&self) {
        if let Ok(mut client) = self.client.lock() {
            client.disconnect();
        }
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
}
