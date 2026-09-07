use iced::widget::{Space, row, text};
use iced::{Alignment, Element, Fill};
use strukt_editor::GrammarRegistry;
use strukt_ui::{ChromeRole, UiTheme, chrome, semantic_color};

use super::layout::{STATUS_STRIP_BOUNDARY_BADGES, STATUS_STRIP_HEIGHT};
use crate::app::{Message, StruktApp};
use crate::remote::RemoteStatus;

pub(super) fn strip(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    debug_assert_eq!(STATUS_STRIP_BOUNDARY_BADGES, 0);
    let counts = app.language.problem_counts();
    let boundary = if app.remote.status == RemoteStatus::Disconnected {
        "local".to_owned()
    } else {
        app.remote
            .host_label
            .as_deref()
            .map_or_else(|| "remote".to_owned(), |host| format!("remote · {host}"))
    };
    let surface_relationship = if app.language.problems_visible() {
        "Problems supporting"
    } else if app.shell.canvas.primary().as_str() == crate::app::EDITOR_SUPPORTING_SURFACE_ID {
        "Editor primary"
    } else if matches!(
        &app.shell.canvas,
        strukt_shell::CanvasLayout::Split { secondary, .. }
            if secondary.as_str() == crate::app::EDITOR_SUPPORTING_SURFACE_ID
    ) {
        "Session + editor split"
    } else if app.shell.drawer.visible
        && app
            .shell
            .drawer
            .surface
            .as_ref()
            .is_some_and(|surface| surface.as_str() == crate::app::EDITOR_SUPPORTING_SURFACE_ID)
    {
        "Session + editor peek"
    } else if app.shell.drawer.visible {
        "Terminal supporting"
    } else if app.shell.canvas.primary().as_str() == "sessions" {
        "Session primary"
    } else {
        "Canvas primary"
    };
    let editor_position = active_editor_position(app);
    let active_session = app
        .sessions
        .selected_session()
        .and_then(|id| app.sessions.catalog()?.catalog().session(id))
        .map_or_else(
            || "No session".to_owned(),
            |session| session.name().to_owned(),
        );
    let branch = if app.remote.status == RemoteStatus::Disconnected {
        app.repository_branch.as_deref()
    } else {
        app.remote
            .git_summary
            .as_deref()
            .and_then(|summary| summary.strip_prefix("branch: "))
            .and_then(|summary| summary.split(" · ").next())
    }
    .map_or_else(
        || "branch —".to_owned(),
        |branch| format!("branch {branch}"),
    );
    let latency = (app.remote.status != RemoteStatus::Disconnected).then(|| {
        app.remote.connection_latency_ms.map_or_else(
            || "latency —".to_owned(),
            |latency| format!("latency {latency} ms"),
        )
    });
    let content = row![
        text(format!("{boundary} · {active_session}")).size(11),
        text(branch).size(11),
        text(latency.unwrap_or_default()).size(11),
        text(format!("{} errors · {surface_relationship}", counts.errors)).size(11),
        Space::new().width(Fill),
        text(editor_position)
            .size(11)
            .color(semantic_color(theme.tokens.text_muted)),
    ]
    .align_y(Alignment::Center)
    .spacing(theme.metrics.space_3);
    chrome(content, theme, ChromeRole::Status)
        .height(STATUS_STRIP_HEIGHT)
        .center_y(STATUS_STRIP_HEIGHT)
        .padding([0.0, theme.metrics.space_3])
        .into()
}

fn active_editor_position(app: &StruktApp) -> String {
    let Some(workspace) = &app.editor else {
        return "Ready".to_owned();
    };
    let Some(id) = workspace.active_document_id() else {
        return "Ready".to_owned();
    };
    let Some(document) = workspace.document(id) else {
        return "Ready".to_owned();
    };
    let override_id = app.editor_language_overrides.get(&id).map(String::as_str);
    let grammar =
        GrammarRegistry::detect(std::path::Path::new(document.path().as_str()), override_id);
    let Some(content) = app.editor_surfaces.content(id) else {
        return grammar.display_name.to_owned();
    };
    let position = content.cursor().position;
    format!(
        "Ln {}, Col {} · {}",
        position.line + 1,
        position.column + 1,
        grammar.display_name
    )
}
