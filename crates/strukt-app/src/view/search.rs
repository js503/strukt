use iced::widget::{Space, column, container, scrollable, text, text_input};
use iced::{Element, Fill, Length};
use strukt_ui::{ChromeRole, StateKind, UiTheme, chrome, panel_header, state_panel};

use crate::app::{Message, StruktApp};

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !app.shell.sidebar.visible {
        return container(Space::new()).width(Length::Shrink).into();
    }
    let filters = if app.search_include_ignored {
        "Ignored files included"
    } else {
        "Ignored files excluded"
    };
    let content = column![
        panel_header("SEARCH", theme),
        text_input("Search workspace", &app.search_query)
            .on_input(Message::SearchChanged)
            .padding(theme.metrics.space_2),
        strukt_ui::quiet_button(
            filters,
            app.workspace
                .is_some()
                .then_some(Message::ToggleSearchIgnored),
            theme,
        ),
        text("Results update as you type.").size(11),
    ]
    .spacing(theme.metrics.space_2);
    chrome(content, theme, ChromeRole::Panel)
        .padding(theme.metrics.space_2)
        .width(Length::Fixed(f32::from(app.shell.sidebar.width)))
        .height(Fill)
        .into()
}

pub(super) fn canvas<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if app.workspace.is_none() {
        return state_panel(
            "Search across the workspace",
            "Open a folder before starting a workspace search.",
            StateKind::Empty,
            theme,
        );
    }
    if app.search_query.is_empty() {
        return state_panel(
            "Search across the workspace",
            "Enter a query in the Search sidebar. Results update as you type.",
            StateKind::Empty,
            theme,
        );
    }
    let mut results = column![].spacing(theme.metrics.space_1);
    for result in &app.search_results.matches {
        results = results.push(strukt_ui::list_row_owned(
            format!(
                "{}:{}  {}",
                result.relative_path.display(),
                result.line,
                result.preview
            ),
            false,
            None,
            theme,
        ));
    }
    if app.search_results.matches.is_empty() {
        results = results.push(text("No matches in synchronized workspace files.").size(12));
    }
    if app.search_results.truncated {
        results = results.push(text("Results truncated").size(11));
    }
    chrome(
        column![
            text("Search Results").size(16),
            text(format!("Query · {}", app.search_query)).size(11),
            scrollable(results).height(Fill),
        ]
        .spacing(theme.metrics.space_2),
        theme,
        ChromeRole::Canvas,
    )
    .padding(theme.metrics.space_3)
    .height(Fill)
    .into()
}
