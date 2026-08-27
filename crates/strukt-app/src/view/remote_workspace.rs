use iced::widget::{Space, column, container, text};
use iced::{Element, Fill, Length};
use strukt_shell::Activity;
use strukt_ui::{BadgeKind, ChromeRole, UiTheme, badge, chrome, list_row_owned, panel_header};

use crate::app::{Message, StruktApp};

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !app.shell.sidebar.visible {
        return container(Space::new()).width(Length::Shrink).into();
    }
    let title = match app.shell.active_activity {
        Activity::Files => "REMOTE FILES",
        Activity::Search => "REMOTE SEARCH",
        Activity::SourceControl => "REMOTE SOURCE CONTROL",
        Activity::Tasks => "REMOTE TASKS",
        _ => "REMOTE WORKSPACE",
    };
    let mut content = column![
        panel_header(title, theme),
        badge("REMOTE", BadgeKind::Remote, theme),
        text(
            app.remote
                .host_label
                .as_deref()
                .unwrap_or("SSH boundary")
                .to_owned()
        )
        .size(12),
        text(format!("Status · {}", app.remote.status.label())).size(11),
    ]
    .spacing(theme.metrics.space_1);
    if app.shell.active_activity == Activity::Files {
        for path in &app.remote.files {
            content = content.push(list_row_owned(
                path.clone(),
                app.remote.selected_path.as_ref() == Some(path),
                (!app.remote.operation_in_flight())
                    .then(|| Message::OpenRemoteDocument(path.clone())),
                theme,
            ));
        }
        if app.remote.files.is_empty() {
            content = content.push(text("No synchronized remote files.").size(12));
        }
    } else {
        content = content.push(
            text("Commands execute within the approved remote root and advertised capabilities.")
                .size(11),
        );
    }
    chrome(content, theme, ChromeRole::Panel)
        .padding(theme.metrics.space_2)
        .width(Length::Fixed(f32::from(app.shell.sidebar.width)))
        .height(Fill)
        .into()
}

pub(super) fn search_canvas(app: &StruktApp) -> Element<'_, Message> {
    super::remote_search_canvas(app)
}

pub(super) fn git_canvas(app: &StruktApp) -> Element<'_, Message> {
    super::remote_git_canvas(app)
}

pub(super) fn tasks_canvas(app: &StruktApp) -> Element<'_, Message> {
    super::remote_tasks_canvas(app)
}
