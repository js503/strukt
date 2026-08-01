use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iced::keyboard::{self, Key};
use iced::widget::text_editor;
use iced::{Subscription, Task, Theme, time};
use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry};
use strukt_editor::{
    CloseDecision, CloseOutcome, DocumentId, EditKind, EditTransaction, EditorWorkspace,
    FindOptions, FindQuery, OpenDisposition, RelativeDocumentPath, Revision,
};
use strukt_fs::{
    CancellationToken, DiscoveryOptions, DiscoveryReport, DocumentIoError, DocumentKind,
    DocumentRead, FileEntry, FileEvent, FileKind, FileOperation, QuickOpenCandidate, ReadOptions,
    SaveMode, SaveOutcome, SaveRequest, SearchOptions, SearchResult, WorkspaceWatcher,
    apply_operation, discover_report_for_root, quick_open_candidates_with_ignored, read_document,
    save_document, search_content_cancellable,
};
use strukt_persistence::{
    EditorRecoveryStore, EditorSessionSnapshot, EditorTabSnapshot, RecentWorkspaces, RecoveryKey,
    RecoveryKeyError, RecoveryKeyProvider, RecoveryMetadata, RecoveryPayload, WorkspaceStore,
};
use strukt_shell::{Activity, ShellAction, ShellState};
use strukt_theme::ThemeMode;
use strukt_workspace::{WorkspaceRoot, WorkspaceState};

use crate::editor::EditorSurfaces;
use crate::recovery_key::NativeRecoveryKeyProvider;
use crate::workspace::{OpenedWorkspace, open_workspace_without_store};

const SMOKE_TEST_DURATION: Duration = Duration::from_secs(3);
pub(crate) const MAX_WATCHER_EVENTS_PER_POLL: usize = 64;
pub(crate) const WORKSPACE_FILES_SMOKE_SUCCESS: &str =
    "strukt workspace files smoke: open, discovery, and persistence passed";
pub(crate) const EDITOR_SMOKE_SUCCESS: &str =
    "strukt editor smoke: open, edit, save, and restore passed";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentNotice {
    Binary { path: PathBuf, size: u64 },
    InvalidUtf8 { path: PathBuf, size: u64 },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LaunchMode {
    #[default]
    Interactive,
    SmokeTest,
    WorkspaceFilesSmoke {
        root: PathBuf,
    },
    EditorSmoke {
        root: PathBuf,
    },
}

impl LaunchMode {
    #[must_use]
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let args = args.into_iter().collect::<Vec<_>>();
        match args.as_slice() {
            [flag, root] if flag == "--workspace-files-smoke" && !root.is_empty() => {
                Self::WorkspaceFilesSmoke {
                    root: PathBuf::from(root),
                }
            }
            [flag, root] if flag == "--editor-smoke" && !root.is_empty() => Self::EditorSmoke {
                root: PathBuf::from(root),
            },
            _ if args.iter().any(|argument| argument == "--smoke-test") => Self::SmokeTest,
            _ => Self::Interactive,
        }
    }

    #[must_use]
    pub const fn smoke_timeout(&self) -> Option<Duration> {
        match self {
            Self::Interactive | Self::WorkspaceFilesSmoke { .. } | Self::EditorSmoke { .. } => None,
            Self::SmokeTest => Some(SMOKE_TEST_DURATION),
        }
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent user preferences and in-flight UI gates intentionally remain explicit"
)]
pub struct StruktApp {
    pub capabilities: CapabilityRegistry,
    pub shell: ShellState,
    pub workspace: Option<WorkspaceState>,
    pub files: Vec<FileEntry>,
    pub file_warnings: Vec<String>,
    pub filesystem_truncated: bool,
    pub explorer_options: DiscoveryOptions,
    pub workspace_error: Option<String>,
    pub selected_entry: Option<PathBuf>,
    pub explorer_dialog: ExplorerDialog,
    pub recent_workspaces: Vec<PathBuf>,
    pub quick_open_visible: bool,
    pub quick_open_query: String,
    pub quick_open_results: Vec<QuickOpenCandidate>,
    pub quick_open_include_ignored: bool,
    pub search_query: String,
    pub search_results: SearchResult,
    pub search_include_ignored: bool,
    pub(crate) editor: Option<EditorWorkspace>,
    pub(crate) editor_surfaces: EditorSurfaces,
    pub document_notice: Option<DocumentNotice>,
    pub editor_error: Option<String>,
    pub pending_close: Option<DocumentId>,
    pub editor_find_visible: bool,
    pub editor_find_query: String,
    pub editor_replace_text: String,
    pub editor_find_options: FindOptions,
    pub editor_language_overrides: HashMap<DocumentId, String>,
    editor_scroll_lines: HashMap<DocumentId, f32>,
    editor_restore_active: Option<String>,
    editor_restore_tabs: HashMap<String, EditorTabSnapshot>,
    editor_restore_pending: HashSet<String>,
    launch_mode: LaunchMode,
    store: Option<WorkspaceStore>,
    pub(crate) recovery_store: Option<EditorRecoveryStore>,
    recovery_key_provider: Arc<dyn RecoveryKeyProvider>,
    recovery_generations: HashMap<DocumentId, u64>,
    recent_save_revisions: HashMap<DocumentId, strukt_editor::DiskRevision>,
    watcher: Option<WorkspaceWatcher>,
    watcher_root: Option<PathBuf>,
    manual_open_started: bool,
    open_folder_in_flight: bool,
    recent_mutation_in_flight: bool,
    open_error: Option<String>,
    operation_error: Option<String>,
    refresh_error: Option<String>,
    quick_open_error: Option<String>,
    search_error: Option<String>,
    refresh_generation: u64,
    refresh_in_flight: Option<u64>,
    refresh_pending: bool,
    operation_generation: u64,
    operation_in_flight: Option<(u64, PathBuf)>,
    persistence_generation: u64,
    persistence_in_flight: Option<(u64, PathBuf)>,
    persistence_pending: Option<WorkspaceState>,
    pending_recent_roots: VecDeque<WorkspaceRoot>,
    active_recent_roots: Vec<WorkspaceRoot>,
    persistence_error: Option<String>,
    search_generation: u64,
    search_cancellation: CancellationToken,
    filesystem_revision: u64,
    quick_open_generation: u64,
    quick_open_scan_in_flight: Option<(u64, PathBuf, u64)>,
    quick_open_cache: Option<QuickOpenCache>,
}

struct QuickOpenCache {
    workspace_root: PathBuf,
    filesystem_revision: u64,
    files: Vec<FileEntry>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ExplorerDialog {
    #[default]
    None,
    CreateFile(String),
    CreateDirectory(String),
    Rename {
        from: PathBuf,
        to: String,
    },
    Duplicate {
        from: PathBuf,
        to: String,
    },
    ConfirmTrash(PathBuf),
    ConfirmPermanentDelete(PathBuf),
}

#[derive(Clone, Debug)]
pub enum Message {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
    OpenFolder,
    FolderPicked(Option<PathBuf>),
    WorkspaceOpened(Result<OpenedWorkspace, String>),
    ToggleHiddenFiles,
    ToggleIgnoredFiles,
    FilesRefreshed {
        generation: u64,
        result: Result<DiscoveryReport, String>,
    },
    OpenDocument {
        path: PathBuf,
        disposition: OpenDisposition,
        force_full: bool,
    },
    DocumentOpened {
        workspace_root: PathBuf,
        path: PathBuf,
        disposition: OpenDisposition,
        result: Result<DocumentRead, String>,
    },
    EditorAction {
        id: DocumentId,
        action: text_editor::Action,
    },
    SelectDocument(DocumentId),
    PinDocument(DocumentId),
    CloseDocument(DocumentId),
    ResolveDocumentClose {
        id: DocumentId,
        decision: CloseDecision,
    },
    SaveDocument {
        id: DocumentId,
        mode: SaveMode,
    },
    DocumentSaved {
        workspace_root: PathBuf,
        id: DocumentId,
        expected_revision: Revision,
        result: Result<SaveOutcome, String>,
    },
    DocumentDiskObserved {
        workspace_root: PathBuf,
        id: DocumentId,
        expected_revision: Revision,
        result: Result<DiskObservation, String>,
    },
    ReloadDocumentFromDisk(DocumentId),
    KeepEditingDocument(DocumentId),
    RecoveryDue {
        workspace_root: PathBuf,
        id: DocumentId,
        expected_revision: Revision,
        generation: u64,
    },
    RecoverySaved {
        workspace_root: PathBuf,
        id: DocumentId,
        generation: u64,
        result: Result<(), String>,
    },
    RecoveryLoaded {
        workspace_root: PathBuf,
        id: DocumentId,
        expected_revision: Revision,
        result: Result<Option<RecoveryPayload>, String>,
    },
    RecoveryDeleted {
        workspace_root: PathBuf,
        id: DocumentId,
        result: Result<(), String>,
    },
    UndoDocument(DocumentId),
    RedoDocument(DocumentId),
    ToggleEditorFind,
    EditorFindChanged(String),
    EditorReplaceChanged(String),
    ToggleFindCase,
    ToggleFindWholeWord,
    ToggleFindRegex,
    SetLanguageOverride {
        id: DocumentId,
        language: Option<String>,
    },
    ReplaceAll(DocumentId),
    SelectExplorerEntry(PathBuf),
    BeginCreateFile,
    BeginCreateDirectory,
    BeginRename,
    BeginDuplicate,
    BeginTrash,
    BeginPermanentDelete,
    ExplorerDialogInput(String),
    CancelExplorerDialog,
    SubmitExplorerDialog,
    FileOperationCompleted {
        generation: u64,
        workspace_root: PathBuf,
        result: Result<(), String>,
    },
    PollWatcher,
    FileEvent {
        workspace_root: PathBuf,
        event: FileEvent,
    },
    WorkspacePersisted {
        generation: u64,
        workspace_root: PathBuf,
        recent_roots: Vec<WorkspaceRoot>,
        result: Result<(), String>,
    },
    RecentWorkspaceLoaded(Result<RecentWorkspaces, String>),
    RetryRecentWorkspace(PathBuf),
    LocateRecentWorkspace(PathBuf),
    RecentWorkspaceLocated {
        old_path: PathBuf,
        new_path: Option<PathBuf>,
    },
    RemoveRecentWorkspace(PathBuf),
    RecentWorkspacesUpdated(Result<RecentWorkspaces, String>),
    ToggleQuickOpen,
    QuickOpenChanged(String),
    QuickOpenSelected(PathBuf),
    ToggleQuickOpenIgnored,
    QuickOpenFilesLoaded {
        generation: u64,
        workspace_root: PathBuf,
        filesystem_revision: u64,
        result: Result<Vec<FileEntry>, String>,
    },
    SearchChanged(String),
    SearchDebounced {
        generation: u64,
        workspace_root: PathBuf,
        query: String,
    },
    SearchCompleted {
        generation: u64,
        workspace_root: PathBuf,
        result: Result<SearchResult, String>,
    },
    ToggleSearchIgnored,
    Keyboard(keyboard::Event),
    SmokeTimeout,
    WorkspaceFilesSmokeFinished(Result<(), String>),
}

#[derive(Clone, Debug)]
pub enum DiskObservation {
    Present(DocumentRead),
    Missing,
}

impl Default for StruktApp {
    fn default() -> Self {
        Self::new_with_store(LaunchMode::Interactive, None)
    }
}

impl StruktApp {
    #[must_use]
    pub fn new(launch_mode: LaunchMode) -> Self {
        Self::new_with_store(launch_mode, WorkspaceStore::platform_default().ok())
    }

    #[must_use]
    pub(crate) fn new_with_store(launch_mode: LaunchMode, store: Option<WorkspaceStore>) -> Self {
        let mut capabilities = CapabilityRegistry::new();
        for descriptor in [
            CapabilityDescriptor::new(CapabilityId::FILES, true),
            CapabilityDescriptor::new(CapabilityId::TERMINAL, true),
            CapabilityDescriptor::new(CapabilityId::THEMES, true),
            CapabilityDescriptor::new(CapabilityId::CONNECTIONS, true),
            CapabilityDescriptor::new(CapabilityId::AI, true),
            CapabilityDescriptor::new(CapabilityId::EDITOR_DOCUMENTS, true),
            CapabilityDescriptor::new(CapabilityId::EDITOR_SYNTAX, true),
        ] {
            capabilities
                .register(descriptor)
                .expect("built-in capability identifiers must be unique");
        }

        Self {
            capabilities,
            shell: ShellState::default(),
            workspace: None,
            files: Vec::new(),
            file_warnings: Vec::new(),
            filesystem_truncated: false,
            explorer_options: DiscoveryOptions::default(),
            workspace_error: None,
            selected_entry: None,
            explorer_dialog: ExplorerDialog::None,
            recent_workspaces: Vec::new(),
            quick_open_visible: false,
            quick_open_query: String::new(),
            quick_open_results: Vec::new(),
            quick_open_include_ignored: false,
            search_query: String::new(),
            search_results: SearchResult {
                matches: Vec::new(),
                truncated: false,
            },
            search_include_ignored: false,
            editor: None,
            editor_surfaces: EditorSurfaces::default(),
            document_notice: None,
            editor_error: None,
            pending_close: None,
            editor_find_visible: false,
            editor_find_query: String::new(),
            editor_replace_text: String::new(),
            editor_find_options: FindOptions::default(),
            editor_language_overrides: HashMap::new(),
            editor_scroll_lines: HashMap::new(),
            editor_restore_active: None,
            editor_restore_tabs: HashMap::new(),
            editor_restore_pending: HashSet::new(),
            launch_mode,
            store,
            recovery_store: EditorRecoveryStore::platform_default().ok(),
            recovery_key_provider: Arc::new(NativeRecoveryKeyProvider),
            recovery_generations: HashMap::new(),
            recent_save_revisions: HashMap::new(),
            watcher: None,
            watcher_root: None,
            manual_open_started: false,
            open_folder_in_flight: false,
            recent_mutation_in_flight: false,
            open_error: None,
            operation_error: None,
            refresh_error: None,
            quick_open_error: None,
            search_error: None,
            refresh_generation: 0,
            refresh_in_flight: None,
            refresh_pending: false,
            operation_generation: 0,
            operation_in_flight: None,
            persistence_generation: 0,
            persistence_in_flight: None,
            persistence_pending: None,
            pending_recent_roots: VecDeque::new(),
            active_recent_roots: Vec::new(),
            persistence_error: None,
            search_generation: 0,
            search_cancellation: CancellationToken::new(),
            filesystem_revision: 0,
            quick_open_generation: 0,
            quick_open_scan_in_flight: None,
            quick_open_cache: None,
        }
    }

    pub fn boot(launch_mode: LaunchMode) -> (Self, Task<Message>) {
        if let LaunchMode::WorkspaceFilesSmoke { root } = &launch_mode {
            let root = root.clone();
            let app = Self::new_with_store(launch_mode, None);
            let smoke = Task::perform(
                workspace_files_smoke_task(root),
                Message::WorkspaceFilesSmokeFinished,
            );
            return (app, smoke);
        }

        let mut app = Self::new(launch_mode);
        let Some(store) = app.store.clone() else {
            app.open_error = Some("platform application-data directory is unavailable".to_owned());
            app.recompute_workspace_error();
            return (app, Task::none());
        };
        let restore = Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    store.load_recent().map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            Message::RecentWorkspaceLoaded,
        );
        (app, restore)
    }
}

impl StruktApp {
    #[expect(
        clippy::too_many_lines,
        reason = "the reducer keeps message ownership and task scheduling explicit in one exhaustive match"
    )]
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFolder => {
                if self.open_folder_in_flight || self.recent_mutation_in_flight {
                    return Task::none();
                }
                self.open_folder_in_flight = true;
                self.manual_open_started = true;
                self.open_error = None;
                self.recompute_workspace_error();
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Open a strukt workspace")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::FolderPicked,
                );
            }
            Message::FolderPicked(Some(path)) => {
                return self.open_workspace_task(path);
            }
            Message::FolderPicked(None) => {
                self.open_folder_in_flight = false;
                return Task::none();
            }
            Message::WorkspaceOpened(Ok(mut opened)) => {
                let editor_restore = opened
                    .state
                    .contribution::<EditorSessionSnapshot>("editor")
                    .ok()
                    .flatten();
                let root = opened.state.root.path().to_path_buf();
                match WorkspaceWatcher::start(&root) {
                    Ok(watcher) => {
                        self.watcher = Some(watcher);
                        self.watcher_root = Some(root.clone());
                        opened.state.stale_filesystem = false;
                    }
                    Err(error) => {
                        self.watcher = None;
                        self.watcher_root = None;
                        self.refresh_error = Some(error.to_string());
                    }
                }
                self.explorer_options = DiscoveryOptions {
                    show_hidden: opened.state.explorer.show_hidden,
                    show_ignored: opened.state.explorer.show_ignored,
                    ..DiscoveryOptions::default()
                };
                self.shell.explorer_visible = opened.state.explorer.visible;
                self.files = opened.discovery.entries;
                self.file_warnings = opened.discovery.warnings;
                self.filesystem_truncated = opened.discovery.truncated;
                self.workspace = Some(opened.state);
                self.editor = self
                    .workspace
                    .as_ref()
                    .map(|workspace| EditorWorkspace::new(workspace.root.id().clone()));
                self.editor_surfaces = EditorSurfaces::default();
                self.document_notice = None;
                self.editor_error = None;
                self.pending_close = None;
                self.editor_find_visible = false;
                self.editor_find_query.clear();
                self.editor_replace_text.clear();
                self.editor_language_overrides.clear();
                self.editor_scroll_lines.clear();
                self.recovery_generations.clear();
                self.recent_save_revisions.clear();
                self.editor_restore_active = editor_restore
                    .as_ref()
                    .and_then(|snapshot| snapshot.active_path.clone());
                self.editor_restore_tabs = editor_restore
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .tabs
                            .iter()
                            .cloned()
                            .map(|tab| (tab.path.clone(), tab))
                            .collect()
                    })
                    .unwrap_or_default();
                self.editor_restore_pending = self.editor_restore_tabs.keys().cloned().collect();
                if self.watcher.is_none()
                    && let Some(workspace) = &mut self.workspace
                {
                    workspace.stale_filesystem = true;
                }
                self.selected_entry = None;
                self.explorer_dialog = ExplorerDialog::None;
                self.open_error = None;
                self.operation_error = None;
                self.persistence_error = None;
                self.quick_open_error = None;
                self.search_error = None;
                if self.watcher.is_some() {
                    self.refresh_error = None;
                }
                self.recompute_workspace_error();
                self.open_folder_in_flight = false;
                self.refresh_generation = self.refresh_generation.wrapping_add(1);
                self.refresh_pending = false;
                self.filesystem_revision = self.filesystem_revision.wrapping_add(1);
                self.invalidate_quick_open_cache();
                self.search_generation = self.search_generation.wrapping_add(1);
                self.search_cancellation.cancel();
                self.search_cancellation = CancellationToken::new();
                self.quick_open_visible = false;
                self.quick_open_results.clear();
                self.search_results.matches.clear();
                self.search_results.truncated = false;
                let persistence = self.request_persistence(true);
                let reconciliation = self.request_file_refresh();
                let mut tasks = vec![persistence, reconciliation];
                if let Some(snapshot) = editor_restore {
                    for tab in snapshot.tabs {
                        let disposition =
                            if snapshot.preview_path.as_deref() == Some(tab.path.as_str()) {
                                OpenDisposition::Preview
                            } else {
                                OpenDisposition::Pinned
                            };
                        tasks.push(self.open_document_task(
                            PathBuf::from(tab.path),
                            disposition,
                            !tab.read_only,
                        ));
                    }
                }
                return Task::batch(tasks);
            }
            Message::WorkspaceOpened(Err(error)) => {
                self.open_error = Some(error);
                self.recompute_workspace_error();
                self.open_folder_in_flight = false;
                return Task::none();
            }
            Message::ToggleHiddenFiles => {
                if !self.can_use_explorer_controls() {
                    return Task::none();
                }
                self.explorer_options.show_hidden = !self.explorer_options.show_hidden;
                let refresh = self.request_file_refresh();
                let persist = self.request_persistence(false);
                return Task::batch([refresh, persist]);
            }
            Message::ToggleIgnoredFiles => {
                if !self.can_use_explorer_controls() {
                    return Task::none();
                }
                self.explorer_options.show_ignored = !self.explorer_options.show_ignored;
                let refresh = self.request_file_refresh();
                let persist = self.request_persistence(false);
                return Task::batch([refresh, persist]);
            }
            Message::FilesRefreshed { generation, result } => {
                if self.refresh_in_flight != Some(generation) {
                    return Task::none();
                }
                let mut completion_tasks = Vec::new();
                self.refresh_in_flight = None;
                if generation == self.refresh_generation {
                    match result {
                        Ok(report) => {
                            self.files = report.entries;
                            self.file_warnings = report.warnings;
                            self.filesystem_truncated = report.truncated;
                            self.filesystem_revision = self.filesystem_revision.wrapping_add(1);
                            self.invalidate_quick_open_cache();
                            if self.quick_open_visible {
                                if self.quick_open_include_ignored {
                                    completion_tasks.push(self.start_quick_open_scan());
                                } else {
                                    self.quick_open_results = quick_open_candidates_with_ignored(
                                        &self.files,
                                        &self.quick_open_query,
                                        50,
                                        false,
                                    );
                                }
                            }
                            self.refresh_error = None;
                            if let Some(workspace) = &mut self.workspace {
                                workspace.stale_filesystem = false;
                            }
                            self.reconcile_explorer_targets();
                        }
                        Err(error) => self.refresh_error = Some(error),
                    }
                    self.recompute_workspace_error();
                }
                if self.refresh_pending {
                    self.refresh_pending = false;
                    completion_tasks.push(self.start_file_refresh());
                } else if self.persistence_in_flight.is_none()
                    && let Some(state) = self.persistence_pending.take()
                {
                    completion_tasks.push(self.start_persistence(state));
                }
                return Task::batch(completion_tasks);
            }
            Message::SelectExplorerEntry(path) => {
                if self.explorer_dialog == ExplorerDialog::None
                    && self.operation_in_flight.is_none()
                    && is_scoped_relative_path(&path)
                {
                    let is_file = self
                        .files
                        .iter()
                        .any(|entry| entry.relative_path == path && entry.kind == FileKind::File);
                    self.selected_entry = Some(path.clone());
                    if is_file {
                        self.editor_restore_active = None;
                        return self.open_document_task(path, OpenDisposition::Preview, false);
                    }
                }
                return Task::none();
            }
            Message::OpenDocument {
                path,
                disposition,
                force_full,
            } => {
                self.editor_restore_active = None;
                return self.open_document_task(path, disposition, force_full);
            }
            Message::DocumentOpened {
                workspace_root,
                path,
                disposition,
                result,
            } => {
                if !self.is_current_root(&workspace_root) {
                    return Task::none();
                }
                let path_string = path.to_string_lossy().into_owned();
                let was_restore = self.editor_restore_pending.remove(&path_string);
                match result {
                    Ok(opened) => match opened.kind {
                        DocumentKind::Text { read_only, .. } => {
                            let Some(text) = opened.text else {
                                self.editor_error =
                                    Some("text document had no text payload".into());
                                return Task::none();
                            };
                            let Some(editor) = &mut self.editor else {
                                return Task::none();
                            };
                            let result = RelativeDocumentPath::new(&path_string)
                                .map_err(|error| error.to_string())
                                .and_then(|path| {
                                    editor
                                        .open(
                                            path,
                                            &text,
                                            opened.disk_revision,
                                            read_only,
                                            disposition,
                                        )
                                        .map_err(|error| error.to_string())
                                });
                            match result {
                                Ok(id) => {
                                    let surface_text = editor
                                        .document(id)
                                        .map_or_else(String::new, strukt_editor::Document::text);
                                    self.editor_surfaces.insert(id, &surface_text);
                                    if let Some(snapshot) =
                                        self.editor_restore_tabs.get(&path_string).cloned()
                                    {
                                        let _ = self.editor_surfaces.restore_view(
                                            id,
                                            snapshot.cursor,
                                            snapshot.selection_anchor,
                                            snapshot.scroll_line,
                                        );
                                        self.editor_scroll_lines.insert(id, snapshot.scroll_line);
                                        if let Some(language) = snapshot.language_override {
                                            self.editor_language_overrides.insert(id, language);
                                        }
                                        if self.editor_restore_active.as_deref()
                                            == Some(path_string.as_str())
                                        {
                                            self.editor_find_query = snapshot.find_query;
                                            self.editor_replace_text = snapshot.replace_text;
                                            self.editor_find_options = FindOptions {
                                                case_sensitive: snapshot
                                                    .find_options
                                                    .case_sensitive,
                                                whole_word: snapshot.find_options.whole_word,
                                                regex: snapshot.find_options.regex,
                                            };
                                        }
                                    }
                                    self.document_notice = None;
                                    self.editor_error = None;
                                    if was_restore
                                        && let Some(active_path) = &self.editor_restore_active
                                        && let Some(active) = editor
                                            .view_state()
                                            .tabs
                                            .iter()
                                            .find(|tab| tab.path.as_str() == active_path)
                                    {
                                        let _ = editor.select(active.id);
                                    }
                                    if self.editor_restore_pending.is_empty() {
                                        self.editor_restore_active = None;
                                    }
                                    let recovery = self.load_recovery_task(id);
                                    let persistence = self.request_persistence(false);
                                    return Task::batch([recovery, persistence]);
                                }
                                Err(error) => self.editor_error = Some(error),
                            }
                        }
                        DocumentKind::Binary => {
                            self.document_notice = Some(DocumentNotice::Binary {
                                path,
                                size: opened.size,
                            });
                        }
                        DocumentKind::InvalidUtf8 => {
                            self.document_notice = Some(DocumentNotice::InvalidUtf8 {
                                path,
                                size: opened.size,
                            });
                        }
                    },
                    Err(error) => {
                        let restore_snapshot = self.editor_restore_tabs.get(&path_string).cloned();
                        let baseline = restore_snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.disk_revision.clone());
                        if let (Some(baseline), Some(editor)) = (baseline, &mut self.editor) {
                            let opened = RelativeDocumentPath::new(&path_string)
                                .map_err(|open_error| open_error.to_string())
                                .and_then(|relative| {
                                    editor
                                        .open(
                                            relative,
                                            "",
                                            strukt_editor::DiskRevision::new(baseline),
                                            false,
                                            disposition,
                                        )
                                        .map_err(|open_error| open_error.to_string())
                                });
                            match opened {
                                Ok(id) => {
                                    let revision = editor
                                        .document(id)
                                        .map(strukt_editor::Document::revision)
                                        .unwrap_or_default();
                                    let _ = editor.observe_missing(id, revision);
                                    self.editor_surfaces.insert(id, "");
                                    if let Some(snapshot) = restore_snapshot {
                                        let _ = self.editor_surfaces.restore_view(
                                            id,
                                            snapshot.cursor,
                                            snapshot.selection_anchor,
                                            snapshot.scroll_line,
                                        );
                                        self.editor_scroll_lines.insert(id, snapshot.scroll_line);
                                        if let Some(language) = snapshot.language_override {
                                            self.editor_language_overrides.insert(id, language);
                                        }
                                        if self.editor_restore_active.as_deref()
                                            == Some(path_string.as_str())
                                        {
                                            self.editor_find_query = snapshot.find_query;
                                            self.editor_replace_text = snapshot.replace_text;
                                            self.editor_find_options = FindOptions {
                                                case_sensitive: snapshot
                                                    .find_options
                                                    .case_sensitive,
                                                whole_word: snapshot.find_options.whole_word,
                                                regex: snapshot.find_options.regex,
                                            };
                                        }
                                    }
                                    self.editor_error = Some(format!(
                                        "{error}; showing restorable missing-file placeholder"
                                    ));
                                    if self.editor_restore_pending.is_empty() {
                                        self.editor_restore_active = None;
                                    }
                                    let recovery = self.load_recovery_task(id);
                                    let persistence = self.request_persistence(false);
                                    return Task::batch([recovery, persistence]);
                                }
                                Err(open_error) => self.editor_error = Some(open_error),
                            }
                        } else {
                            self.editor_error = Some(error);
                        }
                    }
                }
                if self.editor_restore_pending.is_empty() {
                    self.editor_restore_active = None;
                }
                return Task::none();
            }
            Message::EditorAction { id, action } => {
                let is_edit = action.is_edit();
                let scroll_delta = match &action {
                    text_editor::Action::Scroll { lines } => Some(scroll_lines_as_f32(*lines)),
                    _ => None,
                };
                let Some(editor) = &mut self.editor else {
                    return Task::none();
                };
                if let Err(error) = self.editor_surfaces.perform(editor, id, action) {
                    self.editor_error = Some(error.to_string());
                } else {
                    self.editor_error = None;
                    if let Some(delta) = scroll_delta {
                        *self.editor_scroll_lines.entry(id).or_default() += delta;
                    }
                    if is_edit {
                        let recovery = self.schedule_recovery(id);
                        let persistence = self.request_persistence(false);
                        return Task::batch([recovery, persistence]);
                    }
                }
                return Task::none();
            }
            Message::SelectDocument(id) => {
                if let Some(editor) = &mut self.editor
                    && let Err(error) = editor.select(id)
                {
                    self.editor_error = Some(error.to_string());
                }
                return self.request_persistence(false);
            }
            Message::PinDocument(id) => {
                if let Some(editor) = &mut self.editor
                    && let Err(error) = editor.pin_document(id)
                {
                    self.editor_error = Some(error.to_string());
                }
                return self.request_persistence(false);
            }
            Message::UndoDocument(id) => {
                let Some(editor) = &mut self.editor else {
                    return Task::none();
                };
                match editor
                    .undo(id)
                    .and_then(|()| self.editor_surfaces.rebuild(editor, id))
                {
                    Ok(()) => {
                        self.editor_error = None;
                        let recovery = self.schedule_recovery(id);
                        let persistence = self.request_persistence(false);
                        return Task::batch([recovery, persistence]);
                    }
                    Err(error) => self.editor_error = Some(error.to_string()),
                }
                return Task::none();
            }
            Message::RedoDocument(id) => {
                let Some(editor) = &mut self.editor else {
                    return Task::none();
                };
                match editor
                    .redo(id)
                    .and_then(|()| self.editor_surfaces.rebuild(editor, id))
                {
                    Ok(()) => {
                        self.editor_error = None;
                        let recovery = self.schedule_recovery(id);
                        let persistence = self.request_persistence(false);
                        return Task::batch([recovery, persistence]);
                    }
                    Err(error) => self.editor_error = Some(error.to_string()),
                }
                return Task::none();
            }
            Message::ToggleEditorFind => {
                self.editor_find_visible = !self.editor_find_visible;
                return Task::none();
            }
            Message::EditorFindChanged(query) => {
                self.editor_find_query = query;
                return self.request_persistence(false);
            }
            Message::EditorReplaceChanged(replacement) => {
                self.editor_replace_text = replacement;
                return self.request_persistence(false);
            }
            Message::ToggleFindCase => {
                self.editor_find_options.case_sensitive = !self.editor_find_options.case_sensitive;
                return self.request_persistence(false);
            }
            Message::ToggleFindWholeWord => {
                self.editor_find_options.whole_word = !self.editor_find_options.whole_word;
                return self.request_persistence(false);
            }
            Message::ToggleFindRegex => {
                self.editor_find_options.regex = !self.editor_find_options.regex;
                return self.request_persistence(false);
            }
            Message::SetLanguageOverride { id, language } => {
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.document(id).is_some())
                {
                    match language {
                        Some(language) => {
                            self.editor_language_overrides.insert(id, language);
                        }
                        None => {
                            self.editor_language_overrides.remove(&id);
                        }
                    }
                }
                return self.request_persistence(false);
            }
            Message::ReplaceAll(id) => {
                let Some(editor) = &mut self.editor else {
                    return Task::none();
                };
                let result = (|| {
                    let document = editor
                        .document(id)
                        .ok_or_else(|| "document is not open".to_owned())?;
                    let query = FindQuery::new(&self.editor_find_query, self.editor_find_options)
                        .map_err(|error| error.to_string())?;
                    let text = document.text();
                    let revision = document.revision();
                    let transaction = query
                        .replace_all(revision, &text, &self.editor_replace_text)
                        .map_err(|error| error.to_string())?;
                    editor
                        .edit(id, transaction, EditKind::Other, 0, 0)
                        .map_err(|error| error.to_string())?;
                    self.editor_surfaces
                        .rebuild(editor, id)
                        .map_err(|error| error.to_string())
                })();
                match result {
                    Ok(()) => {
                        self.editor_error = None;
                        let recovery = self.schedule_recovery(id);
                        let persistence = self.request_persistence(false);
                        return Task::batch([recovery, persistence]);
                    }
                    Err(error) => self.editor_error = Some(error),
                }
                return Task::none();
            }
            Message::CloseDocument(id) => {
                if let Some(editor) = &mut self.editor {
                    match editor.request_close(id) {
                        Ok(CloseOutcome::Closed) => self.editor_surfaces.remove(id),
                        Ok(CloseOutcome::NeedsDecision) => self.pending_close = Some(id),
                        Ok(_) => {}
                        Err(error) => self.editor_error = Some(error.to_string()),
                    }
                }
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.document(id).is_none())
                {
                    self.editor_scroll_lines.remove(&id);
                    self.editor_language_overrides.remove(&id);
                }
                return self.request_persistence(false);
            }
            Message::ResolveDocumentClose { id, decision } => {
                if decision == CloseDecision::Save {
                    return self.save_document_task(id, SaveMode::IfUnchanged);
                }
                let cleanup = self.delete_recovery_task(id);
                if let Some(editor) = &mut self.editor {
                    match editor.resolve_close(id, decision) {
                        Ok(CloseOutcome::Closed) => self.editor_surfaces.remove(id),
                        Ok(_) => {}
                        Err(error) => self.editor_error = Some(error.to_string()),
                    }
                }
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.document(id).is_none())
                {
                    self.editor_scroll_lines.remove(&id);
                    self.editor_language_overrides.remove(&id);
                }
                self.pending_close = None;
                let persistence = self.request_persistence(false);
                return Task::batch([cleanup, persistence]);
            }
            Message::SaveDocument { id, mode } => return self.save_document_task(id, mode),
            Message::DocumentSaved {
                workspace_root,
                id,
                expected_revision,
                result,
            } => {
                if !self.is_current_root(&workspace_root) {
                    return Task::none();
                }
                let cleanup_metadata = self.recovery_metadata(id);
                let mut saved_applied = false;
                match result {
                    Ok(saved) => {
                        if let Some(editor) = &mut self.editor {
                            match editor.complete_save(id, expected_revision, saved.disk_revision) {
                                Ok(()) => {
                                    saved_applied = true;
                                    if let Some(document) = editor.document(id) {
                                        self.recent_save_revisions
                                            .insert(id, document.disk_revision().clone());
                                    }
                                    self.editor_error = None;
                                    if self.pending_close == Some(id) {
                                        if editor.resolve_close(id, CloseDecision::Discard).is_ok()
                                        {
                                            self.editor_surfaces.remove(id);
                                        }
                                        self.pending_close = None;
                                    }
                                }
                                Err(error) => self.editor_error = Some(error.to_string()),
                            }
                        }
                    }
                    Err(error) => self.editor_error = Some(error),
                }
                let cleanup = if saved_applied {
                    cleanup_metadata.map_or_else(Task::none, |metadata| {
                        self.delete_recovery_metadata_task(id, metadata)
                    })
                } else {
                    Task::none()
                };
                let persistence = self.request_persistence(false);
                return Task::batch([cleanup, persistence]);
            }
            Message::DocumentDiskObserved {
                workspace_root,
                id,
                expected_revision,
                result,
            } => {
                if !self.is_current_root(&workspace_root) {
                    return Task::none();
                }
                let Some(editor) = &mut self.editor else {
                    return Task::none();
                };
                if let Ok(DiskObservation::Present(read)) = &result
                    && self.recent_save_revisions.get(&id) == Some(&read.disk_revision)
                {
                    self.recent_save_revisions.remove(&id);
                    return Task::none();
                }
                self.recent_save_revisions.remove(&id);
                let was_clean = editor.document(id).is_some_and(|document| {
                    document.status() == &strukt_editor::DocumentStatus::Clean
                });
                let applied = match result {
                    Ok(DiskObservation::Present(read)) => match read.kind {
                        DocumentKind::Text {
                            read_only: false, ..
                        } => read.text.map_or_else(
                            || Err("text document had no text payload".to_owned()),
                            |text| {
                                editor
                                    .observe_disk_change(
                                        id,
                                        expected_revision,
                                        read.disk_revision,
                                        &text,
                                    )
                                    .map_err(|error| error.to_string())
                            },
                        ),
                        _ => editor
                            .observe_missing(id, expected_revision)
                            .map_err(|error| error.to_string()),
                    },
                    Ok(DiskObservation::Missing) => editor
                        .observe_missing(id, expected_revision)
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(error),
                };
                match applied {
                    Ok(()) => {
                        self.editor_error = None;
                        if was_clean {
                            let _ = self.editor_surfaces.rebuild(editor, id);
                        }
                    }
                    Err(error) => self.editor_error = Some(error),
                }
                return self.request_persistence(false);
            }
            Message::ReloadDocumentFromDisk(id) => {
                let Some(editor) = &mut self.editor else {
                    return Task::none();
                };
                match editor
                    .reload_from_disk(id)
                    .and_then(|()| self.editor_surfaces.rebuild(editor, id))
                {
                    Ok(()) => self.editor_error = None,
                    Err(error) => self.editor_error = Some(error.to_string()),
                }
                return self.request_persistence(false);
            }
            Message::KeepEditingDocument(id) => {
                if let Some(editor) = &mut self.editor {
                    match editor.keep_editing(id) {
                        Ok(()) => self.editor_error = None,
                        Err(error) => self.editor_error = Some(error.to_string()),
                    }
                }
                return self.request_persistence(false);
            }
            Message::RecoveryDue {
                workspace_root,
                id,
                expected_revision,
                generation,
            } => {
                if !self.is_current_root(&workspace_root)
                    || self.recovery_generations.get(&id) != Some(&generation)
                {
                    return Task::none();
                }
                return self.save_recovery_task(id, expected_revision, generation);
            }
            Message::RecoverySaved {
                workspace_root,
                id,
                generation,
                result,
            } => {
                if !self.is_current_root(&workspace_root)
                    || self.recovery_generations.get(&id) != Some(&generation)
                {
                    return Task::none();
                }
                if let Err(error) = result {
                    self.editor_error = Some(format!("recovery disabled: {error}"));
                }
                return Task::none();
            }
            Message::RecoveryLoaded {
                workspace_root,
                id,
                expected_revision,
                result,
            } => {
                if !self.is_current_root(&workspace_root) {
                    return Task::none();
                }
                match result {
                    Ok(Some(payload)) => {
                        let Some(editor) = &mut self.editor else {
                            return Task::none();
                        };
                        if editor.document(id).map(strukt_editor::Document::revision)
                            != Some(expected_revision)
                        {
                            return Task::none();
                        }
                        match editor
                            .restore_recovery(id, &payload.text)
                            .and_then(|()| self.editor_surfaces.rebuild(editor, id))
                        {
                            Ok(()) => self.editor_error = None,
                            Err(error) => self.editor_error = Some(error.to_string()),
                        }
                    }
                    Ok(None) => {}
                    Err(error) => self.editor_error = Some(format!("recovery disabled: {error}")),
                }
                return self.request_persistence(false);
            }
            Message::RecoveryDeleted {
                workspace_root,
                id,
                result,
            } => {
                if self.is_current_root(&workspace_root) {
                    self.recovery_generations.remove(&id);
                    if let Err(error) = result {
                        self.editor_error = Some(format!("recovery cleanup failed: {error}"));
                    }
                }
                return Task::none();
            }
            Message::BeginCreateFile => {
                if self.can_begin_operation() {
                    self.explorer_dialog = ExplorerDialog::CreateFile(String::new());
                }
                return Task::none();
            }
            Message::BeginCreateDirectory => {
                if self.can_begin_operation() {
                    self.explorer_dialog = ExplorerDialog::CreateDirectory(String::new());
                }
                return Task::none();
            }
            Message::BeginRename => {
                if self.can_begin_operation()
                    && let Some(from) = self.selected_entry.clone()
                {
                    self.explorer_dialog = ExplorerDialog::Rename {
                        to: from.to_string_lossy().into_owned(),
                        from,
                    };
                }
                return Task::none();
            }
            Message::BeginDuplicate => {
                if self.can_begin_operation()
                    && let Some(from) = self.selected_entry.clone()
                {
                    self.explorer_dialog = ExplorerDialog::Duplicate {
                        to: from.to_string_lossy().into_owned(),
                        from,
                    };
                }
                return Task::none();
            }
            Message::BeginTrash => {
                if self.can_begin_operation()
                    && let Some(path) = self.selected_entry.clone()
                {
                    self.explorer_dialog = ExplorerDialog::ConfirmTrash(path);
                }
                return Task::none();
            }
            Message::BeginPermanentDelete => {
                if self.operation_in_flight.is_none() {
                    let path = match &self.explorer_dialog {
                        ExplorerDialog::ConfirmTrash(path) => Some(path.clone()),
                        ExplorerDialog::None if self.workspace.is_some() => {
                            self.selected_entry.clone()
                        }
                        _ => None,
                    };
                    if let Some(path) = path {
                        self.explorer_dialog = ExplorerDialog::ConfirmPermanentDelete(path);
                    }
                }
                return Task::none();
            }
            Message::ExplorerDialogInput(input) => {
                if self.operation_in_flight.is_none() {
                    match &mut self.explorer_dialog {
                        ExplorerDialog::CreateFile(path)
                        | ExplorerDialog::CreateDirectory(path) => *path = input,
                        ExplorerDialog::Rename { to, .. }
                        | ExplorerDialog::Duplicate { to, .. } => *to = input,
                        ExplorerDialog::None
                        | ExplorerDialog::ConfirmTrash(_)
                        | ExplorerDialog::ConfirmPermanentDelete(_) => {}
                    }
                }
                return Task::none();
            }
            Message::CancelExplorerDialog => {
                if self.operation_in_flight.is_none() {
                    self.explorer_dialog = ExplorerDialog::None;
                    self.operation_error = None;
                    self.recompute_workspace_error();
                }
                return Task::none();
            }
            Message::SubmitExplorerDialog => return self.submit_explorer_dialog(),
            Message::FileOperationCompleted {
                generation,
                workspace_root,
                result,
            } => {
                if self.operation_in_flight.as_ref() != Some(&(generation, workspace_root.clone()))
                {
                    return Task::none();
                }
                self.operation_in_flight = None;
                let current_root = self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.path());
                if current_root != Some(workspace_root.as_path()) {
                    return Task::none();
                }
                match result {
                    Ok(()) => {
                        self.explorer_dialog = ExplorerDialog::None;
                        self.selected_entry = None;
                        self.operation_error = None;
                        self.recompute_workspace_error();
                        return self.request_file_refresh();
                    }
                    Err(error) => {
                        self.operation_error = Some(error);
                        self.recompute_workspace_error();
                    }
                }
                return Task::none();
            }
            Message::PollWatcher => {
                let Some(root) = self.watcher_root.clone() else {
                    return Task::none();
                };
                let batch = self
                    .watcher
                    .as_ref()
                    .map_or_else(WatcherBatch::default, |watcher| {
                        drain_watcher_batch(|| watcher.try_recv())
                    });
                if let Some(reason) = batch.stale_reason {
                    return self.update(Message::FileEvent {
                        workspace_root: root,
                        event: FileEvent::Stale(reason),
                    });
                }
                if batch.changed {
                    return self.update(Message::FileEvent {
                        workspace_root: root,
                        event: FileEvent::Changed(batch.paths),
                    });
                }
                return Task::none();
            }
            Message::FileEvent {
                workspace_root,
                event,
            } => {
                if !self.is_current_root(&workspace_root) {
                    return Task::none();
                }
                let observations = match event {
                    FileEvent::Stale(reason) => {
                        if let Some(workspace) = &mut self.workspace {
                            workspace.stale_filesystem = true;
                        }
                        self.refresh_error = Some(reason);
                        self.recompute_workspace_error();
                        self.observe_open_documents_task(&workspace_root, &[])
                    }
                    FileEvent::Changed(paths) => {
                        self.observe_open_documents_task(&workspace_root, &paths)
                    }
                };
                let refresh = self.request_file_refresh();
                return Task::batch([refresh, observations]);
            }
            Message::WorkspacePersisted {
                generation,
                workspace_root,
                recent_roots,
                result,
            } => {
                if self.persistence_in_flight.as_ref()
                    != Some(&(generation, workspace_root.clone()))
                    || self.active_recent_roots != recent_roots
                {
                    return Task::none();
                }
                self.persistence_in_flight = None;
                self.active_recent_roots.clear();
                if result.is_err() {
                    self.requeue_recent_roots(recent_roots);
                }
                if self.is_current_root(&workspace_root) {
                    self.persistence_error = result.err();
                    self.recompute_workspace_error();
                }
                if let Some(state) = self.persistence_pending.take() {
                    return self.start_persistence(state);
                }
                return Task::none();
            }
            Message::RecentWorkspaceLoaded(result) => {
                match result {
                    Ok(recent) => {
                        self.recent_workspaces = recent.paths;
                        if !self.manual_open_started
                            && self.workspace.is_none()
                            && let Some(path) = self
                                .recent_workspaces
                                .iter()
                                .find(|path| path.is_dir())
                                .cloned()
                        {
                            return self.open_workspace_task(path);
                        }
                    }
                    Err(error) => {
                        self.open_error = Some(error);
                        self.recompute_workspace_error();
                    }
                }
                return Task::none();
            }
            Message::RetryRecentWorkspace(path) => {
                if self.open_folder_in_flight || self.recent_mutation_in_flight {
                    return Task::none();
                }
                self.manual_open_started = true;
                return self.open_workspace_task(path);
            }
            Message::LocateRecentWorkspace(old_path) => {
                if self.open_folder_in_flight || self.recent_mutation_in_flight {
                    return Task::none();
                }
                self.recent_mutation_in_flight = true;
                return Task::perform(
                    async move {
                        let new_path = rfd::AsyncFileDialog::new()
                            .set_title("Locate strukt workspace")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf());
                        (old_path, new_path)
                    },
                    |(old_path, new_path)| Message::RecentWorkspaceLocated { old_path, new_path },
                );
            }
            Message::RecentWorkspaceLocated {
                old_path,
                new_path: Some(new_path),
            } => {
                let Some(store) = self.store.clone() else {
                    self.recent_mutation_in_flight = false;
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let root = strukt_workspace::WorkspaceRoot::open(new_path)
                                .map_err(|error| error.to_string())?;
                            store
                                .relink_recent(&old_path, &root)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::RecentWorkspacesUpdated,
                );
            }
            Message::RecentWorkspaceLocated { new_path: None, .. } => {
                self.recent_mutation_in_flight = false;
                return Task::none();
            }
            Message::RemoveRecentWorkspace(path) => {
                if self.open_folder_in_flight || self.recent_mutation_in_flight {
                    return Task::none();
                }
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                self.recent_mutation_in_flight = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            store
                                .remove_recent(&path)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::RecentWorkspacesUpdated,
                );
            }
            Message::RecentWorkspacesUpdated(result) => {
                self.recent_mutation_in_flight = false;
                match result {
                    Ok(recent) => {
                        self.recent_workspaces = recent.paths;
                        self.persistence_error = None;
                        self.recompute_workspace_error();
                    }
                    Err(error) => {
                        self.persistence_error = Some(error);
                        self.recompute_workspace_error();
                    }
                }
                return Task::none();
            }
            Message::ToggleQuickOpen => {
                self.quick_open_visible = !self.quick_open_visible;
                self.quick_open_query.clear();
                return if self.quick_open_visible {
                    let files = self.quick_open_source();
                    let has_source = files.is_some();
                    let results = files.map_or_else(Vec::new, |files| {
                        quick_open_candidates_with_ignored(
                            files,
                            "",
                            50,
                            self.quick_open_include_ignored,
                        )
                    });
                    self.quick_open_results = results;
                    let focus = iced::widget::operation::focus(crate::view::quick_open_input_id());
                    if self.quick_open_include_ignored && !has_source {
                        Task::batch([focus, self.start_quick_open_scan()])
                    } else {
                        focus
                    }
                } else {
                    Task::none()
                };
            }
            Message::QuickOpenChanged(query) => {
                self.quick_open_results = self.quick_open_source().map_or_else(Vec::new, |files| {
                    quick_open_candidates_with_ignored(
                        files,
                        &query,
                        50,
                        self.quick_open_include_ignored,
                    )
                });
                self.quick_open_query = query;
                return Task::none();
            }
            Message::QuickOpenSelected(path) => {
                if is_scoped_relative_path(&path) {
                    self.selected_entry = Some(path.clone());
                    self.quick_open_visible = false;
                    self.editor_restore_active = None;
                    return self.open_document_task(path, OpenDisposition::Preview, false);
                }
                self.quick_open_visible = false;
                return Task::none();
            }
            Message::ToggleQuickOpenIgnored => return self.toggle_quick_open_ignored(),
            Message::QuickOpenFilesLoaded {
                generation,
                workspace_root,
                filesystem_revision,
                result,
            } => {
                if self.quick_open_scan_in_flight
                    != Some((generation, workspace_root.clone(), filesystem_revision))
                    || generation != self.quick_open_generation
                    || !self.is_current_root(&workspace_root)
                    || filesystem_revision != self.filesystem_revision
                {
                    return Task::none();
                }
                self.quick_open_scan_in_flight = None;
                match result {
                    Ok(files) => {
                        self.quick_open_results = quick_open_candidates_with_ignored(
                            &files,
                            &self.quick_open_query,
                            50,
                            self.quick_open_include_ignored,
                        );
                        self.quick_open_cache = Some(QuickOpenCache {
                            workspace_root,
                            filesystem_revision,
                            files,
                        });
                        self.quick_open_error = None;
                        self.recompute_workspace_error();
                    }
                    Err(error) => {
                        self.quick_open_error = Some(error);
                        self.recompute_workspace_error();
                    }
                }
                return Task::none();
            }
            Message::SearchChanged(query) => return self.schedule_search(query),
            Message::SearchDebounced {
                generation,
                workspace_root,
                query,
            } => {
                if generation != self.search_generation
                    || !self.is_current_root(&workspace_root)
                    || query != self.search_query
                {
                    return Task::none();
                }
                return self.start_search(generation, workspace_root, query);
            }
            Message::SearchCompleted {
                generation,
                workspace_root,
                result,
            } => {
                if generation != self.search_generation || !self.is_current_root(&workspace_root) {
                    return Task::none();
                }
                match result {
                    Ok(result) => {
                        self.search_results = result;
                        self.search_error = None;
                        self.recompute_workspace_error();
                    }
                    Err(error) => {
                        self.search_error = Some(error);
                        self.recompute_workspace_error();
                    }
                }
                return Task::none();
            }
            Message::ToggleSearchIgnored => {
                self.search_include_ignored = !self.search_include_ignored;
                let query = self.search_query.clone();
                return self.schedule_search(query);
            }
            _ => {}
        }

        self.update_shell(message)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "centralized shortcut routing remains exhaustive and auditable"
    )]
    fn update_shell(&mut self, message: Message) -> Task<Message> {
        let action = match message {
            Message::SelectActivity(activity) => Some(ShellAction::SelectActivity(activity)),
            Message::ToggleContext => Some(ShellAction::ToggleContext),
            Message::ToggleDrawer => Some(ShellAction::ToggleDrawer),
            Message::ToggleExplorer => Some(ShellAction::ToggleExplorer),
            Message::ToggleTheme => Some(ShellAction::ToggleTheme),
            Message::OpenFolder
            | Message::FolderPicked(_)
            | Message::WorkspaceOpened(_)
            | Message::ToggleHiddenFiles
            | Message::ToggleIgnoredFiles
            | Message::FilesRefreshed { .. }
            | Message::SelectExplorerEntry(_)
            | Message::OpenDocument { .. }
            | Message::DocumentOpened { .. }
            | Message::EditorAction { .. }
            | Message::SelectDocument(_)
            | Message::PinDocument(_)
            | Message::CloseDocument(_)
            | Message::ResolveDocumentClose { .. }
            | Message::SaveDocument { .. }
            | Message::DocumentSaved { .. }
            | Message::DocumentDiskObserved { .. }
            | Message::ReloadDocumentFromDisk(_)
            | Message::KeepEditingDocument(_)
            | Message::RecoveryDue { .. }
            | Message::RecoverySaved { .. }
            | Message::RecoveryLoaded { .. }
            | Message::RecoveryDeleted { .. }
            | Message::UndoDocument(_)
            | Message::RedoDocument(_)
            | Message::ToggleEditorFind
            | Message::EditorFindChanged(_)
            | Message::EditorReplaceChanged(_)
            | Message::ToggleFindCase
            | Message::ToggleFindWholeWord
            | Message::ToggleFindRegex
            | Message::SetLanguageOverride { .. }
            | Message::ReplaceAll(_)
            | Message::BeginCreateFile
            | Message::BeginCreateDirectory
            | Message::BeginRename
            | Message::BeginDuplicate
            | Message::BeginTrash
            | Message::BeginPermanentDelete
            | Message::ExplorerDialogInput(_)
            | Message::CancelExplorerDialog
            | Message::SubmitExplorerDialog
            | Message::FileOperationCompleted { .. }
            | Message::PollWatcher
            | Message::FileEvent { .. }
            | Message::WorkspacePersisted { .. }
            | Message::RecentWorkspaceLoaded(_)
            | Message::RetryRecentWorkspace(_)
            | Message::LocateRecentWorkspace(_)
            | Message::RecentWorkspaceLocated { .. }
            | Message::RemoveRecentWorkspace(_)
            | Message::RecentWorkspacesUpdated(_)
            | Message::ToggleQuickOpen
            | Message::QuickOpenChanged(_)
            | Message::QuickOpenSelected(_)
            | Message::ToggleQuickOpenIgnored
            | Message::QuickOpenFilesLoaded { .. }
            | Message::SearchChanged(_)
            | Message::SearchDebounced { .. }
            | Message::SearchCompleted { .. }
            | Message::ToggleSearchIgnored => {
                unreachable!("handled before shell actions")
            }
            Message::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.command() =>
            {
                match key.as_ref() {
                    Key::Character("b") => Some(ShellAction::ToggleExplorer),
                    Key::Character("j") => Some(ShellAction::ToggleDrawer),
                    Key::Character("\\") => Some(ShellAction::ToggleContext),
                    Key::Character("p") => {
                        return self.update(Message::ToggleQuickOpen);
                    }
                    Key::Character("s") => {
                        if let Some(id) = self
                            .editor
                            .as_ref()
                            .and_then(EditorWorkspace::active_document_id)
                        {
                            return self.update(Message::SaveDocument {
                                id,
                                mode: SaveMode::IfUnchanged,
                            });
                        }
                        return Task::none();
                    }
                    Key::Character("z") => {
                        if let Some(id) = self
                            .editor
                            .as_ref()
                            .and_then(EditorWorkspace::active_document_id)
                        {
                            return self.update(if modifiers.shift() {
                                Message::RedoDocument(id)
                            } else {
                                Message::UndoDocument(id)
                            });
                        }
                        return Task::none();
                    }
                    Key::Character("f") => {
                        return self.update(Message::ToggleEditorFind);
                    }
                    _ => None,
                }
            }
            Message::Keyboard(_) => None,
            Message::SmokeTimeout => {
                println!("strukt smoke test: native event loop started");
                return iced::exit();
            }
            Message::WorkspaceFilesSmokeFinished(Ok(())) => {
                println!("{WORKSPACE_FILES_SMOKE_SUCCESS}");
                return iced::exit();
            }
            Message::WorkspaceFilesSmokeFinished(Err(error)) => {
                panic!("strukt workspace files smoke failed: {error}");
            }
        };
        let explorer_was_visible = self.shell.explorer_visible;
        if let Some(action) = action {
            self.shell.apply(action);
        }

        if self.shell.explorer_visible != explorer_was_visible
            && let Some(workspace) = &mut self.workspace
        {
            workspace.explorer.visible = self.shell.explorer_visible;
            return self.request_persistence(false);
        }

        Task::none()
    }

    fn recompute_workspace_error(&mut self) {
        self.workspace_error = self
            .open_error
            .clone()
            .or_else(|| self.operation_error.clone())
            .or_else(|| self.refresh_error.clone())
            .or_else(|| self.quick_open_error.clone())
            .or_else(|| self.search_error.clone())
            .or_else(|| self.persistence_error.clone());
    }

    fn is_current_root(&self, root: &Path) -> bool {
        self.workspace
            .as_ref()
            .is_some_and(|workspace| workspace.root.path() == root)
    }

    fn open_workspace_task(&mut self, path: PathBuf) -> Task<Message> {
        self.open_folder_in_flight = true;
        let store = self.store.clone();
        Task::perform(
            async move {
                match tokio::task::spawn_blocking(move || {
                    if let Some(store) = store {
                        crate::workspace::open_workspace_with_store(path, &store)
                    } else {
                        open_workspace_without_store(path)
                    }
                })
                .await
                {
                    Ok(result) => result,
                    Err(error) => Err(error.to_string()),
                }
            },
            Message::WorkspaceOpened,
        )
    }

    fn open_document_task(
        &self,
        path: PathBuf,
        disposition: OpenDisposition,
        force_full: bool,
    ) -> Task<Message> {
        let Some(workspace) = &self.workspace else {
            return Task::none();
        };
        if !is_scoped_relative_path(&path) {
            return Task::none();
        }
        let root = workspace.root.clone();
        let workspace_root = root.path().to_path_buf();
        let message_path = path.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    read_document(
                        &root,
                        &path,
                        ReadOptions {
                            force_full,
                            ..ReadOptions::default()
                        },
                    )
                    .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::DocumentOpened {
                workspace_root,
                path: message_path,
                disposition,
                result,
            },
        )
    }

    fn save_document_task(&self, id: DocumentId, mode: SaveMode) -> Task<Message> {
        let (Some(workspace), Some(editor)) = (&self.workspace, &self.editor) else {
            return Task::none();
        };
        let Some(document) = editor.document(id) else {
            return Task::none();
        };
        let expected_revision = document.revision();
        let request = SaveRequest::new(
            PathBuf::from(document.path().as_str()),
            document.text().into_bytes(),
            document.disk_revision().clone(),
        )
        .with_mode(mode);
        let root = workspace.root.clone();
        let workspace_root = root.path().to_path_buf();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    save_document(&root, &request).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::DocumentSaved {
                workspace_root,
                id,
                expected_revision,
                result,
            },
        )
    }

    fn observe_open_documents_task(
        &self,
        workspace_root: &Path,
        changed_paths: &[PathBuf],
    ) -> Task<Message> {
        let (Some(workspace), Some(editor)) = (&self.workspace, &self.editor) else {
            return Task::none();
        };
        let normalized: Vec<_> = changed_paths
            .iter()
            .filter_map(|path| {
                if path.is_absolute() {
                    path.strip_prefix(workspace_root)
                        .ok()
                        .map(Path::to_path_buf)
                } else {
                    Some(path.clone())
                }
            })
            .collect();
        let all = changed_paths.is_empty();
        let tasks = editor
            .view_state()
            .tabs
            .into_iter()
            .filter(|tab| {
                let document_path = Path::new(tab.path.as_str());
                all || normalized
                    .iter()
                    .any(|changed| changed == document_path || document_path.starts_with(changed))
            })
            .filter_map(|tab| {
                let document = editor.document(tab.id)?;
                let expected_revision = document.revision();
                let path = PathBuf::from(document.path().as_str());
                let root = workspace.root.clone();
                let message_root = workspace_root.to_path_buf();
                Some(Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            match read_document(&root, &path, ReadOptions::default()) {
                                Ok(read) => Ok(DiskObservation::Present(read)),
                                Err(DocumentIoError::Io(error))
                                    if error.kind() == io::ErrorKind::NotFound =>
                                {
                                    Ok(DiskObservation::Missing)
                                }
                                Err(error) => Err(error.to_string()),
                            }
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    move |result| Message::DocumentDiskObserved {
                        workspace_root: message_root,
                        id: tab.id,
                        expected_revision,
                        result,
                    },
                ))
            })
            .collect::<Vec<_>>();
        Task::batch(tasks)
    }

    fn recovery_metadata(&self, id: DocumentId) -> Option<RecoveryMetadata> {
        let workspace = self.workspace.as_ref()?;
        let document = self.editor.as_ref()?.document(id)?;
        Some(RecoveryMetadata::new(
            workspace.root.id().as_str(),
            document.path().as_str(),
            document.disk_revision().as_str(),
        ))
    }

    fn schedule_recovery(&mut self, id: DocumentId) -> Task<Message> {
        let (Some(workspace), Some(document)) = (
            self.workspace.as_ref(),
            self.editor.as_ref().and_then(|editor| editor.document(id)),
        ) else {
            return Task::none();
        };
        if !document.is_recoverable() {
            return Task::none();
        }
        let generation = self
            .recovery_generations
            .get(&id)
            .copied()
            .unwrap_or_default()
            .wrapping_add(1);
        self.recovery_generations.insert(id, generation);
        let workspace_root = workspace.root.path().to_path_buf();
        let expected_revision = document.revision();
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                (workspace_root, id, expected_revision, generation)
            },
            |(workspace_root, id, expected_revision, generation)| Message::RecoveryDue {
                workspace_root,
                id,
                expected_revision,
                generation,
            },
        )
    }

    fn save_recovery_task(
        &self,
        id: DocumentId,
        expected_revision: Revision,
        generation: u64,
    ) -> Task<Message> {
        let (Some(store), Some(workspace), Some(editor)) = (
            self.recovery_store.clone(),
            self.workspace.as_ref(),
            self.editor.as_ref(),
        ) else {
            return Task::none();
        };
        let Some(document) = editor.document(id) else {
            return Task::none();
        };
        if document.revision() != expected_revision || !document.is_recoverable() {
            return Task::none();
        }
        let metadata = RecoveryMetadata::new(
            workspace.root.id().as_str(),
            document.path().as_str(),
            document.disk_revision().as_str(),
        );
        let payload = RecoveryPayload::new(metadata, document.revision().as_u64(), document.text());
        let provider = Arc::clone(&self.recovery_key_provider);
        let workspace_root = workspace.root.path().to_path_buf();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    store
                        .save(provider.as_ref(), &payload)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::RecoverySaved {
                workspace_root,
                id,
                generation,
                result,
            },
        )
    }

    fn load_recovery_task(&self, id: DocumentId) -> Task<Message> {
        let (Some(store), Some(workspace), Some(document)) = (
            self.recovery_store.clone(),
            self.workspace.as_ref(),
            self.editor.as_ref().and_then(|editor| editor.document(id)),
        ) else {
            return Task::none();
        };
        let metadata = RecoveryMetadata::new(
            workspace.root.id().as_str(),
            document.path().as_str(),
            document.disk_revision().as_str(),
        );
        let provider = Arc::clone(&self.recovery_key_provider);
        let workspace_root = workspace.root.path().to_path_buf();
        let expected_revision = document.revision();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    store
                        .load(provider.as_ref(), &metadata)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::RecoveryLoaded {
                workspace_root,
                id,
                expected_revision,
                result,
            },
        )
    }

    fn delete_recovery_task(&self, id: DocumentId) -> Task<Message> {
        let Some(metadata) = self.recovery_metadata(id) else {
            return Task::none();
        };
        self.delete_recovery_metadata_task(id, metadata)
    }

    fn delete_recovery_metadata_task(
        &self,
        id: DocumentId,
        metadata: RecoveryMetadata,
    ) -> Task<Message> {
        let (Some(store), Some(workspace)) = (self.recovery_store.clone(), self.workspace.as_ref())
        else {
            return Task::none();
        };
        let workspace_root = workspace.root.path().to_path_buf();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    store.delete(&metadata).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::RecoveryDeleted {
                workspace_root,
                id,
                result,
            },
        )
    }

    fn editor_session_snapshot(&self) -> Option<EditorSessionSnapshot> {
        let editor = self.editor.as_ref()?;
        let state = editor.view_state();
        let tabs = state
            .tabs
            .iter()
            .filter_map(|tab| {
                let document = editor.document(tab.id)?;
                let (cursor, selection_anchor) = self
                    .editor_surfaces
                    .cursor_offsets(tab.id)
                    .unwrap_or_default();
                let mut snapshot = EditorTabSnapshot::new(
                    tab.path.as_str(),
                    cursor,
                    selection_anchor,
                    self.editor_scroll_lines
                        .get(&tab.id)
                        .copied()
                        .unwrap_or_default(),
                );
                snapshot.find_query.clone_from(&self.editor_find_query);
                snapshot.replace_text.clone_from(&self.editor_replace_text);
                snapshot.find_options.case_sensitive = self.editor_find_options.case_sensitive;
                snapshot.find_options.whole_word = self.editor_find_options.whole_word;
                snapshot.find_options.regex = self.editor_find_options.regex;
                snapshot.language_override = self.editor_language_overrides.get(&tab.id).cloned();
                snapshot.read_only = document.is_read_only();
                snapshot.disk_revision = Some(document.disk_revision().as_str().to_owned());
                Some(snapshot)
            })
            .collect();
        let active_path = state.active.and_then(|id| {
            editor
                .document(id)
                .map(|document| document.path().as_str().to_owned())
        });
        let preview_path = state
            .tabs
            .iter()
            .find(|tab| !tab.pinned)
            .map(|tab| tab.path.as_str().to_owned());
        Some(EditorSessionSnapshot::new(tabs, active_path, preview_path))
    }

    fn request_persistence(&mut self, record_recent: bool) -> Task<Message> {
        let Some(mut state) = self.workspace.clone() else {
            return Task::none();
        };
        if let Some(snapshot) = self.editor_session_snapshot() {
            let _ = state.set_contribution("editor", &snapshot);
            if let Some(workspace) = &mut self.workspace {
                let _ = workspace.set_contribution("editor", &snapshot);
            }
        }
        if record_recent {
            self.enqueue_recent_root(state.root.clone());
        }
        if self.persistence_in_flight.is_some() || self.refresh_in_flight.is_some() {
            self.persistence_pending = Some(state);
            Task::none()
        } else {
            self.start_persistence(state)
        }
    }

    fn start_persistence(&mut self, state: WorkspaceState) -> Task<Message> {
        let Some(store) = self.store.clone() else {
            return Task::none();
        };
        self.persistence_generation = self.persistence_generation.wrapping_add(1);
        let generation = self.persistence_generation;
        let workspace_root = state.root.path().to_path_buf();
        let recent_roots: Vec<_> = self.pending_recent_roots.drain(..).collect();
        self.active_recent_roots.clone_from(&recent_roots);
        self.persistence_in_flight = Some((generation, workspace_root.clone()));
        Task::perform(
            async move {
                let task_root = workspace_root.clone();
                let task_recent_roots = recent_roots.clone();
                let result = tokio::task::spawn_blocking(move || {
                    persist_workspace_batch(&store, &state, &recent_roots)
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                (generation, task_root, task_recent_roots, result)
            },
            |(generation, workspace_root, recent_roots, result)| Message::WorkspacePersisted {
                generation,
                workspace_root,
                recent_roots,
                result,
            },
        )
    }

    fn enqueue_recent_root(&mut self, root: WorkspaceRoot) {
        self.pending_recent_roots
            .retain(|candidate| candidate != &root);
        self.pending_recent_roots.push_back(root);
    }

    fn requeue_recent_roots(&mut self, roots: Vec<WorkspaceRoot>) {
        for root in roots.into_iter().rev() {
            self.pending_recent_roots
                .retain(|candidate| candidate != &root);
            self.pending_recent_roots.push_front(root);
        }
    }

    fn quick_open_source(&self) -> Option<&[FileEntry]> {
        if !self.quick_open_include_ignored {
            return Some(&self.files);
        }
        let workspace = self.workspace.as_ref()?;
        self.quick_open_cache.as_ref().and_then(|cache| {
            (cache.workspace_root == workspace.root.path()
                && cache.filesystem_revision == self.filesystem_revision)
                .then_some(cache.files.as_slice())
        })
    }

    fn invalidate_quick_open_cache(&mut self) {
        self.quick_open_generation = self.quick_open_generation.wrapping_add(1);
        self.quick_open_scan_in_flight = None;
        self.quick_open_cache = None;
        if self.quick_open_include_ignored {
            self.quick_open_results.clear();
        }
    }

    fn toggle_quick_open_ignored(&mut self) -> Task<Message> {
        self.quick_open_include_ignored = !self.quick_open_include_ignored;
        self.quick_open_error = None;
        self.recompute_workspace_error();
        if !self.quick_open_include_ignored {
            self.quick_open_generation = self.quick_open_generation.wrapping_add(1);
            self.quick_open_scan_in_flight = None;
            self.quick_open_results =
                quick_open_candidates_with_ignored(&self.files, &self.quick_open_query, 50, false);
            return Task::none();
        }
        if let Some(files) = self.quick_open_source() {
            self.quick_open_results = quick_open_candidates_with_ignored(
                files,
                &self.quick_open_query,
                50,
                self.quick_open_include_ignored,
            );
            return Task::none();
        }
        self.quick_open_results.clear();
        self.start_quick_open_scan()
    }

    fn start_quick_open_scan(&mut self) -> Task<Message> {
        let Some((workspace_root, task_root)) = self
            .workspace
            .as_ref()
            .map(|workspace| (workspace.root.path().to_path_buf(), workspace.root.clone()))
        else {
            return Task::none();
        };
        if self
            .quick_open_scan_in_flight
            .as_ref()
            .is_some_and(|(_, root, revision)| {
                root == &workspace_root && *revision == self.filesystem_revision
            })
        {
            return Task::none();
        }
        self.quick_open_generation = self.quick_open_generation.wrapping_add(1);
        let generation = self.quick_open_generation;
        let filesystem_revision = self.filesystem_revision;
        self.quick_open_scan_in_flight =
            Some((generation, workspace_root.clone(), filesystem_revision));
        let options = DiscoveryOptions {
            show_ignored: true,
            ..self.explorer_options
        };
        Task::perform(
            async move {
                let result = tokio::task::spawn_blocking(move || {
                    discover_report_for_root(&task_root, options)
                        .map(|report| report.entries)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                (generation, workspace_root, filesystem_revision, result)
            },
            |(generation, workspace_root, filesystem_revision, result)| {
                Message::QuickOpenFilesLoaded {
                    generation,
                    workspace_root,
                    filesystem_revision,
                    result,
                }
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn active_recent_roots_for_test(&self) -> Vec<WorkspaceRoot> {
        self.active_recent_roots.clone()
    }

    #[cfg(test)]
    pub(crate) fn pending_recent_roots_for_test(&self) -> Vec<WorkspaceRoot> {
        self.pending_recent_roots.iter().cloned().collect()
    }

    fn schedule_search(&mut self, query: String) -> Task<Message> {
        self.search_cancellation.cancel();
        self.search_cancellation = CancellationToken::new();
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_query.clone_from(&query);
        self.search_results = SearchResult {
            matches: Vec::new(),
            truncated: false,
        };
        self.search_error = None;
        self.recompute_workspace_error();
        if query.is_empty() {
            return Task::none();
        }
        let Some(workspace) = &self.workspace else {
            return Task::none();
        };
        let generation = self.search_generation;
        let workspace_root = workspace.root.path().to_path_buf();
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                (generation, workspace_root, query)
            },
            |(generation, workspace_root, query)| Message::SearchDebounced {
                generation,
                workspace_root,
                query,
            },
        )
    }

    fn start_search(
        &self,
        generation: u64,
        workspace_root: PathBuf,
        query: String,
    ) -> Task<Message> {
        let Some(task_root) = self
            .workspace
            .as_ref()
            .filter(|workspace| workspace.root.path() == workspace_root)
            .map(|workspace| workspace.root.clone())
        else {
            return Task::none();
        };
        let options = SearchOptions {
            max_results: 500,
            max_file_bytes: 2 * 1024 * 1024,
            discovery: DiscoveryOptions {
                show_ignored: self.search_include_ignored,
                ..self.explorer_options
            },
        };
        let cancellation = self.search_cancellation.clone();
        Task::perform(
            async move {
                let result = tokio::task::spawn_blocking(move || {
                    search_content_cancellable(&task_root, &query, options, &cancellation)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                (generation, workspace_root, result)
            },
            |(generation, workspace_root, result)| Message::SearchCompleted {
                generation,
                workspace_root,
                result,
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn search_cancellation_for_test(&self) -> CancellationToken {
        self.search_cancellation.clone()
    }

    fn can_begin_operation(&self) -> bool {
        self.can_use_explorer_controls()
    }

    fn can_use_explorer_controls(&self) -> bool {
        self.workspace.is_some()
            && self.explorer_dialog == ExplorerDialog::None
            && self.operation_in_flight.is_none()
    }

    pub(crate) fn file_operation_in_flight(&self) -> bool {
        self.operation_in_flight.is_some()
    }

    pub(crate) fn folder_picker_in_flight(&self) -> bool {
        self.open_folder_in_flight || self.recent_mutation_in_flight
    }

    fn reconcile_explorer_targets(&mut self) {
        let entry_exists = |path: &Path| self.files.iter().any(|entry| entry.relative_path == path);
        if self
            .selected_entry
            .as_deref()
            .is_some_and(|path| !entry_exists(path))
        {
            self.selected_entry = None;
        }
        if self.operation_in_flight.is_none()
            && dialog_source(&self.explorer_dialog).is_some_and(|path| !entry_exists(path))
        {
            self.explorer_dialog = ExplorerDialog::None;
            self.operation_error = None;
        }
    }

    fn submit_explorer_dialog(&mut self) -> Task<Message> {
        if self.operation_in_flight.is_some() {
            return Task::none();
        }
        let Some(operation) = operation_from_dialog(&self.explorer_dialog) else {
            return Task::none();
        };
        let Some(workspace) = &self.workspace else {
            return Task::none();
        };

        let workspace_root = workspace.root.path().to_path_buf();
        let operation_root = workspace.root.clone();
        self.operation_generation = self.operation_generation.wrapping_add(1);
        let generation = self.operation_generation;
        self.operation_in_flight = Some((generation, workspace_root.clone()));
        self.operation_error = None;
        self.recompute_workspace_error();

        Task::perform(
            async move {
                let result = match tokio::task::spawn_blocking(move || {
                    apply_operation(&operation_root, operation)
                })
                .await
                {
                    Ok(result) => result.map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                };
                (generation, workspace_root, result)
            },
            |(generation, workspace_root, result)| Message::FileOperationCompleted {
                generation,
                workspace_root,
                result,
            },
        )
    }

    fn request_file_refresh(&mut self) -> Task<Message> {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        let Some(workspace) = &mut self.workspace else {
            return Task::none();
        };
        workspace.explorer.show_hidden = self.explorer_options.show_hidden;
        workspace.explorer.show_ignored = self.explorer_options.show_ignored;

        if self.refresh_in_flight.is_some() {
            self.refresh_pending = true;
            Task::none()
        } else {
            self.start_file_refresh()
        }
    }

    fn start_file_refresh(&mut self) -> Task<Message> {
        let Some(workspace) = &self.workspace else {
            return Task::none();
        };
        let root = workspace.root.clone();
        let options = self.explorer_options;
        let generation = self.refresh_generation;
        self.refresh_in_flight = Some(generation);

        Task::perform(
            async move {
                let result = match tokio::task::spawn_blocking(move || {
                    discover_report_for_root(&root, options)
                })
                .await
                {
                    Ok(result) => result.map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                };
                (generation, result)
            },
            |(generation, result)| Message::FilesRefreshed { generation, result },
        )
    }

    #[must_use]
    pub fn theme(&self) -> Theme {
        match self.shell.theme_mode {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::Dark,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keyboard = keyboard::listen().map(Message::Keyboard);
        let mut subscriptions = vec![keyboard];
        if self.watcher.is_some() {
            subscriptions
                .push(time::every(Duration::from_millis(250)).map(|_| Message::PollWatcher));
        }
        if let Some(timeout) = self.launch_mode.smoke_timeout() {
            subscriptions.push(time::every(timeout).map(|_| Message::SmokeTimeout));
        }
        Subscription::batch(subscriptions)
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct WatcherBatch {
    pub(crate) changed: bool,
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) stale_reason: Option<String>,
    pub(crate) drained: usize,
}

pub(crate) fn drain_watcher_batch(
    mut next_event: impl FnMut() -> Option<FileEvent>,
) -> WatcherBatch {
    let mut batch = WatcherBatch::default();
    for _ in 0..MAX_WATCHER_EVENTS_PER_POLL {
        let Some(event) = next_event() else {
            break;
        };
        batch.drained += 1;
        match event {
            FileEvent::Changed(paths) => {
                batch.changed |= !paths.is_empty();
                batch.paths.extend(paths);
            }
            FileEvent::Stale(reason) => batch.stale_reason = Some(reason),
        }
    }
    batch
}

pub(crate) fn run_workspace_files_smoke(root: PathBuf) -> Result<(), String> {
    const SENTINEL: &str = "strukt-smoke.txt";

    let store_directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = WorkspaceStore::at(store_directory.path().join("workspaces"));
    let opened = crate::workspace::open_workspace_with_store(root, &store)?;

    let sentinel_found =
        opened.discovery.entries.iter().any(|entry| {
            entry.relative_path == Path::new(SENTINEL) && entry.kind == FileKind::File
        });
    if !sentinel_found {
        return Err(format!(
            "workspace discovery did not contain the required {SENTINEL} sentinel"
        ));
    }

    store
        .save(&opened.state)
        .map_err(|error| error.to_string())?;
    let snapshot = store
        .load_for_root(&opened.state.root)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "persisted workspace snapshot could not be reloaded".to_owned())?;
    if snapshot.state != opened.state {
        return Err("reloaded workspace snapshot did not match the opened workspace".to_owned());
    }

    Ok(())
}

pub(crate) async fn workspace_files_smoke_task(root: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || run_workspace_files_smoke(root))
        .await
        .map_err(|error| error.to_string())?
}

struct SmokeRecoveryKeyProvider;

impl RecoveryKeyProvider for SmokeRecoveryKeyProvider {
    fn load_or_create(&self) -> Result<RecoveryKey, RecoveryKeyError> {
        Ok(RecoveryKey::new([0x5a; 32]))
    }

    fn delete(&self) -> Result<(), RecoveryKeyError> {
        Ok(())
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the smoke keeps its end-to-end contract explicit in one linear workflow"
)]
pub(crate) fn run_editor_smoke(root: &Path) -> Result<(), String> {
    const SENTINEL: &str = "strukt-editor-smoke.txt";
    const EDIT: &str = "edited by strukt\n";

    let workspace_root = WorkspaceRoot::open(root).map_err(|error| error.to_string())?;
    let opened = read_document(&workspace_root, SENTINEL, ReadOptions::default())
        .map_err(|error| error.to_string())?;
    let DocumentKind::Text {
        read_only: false, ..
    } = opened.kind
    else {
        return Err(format!("{SENTINEL} must be an editable UTF-8 text file"));
    };
    let initial = opened
        .text
        .ok_or_else(|| format!("{SENTINEL} had no text payload"))?;
    let mut editor = EditorWorkspace::new(workspace_root.id().clone());
    let id = editor
        .open(
            RelativeDocumentPath::new(SENTINEL).map_err(|error| error.to_string())?,
            &initial,
            opened.disk_revision,
            false,
            OpenDisposition::Preview,
        )
        .map_err(|error| error.to_string())?;
    let revision = editor
        .document(id)
        .ok_or_else(|| "smoke document was not opened".to_owned())?
        .revision();
    editor
        .edit(
            id,
            EditTransaction::insert(revision, initial.chars().count(), EDIT),
            EditKind::Other,
            0,
            0,
        )
        .map_err(|error| error.to_string())?;
    if !editor.view_state().tabs[0].pinned {
        return Err("editing did not pin the preview tab".into());
    }
    editor.undo(id).map_err(|error| error.to_string())?;
    editor.redo(id).map_err(|error| error.to_string())?;

    let application_data = tempfile::tempdir().map_err(|error| error.to_string())?;
    let recovery_store = EditorRecoveryStore::at(application_data.path().join("recovery"));
    let document = editor
        .document(id)
        .ok_or_else(|| "smoke document disappeared".to_owned())?;
    let metadata = RecoveryMetadata::new(
        workspace_root.id().as_str(),
        SENTINEL,
        document.disk_revision().as_str(),
    );
    let payload = RecoveryPayload::new(
        metadata.clone(),
        document.revision().as_u64(),
        document.text(),
    );
    recovery_store
        .save(&SmokeRecoveryKeyProvider, &payload)
        .map_err(|error| error.to_string())?;
    let recovered = recovery_store
        .load(&SmokeRecoveryKeyProvider, &metadata)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "encrypted recovery did not round trip".to_owned())?;
    if recovered.text != document.text() {
        return Err("encrypted recovery content changed".into());
    }

    let expected_revision = document.revision();
    let request = SaveRequest::new(
        SENTINEL,
        document.text().into_bytes(),
        document.disk_revision().clone(),
    );
    let saved = save_document(&workspace_root, &request).map_err(|error| error.to_string())?;
    editor
        .complete_save(id, expected_revision, saved.disk_revision)
        .map_err(|error| error.to_string())?;
    recovery_store
        .delete(&metadata)
        .map_err(|error| error.to_string())?;

    let document = editor
        .document(id)
        .ok_or_else(|| "saved smoke document disappeared".to_owned())?;
    let disk = read_document(&workspace_root, SENTINEL, ReadOptions::default())
        .map_err(|error| error.to_string())?;
    let document_text = document.text();
    if disk.text.as_deref() != Some(document_text.as_str()) {
        return Err("saved editor content did not round trip through disk".into());
    }

    let mut tab = EditorTabSnapshot::new(SENTINEL, 0, 0, 0.0);
    tab.disk_revision = Some(document.disk_revision().as_str().to_owned());
    let session = EditorSessionSnapshot::new(vec![tab], Some(SENTINEL.into()), None);
    let mut state = WorkspaceState::new(workspace_root);
    state
        .set_contribution("editor", &session)
        .map_err(|error| error.to_string())?;
    let workspace_store = WorkspaceStore::at(application_data.path().join("workspaces"));
    workspace_store
        .save(&state)
        .map_err(|error| error.to_string())?;
    let restored = workspace_store
        .load_for_root(&state.root)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "workspace snapshot did not restore".to_owned())?;
    let restored_editor = restored
        .state
        .contribution::<EditorSessionSnapshot>("editor")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "editor snapshot did not restore".to_owned())?;
    if restored_editor.active_path.as_deref() != Some(SENTINEL) {
        return Err("restored editor snapshot lost its active tab".into());
    }
    if root.join(".strukt").exists() {
        return Err("editor smoke created forbidden workspace metadata".into());
    }
    Ok(())
}

pub(crate) async fn editor_smoke_task(root: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || run_editor_smoke(&root))
        .await
        .map_err(|error| error.to_string())?
}

pub(crate) fn persist_workspace_batch(
    store: &WorkspaceStore,
    state: &WorkspaceState,
    recent_roots: &[WorkspaceRoot],
) -> Result<(), String> {
    store.save(state).map_err(|error| error.to_string())?;
    for root in recent_roots {
        store
            .record_recent(root)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn operation_from_dialog(dialog: &ExplorerDialog) -> Option<FileOperation> {
    match dialog {
        ExplorerDialog::CreateFile(path) if !path.is_empty() => {
            Some(FileOperation::CreateFile(path.into()))
        }
        ExplorerDialog::CreateDirectory(path) if !path.is_empty() => {
            Some(FileOperation::CreateDirectory(path.into()))
        }
        ExplorerDialog::Rename { from, to } if !to.is_empty() => Some(FileOperation::Rename {
            from: from.clone(),
            to: to.into(),
        }),
        ExplorerDialog::Duplicate { from, to } if !to.is_empty() => {
            Some(FileOperation::Duplicate {
                from: from.clone(),
                to: to.into(),
            })
        }
        ExplorerDialog::ConfirmTrash(path) => Some(FileOperation::MoveToTrash(path.clone())),
        ExplorerDialog::ConfirmPermanentDelete(path) => {
            Some(FileOperation::DeletePermanently(path.clone()))
        }
        ExplorerDialog::None
        | ExplorerDialog::CreateFile(_)
        | ExplorerDialog::CreateDirectory(_)
        | ExplorerDialog::Rename { .. }
        | ExplorerDialog::Duplicate { .. } => None,
    }
}

fn is_scoped_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Iced reports integral line deltas while persisted scroll state uses f32"
)]
fn scroll_lines_as_f32(lines: i32) -> f32 {
    lines as f32
}

fn dialog_source(dialog: &ExplorerDialog) -> Option<&Path> {
    match dialog {
        ExplorerDialog::Rename { from, .. }
        | ExplorerDialog::Duplicate { from, .. }
        | ExplorerDialog::ConfirmTrash(from)
        | ExplorerDialog::ConfirmPermanentDelete(from) => Some(from),
        ExplorerDialog::None
        | ExplorerDialog::CreateFile(_)
        | ExplorerDialog::CreateDirectory(_) => None,
    }
}
