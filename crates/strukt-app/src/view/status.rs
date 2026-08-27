use iced::widget::{Space, row, text};
use iced::{Alignment, Element, Fill};
use strukt_ui::{BadgeKind, ChromeRole, UiTheme, badge, chrome};

use crate::app::{Message, StruktApp};
use crate::remote::RemoteStatus;

pub(super) fn strip(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    let counts = app.language.problem_counts();
    let (boundary, boundary_detail, kind) = if app.remote.status == RemoteStatus::Disconnected {
        ("LOCAL", String::new(), BadgeKind::Neutral)
    } else {
        (
            "REMOTE",
            app.remote.status.label().to_owned(),
            BadgeKind::Remote,
        )
    };
    let workspace = app.workspace.as_ref().map_or_else(
        || "No workspace".to_owned(),
        |workspace| workspace.root.display_name().to_owned(),
    );
    let content = row![
        badge(boundary, kind, theme),
        text(boundary_detail).size(11),
        text(workspace).size(11),
        text(format!(
            "{} errors · {} warnings",
            counts.errors, counts.warnings
        ))
        .size(11),
        Space::new().width(Fill),
        text(if app.language.running_servers() == 0 {
            "Language agnostic"
        } else {
            "Language services active"
        })
        .size(11),
    ]
    .align_y(Alignment::Center)
    .spacing(theme.metrics.space_3);
    chrome(content, theme, ChromeRole::Status)
        .height(25)
        .padding([0.0, theme.metrics.space_2])
        .into()
}
