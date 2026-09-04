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
    let terminal = if app.shell.drawer.visible {
        "Terminal open"
    } else {
        "Terminal closed"
    };
    let editor_position = active_editor_position(app);
    let content = row![
        text(boundary).size(11),
        text(format!("{} errors", counts.errors)).size(11),
        text(terminal).size(11),
        Space::new().width(Fill),
        text(editor_position)
            .size(11)
            .color(semantic_color(theme.tokens.text_muted)),
    ]
    .align_y(Alignment::Center)
    .spacing(theme.metrics.space_3);
    chrome(content, theme, ChromeRole::Status)
        .height(STATUS_STRIP_HEIGHT)
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
