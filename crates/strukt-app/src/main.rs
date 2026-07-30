#![forbid(unsafe_code)]

mod app;
mod view;
mod workspace;

use app::{LaunchMode, StruktApp};

fn main() -> iced::Result {
    let launch_mode = LaunchMode::from_args(std::env::args().skip(1));

    iced::application(
        move || StruktApp::boot(launch_mode),
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

    use crate::app::{ExplorerDialog, LaunchMode, Message, StruktApp, operation_from_dialog};

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

    #[test]
    fn explorer_labels_use_real_relative_paths() {
        let label = crate::view::file_entry_label(&FileEntry {
            relative_path: "src/main.rs".into(),
            kind: FileKind::File,
            depth: 2,
            hidden: false,
            ignored: false,
        });

        assert_eq!(label, "    main.rs");
    }

    #[test]
    fn dialog_builds_only_complete_file_operations() {
        assert_eq!(
            operation_from_dialog(&ExplorerDialog::Rename {
                from: "old.txt".into(),
                to: "new.txt".into(),
            }),
            Some(strukt_fs::FileOperation::Rename {
                from: "old.txt".into(),
                to: "new.txt".into(),
            })
        );
        assert_eq!(
            operation_from_dialog(&ExplorerDialog::CreateFile(String::new())),
            None
        );
    }

    #[test]
    fn explorer_selection_rejects_paths_outside_the_workspace() {
        let mut app = StruktApp::default();

        let _ = app.update(Message::SelectExplorerEntry("../outside".into()));
        assert_eq!(app.selected_entry, None);

        let _ = app.update(Message::SelectExplorerEntry("/absolute".into()));
        assert_eq!(app.selected_entry, None);

        let _ = app.update(Message::SelectExplorerEntry("src/main.rs".into()));
        assert_eq!(app.selected_entry, Some("src/main.rs".into()));
    }

    #[test]
    fn ordinary_trash_and_permanent_delete_are_distinct_dialogs() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.selected_entry = Some("notes.txt".into());

        let _ = app.update(Message::BeginTrash);
        assert_eq!(
            app.explorer_dialog,
            ExplorerDialog::ConfirmTrash("notes.txt".into())
        );
        assert_eq!(
            operation_from_dialog(&app.explorer_dialog),
            Some(strukt_fs::FileOperation::MoveToTrash("notes.txt".into()))
        );

        let _ = app.update(Message::BeginPermanentDelete);
        assert_eq!(
            app.explorer_dialog,
            ExplorerDialog::ConfirmPermanentDelete("notes.txt".into())
        );
        assert_eq!(
            operation_from_dialog(&app.explorer_dialog),
            Some(strukt_fs::FileOperation::DeletePermanently(
                "notes.txt".into()
            ))
        );
    }

    #[test]
    fn one_file_operation_can_be_in_flight() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.explorer_dialog = ExplorerDialog::CreateFile("first.txt".to_owned());

        assert_eq!(app.update(Message::SubmitExplorerDialog).units(), 1);
        app.explorer_dialog = ExplorerDialog::CreateFile("second.txt".to_owned());
        assert_eq!(app.update(Message::SubmitExplorerDialog).units(), 0);
    }

    #[test]
    fn stale_file_operation_completion_cannot_affect_a_replaced_workspace() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(first.path()));
        app.explorer_dialog = ExplorerDialog::CreateFile("first.txt".to_owned());
        let _ = app.update(Message::SubmitExplorerDialog);
        let opened = open_workspace(&second);
        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));
        app.explorer_dialog = ExplorerDialog::CreateFile("second.txt".to_owned());

        let task = app.update(Message::FileOperationCompleted {
            generation: 1,
            workspace_root: first.path().canonicalize().unwrap(),
            result: Err("stale failure".to_owned()),
        });

        assert_eq!(task.units(), 0);
        assert_eq!(
            app.explorer_dialog,
            ExplorerDialog::CreateFile("second.txt".to_owned())
        );
        assert_eq!(app.workspace_error, None);
    }

    #[test]
    fn failed_file_operation_keeps_dialog_open_and_owns_its_error() {
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.explorer_dialog = ExplorerDialog::CreateFile("notes.txt".to_owned());
        let _ = app.update(Message::SubmitExplorerDialog);

        let _ = app.update(Message::FileOperationCompleted {
            generation: 1,
            workspace_root: root.clone(),
            result: Err("cannot create".to_owned()),
        });

        assert_eq!(
            app.explorer_dialog,
            ExplorerDialog::CreateFile("notes.txt".to_owned())
        );
        assert_eq!(app.workspace_error.as_deref(), Some("cannot create"));
    }

    #[test]
    fn accepted_refresh_clears_a_selection_that_disappeared() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.files = vec![file_entry("removed.txt")];
        app.selected_entry = Some("removed.txt".into());
        let _ = app.update(Message::ToggleHiddenFiles);

        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["remaining.txt"])),
        });

        assert_eq!(app.selected_entry, None);
    }

    #[test]
    fn accepted_refresh_cancels_only_source_bound_dialogs_when_the_source_disappears() {
        let project = tempdir().unwrap();
        for dialog in [
            ExplorerDialog::Rename {
                from: "removed.txt".into(),
                to: "renamed.txt".to_owned(),
            },
            ExplorerDialog::Duplicate {
                from: "removed.txt".into(),
                to: "copy.txt".to_owned(),
            },
            ExplorerDialog::ConfirmTrash("removed.txt".into()),
            ExplorerDialog::ConfirmPermanentDelete("removed.txt".into()),
        ] {
            let mut app = StruktApp::default();
            app.workspace = Some(workspace_state(project.path()));
            let _ = app.update(Message::ToggleHiddenFiles);
            app.explorer_dialog = dialog;

            let _ = app.update(Message::FilesRefreshed {
                generation: 1,
                result: Ok(discovery(&["remaining.txt"])),
            });
            assert_eq!(app.explorer_dialog, ExplorerDialog::None);
        }

        for dialog in [
            ExplorerDialog::CreateFile("new.txt".to_owned()),
            ExplorerDialog::CreateDirectory("new".to_owned()),
        ] {
            let mut app = StruktApp::default();
            app.workspace = Some(workspace_state(project.path()));
            let _ = app.update(Message::ToggleHiddenFiles);
            app.explorer_dialog = dialog.clone();

            let _ = app.update(Message::FilesRefreshed {
                generation: 1,
                result: Ok(discovery(&["remaining.txt"])),
            });
            assert_eq!(app.explorer_dialog, dialog);
        }
    }

    #[test]
    fn an_open_dialog_cannot_be_replaced_or_change_visibility() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));

        let _ = app.update(Message::BeginCreateFile);
        let _ = app.update(Message::BeginCreateDirectory);
        let refresh = app.update(Message::ToggleHiddenFiles);

        assert_eq!(
            app.explorer_dialog,
            ExplorerDialog::CreateFile(String::new())
        );
        assert!(!app.explorer_options.show_hidden);
        assert_eq!(refresh.units(), 0);
    }

    #[test]
    fn trash_confirmation_freezes_selection_and_escalates_its_original_path() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.selected_entry = Some("a.txt".into());
        let _ = app.update(Message::BeginTrash);

        let _ = app.update(Message::SelectExplorerEntry("b.txt".into()));
        let _ = app.update(Message::BeginRename);
        let _ = app.update(Message::BeginPermanentDelete);

        assert_eq!(app.selected_entry, Some("a.txt".into()));
        assert_eq!(
            app.explorer_dialog,
            ExplorerDialog::ConfirmPermanentDelete("a.txt".into())
        );
    }

    #[test]
    fn refresh_cannot_clear_the_dialog_owned_by_an_in_flight_operation() {
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.files = vec![file_entry("source.txt")];
        app.selected_entry = Some("source.txt".into());
        let _ = app.update(Message::ToggleHiddenFiles);
        let _ = app.update(Message::BeginRename);
        let original_dialog = app.explorer_dialog.clone();
        let _ = app.update(Message::ExplorerDialogInput("renamed.txt".to_owned()));
        let original_dialog = match original_dialog {
            ExplorerDialog::Rename { from, .. } => ExplorerDialog::Rename {
                from,
                to: "renamed.txt".to_owned(),
            },
            _ => panic!("rename dialog must be open"),
        };
        let _ = app.update(Message::SubmitExplorerDialog);

        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["remaining.txt"])),
        });
        assert_eq!(app.selected_entry, None);
        assert_eq!(app.explorer_dialog, original_dialog);

        let _ = app.update(Message::FileOperationCompleted {
            generation: 1,
            workspace_root: root,
            result: Err("rename failed".to_owned()),
        });

        assert_eq!(app.explorer_dialog, original_dialog);
        assert_eq!(app.workspace_error.as_deref(), Some("rename failed"));
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
    fn opening_without_an_application_data_store_still_creates_a_local_workspace() {
        let project = tempdir().unwrap();

        let opened =
            crate::workspace::open_workspace_without_store(project.path().to_path_buf()).unwrap();

        assert_eq!(
            opened.state.root.path(),
            project.path().canonicalize().unwrap()
        );
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

    #[test]
    fn stale_watcher_event_cannot_mark_a_replaced_workspace_stale() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&first))));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        let task = app.update(Message::FileEvent {
            workspace_root: first.path().canonicalize().unwrap(),
            event: strukt_fs::FileEvent::Stale("old overflow".to_owned()),
        });

        assert_eq!(task.units(), 0);
        assert!(!app.workspace.as_ref().unwrap().stale_filesystem);
        assert_ne!(app.workspace_error.as_deref(), Some("old overflow"));
    }

    #[test]
    fn current_stale_watcher_event_marks_state_and_requests_one_refresh() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = project.path().canonicalize().unwrap();

        let first = app.update(Message::FileEvent {
            workspace_root: root.clone(),
            event: strukt_fs::FileEvent::Stale("overflow".to_owned()),
        });
        let second = app.update(Message::FileEvent {
            workspace_root: root,
            event: strukt_fs::FileEvent::Changed(vec!["file.txt".into()]),
        });

        assert_eq!(first.units(), 1);
        assert_eq!(second.units(), 0);
        assert!(app.workspace.as_ref().unwrap().stale_filesystem);
        assert_eq!(app.workspace_error.as_deref(), Some("overflow"));
    }

    #[test]
    fn stale_search_completion_cannot_replace_newer_results() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let root = project.path().canonicalize().unwrap();
        let _ = app.update(Message::SearchChanged("old".to_owned()));
        let _ = app.update(Message::SearchChanged("new".to_owned()));

        let _ = app.update(Message::SearchCompleted {
            generation: 1,
            workspace_root: root,
            result: Ok(strukt_fs::SearchResult {
                matches: vec![strukt_fs::SearchMatch {
                    relative_path: "old.txt".into(),
                    line: 1,
                    preview: "old".to_owned(),
                }],
                truncated: false,
            }),
        });

        assert!(app.search_results.matches.is_empty());
    }

    #[test]
    fn quick_open_ignored_results_are_guarded_by_workspace_identity() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(second.path()));
        app.quick_open_query = "secret".to_owned();

        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: first.path().canonicalize().unwrap(),
            filesystem_revision: 0,
            result: Ok(vec![file_entry("secret.txt")]),
        });

        assert!(app.quick_open_results.is_empty());
    }

    #[test]
    fn manual_open_prevents_late_auto_restore_from_replacing_user_intent() {
        let auto = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::OpenFolder);

        let task = app.update(Message::RecentWorkspaceLoaded(Ok(
            strukt_persistence::RecentWorkspaces {
                paths: vec![auto.path().canonicalize().unwrap()],
            },
        )));

        assert_eq!(task.units(), 0);
        assert!(app.workspace.is_none());
        assert_eq!(app.recent_workspaces.len(), 1);
    }

    #[test]
    fn quick_open_and_search_ignore_preferences_do_not_change_explorer_visibility() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::ToggleQuickOpenIgnored);
        let _ = app.update(Message::ToggleSearchIgnored);

        assert!(app.quick_open_include_ignored);
        assert!(app.search_include_ignored);
        assert!(!app.explorer_options.show_ignored);
    }

    #[test]
    fn quick_open_reuses_a_valid_ignored_cache_after_close_and_reopen() {
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.files = vec![file_entry("visible.txt")];
        assert_eq!(app.update(Message::ToggleQuickOpenIgnored).units(), 1);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: root,
            filesystem_revision: 0,
            result: Ok(vec![file_entry("ignored-secret.txt")]),
        });

        let _ = app.update(Message::ToggleQuickOpen);
        assert_eq!(
            app.quick_open_results[0].relative_path,
            std::path::Path::new("ignored-secret.txt")
        );
        let _ = app.update(Message::ToggleQuickOpen);
        let reopen = app.update(Message::ToggleQuickOpen);

        assert_eq!(reopen.units(), 1);
        assert_eq!(
            app.quick_open_results[0].relative_path,
            std::path::Path::new("ignored-secret.txt")
        );
    }

    #[test]
    fn accepted_refresh_invalidates_ignored_quick_open_cache_before_reopen() {
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::ToggleQuickOpenIgnored);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: root.clone(),
            filesystem_revision: 0,
            result: Ok(vec![file_entry("stale-secret.txt")]),
        });
        let _ = app.update(Message::ToggleHiddenFiles);
        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["current.txt"])),
        });

        let open = app.update(Message::ToggleQuickOpen);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: root,
            filesystem_revision: 0,
            result: Ok(vec![file_entry("late-stale-secret.txt")]),
        });

        assert_eq!(open.units(), 2);
        assert!(app.quick_open_results.is_empty());
    }

    #[test]
    fn accepted_refresh_restarts_an_open_ignored_quick_open_without_stale_results() {
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::ToggleQuickOpenIgnored);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: root.clone(),
            filesystem_revision: 0,
            result: Ok(vec![file_entry("old-secret.txt")]),
        });
        let _ = app.update(Message::ToggleQuickOpen);
        let _ = app.update(Message::ToggleHiddenFiles);

        let replacement = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["visible-now.txt"])),
        });
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: root,
            filesystem_revision: 0,
            result: Ok(vec![file_entry("late-old-secret.txt")]),
        });

        assert_eq!(replacement.units(), 1);
        assert!(app.quick_open_results.is_empty());
    }

    #[test]
    fn ignored_quick_open_reload_batches_with_pending_workspace_persistence() {
        let data = tempdir().unwrap();
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::new_with_store(
            LaunchMode::Interactive,
            Some(WorkspaceStore::at(data.path())),
        );
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::ToggleQuickOpenIgnored);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: root,
            filesystem_revision: 0,
            result: Ok(vec![file_entry("old-secret.txt")]),
        });
        let _ = app.update(Message::ToggleQuickOpen);
        let _ = app.update(Message::ToggleHiddenFiles);

        let completion = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["visible-now.txt"])),
        });

        assert_eq!(completion.units(), 2);
        assert!(app.quick_open_results.is_empty());
    }

    #[test]
    fn accepted_refresh_reranks_an_open_normal_quick_open_from_refreshed_files() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.files = vec![file_entry("old-target.txt")];
        let _ = app.update(Message::ToggleQuickOpen);
        let _ = app.update(Message::QuickOpenChanged("target".to_owned()));
        assert_eq!(
            app.quick_open_results[0].relative_path,
            std::path::Path::new("old-target.txt")
        );
        let _ = app.update(Message::ToggleHiddenFiles);

        let completion = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["new-target.txt", "unrelated.txt"])),
        });

        assert_eq!(completion.units(), 0);
        assert_eq!(
            app.quick_open_results
                .iter()
                .map(|candidate| candidate.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![std::path::Path::new("new-target.txt")]
        );
    }

    #[test]
    fn workspace_replacement_invalidates_ignored_quick_open_cache() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let first_root = first.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(first.path()));
        let _ = app.update(Message::ToggleQuickOpenIgnored);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 1,
            workspace_root: first_root,
            filesystem_revision: 0,
            result: Ok(vec![file_entry("first-secret.txt")]),
        });
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        let open = app.update(Message::ToggleQuickOpen);

        assert_eq!(open.units(), 2);
        assert!(app.quick_open_results.is_empty());
    }

    #[test]
    fn successful_rescan_clears_the_stale_filesystem_flag() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = project.path().canonicalize().unwrap();
        let _ = app.update(Message::FileEvent {
            workspace_root: root,
            event: strukt_fs::FileEvent::Stale("overflow".to_owned()),
        });

        let _ = app.update(Message::FilesRefreshed {
            generation: 2,
            result: Ok(discovery(&["current.rs"])),
        });

        assert!(!app.workspace.as_ref().unwrap().stale_filesystem);
        assert_eq!(app.workspace_error, None);
    }

    #[test]
    fn persistence_writes_are_serialized_and_old_errors_do_not_affect_new_workspace() {
        let data = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::new_with_store(
            LaunchMode::Interactive,
            Some(WorkspaceStore::at(data.path())),
        );
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&first))));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        let follow_up = app.update(Message::WorkspacePersisted {
            generation: 1,
            workspace_root: first.path().canonicalize().unwrap(),
            recent_roots: vec![WorkspaceRoot::open(first.path()).unwrap()],
            result: Err("old save failed".to_owned()),
        });

        assert_eq!(follow_up.units(), 1);
        assert_eq!(app.workspace_error, None);

        let _ = app.update(Message::WorkspacePersisted {
            generation: 2,
            workspace_root: second.path().canonicalize().unwrap(),
            recent_roots: vec![
                WorkspaceRoot::open(first.path()).unwrap(),
                WorkspaceRoot::open(second.path()).unwrap(),
            ],
            result: Ok(()),
        });
        assert_eq!(app.workspace_error, None);
    }

    #[test]
    fn opening_a_new_workspace_clears_the_previous_workspace_persistence_error() {
        let data = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::new_with_store(
            LaunchMode::Interactive,
            Some(WorkspaceStore::at(data.path())),
        );
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&first))));
        let _ = app.update(Message::WorkspacePersisted {
            generation: 1,
            workspace_root: first.path().canonicalize().unwrap(),
            recent_roots: vec![WorkspaceRoot::open(first.path()).unwrap()],
            result: Err("first save failed".to_owned()),
        });
        assert_eq!(app.workspace_error.as_deref(), Some("first save failed"));

        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        assert_eq!(app.workspace_error, None);
    }

    #[test]
    fn search_error_is_not_cleared_by_an_unrelated_explorer_refresh() {
        let project = tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::SearchChanged("needle".to_owned()));
        let _ = app.update(Message::SearchCompleted {
            generation: 1,
            workspace_root: root,
            result: Err("search failed".to_owned()),
        });
        let _ = app.update(Message::ToggleHiddenFiles);

        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(discovery(&["current.rs"])),
        });

        assert_eq!(app.workspace_error.as_deref(), Some("search failed"));
    }

    #[test]
    fn coalesced_snapshot_saves_preserve_every_recent_root_in_open_order() {
        let data = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let third = tempdir().unwrap();
        let store = WorkspaceStore::at(data.path());
        let first_root = WorkspaceRoot::open(first.path()).unwrap();
        let second_root = WorkspaceRoot::open(second.path()).unwrap();
        let third_root = WorkspaceRoot::open(third.path()).unwrap();
        let mut app = StruktApp::new_with_store(LaunchMode::Interactive, Some(store.clone()));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&first))));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&third))));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        assert_eq!(app.active_recent_roots_for_test(), vec![first_root.clone()]);
        assert_eq!(
            app.pending_recent_roots_for_test(),
            vec![third_root.clone(), second_root.clone()]
        );
        crate::app::persist_workspace_batch(
            &store,
            app.workspace.as_ref().unwrap(),
            &[first_root, third_root.clone(), second_root.clone()],
        )
        .unwrap();

        assert_eq!(
            store.load_recent().unwrap().paths,
            vec![
                second_root.path().to_path_buf(),
                third_root.path().to_path_buf(),
                first.path().canonicalize().unwrap(),
            ]
        );
    }

    #[test]
    fn command_p_toggles_quick_open_without_changing_the_explorer() {
        let mut app = StruktApp::default();
        let explorer_visible = app.shell.explorer_visible;

        let focus = app.update(key_pressed("p", key::Code::KeyP, Modifiers::COMMAND));

        assert!(app.quick_open_visible);
        assert_eq!(focus.units(), 1);
        assert_eq!(app.shell.explorer_visible, explorer_visible);

        let close = app.update(Message::ToggleQuickOpen);
        assert!(!app.quick_open_visible);
        assert_eq!(close.units(), 0);
    }

    #[test]
    fn every_recent_workspace_offers_locate_even_when_the_path_exists() {
        let existing = tempdir().unwrap();

        assert!(crate::view::recent_workspace_offers_locate(existing.path()));
    }
}
