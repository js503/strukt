use iced::widget::{
    Space, button, column, container, pick_list, row, scrollable, text, text_editor, text_input,
};
use iced::{Background, Border, Color, Element, Fill, Length};
use strukt_core::CapabilityId;
use strukt_editor::{CloseDecision, DocumentStatus, FindQuery, GrammarRegistry, OpenDisposition};
use strukt_fs::{FileEntry, FileKind};
use strukt_shell::Activity;
use strukt_terminal::{LayoutNode, PaneState, SplitAxis, TerminalPaneId};
use strukt_theme::{Rgb, ThemeTokens};

use crate::app::{DocumentNotice, ExplorerDialog, Message, StruktApp};
use crate::language::{DiagnosticSeverity, LanguageState, ProblemFilter};
use crate::terminal_widget::TerminalWidget;

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
            button(if app.language.problems_visible() {
                "Hide problems"
            } else {
                "Problems"
            })
            .on_press(Message::ToggleProblems),
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
    } else if app.terminal_expanded {
        app.terminal.workspace().active_tab().map_or_else(
            || text("Create a terminal to use the expanded terminal canvas").into(),
            |tab| {
                column![
                    text(format!("{} · local terminal workspace", tab.name())).size(14),
                    terminal_layout(app, tab.root(), tab.focused_pane(), tokens),
                ]
                .spacing(6)
                .into()
            },
        )
    } else if app.quick_open_visible {
        quick_open_canvas(app).into()
    } else if app.shell.active_activity == Activity::Search {
        search_canvas(app).into()
    } else if app
        .editor
        .as_ref()
        .and_then(strukt_editor::EditorWorkspace::active_document_id)
        .is_some()
        || app.document_notice.is_some()
    {
        editor_canvas(app)
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

#[expect(
    clippy::too_many_lines,
    reason = "the native editor canvas keeps tab, toolbar, find, status, and close-dialog composition together"
)]
fn editor_canvas(app: &StruktApp) -> Element<'_, Message> {
    if let Some(notice) = &app.document_notice {
        return document_notice_canvas(notice).into();
    }
    let Some(workspace) = &app.editor else {
        return text("No editor workspace").into();
    };
    let state = workspace.view_state();
    let Some(active_id) = state.active else {
        return text("Open a file from Explorer or Quick Open.").into();
    };
    let Some(document) = workspace.document(active_id) else {
        return text("The active document is unavailable.").into();
    };
    let Some(content) = app.editor_surfaces.content(active_id) else {
        return text("The native editor surface is unavailable.").into();
    };

    let mut tabs = row![].spacing(4);
    for tab in state.tabs {
        let status = workspace
            .document(tab.id)
            .map_or("", |document| match document.status() {
                DocumentStatus::Clean => "",
                DocumentStatus::Dirty => " ●",
                DocumentStatus::Conflict { .. } => " !",
                DocumentStatus::Missing => " ?",
            });
        let preview = if tab.pinned { "" } else { " (preview)" };
        tabs = tabs.push(
            row![
                button(text(format!("{}{}{}", tab.path.as_str(), preview, status)))
                    .on_press(Message::SelectDocument(tab.id)),
                button(if tab.pinned { "×" } else { "Pin" }).on_press(if tab.pinned {
                    Message::CloseDocument(tab.id)
                } else {
                    Message::PinDocument(tab.id)
                }),
            ]
            .spacing(2),
        );
    }

    let language_override = app.editor_language_overrides.get(&active_id);
    let grammar = GrammarRegistry::detect(
        std::path::Path::new(document.path().as_str()),
        language_override.map(String::as_str),
    );
    let highlighter_theme = match app.shell.theme_mode {
        strukt_theme::ThemeMode::Light => iced::highlighter::Theme::InspiredGitHub,
        strukt_theme::ThemeMode::Dark => iced::highlighter::Theme::Base16Ocean,
    };
    let editor = text_editor(content)
        .height(Fill)
        .highlight(grammar.iced_token, highlighter_theme);
    let editor = if document.is_read_only() {
        editor
    } else {
        editor.on_action(move |action| Message::EditorAction {
            id: active_id,
            action,
        })
    };

    let mut controls = row![
        button("Save").on_press_maybe((!document.is_read_only()).then_some(
            Message::SaveDocument {
                id: active_id,
                mode: strukt_fs::SaveMode::IfUnchanged,
            }
        )),
        button("Undo")
            .on_press_maybe((!document.is_read_only()).then_some(Message::UndoDocument(active_id))),
        button("Redo")
            .on_press_maybe((!document.is_read_only()).then_some(Message::RedoDocument(active_id))),
        button("Find").on_press(Message::ToggleEditorFind),
        button("Complete").on_press(Message::RequestLanguageFeature(
            strukt_language::FeatureRequestKind::Completion,
        )),
        button("Hover").on_press(Message::RequestLanguageFeature(
            strukt_language::FeatureRequestKind::Hover,
        )),
        button("Definition").on_press(Message::RequestLanguageFeature(
            strukt_language::FeatureRequestKind::Definition,
        )),
        button("Back").on_press_maybe(
            (!app.language_navigation_back.is_empty()).then_some(Message::NavigateLanguageBack),
        ),
        pick_list(
            std::iter::once("auto")
                .chain(GrammarRegistry::all().iter().map(|grammar| grammar.id))
                .collect::<Vec<_>>(),
            Some(language_override.map_or("auto", String::as_str)),
            move |language| Message::SetLanguageOverride {
                id: active_id,
                language: (language != "auto").then(|| language.to_owned()),
            },
        ),
    ]
    .spacing(6);
    if document.is_read_only() {
        controls = controls.push(button("Open full file").on_press(Message::OpenDocument {
            path: std::path::PathBuf::from(document.path().as_str()),
            disposition: OpenDisposition::Pinned,
            force_full: true,
        }));
    }

    let status = format!(
        "{}  ·  {} lines  ·  {}{}",
        grammar.display_name,
        content.line_count(),
        if document.is_read_only() {
            "read-only"
        } else {
            "editable"
        },
        if document.is_recovered() {
            "  ·  recovered"
        } else {
            ""
        },
    );
    let mut body = column![tabs, controls].spacing(8);
    if let Some((document_id, _, items)) = app.language.completion()
        && document_id == active_id
    {
        let mut menu = column![text("COMPLETIONS").size(12)].spacing(3);
        for (index, item) in items.iter().take(12).enumerate() {
            menu = menu.push(
                button(text(item.label().to_owned()).size(12))
                    .on_press(Message::ApplyCompletion(index)),
            );
        }
        menu = menu.push(button("Dismiss").on_press(Message::DismissLanguageFeatures));
        body = body.push(container(menu).padding(8));
    }
    if let Some(hover) = app.language.hover_text() {
        body = body.push(
            container(
                column![
                    text("HOVER").size(12),
                    text(hover.to_owned()).size(12),
                    button("Dismiss").on_press(Message::DismissLanguageFeatures),
                ]
                .spacing(4),
            )
            .padding(8),
        );
    }
    if !app.language.definitions().is_empty() {
        let mut definitions = column![text("DEFINITIONS").size(12)].spacing(3);
        for (index, location) in app.language.definitions().iter().enumerate() {
            definitions = definitions.push(
                button(text(location.label()).size(12)).on_press(Message::OpenDefinition(index)),
            );
        }
        definitions =
            definitions.push(button("Dismiss").on_press(Message::DismissLanguageFeatures));
        body = body.push(container(definitions).padding(8));
    }
    if let DocumentStatus::Conflict { disk_text, .. } = document.status() {
        body = body.push(
            column![
                text("This file changed on disk. Your local edits are preserved."),
                row![
                    button("Reload from disk").on_press(Message::ReloadDocumentFromDisk(active_id)),
                    button("Keep editing").on_press(Message::KeepEditingDocument(active_id)),
                    button("Force save").on_press(Message::SaveDocument {
                        id: active_id,
                        mode: strukt_fs::SaveMode::Force,
                    }),
                ]
                .spacing(6),
                text(format!("Disk version:\n{disk_text}")),
            ]
            .spacing(6),
        );
    }
    if app.editor_find_visible {
        let match_label = if app.editor_find_query.is_empty() {
            "0 matches".to_owned()
        } else {
            FindQuery::new(&app.editor_find_query, app.editor_find_options).map_or_else(
                |error| error.to_string(),
                |query| {
                    format!(
                        "{} matches",
                        query.find_all(&document.text()).matches().len()
                    )
                },
            )
        };
        body = body.push(
            row![
                text_input("Find", &app.editor_find_query).on_input(Message::EditorFindChanged),
                text_input("Replace", &app.editor_replace_text)
                    .on_input(Message::EditorReplaceChanged),
                button(if app.editor_find_options.case_sensitive {
                    "Aa ✓"
                } else {
                    "Aa"
                })
                .on_press(Message::ToggleFindCase),
                button(if app.editor_find_options.whole_word {
                    "Word ✓"
                } else {
                    "Word"
                })
                .on_press(Message::ToggleFindWholeWord),
                button(if app.editor_find_options.regex {
                    ".* ✓"
                } else {
                    ".*"
                })
                .on_press(Message::ToggleFindRegex),
                button("Replace all").on_press_maybe(
                    (!document.is_read_only() && !app.editor_find_query.is_empty())
                        .then_some(Message::ReplaceAll(active_id)),
                ),
                text(match_label),
            ]
            .spacing(4),
        );
    }
    body = body.push(editor).push(text(status).size(12));
    if let Some(error) = &app.editor_error {
        body = body.push(text(format!("Editor error: {error}")));
    }
    if app.pending_close == Some(active_id) {
        body = body.push(
            row![
                text("Save changes before closing?"),
                button("Save").on_press(Message::ResolveDocumentClose {
                    id: active_id,
                    decision: CloseDecision::Save,
                }),
                button("Discard").on_press(Message::ResolveDocumentClose {
                    id: active_id,
                    decision: CloseDecision::Discard,
                }),
                button("Cancel").on_press(Message::ResolveDocumentClose {
                    id: active_id,
                    decision: CloseDecision::Cancel,
                }),
            ]
            .spacing(6),
        );
    }
    body.into()
}

fn document_notice_canvas(notice: &DocumentNotice) -> iced::widget::Column<'_, Message> {
    match notice {
        DocumentNotice::Binary { path, size } => column![
            text("Binary file").size(22),
            text(path.display().to_string()),
            text(format!("{size} bytes")),
            text("Binary content is not opened as text."),
        ]
        .spacing(8),
        DocumentNotice::InvalidUtf8 { path, size } => column![
            text("Unsupported text encoding").size(22),
            text(path.display().to_string()),
            text(format!("{size} bytes")),
            text("Public alpha editing requires valid UTF-8."),
        ]
        .spacing(8),
    }
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
        if recent_workspace_offers_locate(path) {
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

pub(crate) const fn recent_workspace_offers_locate(_path: &std::path::Path) -> bool {
    true
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
            .id(quick_open_input_id())
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

pub(crate) fn quick_open_input_id() -> iced::widget::Id {
    iced::widget::Id::new("strukt.quick-open.input")
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

#[expect(
    clippy::too_many_lines,
    reason = "context, language status, and Problems presentation remain colocated for a coherent panel"
)]
fn context_panel(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.context_visible && !app.language.problems_visible() {
        return container(Space::new()).width(Length::Shrink).into();
    }

    let ai_status = if app.capabilities.is_enabled(CapabilityId::AI) {
        "AI · WORKSPACE CONTEXT"
    } else {
        "WORKSPACE CONTEXT"
    };

    let counts = app.language.problem_counts();
    let mut content = column![].spacing(8);
    if app.language.problems_visible() {
        content = content.push(
            row![
                text("PROBLEMS").size(12),
                Space::new().width(Fill),
                text(format!("×{}  ⚠{}", counts.errors, counts.warnings)).size(12),
                button("Hide").on_press(Message::ToggleProblems),
            ]
            .align_y(iced::Alignment::Center)
            .spacing(5),
        );
        content = content.push(
            row![
                button("All").on_press(Message::SetProblemFilter(ProblemFilter::All)),
                button("Errors").on_press(Message::SetProblemFilter(ProblemFilter::Errors)),
                button("Warnings").on_press(Message::SetProblemFilter(ProblemFilter::Warnings)),
            ]
            .spacing(4),
        );
        let mut problems = column![].spacing(5);
        let visible_problems = app.language.visible_problems();
        for problem in &visible_problems {
            let severity_color = match problem.severity() {
                DiagnosticSeverity::Error => tokens.diagnostic_error,
                DiagnosticSeverity::Warning => tokens.diagnostic_warning,
                DiagnosticSeverity::Information => tokens.diagnostic_information,
                DiagnosticSeverity::Hint => tokens.diagnostic_hint,
            };
            let source = problem
                .source()
                .map_or_else(String::new, |source| format!(" · {source}"));
            let label = format!(
                "{}:{}:{} · {}{}",
                problem.path().display(),
                problem.line() + 1,
                problem.character() + 1,
                problem.message(),
                source,
            );
            problems = problems.push(
                button(text(label).size(12).color(color(severity_color))).on_press(
                    Message::OpenProblem {
                        id: problem.document_id(),
                        line: problem.line(),
                        character: problem.character(),
                    },
                ),
            );
        }
        if visible_problems.is_empty() {
            problems = problems.push(text("No problems in synchronized files").size(12));
        }
        content = content.push(scrollable(problems).height(Fill));
    }
    if app.shell.context_visible {
        content = content
            .push(text(ai_status))
            .push(text("LANGUAGE SERVERS").size(12));
        let states = app.language.server_states();
        if states.is_empty() {
            content = content.push(text("Open a code file to discover a server").size(12));
        }
        for (language, state) in states {
            let state_label = match state {
                LanguageState::Stopped => "stopped",
                LanguageState::Discovering => "discovering",
                LanguageState::Unavailable => "not installed",
                LanguageState::ApprovalRequired => "approval required",
                LanguageState::Disabled => "disabled",
                LanguageState::Starting => "starting",
                LanguageState::Ready => "ready",
                LanguageState::Failed => "failed",
            };
            let mut server_row = row![text(format!("{language} · {state_label}")).size(12)]
                .spacing(4)
                .align_y(iced::Alignment::Center);
            match state {
                LanguageState::ApprovalRequired => {
                    server_row = server_row
                        .push(
                            button("Approve").on_press(Message::ApproveLanguage(language.clone())),
                        )
                        .push(button("Deny").on_press(Message::DenyLanguage(language.clone())));
                    if let Some(command) = app.language.approval_command(&language) {
                        content = content.push(text(command).size(11));
                    }
                }
                LanguageState::Unavailable | LanguageState::Disabled | LanguageState::Failed => {
                    server_row = server_row
                        .push(button("Retry").on_press(Message::RetryLanguage(language.clone())));
                }
                LanguageState::Stopped
                | LanguageState::Discovering
                | LanguageState::Starting
                | LanguageState::Ready => {}
            }
            content = content.push(server_row);
        }
        content = content
            .push(text(format!(
                "{} capabilities enabled",
                app.capabilities.enabled_count()
            )))
            .push(button("Hide context").on_press(Message::ToggleContext));
    }

    container(content)
        .padding(10)
        .width(Length::Fixed(250.0))
        .style(panel_style(tokens, tokens.panel))
        .into()
}

#[expect(
    clippy::too_many_lines,
    reason = "terminal drawer chrome keeps its empty, active, and confirmation states together"
)]
fn drawer(app: &StruktApp, tokens: ThemeTokens) -> Element<'static, Message> {
    if !app.shell.drawer_visible {
        return button("Open terminal drawer")
            .on_press(Message::ToggleDrawer)
            .width(Fill)
            .into();
    }

    let enabled = app.capabilities.is_enabled(CapabilityId::TERMINAL);
    let has_workspace = app.workspace.is_some();
    let mut tabs = row![].spacing(4);
    for tab in app.terminal.workspace().tabs() {
        tabs = tabs.push(
            button(text(tab.name().to_owned()).size(12))
                .on_press(Message::ActivateTerminalTab(tab.id())),
        );
    }

    let controls = row![
        text("TERMINAL  ·  LOCAL").size(12),
        tabs,
        Space::new().width(Fill),
        button("New").on_press_maybe((enabled && has_workspace).then_some(Message::NewTerminal)),
        button("Split →").on_press_maybe(
            (enabled && app.terminal.workspace().active_tab().is_some())
                .then_some(Message::SplitTerminal(SplitAxis::Vertical)),
        ),
        button("Split ↓").on_press_maybe(
            (enabled && app.terminal.workspace().active_tab().is_some())
                .then_some(Message::SplitTerminal(SplitAxis::Horizontal)),
        ),
        button(if app.terminal_expanded {
            "Collapse"
        } else {
            "Expand"
        })
        .on_press(Message::ToggleTerminalExpanded),
        button("Hide").on_press(Message::ToggleDrawer),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(6);

    let mut content = column![controls].spacing(6);
    if let Some(tab) = app.terminal.workspace().active_tab() {
        content = content.push(
            row![
                text_input("Terminal name", &app.terminal_tab_name)
                    .on_input(Message::TerminalTabNameChanged)
                    .on_submit(Message::RenameTerminalTab)
                    .width(Length::Fixed(180.0)),
                button("Rename").on_press(Message::RenameTerminalTab),
                text(format!("{} pane workspace", count_layout_panes(tab.root()))).size(12),
            ]
            .spacing(6),
        );
        if !app.terminal_expanded {
            content = content.push(terminal_layout(app, tab.root(), tab.focused_pane(), tokens));
        }
    } else {
        content = content.push(
            container(
                column![
                    text(if has_workspace {
                        "No local terminal yet"
                    } else {
                        "Open a workspace before starting a terminal"
                    }),
                    text("Processes start only after an explicit New command").size(12),
                ]
                .spacing(4),
            )
            .padding(12)
            .width(Fill)
            .height(Fill),
        );
    }

    if let Some(pane) = app.pending_terminal_close {
        content = content.push(
            row![
                text(format!(
                    "Terminal {} is still running. Stop and close it?",
                    pane.value()
                )),
                button("Stop & close").on_press(Message::ResolveCloseTerminal(true)),
                button("Keep open").on_press(Message::ResolveCloseTerminal(false)),
            ]
            .spacing(6),
        );
    }
    if let Some((_, pasted_text)) = &app.pending_terminal_paste {
        content = content.push(
            row![
                text(format!(
                    "Paste {} bytes into this terminal?",
                    pasted_text.len()
                )),
                button("Paste").on_press(Message::ResolveTerminalPaste(true)),
                button("Cancel").on_press(Message::ResolveTerminalPaste(false)),
            ]
            .spacing(6),
        );
    }
    if let Some(target) = &app.pending_terminal_link {
        content = content.push(
            column![
                text("Open this exact terminal link?").size(12),
                text(target.clone())
                    .size(12)
                    .color(color(tokens.terminal_link)),
                row![
                    button("Open link").on_press(Message::ResolveTerminalLink(true)),
                    button("Cancel").on_press(Message::ResolveTerminalLink(false)),
                ]
                .spacing(6),
            ]
            .spacing(4),
        );
    }
    if let Some(error) = &app.terminal_error {
        content = content.push(text(format!("Terminal: {error}")).size(12));
    }

    container(content)
        .padding(8)
        .height(Length::Fixed(if app.terminal_expanded {
            150.0
        } else {
            330.0
        }))
        .style(panel_style(tokens, tokens.terminal_background))
        .into()
}

fn terminal_layout(
    app: &StruktApp,
    node: &LayoutNode,
    focused: TerminalPaneId,
    tokens: ThemeTokens,
) -> Element<'static, Message> {
    match node {
        LayoutNode::Pane(pane) => terminal_pane(app, *pane, *pane == focused, tokens),
        LayoutNode::Split {
            axis,
            ratio_basis_points,
            first,
            second,
        } => {
            let first_portion = (*ratio_basis_points).max(1);
            let second_portion = 10_000_u16.saturating_sub(*ratio_basis_points).max(1);
            let first = terminal_layout(app, first, focused, tokens);
            let second = terminal_layout(app, second, focused, tokens);
            match axis {
                SplitAxis::Vertical => row![
                    container(first).width(Length::FillPortion(first_portion)),
                    container(second).width(Length::FillPortion(second_portion)),
                ]
                .spacing(4)
                .height(Fill)
                .into(),
                SplitAxis::Horizontal => column![
                    container(first).height(Length::FillPortion(first_portion)),
                    container(second).height(Length::FillPortion(second_portion)),
                ]
                .spacing(4)
                .width(Fill)
                .into(),
            }
        }
    }
}

fn terminal_pane(
    app: &StruktApp,
    pane_id: TerminalPaneId,
    focused: bool,
    tokens: ThemeTokens,
) -> Element<'static, Message> {
    let Some(pane) = app.terminal.workspace().pane(pane_id) else {
        return container(text("Terminal pane unavailable")).into();
    };
    let (state_label, state_color) = match pane.state() {
        PaneState::Stopped => ("stopped".to_owned(), tokens.terminal_exited),
        PaneState::Starting => ("starting".to_owned(), tokens.status_warning),
        PaneState::Running => ("running".to_owned(), tokens.status_success),
        PaneState::Exited { code } => (format!("exited {code:?}"), tokens.terminal_exited),
        PaneState::Failed { message } => (format!("failed: {message}"), tokens.editor_conflict),
        PaneState::Backpressured => ("backpressure".to_owned(), tokens.terminal_backpressure),
    };
    let can_start = !matches!(pane.state(), PaneState::Starting);
    let cwd = pane.working_directory().display().to_string();
    let sustained_output = app
        .terminal
        .health(pane_id)
        .is_some_and(|health| health.sustained_output);
    let pane_title = app
        .terminal
        .snapshot(pane_id)
        .and_then(|snapshot| snapshot.title().map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    let header = row![
        button(if focused { "●" } else { "○" }).on_press(Message::TerminalWidget(
            crate::terminal_widget::TerminalWidgetEvent::Focus(pane_id),
        )),
        text(pane_title).size(11),
        text(format!("local · {cwd}")).size(11),
        text(state_label).size(11).color(color(state_color)),
        text(if sustained_output { "high output" } else { "" })
            .size(11)
            .color(color(tokens.terminal_backpressure)),
        Space::new().width(Fill),
        button("Copy").on_press(Message::CopyTerminal(pane_id)),
        button("Paste").on_press(Message::RequestTerminalPaste(pane_id)),
        button("Start / restart")
            .on_press_maybe(can_start.then_some(Message::RestartTerminal(pane_id))),
        button("Close").on_press(Message::RequestCloseTerminal(pane_id)),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(5);

    let mut link_actions = row![].spacing(4);
    if let Ok(links) = app.terminal.links(pane_id) {
        for link in links.into_iter().take(4) {
            let target = link.target().to_owned();
            let label = if target.chars().count() > 40 {
                format!("{}…", target.chars().take(39).collect::<String>())
            } else {
                target.clone()
            };
            link_actions = link_actions
                .push(button(text(label).size(11)).on_press(Message::InspectTerminalLink(target)));
        }
    }

    let surface: Element<'static, Message> = app.terminal.snapshot(pane_id).map_or_else(
        || {
            container(text("Terminal surface unavailable"))
                .height(Fill)
                .into()
        },
        |snapshot| {
            TerminalWidget::new(
                pane_id,
                snapshot,
                tokens,
                focused,
                app.terminal.selection(pane_id),
            )
            .view()
            .map(Message::TerminalWidget)
        },
    );

    container(column![header, link_actions, surface].spacing(3))
        .padding(4)
        .width(Fill)
        .height(Fill)
        .style(panel_style(
            tokens,
            if focused {
                tokens.panel_active
            } else {
                tokens.terminal_background
            },
        ))
        .into()
}

fn count_layout_panes(node: &LayoutNode) -> usize {
    match node {
        LayoutNode::Pane(_) => 1,
        LayoutNode::Split { first, second, .. } => {
            count_layout_panes(first) + count_layout_panes(second)
        }
    }
}
