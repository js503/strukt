use iced::widget::{Space, column, container, row, text};
use iced::{Element, Fill, Length};
use strukt_theme::ThemeMode;
use strukt_ui::{BadgeKind, ChromeRole, UiTheme, badge, chrome, panel_header, quiet_button};

use crate::app::{Message, StruktApp};

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !app.shell.sidebar.visible {
        return container(Space::new()).width(Length::Shrink).into();
    }
    chrome(
        column![
            panel_header("SETTINGS", theme),
            text("Appearance").size(12),
            text("Workspace layout").size(12),
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

pub(super) fn canvas(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    let dark = app.shell.theme_mode == ThemeMode::Dark;
    let content = column![
        text("Appearance").size(22),
        text("Built-in themes and future custom themes share the same versioned contract.")
            .size(12),
        row![
            badge("THEME", BadgeKind::Neutral, theme),
            text(app.shell.theme_id.as_str().to_owned()).size(12),
        ]
        .spacing(theme.metrics.space_2),
        row![
            quiet_button(
                if dark { "✓ Dark" } else { "Dark" },
                Some(Message::SetThemeMode(ThemeMode::Dark)),
                theme,
            ),
            quiet_button(
                if dark { "Light" } else { "✓ Light" },
                Some(Message::SetThemeMode(ThemeMode::Light)),
                theme,
            ),
        ]
        .spacing(theme.metrics.space_2),
        text("Motion").size(16),
        text("Reduce interface animation while preserving selected states.").size(12),
        row![
            quiet_button(
                if app.shell.reduced_motion {
                    "✓ Reduced motion"
                } else {
                    "Reduced motion"
                },
                Some(Message::SetReducedMotion(true)),
                theme,
            ),
            quiet_button(
                if app.shell.reduced_motion {
                    "Normal motion"
                } else {
                    "✓ Normal motion"
                },
                Some(Message::SetReducedMotion(false)),
                theme,
            ),
        ]
        .spacing(theme.metrics.space_2),
        text("Layout").size(16),
        row![
            quiet_button("Toggle sidebar", Some(Message::ToggleExplorer), theme),
            quiet_button("Toggle context", Some(Message::ToggleContext), theme),
            quiet_button("Toggle drawer", Some(Message::ToggleDrawer), theme),
        ]
        .spacing(theme.metrics.space_2),
    ]
    .spacing(theme.metrics.space_3);
    chrome(content, theme, ChromeRole::Canvas)
        .padding(theme.metrics.space_4)
        .height(Fill)
        .into()
}
