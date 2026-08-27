use iced::widget::{Space, column, container, tooltip};
use iced::{Element, Fill, Length};
use strukt_shell::Activity;
use strukt_ui::{ChromeRole, Icon, UiTheme, activity_rail_item, chrome};

use crate::app::{Message, StruktApp};

pub(super) fn rail(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    let active = app.shell.active_activity;
    let item = |icon, label, activity| {
        tooltip(
            activity_rail_item(
                icon,
                "",
                active == activity,
                Some(Message::SelectActivity(activity)),
                theme,
            ),
            label,
            tooltip::Position::Right,
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
    .padding([theme.metrics.space_2, theme.metrics.space_1])
    .spacing(theme.metrics.space_1);

    container(chrome(content, theme, ChromeRole::Panel))
        .width(Length::Fixed(48.0))
        .height(Fill)
        .into()
}
