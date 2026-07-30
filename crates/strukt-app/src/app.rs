use std::path::PathBuf;
use std::time::Duration;

use iced::keyboard::{self, Key};
use iced::{Subscription, Task, Theme, time};
use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry};
use strukt_fs::{DiscoveryOptions, DiscoveryReport, FileEntry, discover_report};
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
    launch_mode: LaunchMode,
    open_folder_in_flight: bool,
    refresh_generation: u64,
}

#[derive(Clone, Debug)]
pub enum Message {
    SelectActivity(Activity),
    ToggleContext,
    ToggleDrawer,
    ToggleExplorer,
    ToggleTheme,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed by the explorer view in Task 8")
    )]
    OpenFolder,
    FolderPicked(Option<PathBuf>),
    WorkspaceOpened(Result<OpenedWorkspace, String>),
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed by the explorer view in Task 8")
    )]
    ToggleHiddenFiles,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed by the explorer view in Task 8")
    )]
    ToggleIgnoredFiles,
    FilesRefreshed {
        generation: u64,
        result: Result<DiscoveryReport, String>,
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
            launch_mode,
            open_folder_in_flight: false,
            refresh_generation: 0,
        }
    }
}

impl StruktApp {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFolder => {
                if self.open_folder_in_flight {
                    return Task::none();
                }
                self.open_folder_in_flight = true;
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
                self.workspace_error = None;
                self.open_folder_in_flight = false;
                self.refresh_generation = self.refresh_generation.wrapping_add(1);
                return Task::none();
            }
            Message::WorkspaceOpened(Err(error)) => {
                self.workspace_error = Some(error);
                self.open_folder_in_flight = false;
                return Task::none();
            }
            Message::ToggleHiddenFiles => {
                self.explorer_options.show_hidden = !self.explorer_options.show_hidden;
                return self.refresh_files();
            }
            Message::ToggleIgnoredFiles => {
                self.explorer_options.show_ignored = !self.explorer_options.show_ignored;
                return self.refresh_files();
            }
            Message::FilesRefreshed { generation, result } => {
                if generation != self.refresh_generation {
                    return Task::none();
                }
                match result {
                    Ok(report) => {
                        self.files = report.entries;
                        self.file_warnings = report.warnings;
                        self.filesystem_truncated = report.truncated;
                        self.workspace_error = None;
                    }
                    Err(error) => self.workspace_error = Some(error),
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
            | Message::FilesRefreshed { .. } => unreachable!("handled before shell actions"),
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

    fn refresh_files(&mut self) -> Task<Message> {
        let Some(workspace) = &mut self.workspace else {
            return Task::none();
        };

        workspace.explorer.show_hidden = self.explorer_options.show_hidden;
        workspace.explorer.show_ignored = self.explorer_options.show_ignored;
        let root = workspace.root.path().to_path_buf();
        let options = self.explorer_options;
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        let generation = self.refresh_generation;

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
