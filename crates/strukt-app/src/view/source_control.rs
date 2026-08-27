use iced::widget::{Space, column, container, text};
use iced::{Element, Fill, Length};
use strukt_ui::{ChromeRole, StateKind, UiTheme, chrome, panel_header, state_panel};

use crate::app::{Message, StruktApp};

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !app.shell.sidebar.visible {
        return container(Space::new()).width(Length::Shrink).into();
    }
    chrome(
        column![
            panel_header("SOURCE CONTROL", theme),
            text("Repository state appears here when Git support is available.").size(11),
        ]
        .spacing(theme.metrics.space_2),
        theme,
        ChromeRole::Panel,
    )
    .padding(theme.metrics.space_2)
    .width(Length::Fixed(f32::from(app.shell.sidebar.width)))
    .height(Fill)
    .into()
}

pub(super) fn canvas<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    let (title, detail, kind) = if app.workspace.is_some() {
        (
            "Source control is not active",
            "This alpha keeps the existing read-only Git boundary. Remote Git appears after an SSH workspace connects.",
            StateKind::Unavailable,
        )
    } else {
        (
            "Open a Git workspace",
            "Open a folder to inspect its repository state.",
            StateKind::Empty,
        )
    };
    state_panel(title, detail, kind, theme)
}
