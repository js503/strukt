use iced::Element;
use strukt_ui::{StateKind, UiTheme, state_panel};

use crate::app::Message;
#[cfg(test)]
use crate::app::StruktApp;
#[cfg(test)]
use strukt_shell::{Activity, FocusRegion};

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalActivityComposition {
    pub sidebar_title: &'static str,
    pub canvas_owner: &'static str,
    pub empty_state: &'static str,
    pub contextual_actions: &'static [&'static str],
    pub focus_target: FocusRegion,
}

#[cfg(test)]
pub(crate) fn local_activity_composition(app: &StruktApp) -> LocalActivityComposition {
    let (sidebar_title, canvas_owner, empty_state, contextual_actions): (
        &'static str,
        &'static str,
        &'static str,
        &'static [&'static str],
    ) = match app.shell.active_activity {
        Activity::Files => (
            "EXPLORER",
            "files",
            "Open a folder to browse files",
            &["files.open-folder", "files.new", "files.filters"],
        ),
        Activity::Search => (
            "SEARCH",
            "search",
            "Search across the workspace",
            &["search.run", "search.filters"],
        ),
        Activity::SourceControl => (
            "SOURCE CONTROL",
            "source-control",
            "Open a Git workspace",
            &["source-control.refresh"],
        ),
        Activity::Settings => (
            "SETTINGS",
            "settings",
            "Workspace and appearance settings",
            &["settings.theme"],
        ),
        Activity::Sessions => (
            "SESSIONS",
            "sessions",
            "Create or attach a persistent session",
            &[],
        ),
        Activity::Tasks => ("TASKS", "tasks", "No task selected", &[]),
        Activity::Connections => (
            "CONNECTIONS",
            "connections",
            "Connect to a development host",
            &[],
        ),
        Activity::Extensions => (
            "EXTENSIONS",
            "extensions",
            "No extensions are installed",
            &[],
        ),
    };
    LocalActivityComposition {
        sidebar_title,
        canvas_owner,
        empty_state,
        contextual_actions,
        focus_target: app.shell.focus_region,
    }
}

pub(super) fn empty<'a>(title: &'a str, detail: &'a str, theme: &UiTheme) -> Element<'a, Message> {
    state_panel(title, detail, StateKind::Empty, theme)
}
