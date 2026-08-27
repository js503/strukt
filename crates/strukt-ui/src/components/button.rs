use iced::widget::{Button, button, row, text};
use iced::{Background, Border, Fill, Theme};

use crate::{ComponentState, Emphasis, Icon, UiTheme, button_appearance};

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
pub fn icon_button<'a, Message: Clone + 'a>(
    icon: Icon,
    accessible_label: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(row![text(icon.glyph()), text(accessible_label)].spacing(theme.metrics.space_2))
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
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(row![text(icon.glyph()), text(accessible_label)].spacing(theme.metrics.space_2))
        .height(theme.metrics.control_height)
        .width(Fill)
        .padding([0.0, theme.metrics.space_2])
        .on_press_maybe(on_press)
        .style(move |_: &Theme, status| {
            let mut component_state = match status {
                button::Status::Active => ComponentState::Resting,
                button::Status::Hovered => ComponentState::Hovered,
                button::Status::Pressed => ComponentState::Pressed,
                button::Status::Disabled => ComponentState::Disabled,
            };
            if selected && component_state == ComponentState::Resting {
                component_state = ComponentState::Focused;
            }
            style_for_state(&owned_theme, Emphasis::Quiet, component_state)
        })
}

fn themed_button<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Option<Message>,
    theme: &UiTheme,
    emphasis: Emphasis,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(text(label).width(Fill))
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
