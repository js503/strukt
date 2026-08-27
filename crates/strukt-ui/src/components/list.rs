use iced::widget::{Button, button, text};
use iced::{Background, Border, Fill, Theme};

use crate::{SelectionState, UiTheme, list_row_appearance};

#[must_use]
pub fn list_row<'a, Message: Clone + 'a>(
    label: &'a str,
    selected: bool,
    on_press: Option<Message>,
    theme: &UiTheme,
) -> Button<'a, Message> {
    let owned_theme = theme.clone();
    button(text(label))
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
                    color: if state == SelectionState::Selected {
                        appearance.focus
                    } else {
                        appearance.border
                    },
                    width: if state == SelectionState::Selected {
                        2.0
                    } else {
                        0.0
                    },
                    radius: owned_theme.metrics.radius_small.into(),
                },
                ..button::Style::default()
            }
        })
}
