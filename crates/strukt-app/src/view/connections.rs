use iced::widget::{Space, column, container, text, text_input};
use iced::{Element, Fill, Length};
use strukt_ui::{
    ChromeRole, StateKind, UiTheme, chrome, list_row_owned, panel_header, state_panel,
};

use crate::app::{Message, StruktApp};
use crate::remote::RemoteStatus;

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !app.shell.sidebar.visible {
        return container(Space::new()).width(Length::Shrink).into();
    }
    let mut hosts = column![
        panel_header("CONNECTIONS", theme),
        text_input("Filter or enter SSH host alias", &app.remote.alias_input)
            .on_input(Message::RemoteAliasChanged)
            .style(super::text_input_style(theme))
            .padding(theme.metrics.space_2),
    ]
    .spacing(theme.metrics.space_1);
    let filter = app.remote.alias_input.to_ascii_lowercase();
    for (index, record) in app.remote.records.iter().enumerate().filter(|(_, record)| {
        filter.is_empty() || record.alias.to_ascii_lowercase().contains(&filter)
    }) {
        hosts = hosts.push(list_row_owned(
            format!("{} · SSH", record.alias),
            app.remote.host_label.as_deref() == Some(record.alias.as_str()),
            (app.remote.status == RemoteStatus::Disconnected)
                .then_some(Message::SelectRemoteRecord(index)),
            theme,
        ));
    }
    if app.remote.records.is_empty() {
        hosts = hosts.push(text("No saved SSH hosts.").size(12));
    }
    chrome(hosts, theme, ChromeRole::Panel)
        .padding(theme.metrics.space_2)
        .width(Length::Fixed(f32::from(app.shell.sidebar.width)))
        .height(Fill)
        .into()
}

pub(super) fn canvas<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if app.remote.status == RemoteStatus::Disconnected
        && app.remote.alias_input.is_empty()
        && app.remote.records.is_empty()
    {
        return state_panel(
            "Connect an SSH development box",
            "Enter a host alias from your OpenSSH config and choose a remote workspace root.",
            StateKind::Empty,
            theme,
        );
    }
    super::connection_workspace_canvas(app)
}

#[cfg(test)]
pub(crate) struct RemoteSurfaceContract {
    pub(crate) boundary_label: &'static str,
    pub(crate) states: &'static [&'static str],
    pub(crate) recovery_actions: &'static [&'static str],
}

#[cfg(test)]
pub(crate) const fn remote_surface_contract() -> RemoteSurfaceContract {
    RemoteSurfaceContract {
        boundary_label: "REMOTE",
        states: &[
            "Host key confirmation required",
            "Authentication failed",
            "Helper unavailable",
            "Helper incompatible",
        ],
        recovery_actions: &["Reconnect", "Repair helper", "Return to local workspace"],
    }
}
