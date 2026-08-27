use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Fill, Length};
use strukt_ui::{BadgeKind, ChromeRole, UiTheme, badge, chrome, divider};

use crate::app::{Message, StruktApp};

use super::state;

pub(super) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("strukt-command-center-input")
}

pub(super) fn overlay<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    let catalog = app.command_catalog();
    let matches = catalog.search(&app.command_query);
    let has_matches = !matches.is_empty();
    let remote_alias = app.remote.host_label.as_deref().or_else(|| {
        (!app.remote.alias_input.is_empty()).then_some(app.remote.alias_input.as_str())
    });
    let mut results = column![].spacing(theme.metrics.space_1);
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
            button(label)
                .width(Fill)
                .padding(theme.metrics.space_2)
                .on_press_maybe(enabled.then_some(Message::ExecuteCommandIndex(index))),
        );
    }
    if !has_matches {
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

fn display_shortcut(shortcut: &str) -> String {
    #[cfg(target_os = "macos")]
    let primary = "⌘";
    #[cfg(not(target_os = "macos"))]
    let primary = "Ctrl+";
    shortcut.replace("Primary+", primary)
}
