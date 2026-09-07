use iced::widget::{Container, container, text};
use iced::{Background, Border, Element, Fill, Theme};

use crate::{UiTheme, chrome_appearance};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromeRole {
    Canvas,
    Panel,
    ActivePanel,
    Overlay,
    Status,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BadgeKind {
    Neutral,
    Success,
    Warning,
    Error,
    Remote,
}

#[must_use]
pub fn chrome<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    theme: &UiTheme,
    role: ChromeRole,
) -> Container<'a, Message> {
    let owned_theme = theme.clone();
    container(content)
        .style(move |_: &Theme| style(&owned_theme, role))
        .width(Fill)
}

#[must_use]
pub fn panel_header<'a, Message: 'a>(label: &'a str, theme: &UiTheme) -> Container<'a, Message> {
    container(text(label).size(11))
        .height(theme.metrics.control_height)
        .center_y(theme.metrics.control_height)
        .padding([0.0, theme.metrics.space_3])
}

#[must_use]
pub fn toolbar_group<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    theme: &UiTheme,
) -> Container<'a, Message> {
    chrome(content, theme, ChromeRole::Panel)
        .width(iced::Length::Shrink)
        .padding([theme.metrics.space_1, theme.metrics.space_2])
}

#[must_use]
pub fn badge<'a, Message: 'a>(
    label: &'a str,
    kind: BadgeKind,
    theme: &UiTheme,
) -> Container<'a, Message> {
    let owned_theme = theme.clone();
    container(text(label).size(11))
        .padding([theme.metrics.space_1, theme.metrics.space_2])
        .style(move |_: &Theme| {
            let tokens = owned_theme.tokens;
            let accent = match kind {
                BadgeKind::Neutral => tokens.text_muted,
                BadgeKind::Success => tokens.status_success,
                BadgeKind::Warning => tokens.status_warning,
                BadgeKind::Error => tokens.diagnostic_error,
                BadgeKind::Remote => tokens.connection_remote,
            };
            container::Style {
                text_color: Some(crate::theme::color(accent)),
                background: Some(Background::Color(crate::theme::color(tokens.panel_active))),
                border: Border {
                    color: crate::theme::color(accent),
                    width: 1.0,
                    radius: owned_theme.metrics.radius_small.into(),
                },
                ..container::Style::default()
            }
        })
}

#[must_use]
pub fn divider<'a, Message: 'a>(theme: &UiTheme) -> Container<'a, Message> {
    let owned_theme = theme.clone();
    container(text(""))
        .height(1)
        .width(Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(crate::theme::color(
                owned_theme.tokens.border,
            ))),
            ..container::Style::default()
        })
}

fn style(theme: &UiTheme, role: ChromeRole) -> container::Style {
    let appearance = chrome_appearance(theme, role);
    container::Style {
        text_color: Some(appearance.text),
        background: Some(Background::Color(appearance.background)),
        border: Border {
            color: appearance.border,
            width: 1.0,
            radius: if role == ChromeRole::Overlay {
                theme.metrics.radius_medium.into()
            } else {
                0.0.into()
            },
        },
        ..container::Style::default()
    }
}
