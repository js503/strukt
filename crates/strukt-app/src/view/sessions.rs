use iced::widget::{Space, column, container, scrollable, text};
use iced::{Element, Fill, Length};
use strukt_session::{AttentionState, PaneLifecycle};
use strukt_ui::{ChromeRole, UiTheme, chrome, list_row_owned, panel_header};

use crate::app::{Message, StruktApp};

const SURFACE_ID: &str = "sessions";

pub(super) fn is_primary_canvas(app: &StruktApp) -> bool {
    app.shell.canvas.primary().as_str() == SURFACE_ID
}

pub(super) fn sidebar<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    if !app.shell.sidebar.visible {
        return container(Space::new()).width(Length::Shrink).into();
    }

    let boundary = app.sessions.remote_host().map_or_else(
        || "LOCAL · PERSISTENT".to_owned(),
        |host| format!("REMOTE · {host}"),
    );
    let mut inventory = column![
        panel_header("SESSIONS", theme),
        text(boundary)
            .size(11)
            .color(strukt_ui::semantic_color(theme.tokens.text_muted)),
        health_text(app, theme),
        Space::new().height(theme.metrics.space_2),
        list_row_owned(
            "Files".to_owned(),
            false,
            Some(Message::SelectActivity(strukt_shell::Activity::Files)),
            theme,
        ),
        list_row_owned(
            "Source Control".to_owned(),
            false,
            Some(Message::SelectActivity(
                strukt_shell::Activity::SourceControl
            )),
            theme,
        ),
        list_row_owned(
            "Problems".to_owned(),
            app.language.problems_visible(),
            Some(Message::ShowProblemsDrawer),
            theme,
        ),
        Space::new().height(theme.metrics.space_2),
    ]
    .spacing(theme.metrics.space_1);
    if let Some(snapshot) = app.sessions.catalog() {
        for session in snapshot.catalog().sessions() {
            let selected = app.sessions.selected_session() == Some(session.id());
            let window_count = session.windows().len();
            let pane_count = session
                .windows()
                .iter()
                .flat_map(strukt_session::SessionWindow::panes)
                .count();
            inventory = inventory.push(list_row_owned(
                format!("{}  ·  {window_count}w {pane_count}p", session.name()),
                selected && app.sessions.selected_pane().is_none(),
                Some(Message::SelectSession(session.id())),
                theme,
            ));
            if selected {
                for window in session.windows() {
                    let window_selected = app.sessions.selected_window() == Some(window.id());
                    inventory = inventory.push(list_row_owned(
                        format!("  Window · {}", window.name()),
                        window_selected && app.sessions.selected_pane().is_none(),
                        Some(Message::SelectSessionWindow(window.id())),
                        theme,
                    ));
                    if window_selected {
                        for pane in window.panes() {
                            let pane_selected = app.sessions.selected_pane() == Some(pane.id());
                            let lifecycle = pane_lifecycle_label(pane.lifecycle());
                            let (unread, attention) = snapshot.pane_status(pane.id());
                            let signal = match attention {
                                AttentionState::Attention => " · attention".to_owned(),
                                AttentionState::Unread if unread > 0 => {
                                    format!(" · {unread} unread")
                                }
                                AttentionState::None | AttentionState::Unread => String::new(),
                            };
                            inventory = inventory.push(list_row_owned(
                                format!("    Pane · {lifecycle}{signal}"),
                                pane_selected,
                                Some(Message::SelectSessionPane(pane.id())),
                                theme,
                            ));
                        }
                    }
                }
            }
        }
        if snapshot.catalog().sessions().next().is_none() {
            inventory = inventory.push(text("No persistent sessions yet.").size(12));
        }
    } else {
        inventory = inventory.push(text("Attach to view persistent sessions.").size(12));
    }

    chrome(scrollable(inventory), theme, ChromeRole::Panel)
        .padding(theme.metrics.space_2)
        .width(Length::Fixed(f32::from(app.shell.sidebar.width)))
        .height(Fill)
        .into()
}

pub(super) fn canvas<'a>(app: &'a StruktApp, theme: &UiTheme) -> Element<'a, Message> {
    super::session_detail_canvas(app, theme)
}

fn health_text(app: &StruktApp, theme: &UiTheme) -> iced::widget::Text<'static> {
    let failed = app.session_error.is_some();
    text(if failed {
        "Operation failed"
    } else {
        super::session_health_label(app.sessions.health())
    })
    .size(11)
    .color(strukt_ui::semantic_color(if failed {
        theme.tokens.diagnostic_error
    } else {
        theme.tokens.text_muted
    }))
}

fn pane_lifecycle_label(lifecycle: &PaneLifecycle) -> &'static str {
    match lifecycle {
        PaneLifecycle::Stopped => "stopped",
        PaneLifecycle::Starting => "starting",
        PaneLifecycle::Running => "running",
        PaneLifecycle::Exited { .. } => "exited",
        PaneLifecycle::Failed { .. } => "failed",
        PaneLifecycle::Backpressured => "busy",
    }
}

#[cfg(test)]
pub(crate) struct SessionSurfaceContract {
    pub(crate) sidebar_title: &'static str,
    pub(crate) empty_state: &'static str,
    pub(crate) actions: &'static [&'static str],
    pub(crate) stable_links: &'static [&'static str],
    pub(crate) close_label: &'static str,
    pub(crate) terminate_label: &'static str,
}

#[cfg(test)]
pub(crate) const fn session_surface_contract() -> SessionSurfaceContract {
    SessionSurfaceContract {
        sidebar_title: "SESSIONS",
        empty_state: "No persistent sessions yet.",
        actions: &["Create", "Attach", "Detach", "Rename", "Terminate"],
        stable_links: &["Files", "Source Control", "Problems"],
        close_label: "Close view",
        terminate_label: "Terminate session",
    }
}
