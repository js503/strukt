use iced::widget::{Button, button, container, text};
use iced::{Background, Border, Element, Fill, Theme};

use crate::{SelectionState, UiTheme, list_row_appearance};

#[must_use]
pub fn list_row<'a, Message: Clone + 'a>(
    label: &'a str,
    selected: bool,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    list_row_content(text(label).size(13), selected, on_press, theme)
}

#[must_use]
pub fn list_row_owned<Message: Clone + 'static>(
    label: String,
    selected: bool,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'static, Message> {
    list_row_content(text(label).size(13), selected, on_press, theme)
}

#[must_use]
pub fn list_row_content<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    selected: bool,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(container(content).center_y(Fill))
        .height(theme.metrics.row_height)
        .width(Fill)
        .padding([0.0, theme.metrics.space_2])
        .on_press_maybe(on_press)
        .style(move |_: &Theme, status| {
            let state = match status {
                button::Status::Disabled => SelectionState::Disabled,
                button::Status::Hovered | button::Status::Pressed => SelectionState::Hovered,
                button::Status::Active if selected => SelectionState::Selected,
                button::Status::Active => SelectionState::Resting,
            };
            let appearance = list_row_appearance(&owned_theme, state);
            button::Style {
                background: Some(Background::Color(appearance.background)),
                text_color: appearance.text,
                border: Border {
                    color: appearance.border,
                    width: 0.0,
                    radius: owned_theme.metrics.radius_small.into(),
                },
                ..button::Style::default()
            }
        })
}
