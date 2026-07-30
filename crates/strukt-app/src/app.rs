use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use iced::keyboard::{self, Key};
use iced::{Subscription, Task, Theme, time};
use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry};
use strukt_fs::{
    DiscoveryOptions, DiscoveryReport, FileEntry, FileOperation, apply_operation, discover_report,
};
use strukt_shell::{Activity, ShellAction, ShellState};
use strukt_theme::ThemeMode;
use strukt_workspace::WorkspaceState;

use crate::workspace::{OpenedWorkspace, open_workspace};

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

#[derive(Debug)]
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
    launch_mode: LaunchMode,
    open_folder_in_flight: bool,
    open_error: Option<String>,
    operation_error: Option<String>,
    refresh_error: Option<String>,
    refresh_generation: u64,
    refresh_in_flight: Option<u64>,
    refresh_pending: bool,
    operation_generation: u64,
    operation_in_flight: Option<(u64, PathBuf)>,
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
    Keyboard(keyboard::Event),
    SmokeTimeout,
}

impl Default for StruktApp {
    fn default() -> Self {
        Self::new(LaunchMode::Interactive)
    }
}

impl StruktApp {
    #[must_use]
    pub fn new(launch_mode: LaunchMode) -> Self {
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
            launch_mode,
            open_folder_in_flight: false,
            open_error: None,
            operation_error: None,
            refresh_error: None,
            refresh_generation: 0,
            refresh_in_flight: None,
            refresh_pending: false,
            operation_generation: 0,
            operation_in_flight: None,
        }
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
                if self.open_folder_in_flight {
                    return Task::none();
                }
                self.open_folder_in_flight = true;
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
                return Task::perform(
                    async move {
                        match tokio::task::spawn_blocking(move || open_workspace(path)).await {
                            Ok(result) => result,
                            Err(error) => Err(error.to_string()),
                        }
                    },
                    Message::WorkspaceOpened,
                );
            }
            Message::FolderPicked(None) => {
                self.open_folder_in_flight = false;
                return Task::none();
            }
            Message::WorkspaceOpened(Ok(opened)) => {
                self.explorer_options = DiscoveryOptions {
                    show_hidden: opened.state.explorer.show_hidden,
                    show_ignored: opened.state.explorer.show_ignored,
                    ..DiscoveryOptions::default()
                };
                self.files = opened.discovery.entries;
                self.file_warnings = opened.discovery.warnings;
                self.filesystem_truncated = opened.discovery.truncated;
                self.workspace = Some(opened.state);
                self.selected_entry = None;
                self.explorer_dialog = ExplorerDialog::None;
                self.open_error = None;
                self.operation_error = None;
                self.refresh_error = None;
                self.recompute_workspace_error();
                self.open_folder_in_flight = false;
                self.refresh_generation = self.refresh_generation.wrapping_add(1);
                self.refresh_pending = false;
                return Task::none();
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
                return self.request_file_refresh();
            }
            Message::ToggleIgnoredFiles => {
                if !self.can_use_explorer_controls() {
                    return Task::none();
                }
                self.explorer_options.show_ignored = !self.explorer_options.show_ignored;
                return self.request_file_refresh();
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
            | Message::FileOperationCompleted { .. } => {
                unreachable!("handled before shell actions")
            }
            Message::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.command() =>
            {
                match key.as_ref() {
                    Key::Character("b") => Some(ShellAction::ToggleExplorer),
                    Key::Character("j") => Some(ShellAction::ToggleDrawer),
                    Key::Character("\\") => Some(ShellAction::ToggleContext),
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
            .or_else(|| self.refresh_error.clone());
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
        self.open_folder_in_flight
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

        match self.launch_mode.smoke_timeout() {
            Some(timeout) => Subscription::batch([
                keyboard,
                time::every(timeout).map(|_| Message::SmokeTimeout),
            ]),
            None => keyboard,
        }
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
