use iced::Element;
use strukt_shell::CanvasLayout;
use strukt_theme::ThemeTokens;
use strukt_ui::UiTheme;

use crate::app::{Message, StruktApp};

use super::{terminal_canvas, terminal_drawer};

const SURFACE_ID: &str = "terminal.local.primary";

pub(super) fn is_primary_canvas(app: &StruktApp) -> bool {
    matches!(
        &app.shell.canvas,
        CanvasLayout::Single { primary } if primary.as_str() == SURFACE_ID
    )
}

pub(super) fn is_secondary_canvas(app: &StruktApp) -> bool {
    matches!(
        &app.shell.canvas,
        CanvasLayout::Split { secondary, .. } if secondary.as_str() == SURFACE_ID
    )
}

pub(super) fn canvas(
    app: &StruktApp,
    tokens: ThemeTokens,
    theme: &UiTheme,
) -> Element<'static, Message> {
    terminal_canvas(app, tokens, theme)
}

pub(super) fn drawer(
    app: &StruktApp,
    tokens: ThemeTokens,
    theme: &UiTheme,
) -> Element<'static, Message> {
    terminal_drawer(app, tokens, theme)
}
