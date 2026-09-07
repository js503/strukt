use iced::widget::{Space, column, container};
use iced::{Element, Fill, Length};
use strukt_shell::Activity;
use strukt_ui::{Icon, UiTheme, activity_rail_item, semantic_color};

use super::layout::ACTIVITY_RAIL_WIDTH;
use crate::app::{Message, StruktApp};

pub(super) fn rail(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    let active = app.shell.active_activity;
    let item = |icon, label, activity| {
        activity_rail_item(
            icon,
            label,
            active == activity,
            app.shell.reduced_motion,
            Some(Message::SelectActivity(activity)),
            theme,
        )
    };
    let content = column![
        item(Icon::Files, "Files", Activity::Files),
        item(Icon::Search, "Search", Activity::Search),
        item(
            Icon::SourceControl,
            "Source control",
            Activity::SourceControl
        ),
        item(Icon::Sessions, "Sessions", Activity::Sessions),
        item(Icon::Tasks, "Tasks", Activity::Tasks),
        item(Icon::Connect, "Connections", Activity::Connections),
        item(Icon::Extensions, "Extensions", Activity::Extensions),
        Space::new().height(Fill),
        item(Icon::Settings, "Settings", Activity::Settings),
    ]
    .spacing(0);

    // The adjoining sidebar owns the shared hairline. Children must not
    // overpaint a second border on this edge.
    let background = semantic_color(theme.tokens.panel);
    container(content)
        .style(move |_| container::Style {
            background: Some(background.into()),
            ..container::Style::default()
        })
        .width(Length::Fixed(ACTIVITY_RAIL_WIDTH))
        .height(Fill)
        .into()
}
