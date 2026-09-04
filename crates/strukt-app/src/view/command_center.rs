use iced::widget::{Space, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Fill, Length};
use strukt_ui::{BadgeKind, ChromeRole, UiTheme, badge, chrome, divider};

use crate::app::{CommandResource, Message, StruktApp};

use super::state;

pub(super) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("strukt-command-center-input")
}

pub(super) fn overlay<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    let catalog = app.command_catalog();
    let matches = catalog.search(&app.command_query);
    let has_command_matches = !matches.is_empty();
    let remote_alias = app.remote.host_label.as_deref().or_else(|| {
        (!app.remote.alias_input.is_empty()).then_some(app.remote.alias_input.as_str())
    });
    let (mut results, resource_count) = resource_results(app, theme, remote_alias);
    for (index, entry) in matches.into_iter().take(12).enumerate() {
        let command = entry.command;
        let shortcut = command
            .shortcut
            .as_deref()
            .map_or_else(String::new, display_shortcut);
        let boundary = command.boundary.label(remote_alias).to_owned();
        let enabled = command.enabled;
        let label = column![
            row![
                text(command.title.clone()).size(13),
                Space::new().width(Fill),
                text(shortcut).size(11),
            ]
            .align_y(Alignment::Center),
            row![
                text(command.category.clone()).size(11),
                text(" · ").size(11),
                text(boundary).size(11),
                if enabled {
                    text("").size(11)
                } else {
                    text(" · Unavailable in current context").size(11)
                },
            ],
        ]
        .spacing(theme.metrics.space_1);
        results = results.push(
            strukt_ui::list_row_content(
                label,
                false,
                enabled.then_some(Message::ExecuteCommandIndex(index)),
                theme,
            )
            .height(Length::Shrink)
            .padding(theme.metrics.space_2),
        );
    }
    if !has_command_matches && resource_count == 0 {
        results = results.push(state::empty(
            "No matching results",
            "Try a file, command, session, activity, or setting name.",
            theme,
        ));
    }

    let content = column![
        row![
            text("COMMAND CENTER").size(11),
            Space::new().width(Fill),
            badge("ESC", BadgeKind::Neutral, theme),
        ]
        .align_y(Alignment::Center),
        text_input("Search files, commands, sessions…", &app.command_query)
            .id(input_id())
            .on_input(Message::CommandQueryChanged)
            .on_submit(Message::ExecuteCommandIndex(0))
            .style(super::text_input_style(theme))
            .padding(theme.metrics.space_2),
        divider(theme),
        scrollable(results).height(Length::Fixed(360.0)),
    ]
    .spacing(theme.metrics.space_2);

    container(
        chrome(content, theme, ChromeRole::Overlay)
            .width(Length::Fixed(620.0))
            .padding(theme.metrics.space_3),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn resource_results(
    app: &StruktApp,
    theme: &UiTheme,
    remote_alias: Option<&str>,
) -> (iced::widget::Column<'static, Message>, usize) {
    let query = app.command_query.trim().to_ascii_lowercase();
    let mut count = 0_usize;
    let mut results = column![].spacing(theme.metrics.space_1);
    if query.is_empty() {
        return (results, count);
    }
    for file in app
        .files
        .iter()
        .filter(|entry| entry.kind == strukt_fs::FileKind::File)
        .filter(|entry| {
            entry
                .relative_path
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains(&query)
        })
        .take(4)
    {
        count += 1;
        results = results.push(strukt_ui::list_row_owned(
            format!("File · {}  ·  LOCAL", file.relative_path.display()),
            false,
            Some(Message::CommandResourceSelected(CommandResource::File(
                file.relative_path.clone(),
            ))),
            theme,
        ));
    }
    for root in app
        .recent_workspaces
        .iter()
        .filter(|root| root.to_string_lossy().to_ascii_lowercase().contains(&query))
        .take(3)
    {
        count += 1;
        results = results.push(strukt_ui::list_row_owned(
            format!("Workspace · {}  ·  LOCAL", root.display()),
            false,
            Some(Message::CommandResourceSelected(
                CommandResource::Workspace(root.clone()),
            )),
            theme,
        ));
    }
    if let Some(snapshot) = app.sessions.catalog() {
        for session in snapshot
            .catalog()
            .sessions()
            .filter(|session| session.name().to_ascii_lowercase().contains(&query))
            .take(3)
        {
            count += 1;
            results = results.push(strukt_ui::list_row_owned(
                format!(
                    "Session · {}  ·  {}",
                    session.name(),
                    remote_alias.unwrap_or("LOCAL")
                ),
                false,
                Some(Message::CommandResourceSelected(CommandResource::Session(
                    session.id(),
                ))),
                theme,
            ));
        }
    }
    for (index, record) in app
        .remote
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.alias.to_ascii_lowercase().contains(&query))
        .take(3)
    {
        count += 1;
        results = results.push(strukt_ui::list_row_owned(
            format!("Remote workspace · {}  ·  REMOTE", record.alias),
            false,
            Some(Message::CommandResourceSelected(
                CommandResource::RemoteRecord(index),
            )),
            theme,
        ));
    }
    (results, count)
}

fn display_shortcut(shortcut: &str) -> String {
    #[cfg(target_os = "macos")]
    let primary = "⌘";
    #[cfg(not(target_os = "macos"))]
    let primary = "Ctrl+";
    shortcut.replace("Primary+", primary)
}
