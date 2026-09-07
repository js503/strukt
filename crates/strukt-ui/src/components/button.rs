use iced::widget::{Button, Space, button, container, row, text, tooltip};
use iced::{Background, Border, Element, Fill, Theme};

use crate::{ComponentState, Emphasis, Icon, UiTheme, button_appearance, icon_view};

pub const ACTIVITY_ITEM_SIZE: f32 = 48.0;
pub const ACTIVITY_SELECTION_INDICATOR_WIDTH: f32 = 0.0;

#[must_use]
pub fn quiet_button<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    themed_button(label, on_press, theme, Emphasis::Quiet)
}

#[must_use]
pub fn primary_button<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    themed_button(label, on_press, theme, Emphasis::Strong)
}

#[must_use]
pub fn command_button<'a, Message: Clone + 'a>(
    label: &'a str,
    shortcut: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(
        container(
            row![
                text(label).size(12),
                Space::new().width(Fill),
                text(shortcut).size(11),
            ]
            .align_y(iced::Alignment::Center),
        )
        .center_y(Fill),
    )
    .height(theme.metrics.control_height)
    .padding([0.0, theme.metrics.space_2])
    .on_press_maybe(on_press)
    .style(move |_: &Theme, status| {
        let mut style = style(&owned_theme, Emphasis::Quiet, status);
        style.border.width = 0.0;
        style.border.radius = 0.0.into();
        if matches!(status, button::Status::Active | button::Status::Disabled) {
            style.background = None;
        }
        style
    })
}

#[must_use]
pub fn icon_button<'a, Message: Clone + 'a>(
    icon: Icon,
    accessible_label: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(
        row![
            icon_view(icon, false, true, false, 18.0, theme),
            text(accessible_label)
        ]
        .spacing(theme.metrics.space_2),
    )
    .height(theme.metrics.control_height)
    .padding([0.0, theme.metrics.space_2])
    .on_press_maybe(on_press)
    .style(move |_: &Theme, status| style(&owned_theme, Emphasis::Quiet, status))
}

#[must_use]
pub fn activity_rail_item<'a, Message: Clone + 'a>(
    icon: Icon,
    accessible_label: &'a str,
    selected: bool,
    reduced_motion: bool,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Element<'a, Message> {
    let owned_theme = theme.clone();
    let icon = icon_view(
        icon,
        selected,
        reduced_motion,
        true,
        ACTIVITY_ITEM_SIZE,
        theme,
    );
    let control = button(icon)
        .height(ACTIVITY_ITEM_SIZE)
        .width(ACTIVITY_ITEM_SIZE)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_: &Theme, status| {
            let state = match status {
                button::Status::Active => ComponentState::Resting,
                button::Status::Hovered => ComponentState::Hovered,
                button::Status::Pressed => ComponentState::Pressed,
                button::Status::Disabled => ComponentState::Disabled,
            };
            let mut style = style_for_state(&owned_theme, Emphasis::Quiet, state);
            style.background = (state != ComponentState::Resting)
                .then(|| Background::Color(crate::semantic_color(owned_theme.tokens.panel_active)));
            style.border.width = 0.0;
            style.border.radius = 0.0.into();
            style
        });
    tooltip(control, accessible_label, tooltip::Position::Right).into()
}

fn themed_button<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
    emphasis: Emphasis,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(container(text(label).size(13)).center_y(Fill))
        .height(theme.metrics.control_height)
        .padding([0.0, theme.metrics.space_2])
        .on_press_maybe(on_press)
        .style(move |_: &Theme, status| style(&owned_theme, emphasis, status))
}

fn style(theme: &UiTheme, emphasis: Emphasis, status: button::Status) -> button::Style {
    let state = match status {
        button::Status::Active => ComponentState::Resting,
        button::Status::Hovered => ComponentState::Hovered,
        button::Status::Pressed => ComponentState::Pressed,
        button::Status::Disabled => ComponentState::Disabled,
    };
    style_for_state(theme, emphasis, state)
}

fn style_for_state(theme: &UiTheme, emphasis: Emphasis, state: ComponentState) -> button::Style {
    let appearance = button_appearance(theme, emphasis, state);
    button::Style {
        background: Some(Background::Color(appearance.background)),
        text_color: appearance.text,
        border: Border {
            color: appearance.border,
            width: if state == ComponentState::Focused {
                2.0
            } else {
                1.0
            },
            radius: theme.metrics.radius_small.into(),
        },
        ..button::Style::default()
    }
}
#[cfg(test)]
mod header_tests {
    #[test]
    fn command_control_has_no_resting_frame() {
        let source = include_str!("button.rs");
        let command = source
            .split("pub fn command_button")
            .nth(1)
            .unwrap()
            .split("pub fn icon_button")
            .next()
            .unwrap();
        assert!(command.contains("style.border.width = 0.0"));
        assert!(command.contains("style.background = None"));
    }
}
