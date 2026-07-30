use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use iced::keyboard::{self, Key};
use iced::{Subscription, Task, Theme, time};
use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry};
use strukt_fs::{
    DiscoveryOptions, DiscoveryReport, FileEntry, FileEvent, FileOperation, QuickOpenCandidate,
    SearchOptions, SearchResult, WorkspaceWatcher, apply_operation, discover_report,
    quick_open_candidates, search_content,
};
use strukt_persistence::{RecentWorkspaces, WorkspaceStore};
use strukt_shell::{Activity, ShellAction, ShellState};
use strukt_theme::ThemeMode;
use strukt_workspace::WorkspaceState;

use crate::workspace::{OpenedWorkspace, open_workspace_without_store};

const SMOKE_TEST_DURATION: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LaunchMode {
    #[default]
    Interactive,
    SmokeTest,
}

impl LaunchMode {
    #[must_use]
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        if args.into_iter().any(|argument| argument == "--smoke-test") {
            Self::SmokeTest
        } else {
            Self::Interactive
        }
    }

    #[must_use]
    pub const fn smoke_timeout(self) -> Option<Duration> {
        match self {
            Self::Interactive => None,
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
    launch_mode: LaunchMode,
    store: Option<WorkspaceStore>,
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
    persistence_pending: Option<(WorkspaceState, bool)>,
    persistence_error: Option<String>,
    search_generation: u64,
    quick_open_generation: u64,
    quick_open_files: Vec<FileEntry>,
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
            launch_mode,
            store,
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
            persistence_error: None,
            search_generation: 0,
            quick_open_generation: 0,
            quick_open_files: Vec::new(),
        }
    }

    pub fn boot(launch_mode: LaunchMode) -> (Self, Task<Message>) {
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
                self.files = opened.discovery.entries;
                self.file_warnings = opened.discovery.warnings;
                self.filesystem_truncated = opened.discovery.truncated;
                self.workspace = Some(opened.state);
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
                self.quick_open_generation = self.quick_open_generation.wrapping_add(1);
                self.search_generation = self.search_generation.wrapping_add(1);
                self.quick_open_visible = false;
                self.quick_open_results.clear();
                self.quick_open_files.clear();
                self.search_results.matches.clear();
                self.search_results.truncated = false;
                return self.request_persistence(true);
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
                self.refresh_in_flight = None;
                if generation == self.refresh_generation {
                    match result {
                        Ok(report) => {
                            self.files = report.entries;
                            self.file_warnings = report.warnings;
                            self.filesystem_truncated = report.truncated;
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
                    return self.start_file_refresh();
                }
                if self.persistence_in_flight.is_none()
                    && let Some((state, record_recent)) = self.persistence_pending.take()
                {
                    return self.start_persistence(state, record_recent);
                }
                return Task::none();
            }
            Message::SelectExplorerEntry(path) => {
                if self.explorer_dialog == ExplorerDialog::None
                    && self.operation_in_flight.is_none()
                    && is_scoped_relative_path(&path)
                {
                    self.selected_entry = Some(path);
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
                let mut stale_reason = None;
                let mut changed = false;
                if let Some(watcher) = &self.watcher {
                    while let Some(event) = watcher.try_recv() {
                        match event {
                            FileEvent::Changed(paths) => changed |= !paths.is_empty(),
                            FileEvent::Stale(reason) => stale_reason = Some(reason),
                        }
                    }
                }
                if let Some(reason) = stale_reason {
                    return self.update(Message::FileEvent {
                        workspace_root: root,
                        event: FileEvent::Stale(reason),
                    });
                }
                if changed {
                    return self.update(Message::FileEvent {
                        workspace_root: root,
                        event: FileEvent::Changed(Vec::new()),
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
                if let FileEvent::Stale(reason) = event {
                    if let Some(workspace) = &mut self.workspace {
                        workspace.stale_filesystem = true;
                    }
                    self.refresh_error = Some(reason);
                    self.recompute_workspace_error();
                }
                return self.request_file_refresh();
            }
            Message::WorkspacePersisted {
                generation,
                workspace_root,
                result,
            } => {
                if self.persistence_in_flight.as_ref()
                    != Some(&(generation, workspace_root.clone()))
                {
                    return Task::none();
                }
                self.persistence_in_flight = None;
                if self.is_current_root(&workspace_root) {
                    self.persistence_error = result.err();
                    self.recompute_workspace_error();
                }
                if let Some((state, record_recent)) = self.persistence_pending.take() {
                    return self.start_persistence(state, record_recent);
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
                self.quick_open_results = quick_open_candidates(&self.files, "", 50);
                return if self.quick_open_visible {
                    iced::widget::operation::focus(crate::view::quick_open_input_id())
                } else {
                    Task::none()
                };
            }
            Message::QuickOpenChanged(query) => {
                let files = if self.quick_open_include_ignored && !self.quick_open_files.is_empty()
                {
                    &self.quick_open_files
                } else {
                    &self.files
                };
                self.quick_open_results = quick_open_candidates(files, &query, 50);
                self.quick_open_query = query;
                return Task::none();
            }
            Message::QuickOpenSelected(path) => {
                if is_scoped_relative_path(&path) {
                    self.selected_entry = Some(path);
                }
                self.quick_open_visible = false;
                return Task::none();
            }
            Message::ToggleQuickOpenIgnored => return self.toggle_quick_open_ignored(),
            Message::QuickOpenFilesLoaded {
                generation,
                workspace_root,
                result,
            } => {
                if generation != self.quick_open_generation
                    || !self.is_current_root(&workspace_root)
                {
                    return Task::none();
                }
                match result {
                    Ok(files) => {
                        self.quick_open_results =
                            quick_open_candidates(&files, &self.quick_open_query, 50);
                        self.quick_open_files = files;
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
                    _ => None,
                }
            }
            Message::Keyboard(_) => None,
            Message::SmokeTimeout => {
                println!("strukt smoke test: native event loop started");
                return iced::exit();
            }
        };
        if let Some(action) = action {
            self.shell.apply(action);
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

    fn request_persistence(&mut self, record_recent: bool) -> Task<Message> {
        let Some(state) = self.workspace.clone() else {
            return Task::none();
        };
        if self.persistence_in_flight.is_some() || self.refresh_in_flight.is_some() {
            let record_recent = self
                .persistence_pending
                .as_ref()
                .is_some_and(|(_, pending_recent)| *pending_recent)
                || record_recent;
            self.persistence_pending = Some((state, record_recent));
            Task::none()
        } else {
            self.start_persistence(state, record_recent)
        }
    }

    fn start_persistence(&mut self, state: WorkspaceState, record_recent: bool) -> Task<Message> {
        let Some(store) = self.store.clone() else {
            return Task::none();
        };
        self.persistence_generation = self.persistence_generation.wrapping_add(1);
        let generation = self.persistence_generation;
        let workspace_root = state.root.path().to_path_buf();
        self.persistence_in_flight = Some((generation, workspace_root.clone()));
        Task::perform(
            async move {
                let task_root = workspace_root.clone();
                let result = tokio::task::spawn_blocking(move || {
                    store.save(&state).map_err(|error| error.to_string())?;
                    if record_recent {
                        store
                            .record_recent(&state.root)
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                (generation, task_root, result)
            },
            |(generation, workspace_root, result)| Message::WorkspacePersisted {
                generation,
                workspace_root,
                result,
            },
        )
    }

    fn toggle_quick_open_ignored(&mut self) -> Task<Message> {
        self.quick_open_include_ignored = !self.quick_open_include_ignored;
        self.quick_open_error = None;
        self.recompute_workspace_error();
        self.quick_open_generation = self.quick_open_generation.wrapping_add(1);
        let generation = self.quick_open_generation;
        let Some(workspace) = &self.workspace else {
            return Task::none();
        };
        if !self.quick_open_include_ignored {
            self.quick_open_files.clear();
            self.quick_open_results =
                quick_open_candidates(&self.files, &self.quick_open_query, 50);
            return Task::none();
        }
        let workspace_root = workspace.root.path().to_path_buf();
        let options = DiscoveryOptions {
            show_ignored: true,
            ..self.explorer_options
        };
        Task::perform(
            async move {
                let task_root = workspace_root.clone();
                let result = tokio::task::spawn_blocking(move || {
                    discover_report(&task_root, options)
                        .map(|report| report.entries)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                (generation, workspace_root, result)
            },
            |(generation, workspace_root, result)| Message::QuickOpenFilesLoaded {
                generation,
                workspace_root,
                result,
            },
        )
    }

    fn schedule_search(&mut self, query: String) -> Task<Message> {
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
        let options = SearchOptions {
            max_results: 500,
            max_file_bytes: 2 * 1024 * 1024,
            discovery: DiscoveryOptions {
                show_ignored: self.search_include_ignored,
                ..self.explorer_options
            },
        };
        Task::perform(
            async move {
                let task_root = workspace_root.clone();
                let result = tokio::task::spawn_blocking(move || {
                    search_content(task_root, &query, options).map_err(|error| error.to_string())
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
        self.operation_generation = self.operation_generation.wrapping_add(1);
        let generation = self.operation_generation;
        self.operation_in_flight = Some((generation, workspace_root.clone()));
        self.operation_error = None;
        self.recompute_workspace_error();

        Task::perform(
            async move {
                let operation_root = workspace_root.clone();
                let result = match tokio::task::spawn_blocking(move || {
                    apply_operation(operation_root, operation)
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
        let root = workspace.root.path().to_path_buf();
        let options = self.explorer_options;
        let generation = self.refresh_generation;
        self.refresh_in_flight = Some(generation);

        Task::perform(
            async move {
                let result =
                    match tokio::task::spawn_blocking(move || discover_report(root, options)).await
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
