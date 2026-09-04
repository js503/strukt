use iced::Color;
use strukt_theme::{ResolvedTheme, Rgb, ThemeMetricsV1, ThemeTokens};

#[derive(Clone, Debug)]
pub struct UiTheme {
    pub tokens: ThemeTokens,
    pub metrics: ThemeMetricsV1,
}

impl From<ResolvedTheme> for UiTheme {
    fn from(value: ResolvedTheme) -> Self {
        Self {
            tokens: value.tokens,
            metrics: value.metrics,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Emphasis {
    Quiet,
    Standard,
    Strong,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentState {
    Resting,
    Hovered,
    Focused,
    Pressed,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionState {
    Resting,
    Hovered,
    Focused,
    Selected,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComponentAppearance {
    pub background: Color,
    pub text: Color,
    pub border: Color,
    pub focus: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeAppearance {
    pub background: Color,
    pub text: Color,
    pub border: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateAppearance {
    pub background: Color,
    pub text: Color,
    pub accent: Color,
}

#[must_use]
pub fn button_appearance(
    theme: &UiTheme,
    emphasis: Emphasis,
    state: ComponentState,
) -> ComponentAppearance {
    let tokens = theme.tokens;
    let background = match (emphasis, state) {
        (_, ComponentState::Hovered | ComponentState::Pressed) => tokens.panel_active,
        (Emphasis::Strong, _) => tokens.accent,
        _ => tokens.panel,
    };
    let text = match (emphasis, state) {
        (_, ComponentState::Disabled) => tokens.text_muted,
        (Emphasis::Destructive, _) => tokens.diagnostic_error,
        (Emphasis::Strong, _) => tokens.canvas,
        _ => tokens.text_primary,
    };
    let border = match (emphasis, state) {
        (_, ComponentState::Focused) => tokens.focus,
        (Emphasis::Quiet, _) => Rgb::new(0, 0, 0),
        (Emphasis::Strong, _) => tokens.accent,
        (Emphasis::Destructive, _) => tokens.diagnostic_error,
        (Emphasis::Standard, _) => tokens.border,
    };
    ComponentAppearance {
        background: color(background),
        text: color(text),
        border: if matches!(
            (emphasis, state),
            (
                Emphasis::Quiet,
                ComponentState::Resting | ComponentState::Disabled
            )
        ) {
            Color::TRANSPARENT
        } else {
            color(border)
        },
        focus: color(tokens.focus),
    }
}

#[must_use]
pub fn list_row_appearance(theme: &UiTheme, state: SelectionState) -> ComponentAppearance {
    let tokens = theme.tokens;
    let background = match state {
        SelectionState::Hovered | SelectionState::Focused | SelectionState::Selected => {
            tokens.panel_active
        }
        SelectionState::Resting | SelectionState::Disabled => tokens.panel,
    };
    let text = if state == SelectionState::Disabled {
        tokens.text_muted
    } else {
        tokens.text_primary
    };
    ComponentAppearance {
        background: color(background),
        text: color(text),
        border: color(tokens.border),
        focus: color(tokens.accent),
    }
}

#[must_use]
pub fn chrome_appearance(theme: &UiTheme, role: crate::ChromeRole) -> ChromeAppearance {
    let tokens = theme.tokens;
    let background = match role {
        crate::ChromeRole::Canvas => tokens.canvas,
        crate::ChromeRole::Panel | crate::ChromeRole::Status => tokens.panel,
        crate::ChromeRole::ActivePanel | crate::ChromeRole::Overlay => tokens.panel_active,
    };
    ChromeAppearance {
        background: color(background),
        text: color(tokens.text_primary),
        border: color(tokens.border),
    }
}

#[must_use]
pub fn state_appearance(theme: &UiTheme, kind: crate::StateKind) -> StateAppearance {
    let tokens = theme.tokens;
    let accent = match kind {
        crate::StateKind::Empty | crate::StateKind::Loading => tokens.text_muted,
        crate::StateKind::Unavailable | crate::StateKind::Warning => tokens.status_warning,
        crate::StateKind::Stale => tokens.session_stale,
        crate::StateKind::Error => tokens.diagnostic_error,
        crate::StateKind::Success => tokens.status_success,
    };
    StateAppearance {
        background: color(tokens.panel),
        text: color(tokens.text_primary),
        accent: color(accent),
    }
}

#[must_use]
pub const fn semantic_color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

pub(crate) const fn color(rgb: Rgb) -> Color {
    semantic_color(rgb)
}
