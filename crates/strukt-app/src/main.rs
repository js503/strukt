#![forbid(unsafe_code)]

mod app;
mod view;
mod workspace;

use app::{LaunchMode, StruktApp};

fn main() -> iced::Result {
    let launch_mode = LaunchMode::from_args(std::env::args().skip(1));

    iced::application(
        move || StruktApp::new(launch_mode),
        StruktApp::update,
        view::view,
    )
    .title("strukt")
    .subscription(StruktApp::subscription)
    .theme(StruktApp::theme)
    .run()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::keyboard::{self, Key, Location, Modifiers, key};
    use strukt_core::CapabilityId;
    use strukt_fs::{DiscoveryReport, FileEntry, FileKind};
    use strukt_persistence::WorkspaceStore;
    use strukt_shell::Activity;
    use strukt_workspace::{WorkspaceRoot, WorkspaceState};
    use tempfile::{TempDir, tempdir};

    use crate::app::{LaunchMode, Message, StruktApp};

    fn key_pressed(character: &'static str, code: key::Code, modifiers: Modifiers) -> Message {
        let key = Key::Character(character.into());

        Message::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: key::Physical::Code(code),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    fn file_entry(path: &str) -> FileEntry {
        FileEntry {
            relative_path: path.into(),
            kind: FileKind::File,
            depth: 1,
            hidden: false,
            ignored: false,
        }
    }

    fn discovery(paths: &[&str]) -> DiscoveryReport {
        DiscoveryReport {
            entries: paths.iter().map(|path| file_entry(path)).collect(),
            warnings: Vec::new(),
            truncated: false,
        }
    }

    fn workspace_state(path: &std::path::Path) -> WorkspaceState {
        WorkspaceState::new(WorkspaceRoot::open(path).unwrap())
    }

    fn open_workspace(project: &TempDir) -> crate::workspace::OpenedWorkspace {
        let app_data = tempdir().unwrap();
        let store = WorkspaceStore::at(app_data.path());
        crate::workspace::open_workspace_with_store(project.path().to_path_buf(), &store).unwrap()
    }

    #[test]
    fn built_in_capabilities_are_registered() {
        let app = StruktApp::default();

        assert!(app.capabilities.is_enabled(CapabilityId::FILES));
        assert!(app.capabilities.is_enabled(CapabilityId::TERMINAL));
        assert!(app.capabilities.is_enabled(CapabilityId::AI));
    }

    #[test]
    fn application_messages_drive_shell_state() {
        let mut app = StruktApp::default();

        let _ = app.update(Message::ToggleExplorer);
        assert!(!app.shell.explorer_visible);

        let _ = app.update(Message::SelectActivity(Activity::Files));
        assert!(app.shell.explorer_visible);

        let _ = app.update(Message::ToggleContext);
        let _ = app.update(Message::ToggleDrawer);
        assert!(!app.shell.context_visible);
        assert!(app.shell.drawer_visible);
    }

    #[test]
    fn launch_mode_requires_the_exact_smoke_flag() {
        assert_eq!(
            LaunchMode::from_args(Vec::<String>::new()),
            LaunchMode::Interactive
        );
        assert_eq!(
            LaunchMode::from_args(["--smoke-test".to_owned()]),
            LaunchMode::SmokeTest
        );
        assert_eq!(
            LaunchMode::from_args(["--smoke-testing".to_owned()]),
            LaunchMode::Interactive
        );
    }

    #[test]
    fn only_smoke_mode_has_a_runtime_timeout() {
        assert_eq!(LaunchMode::Interactive.smoke_timeout(), None);
        assert_eq!(
            LaunchMode::SmokeTest.smoke_timeout(),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn smoke_timeout_requests_runtime_work() {
        let mut app = StruktApp::new(LaunchMode::SmokeTest);

        let task = app.update(Message::SmokeTimeout);

        assert_eq!(task.units(), 1);
    }

    #[test]
    fn platform_command_shortcuts_toggle_shell_panels() {
        let mut app = StruktApp::default();

        let _ = app.update(key_pressed("b", key::Code::KeyB, Modifiers::COMMAND));
        let _ = app.update(key_pressed("j", key::Code::KeyJ, Modifiers::COMMAND));
        let _ = app.update(key_pressed("\\", key::Code::Backslash, Modifiers::COMMAND));

        assert!(!app.shell.explorer_visible);
        assert!(app.shell.drawer_visible);
        assert!(!app.shell.context_visible);
    }

    #[test]
    fn unmodified_shortcut_keys_do_not_toggle_shell_panels() {
        let mut app = StruktApp::default();

        let _ = app.update(key_pressed("b", key::Code::KeyB, Modifiers::empty()));
        let _ = app.update(key_pressed("j", key::Code::KeyJ, Modifiers::empty()));
        let _ = app.update(key_pressed("\\", key::Code::Backslash, Modifiers::empty()));

        assert!(app.shell.explorer_visible);
        assert!(!app.shell.drawer_visible);
        assert!(app.shell.context_visible);
    }

    #[test]
    fn opened_workspace_replaces_the_representative_file_view() {
        let project = tempdir().unwrap();
        std::fs::write(project.path().join("README.md"), "strukt").unwrap();
        let opened = open_workspace(&project);
        let mut app = StruktApp::default();

        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

        assert_eq!(
            app.workspace.as_ref().unwrap().root.path(),
            project.path().canonicalize().unwrap()
        );
        assert!(
            app.files
                .iter()
                .any(|entry| entry.relative_path == std::path::Path::new("README.md"))
        );
    }

    #[test]
    fn visibility_messages_refresh_discovery_options() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));

        let _ = app.update(Message::ToggleHiddenFiles);
        let _ = app.update(Message::ToggleIgnoredFiles);

        assert!(app.explorer_options.show_hidden);
        assert!(app.explorer_options.show_ignored);
        let explorer = &app.workspace.as_ref().unwrap().explorer;
        assert!(explorer.show_hidden);
        assert!(explorer.show_ignored);
    }

    #[test]
    fn opening_a_workspace_does_not_create_repository_metadata() {
        let project = tempdir().unwrap();

        let _ = open_workspace(&project);

        assert!(!project.path().join(".strukt").exists());
    }

    #[test]
    fn workspace_opening_restores_explorer_state_from_the_injected_store() {
        let app_data = tempdir().unwrap();
        let project = tempdir().unwrap();
        std::fs::write(project.path().join(".hidden"), "visible").unwrap();
        let store = WorkspaceStore::at(app_data.path());
        let mut state = workspace_state(project.path());
        state.explorer.show_hidden = true;
        store.save(&state).unwrap();

        let opened =
            crate::workspace::open_workspace_with_store(project.path().to_path_buf(), &store)
                .unwrap();

        assert_eq!(opened.state, state);
        assert!(
            opened
                .discovery
                .entries
                .iter()
                .any(|entry| entry.relative_path == std::path::Path::new(".hidden"))
        );
        assert!(!project.path().join(".strukt").exists());
    }

    #[test]
    fn overlapping_folder_pickers_are_suppressed_and_cancel_reenables_opening() {
        let mut app = StruktApp::default();

        assert_eq!(app.update(Message::OpenFolder).units(), 1);
        assert_eq!(app.update(Message::OpenFolder).units(), 0);
        assert_eq!(app.update(Message::FolderPicked(None)).units(), 0);
        assert_eq!(app.update(Message::OpenFolder).units(), 1);
    }

    #[test]
    fn stale_file_refresh_results_cannot_replace_current_state_or_error() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));

        assert_eq!(app.update(Message::ToggleHiddenFiles).units(), 1);
        assert_eq!(app.update(Message::ToggleIgnoredFiles).units(), 0);
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 1,
                result: Ok(discovery(&["obsolete.rs"])),
            })
            .units(),
            1
        );
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 2,
                result: Ok(discovery(&["current.rs"])),
            })
            .units(),
            0
        );
        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["stale.rs"])),
        });
        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Err("stale failure".to_owned()),
        });

        assert_eq!(app.files, vec![file_entry("current.rs")]);
        assert_eq!(app.workspace_error, None);
    }

    #[test]
    fn workspace_open_error_survives_a_successful_in_flight_refresh() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        assert_eq!(app.update(Message::ToggleHiddenFiles).units(), 1);

        let _ = app.update(Message::WorkspaceOpened(Err("cannot open B".to_owned())));
        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["from-a.rs"])),
        });

        assert_eq!(app.workspace_error.as_deref(), Some("cannot open B"));
        assert_eq!(app.files, vec![file_entry("from-a.rs")]);
    }

    #[test]
    fn rapid_visibility_toggles_coalesce_to_one_follow_up_refresh() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));

        assert_eq!(app.update(Message::ToggleHiddenFiles).units(), 1);
        assert_eq!(app.update(Message::ToggleIgnoredFiles).units(), 0);
        assert_eq!(app.update(Message::ToggleHiddenFiles).units(), 0);
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 1,
                result: Ok(discovery(&["obsolete.rs"])),
            })
            .units(),
            1
        );
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 1,
                result: Err("duplicate stale completion".to_owned()),
            })
            .units(),
            0
        );
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 3,
                result: Ok(discovery(&["latest.rs"])),
            })
            .units(),
            0
        );

        assert_eq!(app.files, vec![file_entry("latest.rs")]);
        assert_eq!(app.workspace_error, None);
    }

    #[test]
    fn failed_workspace_open_preserves_current_workspace_and_files() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let state = workspace_state(project.path());
        app.workspace = Some(state.clone());
        app.files = vec![file_entry("existing.rs")];

        let _ = app.update(Message::WorkspaceOpened(Err("cannot open".to_owned())));

        assert_eq!(app.workspace, Some(state));
        assert_eq!(app.files, vec![file_entry("existing.rs")]);
        assert_eq!(app.workspace_error.as_deref(), Some("cannot open"));
    }

    #[test]
    fn successful_workspace_open_invalidates_pending_file_refreshes() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        std::fs::write(second.path().join("README.md"), "strukt").unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(first.path()));
        let _ = app.update(Message::ToggleHiddenFiles);
        let opened = open_workspace(&second);

        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));
        let expected_files = app.files.clone();
        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["stale.rs"])),
        });

        assert_eq!(app.files, expected_files);
        assert_eq!(
            app.workspace.as_ref().unwrap().root.path(),
            second.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn new_workspace_toggle_waits_for_the_old_refresh_to_finish() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(first.path()));
        assert_eq!(app.update(Message::ToggleHiddenFiles).units(), 1);
        let opened = open_workspace(&second);

        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));
        assert_eq!(app.update(Message::ToggleIgnoredFiles).units(), 0);
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 1,
                result: Ok(discovery(&["old-workspace.rs"])),
            })
            .units(),
            1
        );
        assert_eq!(
            app.update(Message::FilesRefreshed {
                generation: 3,
                result: Ok(discovery(&["new-workspace.rs"])),
            })
            .units(),
            0
        );

        assert_eq!(app.files, vec![file_entry("new-workspace.rs")]);
        assert_eq!(
            app.workspace.as_ref().unwrap().root.path(),
            second.path().canonicalize().unwrap()
        );
    }
}
