use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Background, Border, Color, Element, Fill, Length};
use strukt_core::CapabilityId;
use strukt_fs::{FileEntry, FileKind};
use strukt_shell::Activity;
use strukt_theme::{Rgb, ThemeTokens};

use crate::app::{ExplorerDialog, Message, StruktApp};

fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

fn panel_style(tokens: ThemeTokens, background: Rgb) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color(background))),
        text_color: Some(color(tokens.text_primary)),
        border: Border {
            color: color(tokens.border),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn activity_button(label: &'static str, activity: Activity) -> Element<'static, Message> {
    button(text(label))
        .width(Fill)
        .on_press(Message::SelectActivity(activity))
        .into()
}

pub fn view(app: &StruktApp) -> Element<'_, Message> {
    let tokens = ThemeTokens::builtin(app.shell.theme_mode);
    let body = row![
        activity_rail(tokens),
        explorer(app, tokens),
        primary_canvas(app, tokens),
        context_panel(app, tokens),
    ]
    .height(Fill);

    container(column![header(app, tokens), body, drawer(app, tokens)].height(Fill))
        .width(Fill)
        .height(Fill)
        .style(panel_style(tokens, tokens.canvas))
        .into()
}

fn header(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    let workspace_label = app.workspace.as_ref().map_or_else(
        || "No folder open".to_owned(),
        |workspace| {
            format!(
                "{}  ·  {}",
                workspace.root.display_name(),
                workspace.root.path().display()
            )
        },
    );
    let open_folder = button("Open Folder…")
        .on_press_maybe((!app.folder_picker_in_flight()).then_some(Message::OpenFolder));

    container(
        row![
            text("strukt").size(16),
            text(format!("  /  {workspace_label}")).size(13),
            Space::new().width(Fill),
            open_folder,
            button("Toggle theme").on_press(Message::ToggleTheme),
            button("Context").on_press(Message::ToggleContext),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Fill)
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn activity_rail(tokens: ThemeTokens) -> Element<'static, Message> {
    container(
        column![
            activity_button("Files", Activity::Files),
            activity_button("Search", Activity::Search),
            activity_button("Git", Activity::SourceControl),
            activity_button("Sessions", Activity::Sessions),
            activity_button("Tasks", Activity::Tasks),
            activity_button("Connect", Activity::Connections),
            activity_button("Extend", Activity::Extensions),
            Space::new().height(Fill),
            activity_button("Settings", Activity::Settings),
        ]
        .spacing(6),
    )
    .padding(6)
    .width(Length::Fixed(92.0))
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn explorer(app: &StruktApp, tokens: ThemeTokens) -> Element<'_, Message> {
    if !app.shell.explorer_visible {
        return container(Space::new()).width(Length::Shrink).into();
    }

    let has_workspace = app.workspace.is_some();
    let operation_ready = has_workspace
        && app.explorer_dialog == ExplorerDialog::None
        && !app.file_operation_in_flight();
    let selection_ready = operation_ready && app.selected_entry.is_some();
    let controls = row![
        button(if app.explorer_options.show_hidden {
            "Hide hidden"
        } else {
            "Show hidden"
        })
        .on_press_maybe(operation_ready.then_some(Message::ToggleHiddenFiles)),
        button(if app.explorer_options.show_ignored {
            "Hide ignored"
        } else {
            "Show ignored"
        })
        .on_press_maybe(operation_ready.then_some(Message::ToggleIgnoredFiles)),
    ]
    .spacing(6);
    let operations = column![
        row![
            button("New File").on_press_maybe(operation_ready.then_some(Message::BeginCreateFile)),
            button("New Folder")
                .on_press_maybe(operation_ready.then_some(Message::BeginCreateDirectory)),
        ]
        .spacing(6),
        row![
            button("Rename").on_press_maybe(selection_ready.then_some(Message::BeginRename)),
            button("Duplicate").on_press_maybe(selection_ready.then_some(Message::BeginDuplicate)),
            button("Trash").on_press_maybe(selection_ready.then_some(Message::BeginTrash)),
        ]
        .spacing(6),
    ]
    .spacing(6);

    let mut file_rows = column![].spacing(4);
    if !has_workspace {
        file_rows = file_rows.push(text("Open a folder to browse real files."));
    } else if app.files.is_empty() {
        file_rows = file_rows.push(text("This workspace has no visible files."));
    } else {
        for entry in &app.files {
            let selected = app.selected_entry.as_ref() == Some(&entry.relative_path);
            let row_label = if selected {
                format!("● {}", file_entry_label(entry))
            } else {
                file_entry_label(entry)
            };
            let label = text(row_label);
            let label = if entry.ignored {
                label.color(color(tokens.text_muted))
            } else {
                label
            };
            file_rows = file_rows.push(button(label).width(Fill).on_press_maybe(
                operation_ready.then(|| Message::SelectExplorerEntry(entry.relative_path.clone())),
            ));
        }
    }

    let mut notices = column![].spacing(4);
    if let Some(error) = &app.workspace_error {
        notices = notices.push(text(format!("Error: {error}")));
    }
    for warning in &app.file_warnings {
        notices = notices.push(text(format!("Warning: {warning}")));
    }
    if app.filesystem_truncated {
        notices = notices.push(text("File list truncated"));
    }

    container(
        column![
            row![
                text("EXPLORER"),
                Space::new().width(Fill),
                button("×").on_press(Message::ToggleExplorer),
            ],
            button("Open Folder…")
                .on_press_maybe((!app.folder_picker_in_flight()).then_some(Message::OpenFolder)),
            controls,
            operations,
            explorer_dialog(app),
            notices,
            scrollable(file_rows),
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fixed(235.0))
    .style(panel_style(tokens, tokens.panel))
    .into()
}

pub(crate) fn file_entry_label(entry: &FileEntry) -> String {
    let indent = "  ".repeat(entry.depth);
    let name = entry.relative_path.file_name().map_or_else(
        || entry.relative_path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let marker = match entry.kind {
        FileKind::Directory => "▸ ",
        FileKind::File => "",
        FileKind::Symlink => "↗ ",
    };
    format!("{indent}{marker}{name}")
}

fn explorer_dialog(app: &StruktApp) -> Element<'_, Message> {
    let in_flight = app.file_operation_in_flight();
    let submit = (!in_flight).then_some(Message::SubmitExplorerDialog);
    let cancel = (!in_flight).then_some(Message::CancelExplorerDialog);
    let pending = in_flight.then_some(text("Working…"));

    let content = match &app.explorer_dialog {
        ExplorerDialog::None => return Space::new().height(Length::Shrink).into(),
        ExplorerDialog::CreateFile(path) => column![
            text("Create file"),
            text_input("relative/path.txt", path)
                .on_input_maybe((!in_flight).then_some(Message::ExplorerDialogInput))
                .on_submit_maybe(submit.clone()),
            row![
                button("Create").on_press_maybe(submit),
                button("Cancel").on_press_maybe(cancel)
            ]
            .spacing(6),
        ],
        ExplorerDialog::CreateDirectory(path) => column![
            text("Create folder"),
            text_input("relative/folder", path)
                .on_input_maybe((!in_flight).then_some(Message::ExplorerDialogInput))
                .on_submit_maybe(submit.clone()),
            row![
                button("Create").on_press_maybe(submit),
                button("Cancel").on_press_maybe(cancel)
            ]
            .spacing(6),
        ],
        ExplorerDialog::Rename { from, to } => column![
            text(format!("Rename {}", from.display())),
            text_input("new relative path", to)
                .on_input_maybe((!in_flight).then_some(Message::ExplorerDialogInput))
                .on_submit_maybe(submit.clone()),
            row![
                button("Rename").on_press_maybe(submit),
                button("Cancel").on_press_maybe(cancel)
            ]
            .spacing(6),
        ],
        ExplorerDialog::Duplicate { from, to } => column![
            text(format!("Duplicate {}", from.display())),
            text_input("copy relative path", to)
                .on_input_maybe((!in_flight).then_some(Message::ExplorerDialogInput))
                .on_submit_maybe(submit.clone()),
            row![
                button("Duplicate").on_press_maybe(submit),
                button("Cancel").on_press_maybe(cancel)
            ]
            .spacing(6),
        ],
        ExplorerDialog::ConfirmTrash(path) => column![
            text(format!("Move {} to Trash?", path.display())),
            button("Move to Trash").width(Fill).on_press_maybe(submit),
            button("Delete Permanently…")
                .width(Fill)
                .on_press_maybe((!in_flight).then_some(Message::BeginPermanentDelete)),
            button("Cancel").width(Fill).on_press_maybe(cancel),
        ],
        ExplorerDialog::ConfirmPermanentDelete(path) => column![
            text(format!(
                "Permanently delete {}? This cannot be undone.",
                path.display()
            )),
            button("Delete Permanently")
                .width(Fill)
                .on_press_maybe(submit),
            button("Cancel").width(Fill).on_press_maybe(cancel),
        ],
    }
    .spacing(6);

    match pending {
        Some(pending) => column![content, pending].spacing(4).into(),
        None => content.into(),
    }
}

fn primary_canvas(app: &StruktApp, tokens: ThemeTokens) -> Element<'_, Message> {
    let content: Element<'_, Message> = if app.workspace.is_none() {
        welcome_canvas(app).into()
    } else if app.quick_open_visible {
        quick_open_canvas(app).into()
    } else if app.shell.active_activity == Activity::Search {
        search_canvas(app).into()
    } else {
        column![
            text("Workspace shell").size(22),
            text("The primary canvas adapts to files, terminals, logs, and tools."),
            Space::new().height(Fill),
            text("Open a file or choose an activity to begin."),
        ]
        .spacing(10)
        .into()
    };

    container(content)
        .padding(20)
        .width(Fill)
        .height(Fill)
        .style(panel_style(tokens, tokens.canvas))
        .into()
}

fn welcome_canvas(app: &StruktApp) -> iced::widget::Column<'_, Message> {
    let mut recent = column![].spacing(8);
    for path in &app.recent_workspaces {
        let actions_enabled = !app.folder_picker_in_flight();
        let mut actions = row![
            text(path.display().to_string()).width(Fill),
            button("Retry").on_press_maybe(
                actions_enabled.then(|| Message::RetryRecentWorkspace(path.clone()))
            ),
            button("Remove").on_press_maybe(
                actions_enabled.then(|| Message::RemoveRecentWorkspace(path.clone()))
            ),
        ]
        .spacing(6);
        if !path.is_dir() {
            actions = actions.push(button("Locate").on_press_maybe(
                actions_enabled.then(|| Message::LocateRecentWorkspace(path.clone())),
            ));
        }
        recent = recent.push(actions);
    }
    column![
        text("Open a local workspace").size(22),
        text("Folders stay local and strukt does not add repository metadata."),
        button("Open Folder…")
            .on_press_maybe((!app.folder_picker_in_flight()).then_some(Message::OpenFolder)),
        text("Recent workspaces").size(16),
        recent,
    ]
    .spacing(12)
}

fn quick_open_canvas(app: &StruktApp) -> iced::widget::Column<'_, Message> {
    let mut results = column![].spacing(4);
    for candidate in &app.quick_open_results {
        results = results.push(
            button(text(candidate.relative_path.display().to_string()))
                .width(Fill)
                .on_press(Message::QuickOpenSelected(candidate.relative_path.clone())),
        );
    }
    column![
        row![
            text("Quick Open").size(20),
            Space::new().width(Fill),
            button("Close").on_press(Message::ToggleQuickOpen),
        ],
        text_input("Quick Open", &app.quick_open_query)
            .on_input(Message::QuickOpenChanged)
            .on_submit(Message::ToggleQuickOpen),
        button(if app.quick_open_include_ignored {
            "Exclude ignored files"
        } else {
            "Include ignored files"
        })
        .on_press(Message::ToggleQuickOpenIgnored),
        scrollable(results).height(Fill),
    ]
    .spacing(10)
}

fn search_canvas(app: &StruktApp) -> iced::widget::Column<'_, Message> {
    let mut results = column![].spacing(6);
    for result in &app.search_results.matches {
        results = results.push(text(format!(
            "{}:{}  {}",
            result.relative_path.display(),
            result.line,
            result.preview
        )));
    }
    if app.search_results.truncated {
        results = results.push(text("Results truncated"));
    }
    column![
        text("Workspace Search").size(20),
        text_input("Search files", &app.search_query).on_input(Message::SearchChanged),
        button(if app.search_include_ignored {
            "Use ignore files"
        } else {
            "Include ignored files"
        })
        .on_press(Message::ToggleSearchIgnored),
        scrollable(results).height(Fill),
    ]
    .spacing(10)
}

fn context_panel(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.context_visible {
        return container(Space::new()).width(Length::Shrink).into();
    }

    let ai_status = if app.capabilities.is_enabled(CapabilityId::AI) {
        "AI · WORKSPACE CONTEXT"
    } else {
        "WORKSPACE CONTEXT"
    };

    container(
        column![
            text(ai_status),
            text("Current workspace"),
            text(format!(
                "{} capabilities enabled",
                app.capabilities.enabled_count()
            )),
            Space::new().height(Fill),
            button("Hide context").on_press(Message::ToggleContext),
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fixed(250.0))
    .style(panel_style(tokens, tokens.panel))
    .into()
}

fn drawer(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.drawer_visible {
        return button("Open terminal drawer")
            .on_press(Message::ToggleDrawer)
            .width(Fill)
            .into();
    }

    container(
        row![
            text("TERMINAL  ·  local shell foundation"),
            Space::new().width(Fill),
            button("Close").on_press(Message::ToggleDrawer),
        ]
        .spacing(8),
    )
    .padding(10)
    .height(Length::Fixed(130.0))
    .style(panel_style(tokens, tokens.terminal_background))
    .into()
}
