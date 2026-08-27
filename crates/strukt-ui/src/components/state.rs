use iced::widget::{column, container, text};
use iced::{Background, Border, Element, Fill, Theme};

use crate::{UiTheme, state_appearance};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateKind {
    Empty,
    Loading,
    Unavailable,
    Stale,
    Warning,
    Error,
    Success,
}

#[must_use]
pub fn state_panel<'a, Message: 'a>(
    title: &'a str,
    detail: &'a str,
    kind: StateKind,
    theme: &UiTheme,
) -> Element<'a, Message> {
    let owned_theme = theme.clone();
    container(column![text(title).size(16), text(detail).size(13)].spacing(theme.metrics.space_2))
        .width(Fill)
        .padding(theme.metrics.space_3)
        .style(move |_: &Theme| {
            let appearance = state_appearance(&owned_theme, kind);
            container::Style {
                text_color: Some(appearance.text),
                background: Some(Background::Color(appearance.background)),
                border: Border {
                    color: appearance.accent,
                    width: 1.0,
                    radius: owned_theme.metrics.radius_small.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}
