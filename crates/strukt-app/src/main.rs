#![forbid(unsafe_code)]

mod app;
mod editor;
mod recovery_key;
mod terminal;
mod terminal_widget;
mod view;
mod workspace;

use app::{LaunchMode, StruktApp};

fn main() -> iced::Result {
    let launch_mode = LaunchMode::from_args(std::env::args().skip(1));

    if let LaunchMode::EditorSmoke { root } = &launch_mode {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("editor smoke runtime must start");
        if let Err(error) = runtime.block_on(app::editor_smoke_task(root.clone())) {
            panic!("strukt editor smoke failed: {error}");
        }
        println!("{}", app::EDITOR_SMOKE_SUCCESS);
        return Ok(());
    }

    if let LaunchMode::TerminalSmoke { root } = &launch_mode {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("terminal smoke runtime must start");
        if let Err(error) = runtime.block_on(app::terminal_smoke_task(root.clone())) {
            panic!("strukt terminal smoke failed: {error}");
        }
        println!("{}", app::TERMINAL_SMOKE_SUCCESS);
        return Ok(());
    }

    if let LaunchMode::WorkspaceFilesSmoke { root } = &launch_mode {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("workspace files smoke runtime must start");
        if let Err(error) = runtime.block_on(app::workspace_files_smoke_task(root.clone())) {
            panic!("strukt workspace files smoke failed: {error}");
        }
        println!("{}", app::WORKSPACE_FILES_SMOKE_SUCCESS);
        return Ok(());
    }

    iced::application(
        move || StruktApp::boot(launch_mode.clone()),
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
    use std::path::PathBuf;
    use std::time::Duration;

    use iced::advanced::text::editor::Edit;
    use iced::keyboard::{self, Key, Location, Modifiers, key};
    use iced::widget::text_editor::Action;
    use strukt_core::CapabilityId;
    use strukt_editor::{DiskRevision, DocumentStatus, OpenDisposition};
    use strukt_fs::{
        DiscoveryOptions, DiscoveryReport, DocumentKind, DocumentRead, FileEntry, FileKind,
    };
    use strukt_persistence::{
        EditorRecoveryStore, EditorSessionSnapshot, RecoveryMetadata, RecoveryPayload,
        TerminalSessionSnapshot, WorkspaceStore, set_terminal_contribution, terminal_contribution,
    };
    use strukt_shell::Activity;
    use strukt_terminal::{PaneState, SplitAxis, TerminalWorkspace};
    use strukt_workspace::{WorkspaceRoot, WorkspaceState};
    use tempfile::{TempDir, tempdir};

    use crate::app::{
        ExplorerDialog, LaunchMode, Message, StruktApp, operation_from_dialog, run_editor_smoke,
        run_workspace_files_smoke, supported_terminal_link_target,
    };

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

    fn named_key_pressed(key: key::Named, code: key::Code, modifiers: Modifiers) -> Message {
        Message::Keyboard(keyboard::Event::KeyPressed {
            modified_key: Key::Named(key),
            key: Key::Named(key),
            physical_key: key::Physical::Code(code),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    fn text_key_pressed(character: &'static str, code: key::Code) -> Message {
        let key = Key::Character(character.into());
        Message::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: key::Physical::Code(code),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: Some(character.into()),
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

    fn ignored_file_entry(path: &str) -> FileEntry {
        FileEntry {
            ignored: true,
            ..file_entry(path)
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

    fn text_document(text: &str, token: &str, read_only: bool) -> DocumentRead {
        DocumentRead {
            kind: DocumentKind::Text {
                read_only,
                truncated: read_only,
            },
            text: Some(text.into()),
            size: text.len() as u64,
            disk_revision: DiskRevision::new(token),
        }
    }

    #[test]
    fn built_in_capabilities_are_registered() {
        let app = StruktApp::default();

        assert!(app.capabilities.is_enabled(CapabilityId::FILES));
        assert!(app.capabilities.is_enabled(CapabilityId::TERMINAL));
        assert!(app.capabilities.is_enabled(CapabilityId::AI));
        assert!(app.capabilities.is_enabled(CapabilityId::EDITOR_DOCUMENTS));
        assert!(app.capabilities.is_enabled(CapabilityId::EDITOR_SYNTAX));
    }

    #[test]
    fn terminal_commands_require_a_workspace_and_never_spawn_on_open() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        assert_eq!(app.update(Message::NewTerminal).units(), 0);

        app.workspace = Some(workspace_state(project.path()));
        assert!(app.terminal.workspace().tabs().is_empty());
        assert_eq!(app.update(Message::NewTerminal).units(), 1);

        assert_eq!(app.terminal.workspace().tabs().len(), 1);
        assert_eq!(app.terminal.running_processes(), 0);
    }

    #[test]
    fn terminal_spawn_is_scheduled_outside_the_update_reducer() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::NewTerminal);
        let pane = app.terminal.workspace().focused_pane().unwrap();

        let task = app.update(Message::StartTerminal(pane));

        assert_eq!(task.units(), 1);
        assert_eq!(app.terminal.running_processes(), 0);
        assert!(matches!(
            app.terminal.workspace().pane(pane).unwrap().state(),
            PaneState::Starting
        ));
    }

    #[test]
    fn control_tab_navigation_precedes_platform_command_shortcuts() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::NewTerminal);
        let first = app.terminal.workspace().active_tab().unwrap().id();
        let _ = app.update(Message::NewTerminal);
        let pane = app.terminal.workspace().focused_pane().unwrap();
        let _ = app.update(Message::TerminalWidget(
            crate::terminal_widget::TerminalWidgetEvent::Focus(pane),
        ));

        let _ = app.update(named_key_pressed(
            key::Named::Tab,
            key::Code::Tab,
            Modifiers::CTRL | Modifiers::COMMAND,
        ));

        assert_eq!(app.terminal.workspace().active_tab().unwrap().id(), first);
    }

    #[test]
    fn editing_a_terminal_tab_name_releases_terminal_input_focus() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::NewTerminal);
        let pane = app.terminal.workspace().focused_pane().unwrap();
        let _ = app.update(Message::TerminalWidget(
            crate::terminal_widget::TerminalWidgetEvent::Focus(pane),
        ));

        let _ = app.update(Message::TerminalTabNameChanged("renamed".to_owned()));
        let _ = app.update(text_key_pressed("x", key::Code::KeyX));

        assert!(app.terminal_error.is_none());
    }

    #[test]
    fn restored_terminal_contribution_creates_only_stopped_placeholders() {
        let project = tempdir().unwrap();
        let mut terminal = TerminalWorkspace::default();
        terminal.create_tab("build", project.path()).unwrap();
        terminal.split_focused(SplitAxis::Vertical).unwrap();
        let snapshot = TerminalSessionSnapshot::from_workspace(&terminal);
        let mut opened = open_workspace(&project);
        set_terminal_contribution(&mut opened.state, &snapshot).unwrap();
        let mut app = StruktApp::default();

        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

        assert!(
            app.terminal
                .workspace()
                .panes()
                .all(|pane| matches!(pane.state(), PaneState::Stopped))
        );
        assert_eq!(app.terminal.running_processes(), 0);
    }

    #[test]
    fn terminal_layout_changes_persist_and_capability_disablement_is_isolated() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.capabilities
            .set_enabled(CapabilityId::TERMINAL, false)
            .unwrap();
        assert_eq!(app.update(Message::NewTerminal).units(), 0);
        assert!(app.terminal.workspace().tabs().is_empty());
        assert!(app.capabilities.is_enabled(CapabilityId::FILES));
        assert!(app.capabilities.is_enabled(CapabilityId::EDITOR_DOCUMENTS));

        app.capabilities
            .set_enabled(CapabilityId::TERMINAL, true)
            .unwrap();
        let _ = app.update(Message::NewTerminal);
        let _ = app.update(Message::SplitTerminal(SplitAxis::Horizontal));
        let saved = terminal_contribution(app.workspace.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(saved.tabs.len(), 1);
        assert!(matches!(
            saved.tabs[0].root,
            strukt_terminal::LayoutNode::Split { .. }
        ));
    }

    #[test]
    fn replacing_a_workspace_discards_the_previous_terminal_runtime() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(first.path()));
        let _ = app.update(Message::NewTerminal);
        assert_eq!(app.terminal.workspace().tabs().len(), 1);

        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        assert!(app.terminal.workspace().tabs().is_empty());
        assert_eq!(app.terminal.running_processes(), 0);
    }

    #[test]
    fn terminal_tabs_can_be_renamed_activated_and_closed_without_spawning() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::NewTerminal);
        let first = app.terminal.workspace().active_tab().unwrap().id();
        let first_pane = app.terminal.workspace().focused_pane().unwrap();
        let _ = app.update(Message::NewTerminal);

        let _ = app.update(Message::ActivateTerminalTab(first));
        let _ = app.update(Message::TerminalTabNameChanged("server".to_owned()));
        let _ = app.update(Message::RenameTerminalTab);
        assert_eq!(
            app.terminal.workspace().active_tab().unwrap().name(),
            "server"
        );

        let _ = app.update(Message::RequestCloseTerminal(first_pane));
        assert_eq!(app.terminal.workspace().tabs().len(), 1);
        assert_eq!(app.terminal.running_processes(), 0);
    }

    #[test]
    fn oversized_terminal_paste_requires_explicit_confirmation() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        let _ = app.update(Message::NewTerminal);
        let pane = app.terminal.workspace().focused_pane().unwrap();
        let large = "x".repeat(1024 * 1024 + 1);

        let _ = app.update(Message::TerminalClipboardRead {
            pane,
            text: Some(large),
        });

        assert!(app.pending_terminal_paste.is_some());
        let _ = app.update(Message::ResolveTerminalPaste(false));
        assert!(app.pending_terminal_paste.is_none());
    }

    #[test]
    fn terminal_links_require_supported_exact_targets_and_second_action() {
        assert!(supported_terminal_link_target("https://example.com/path"));
        assert!(supported_terminal_link_target("mailto:dev@example.com"));
        assert!(!supported_terminal_link_target("javascript:alert(1)"));
        assert!(!supported_terminal_link_target("HTTPS://example.com"));

        let mut app = StruktApp::default();
        let _ = app.update(Message::InspectTerminalLink(
            "https://example.com/path".to_owned(),
        ));
        assert_eq!(
            app.pending_terminal_link.as_deref(),
            Some("https://example.com/path")
        );
        let _ = app.update(Message::ResolveTerminalLink(false));
        assert!(app.pending_terminal_link.is_none());
    }

    #[test]
    fn document_open_reducer_replaces_preview_and_reuses_an_existing_path() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();

        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "one.rs".into(),
            disposition: OpenDisposition::Preview,
            result: Ok(text_document("one", "disk-1", false)),
        });
        let first = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "two.rs".into(),
            disposition: OpenDisposition::Preview,
            result: Ok(text_document("two", "disk-2", false)),
        });
        assert_eq!(app.editor.as_ref().unwrap().document_count(), 1);
        assert!(app.editor.as_ref().unwrap().document(first).is_none());

        let _ = app.update(Message::DocumentOpened {
            workspace_root: root,
            path: "two.rs".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("ignored", "disk-3", false)),
        });
        assert_eq!(app.editor.as_ref().unwrap().document_count(), 1);
        assert!(app.editor.as_ref().unwrap().view_state().tabs[0].pinned);
    }

    #[test]
    fn native_edit_undo_redo_and_dirty_close_are_reduced_consistently() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root,
            path: "file.txt".into(),
            disposition: OpenDisposition::Preview,
            result: Ok(text_document("abc", "disk", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();

        let _ = app.update(Message::EditorAction {
            id,
            action: Action::Edit(Edit::Insert('x')),
        });
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "xabc"
        );
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().status(),
            &DocumentStatus::Dirty
        );
        let _ = app.update(Message::UndoDocument(id));
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "abc"
        );
        let _ = app.update(Message::RedoDocument(id));
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "xabc"
        );

        let _ = app.update(Message::EditorFindChanged("x".into()));
        let _ = app.update(Message::EditorReplaceChanged("y".into()));
        let _ = app.update(Message::ReplaceAll(id));
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "yabc"
        );

        let _ = app.update(Message::CloseDocument(id));
        assert_eq!(app.pending_close, Some(id));
        assert_eq!(
            app.update(Message::ResolveDocumentClose {
                id,
                decision: strukt_editor::CloseDecision::Save,
            })
            .units(),
            1
        );
    }

    #[test]
    fn watcher_reload_and_conflict_actions_preserve_user_intent() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let revision = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();

        let _ = app.update(Message::DocumentDiskObserved {
            workspace_root: root.clone(),
            id,
            expected_revision: revision,
            result: Ok(crate::app::DiskObservation::Present(text_document(
                "disk", "disk-2", false,
            ))),
        });
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "disk"
        );

        let _ = app.update(Message::EditorAction {
            id,
            action: Action::Edit(Edit::Insert('x')),
        });
        let revision = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();
        let _ = app.update(Message::DocumentDiskObserved {
            workspace_root: root,
            id,
            expected_revision: revision,
            result: Ok(crate::app::DiskObservation::Present(text_document(
                "new disk", "disk-3", false,
            ))),
        });
        assert!(matches!(
            app.editor.as_ref().unwrap().document(id).unwrap().status(),
            DocumentStatus::Conflict { disk_text, .. } if disk_text == "new disk"
        ));

        let _ = app.update(Message::ReloadDocumentFromDisk(id));
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "new disk"
        );
        let _ = app.update(Message::UndoDocument(id));
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "xdisk"
        );
    }

    #[test]
    fn stale_disk_and_recovery_completions_are_rejected() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let workspace_id = app
            .workspace
            .as_ref()
            .unwrap()
            .root
            .id()
            .as_str()
            .to_owned();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let stale = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();
        let _ = app.update(Message::EditorAction {
            id,
            action: Action::Edit(Edit::Insert('x')),
        });

        let _ = app.update(Message::DocumentDiskObserved {
            workspace_root: root.clone(),
            id,
            expected_revision: stale,
            result: Ok(crate::app::DiskObservation::Present(text_document(
                "stale disk",
                "disk-2",
                false,
            ))),
        });
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "xbase"
        );

        let payload = RecoveryPayload::new(
            RecoveryMetadata::new(workspace_id, "file.txt", "disk-1"),
            1,
            "stale recovery",
        );
        let _ = app.update(Message::RecoveryLoaded {
            workspace_root: root,
            id,
            expected_revision: stale,
            result: Ok(Some(payload)),
        });
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "xbase"
        );
    }

    #[test]
    fn watcher_suppresses_only_the_revision_produced_by_the_matching_save() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let expected = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();
        let _ = app.update(Message::DocumentSaved {
            workspace_root: root.clone(),
            id,
            expected_revision: expected,
            result: Ok(strukt_fs::SaveOutcome {
                disk_revision: DiskRevision::new("saved"),
                bytes_written: 4,
            }),
        });
        let after_save = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();

        let _ = app.update(Message::DocumentDiskObserved {
            workspace_root: root.clone(),
            id,
            expected_revision: after_save,
            result: Ok(crate::app::DiskObservation::Present(text_document(
                "base", "saved", false,
            ))),
        });
        assert_eq!(
            app.editor
                .as_ref()
                .unwrap()
                .document(id)
                .unwrap()
                .revision(),
            after_save
        );

        let _ = app.update(Message::DocumentDiskObserved {
            workspace_root: root,
            id,
            expected_revision: after_save,
            result: Ok(crate::app::DiskObservation::Present(text_document(
                "external",
                "after-save",
                false,
            ))),
        });
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "external"
        );
    }

    #[test]
    fn stale_successful_save_never_deletes_newer_unsaved_recovery() {
        let project = tempdir().unwrap();
        let recovery = tempdir().unwrap();
        let mut app = StruktApp::new_with_store(LaunchMode::Interactive, None);
        app.recovery_store = Some(EditorRecoveryStore::at(recovery.path()));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let stale_revision = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();
        let _ = app.update(Message::EditorAction {
            id,
            action: Action::Edit(Edit::Insert('x')),
        });

        let completion = app.update(Message::DocumentSaved {
            workspace_root: root,
            id,
            expected_revision: stale_revision,
            result: Ok(strukt_fs::SaveOutcome {
                disk_revision: DiskRevision::new("saved-old-revision"),
                bytes_written: 4,
            }),
        });

        assert_eq!(completion.units(), 0);
        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().status(),
            &DocumentStatus::Dirty
        );
    }

    #[test]
    fn recovery_deadline_is_coalesced_to_the_latest_document_revision() {
        let project = tempdir().unwrap();
        let recovery = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.recovery_store = Some(EditorRecoveryStore::at(recovery.path()));
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let _ = app.update(Message::EditorAction {
            id,
            action: Action::Edit(Edit::Insert('x')),
        });
        let first_revision = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();
        let _ = app.update(Message::EditorAction {
            id,
            action: Action::Edit(Edit::Insert('y')),
        });
        let latest_revision = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();

        assert_eq!(
            app.update(Message::RecoveryDue {
                workspace_root: root.clone(),
                id,
                expected_revision: first_revision,
                generation: 1,
            })
            .units(),
            0
        );
        assert_eq!(
            app.update(Message::RecoveryDue {
                workspace_root: root,
                id,
                expected_revision: latest_revision,
                generation: 2,
            })
            .units(),
            1
        );
    }

    #[test]
    fn editor_session_contribution_tracks_tabs_and_find_state() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root,
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let _ = app.update(Message::EditorFindChanged("needle".into()));
        let _ = app.update(Message::EditorReplaceChanged("replacement".into()));
        let _ = app.update(Message::ToggleFindCase);
        let _ = app.update(Message::ToggleFindWholeWord);
        let _ = app.update(Message::ToggleFindRegex);

        let snapshot = app
            .workspace
            .as_ref()
            .unwrap()
            .contribution::<EditorSessionSnapshot>("editor")
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.tabs.len(), 1);
        assert_eq!(snapshot.tabs[0].path, "file.txt");
        assert_eq!(snapshot.tabs[0].find_query, "needle");
        assert_eq!(snapshot.tabs[0].replace_text, "replacement");
        assert!(snapshot.tabs[0].find_options.case_sensitive);
        assert!(snapshot.tabs[0].find_options.whole_word);
        assert!(snapshot.tabs[0].find_options.regex);
        assert_eq!(snapshot.active_path.as_deref(), Some("file.txt"));
    }

    #[test]
    fn workspace_restore_rehydrates_tab_metadata_and_missing_placeholder() {
        let project = tempdir().unwrap();
        let mut opened = open_workspace(&project);
        let mut tab = strukt_persistence::EditorTabSnapshot::new("missing.txt", 2, 1, 3.0);
        tab.find_query = "needle".into();
        tab.language_override = Some("rust".into());
        tab.disk_revision = Some("disk-1".into());
        opened
            .state
            .set_contribution(
                "editor",
                &EditorSessionSnapshot::new(vec![tab], Some("missing.txt".into()), None),
            )
            .unwrap();
        let root = opened.state.root.path().to_path_buf();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

        let _ = app.update(Message::DocumentOpened {
            workspace_root: root,
            path: "missing.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Err("document does not exist".into()),
        });

        let editor = app.editor.as_ref().unwrap();
        let id = editor.active_document_id().unwrap();
        assert_eq!(
            editor.document(id).unwrap().status(),
            &DocumentStatus::Missing
        );
        assert_eq!(app.editor_find_query, "needle");
        assert_eq!(
            app.editor_language_overrides.get(&id).map(String::as_str),
            Some("rust")
        );
        assert!(app.editor_error.as_deref().unwrap().contains("placeholder"));
    }

    #[test]
    fn restored_active_tab_does_not_reclaim_focus_from_later_user_opens() {
        let project = tempdir().unwrap();
        let mut opened = open_workspace(&project);
        let mut tab = strukt_persistence::EditorTabSnapshot::new("restored.txt", 0, 0, 0.0);
        tab.disk_revision = Some("disk-1".into());
        opened
            .state
            .set_contribution(
                "editor",
                &EditorSessionSnapshot::new(vec![tab], Some("restored.txt".into()), None),
            )
            .unwrap();
        let root = opened.state.root.path().to_path_buf();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "restored.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("restored", "disk-1", false)),
        });

        let _ = app.update(Message::OpenDocument {
            path: "later.txt".into(),
            disposition: OpenDisposition::Preview,
            force_full: false,
        });
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root,
            path: "later.txt".into(),
            disposition: OpenDisposition::Preview,
            result: Ok(text_document("later", "disk-2", false)),
        });

        let editor = app.editor.as_ref().unwrap();
        let active = editor
            .document(editor.active_document_id().unwrap())
            .unwrap();
        assert_eq!(active.path().as_str(), "later.txt");
    }

    #[test]
    fn unavailable_recovery_key_is_reported_without_applying_content() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "file.txt".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("base", "disk-1", false)),
        });
        let id = app.editor.as_ref().unwrap().active_document_id().unwrap();
        let revision = app
            .editor
            .as_ref()
            .unwrap()
            .document(id)
            .unwrap()
            .revision();

        let _ = app.update(Message::RecoveryLoaded {
            workspace_root: root,
            id,
            expected_revision: revision,
            result: Err("protected recovery key storage is unavailable".into()),
        });

        assert_eq!(
            app.editor.as_ref().unwrap().document(id).unwrap().text(),
            "base"
        );
        assert!(
            app.editor_error
                .as_deref()
                .unwrap()
                .contains("recovery disabled")
        );
    }

    #[test]
    fn binary_and_large_file_results_use_safe_views() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));
        let root = app.workspace.as_ref().unwrap().root.path().to_path_buf();
        let _ = app.update(Message::DocumentOpened {
            workspace_root: root.clone(),
            path: "image.bin".into(),
            disposition: OpenDisposition::Preview,
            result: Ok(DocumentRead {
                kind: DocumentKind::Binary,
                text: None,
                size: 42,
                disk_revision: DiskRevision::new("binary"),
            }),
        });
        assert!(matches!(
            app.document_notice,
            Some(crate::app::DocumentNotice::Binary { size: 42, .. })
        ));

        let _ = app.update(Message::DocumentOpened {
            workspace_root: root,
            path: "large.log".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("preview", "large", true)),
        });
        let document = app
            .editor
            .as_ref()
            .and_then(|editor| {
                editor
                    .active_document_id()
                    .and_then(|id| editor.document(id))
            })
            .unwrap();
        assert!(document.is_read_only());
        assert!(app.document_notice.is_none());

        let _ = app.update(Message::DocumentOpened {
            workspace_root: app.workspace.as_ref().unwrap().root.path().to_path_buf(),
            path: "large.log".into(),
            disposition: OpenDisposition::Pinned,
            result: Ok(text_document("complete file", "large-full", false)),
        });
        let document = app
            .editor
            .as_ref()
            .and_then(|editor| {
                editor
                    .active_document_id()
                    .and_then(|id| editor.document(id))
            })
            .unwrap();
        assert!(!document.is_read_only());
        assert_eq!(document.text(), "complete file");
    }

    #[test]
    fn stale_document_open_cannot_cross_workspace_replacement() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        let old_root = first.path().to_path_buf();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));

        let _ = app.update(Message::DocumentOpened {
            workspace_root: old_root,
            path: "stale.rs".into(),
            disposition: OpenDisposition::Preview,
            result: Ok(text_document("stale", "disk", false)),
        });

        assert_eq!(app.editor.as_ref().unwrap().document_count(), 0);
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
        assert_eq!(
            LaunchMode::from_args(["launcher-argument".to_owned(), "--smoke-test".to_owned()]),
            LaunchMode::SmokeTest
        );
    }

    #[test]
    fn workspace_files_smoke_requires_the_exact_flag_and_one_path() {
        let root = PathBuf::from("fixture");

        assert_eq!(
            LaunchMode::from_args([
                "--workspace-files-smoke".to_owned(),
                root.display().to_string(),
            ]),
            LaunchMode::WorkspaceFilesSmoke { root: root.clone() }
        );
        assert_eq!(
            LaunchMode::from_args(["--workspace-files-smoke".to_owned()]),
            LaunchMode::Interactive
        );
        assert_eq!(
            LaunchMode::from_args(["--workspace-files-smoke".to_owned(), String::new()]),
            LaunchMode::Interactive
        );
        assert_eq!(
            LaunchMode::from_args([
                "--workspace-files-smoke".to_owned(),
                root.display().to_string(),
                "extra".to_owned(),
            ]),
            LaunchMode::Interactive
        );
        assert_eq!(
            LaunchMode::from_args([
                "--workspace-file-smoke".to_owned(),
                root.display().to_string(),
            ]),
            LaunchMode::Interactive
        );
        assert_eq!(
            LaunchMode::from_args([
                "--workspace-files-smoke=true".to_owned(),
                root.display().to_string(),
            ]),
            LaunchMode::Interactive
        );
    }

    #[test]
    fn editor_smoke_requires_the_exact_flag_and_one_path() {
        let root = PathBuf::from("fixture");
        assert_eq!(
            LaunchMode::from_args(["--editor-smoke".to_owned(), root.display().to_string()]),
            LaunchMode::EditorSmoke { root: root.clone() }
        );
        for args in [
            vec!["--editor-smoke".to_owned()],
            vec!["--editor-smoke".to_owned(), String::new()],
            vec![
                "--editor-smoke".to_owned(),
                "fixture".into(),
                "extra".into(),
            ],
            vec!["--editor-smokes".to_owned(), "fixture".into()],
            vec!["--editor-smoke=true".to_owned(), "fixture".into()],
        ] {
            assert_eq!(LaunchMode::from_args(args), LaunchMode::Interactive);
        }
    }

    #[test]
    fn terminal_smoke_requires_the_exact_flag_and_one_existing_root() {
        let root = tempdir().unwrap();
        assert_eq!(
            LaunchMode::from_args([
                "--terminal-smoke".to_owned(),
                root.path().display().to_string(),
            ]),
            LaunchMode::TerminalSmoke {
                root: root.path().to_path_buf(),
            }
        );
        for args in [
            vec!["--terminal-smoke".to_owned()],
            vec![
                "--terminal-smokes".to_owned(),
                root.path().display().to_string(),
            ],
            vec![
                "--terminal-smoke".to_owned(),
                root.path().display().to_string(),
                "extra".to_owned(),
            ],
            vec![
                "--terminal-smoke".to_owned(),
                root.path().join("missing").display().to_string(),
            ],
        ] {
            assert_eq!(LaunchMode::from_args(args), LaunchMode::Interactive);
        }
    }

    #[test]
    fn workspace_files_smoke_opens_discovers_and_round_trips_without_repo_metadata() {
        let project = tempdir().unwrap();
        std::fs::write(project.path().join("strukt-smoke.txt"), "strukt\n").unwrap();

        run_workspace_files_smoke(project.path().to_path_buf()).unwrap();

        assert!(!project.path().join(".strukt").exists());
    }

    #[test]
    fn workspace_files_smoke_rejects_a_fixture_without_the_exact_sentinel() {
        let project = tempdir().unwrap();
        std::fs::write(project.path().join("strukt-smoke.txt.bak"), "strukt\n").unwrap();

        let error = run_workspace_files_smoke(project.path().to_path_buf()).unwrap_err();

        assert!(error.contains("strukt-smoke.txt"));
    }

    #[test]
    fn editor_smoke_edits_saves_and_restores_without_repository_metadata() {
        let project = tempdir().unwrap();
        std::fs::write(project.path().join("strukt-editor-smoke.txt"), "strukt\n").unwrap();

        run_editor_smoke(project.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(project.path().join("strukt-editor-smoke.txt")).unwrap(),
            "strukt\nedited by strukt\n"
        );
        assert!(!project.path().join(".strukt").exists());
    }

    #[test]
    fn editor_smoke_rejects_missing_and_binary_sentinels() {
        let missing = tempdir().unwrap();
        assert!(run_editor_smoke(missing.path()).is_err());

        let binary = tempdir().unwrap();
        std::fs::write(
            binary.path().join("strukt-editor-smoke.txt"),
            b"strukt\0binary",
        )
        .unwrap();
        assert!(run_editor_smoke(binary.path()).is_err());
    }

    #[test]
    fn only_smoke_mode_has_a_runtime_timeout() {
        assert_eq!(LaunchMode::Interactive.smoke_timeout(), None);
        assert_eq!(
            LaunchMode::SmokeTest.smoke_timeout(),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            LaunchMode::WorkspaceFilesSmoke {
                root: PathBuf::from("fixture")
            }
            .smoke_timeout(),
            None
        );
        assert_eq!(
            LaunchMode::EditorSmoke {
                root: PathBuf::from("fixture")
            }
            .smoke_timeout(),
            None
        );
        assert_eq!(
            LaunchMode::TerminalSmoke {
                root: PathBuf::from("fixture")
            }
            .smoke_timeout(),
            None
        );
    }

    #[test]
    fn smoke_timeout_requests_runtime_work() {
        let mut app = StruktApp::new(LaunchMode::SmokeTest);

        let task = app.update(Message::SmokeTimeout);

        assert_eq!(task.units(), 1);
    }

    #[test]
    fn workspace_files_smoke_completion_requests_runtime_exit() {
        let mut app = StruktApp::new_with_store(
            LaunchMode::WorkspaceFilesSmoke {
                root: PathBuf::from("fixture"),
            },
            None,
        );

        let task = app.update(Message::WorkspaceFilesSmokeFinished(Ok(())));

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
    fn workspace_open_schedules_a_post_watcher_reconciliation_without_a_store() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::new_with_store(LaunchMode::Interactive, None);

        let reconciliation = app.update(Message::WorkspaceOpened(Ok(open_workspace(&project))));

        assert_eq!(reconciliation.units(), 1);
    }

    #[test]
    fn post_registration_reconciliation_observes_a_change_without_another_watcher_event() {
        let project = tempdir().unwrap();
        let opened = open_workspace(&project);
        assert!(opened.discovery.entries.is_empty());
        let _watcher = strukt_fs::WorkspaceWatcher::start(opened.state.root.path()).unwrap();
        std::fs::write(project.path().join("created-during-open.txt"), "late").unwrap();

        let report =
            strukt_fs::discover_report_for_root(&opened.state.root, DiscoveryOptions::default())
                .unwrap();

        assert!(report.entries.iter().any(|entry| {
            entry.relative_path == std::path::Path::new("created-during-open.txt")
        }));
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
    fn workspace_open_restores_explorer_visibility() {
        let project = tempdir().unwrap();
        let mut opened = open_workspace(&project);
        opened.state.explorer.visible = false;
        let mut app = StruktApp::default();

        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

        assert!(!app.shell.explorer_visible);
        assert!(!app.workspace.as_ref().unwrap().explorer.visible);
    }

    #[test]
    fn explorer_toggle_updates_workspace_state_and_schedules_persistence() {
        let data = tempdir().unwrap();
        let project = tempdir().unwrap();
        let mut app = StruktApp::new_with_store(
            LaunchMode::Interactive,
            Some(WorkspaceStore::at(data.path())),
        );
        app.workspace = Some(workspace_state(project.path()));

        let persistence = app.update(Message::ToggleExplorer);

        assert!(!app.shell.explorer_visible);
        assert!(!app.workspace.as_ref().unwrap().explorer.visible);
        assert_eq!(persistence.units(), 1);
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
                generation: 4,
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

        assert_eq!(first.units(), 0);
        assert_eq!(second.units(), 0);
        assert!(app.workspace.as_ref().unwrap().stale_filesystem);
        assert_eq!(app.workspace_error.as_deref(), Some("overflow"));
    }

    #[test]
    fn watcher_polling_bounds_each_batch_and_preserves_stale_events_across_batches() {
        use std::collections::VecDeque;

        let mut events = VecDeque::new();
        for index in 0..super::app::MAX_WATCHER_EVENTS_PER_POLL {
            events.push_back(strukt_fs::FileEvent::Changed(vec![PathBuf::from(format!(
                "{index}.txt"
            ))]));
        }
        events.push_back(strukt_fs::FileEvent::Stale("overflow".to_owned()));
        events.push_back(strukt_fs::FileEvent::Changed(vec![PathBuf::from(
            "after.txt",
        )]));

        let first = super::app::drain_watcher_batch(|| events.pop_front());
        assert_eq!(first.drained, super::app::MAX_WATCHER_EVENTS_PER_POLL);
        assert!(first.changed);
        assert_eq!(first.stale_reason, None);
        assert_eq!(events.len(), 2);

        let second = super::app::drain_watcher_batch(|| events.pop_front());
        assert_eq!(second.drained, 2);
        assert!(second.changed);
        assert_eq!(second.stale_reason.as_deref(), Some("overflow"));
        assert!(events.is_empty());
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
    fn replacing_clearing_and_switching_workspace_cancel_active_search_work() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(first.path()));

        let _ = app.update(Message::SearchChanged("old".to_owned()));
        let old = app.search_cancellation_for_test();
        assert!(!old.is_cancelled());

        let _ = app.update(Message::SearchChanged("new".to_owned()));
        let new = app.search_cancellation_for_test();
        assert!(old.is_cancelled());
        assert!(!new.is_cancelled());

        let _ = app.update(Message::SearchChanged(String::new()));
        let cleared = app.search_cancellation_for_test();
        assert!(new.is_cancelled());
        assert!(!cleared.is_cancelled());

        let _ = app.update(Message::SearchChanged("workspace".to_owned()));
        let before_switch = app.search_cancellation_for_test();
        let _ = app.update(Message::WorkspaceOpened(Ok(open_workspace(&second))));
        assert!(before_switch.is_cancelled());
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
    fn quick_open_excludes_ignored_files_when_explorer_shows_them_across_refresh_and_reopen() {
        let project = tempdir().unwrap();
        let mut app = StruktApp::default();
        app.workspace = Some(workspace_state(project.path()));
        app.explorer_options.show_ignored = true;
        app.files = vec![
            file_entry("visible.txt"),
            ignored_file_entry("ignored-secret.txt"),
        ];

        let _ = app.update(Message::ToggleQuickOpen);
        assert_eq!(
            app.quick_open_results
                .iter()
                .map(|candidate| candidate.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![std::path::Path::new("visible.txt")]
        );

        let _ = app.update(Message::ToggleQuickOpen);
        let _ = app.update(Message::ToggleHiddenFiles);
        let mut refreshed = discovery(&["current.txt"]);
        refreshed
            .entries
            .push(ignored_file_entry("ignored-current.txt"));
        let _ = app.update(Message::FilesRefreshed {
            generation: 1,
            result: Ok(refreshed),
        });
        let _ = app.update(Message::ToggleQuickOpen);

        assert_eq!(
            app.quick_open_results
                .iter()
                .map(|candidate| candidate.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![std::path::Path::new("current.txt")]
        );
    }

    #[test]
    fn quick_open_filters_real_heavy_trees_across_query_refresh_and_reopen() {
        let project = tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("target/debug")).unwrap();
        std::fs::write(
            project.path().join("target/debug/generated.rs"),
            "generated",
        )
        .unwrap();
        std::fs::create_dir_all(project.path().join("node_modules/package")).unwrap();
        std::fs::write(
            project.path().join("node_modules/package/index.js"),
            "package",
        )
        .unwrap();
        std::fs::write(project.path().join("visible.rs"), "visible").unwrap();

        let app_data = tempdir().unwrap();
        let store = WorkspaceStore::at(app_data.path());
        let mut state = workspace_state(project.path());
        state.explorer.show_ignored = true;
        store.save(&state).unwrap();
        let opened =
            crate::workspace::open_workspace_with_store(project.path().to_path_buf(), &store)
                .unwrap();
        let root = opened.state.root.path().to_path_buf();
        let mut app = StruktApp::default();
        let _ = app.update(Message::WorkspaceOpened(Ok(opened)));

        for path in [
            std::path::Path::new("target/debug/generated.rs"),
            std::path::Path::new("node_modules/package/index.js"),
        ] {
            assert!(
                app.files
                    .iter()
                    .find(|entry| entry.relative_path == path)
                    .expect("explorer should contain the heavy entry")
                    .ignored
            );
        }

        let _ = app.update(Message::ToggleQuickOpen);
        assert_eq!(
            app.quick_open_results
                .iter()
                .map(|entry| entry.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![std::path::Path::new("visible.rs")]
        );
        let _ = app.update(Message::QuickOpenChanged("generated".to_owned()));
        assert!(app.quick_open_results.is_empty());

        let report = strukt_fs::discover_report_for_root(
            &WorkspaceRoot::open(project.path()).unwrap(),
            DiscoveryOptions {
                show_ignored: true,
                ..DiscoveryOptions::default()
            },
        )
        .unwrap();
        let _ = app.update(Message::ToggleQuickOpen);
        let _ = app.update(Message::FilesRefreshed {
            generation: 2,
            result: Ok(report.clone()),
        });
        let _ = app.update(Message::ToggleQuickOpen);
        assert_eq!(
            app.quick_open_results
                .iter()
                .map(|entry| entry.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![std::path::Path::new("visible.rs")]
        );

        let _ = app.update(Message::ToggleQuickOpenIgnored);
        let _ = app.update(Message::QuickOpenFilesLoaded {
            generation: 3,
            workspace_root: root,
            filesystem_revision: 2,
            result: Ok(report.entries),
        });
        assert!(
            app.quick_open_results
                .iter()
                .any(|entry| entry.relative_path
                    == std::path::Path::new("target/debug/generated.rs"))
        );
        assert!(app.quick_open_results.iter().any(|entry| {
            entry.relative_path == std::path::Path::new("node_modules/package/index.js")
        }));
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
            result: Ok(discovery(&["opening-snapshot.rs"])),
        });
        let _ = app.update(Message::FilesRefreshed {
            generation: 3,
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
