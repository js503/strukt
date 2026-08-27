use iced::widget::{Space, column, container, row, stack, text};
use iced::{Alignment, Element, Fill, Length};
use strukt_shell::{Activity, CanvasLayout};
use strukt_theme::ThemeRegistry;
use strukt_ui::{BadgeKind, ChromeRole, UiTheme, badge, chrome, quiet_button};

use crate::app::{Message, StruktApp};
use crate::remote::RemoteStatus;

use super::{
    activity, command_center, connections, context, files, primary_canvas, remote_workspace,
    search, sessions, settings, source_control, status,
};
use super::{responsive::ResponsivePolicy, terminal};

pub(super) fn view(app: &StruktApp) -> Element<'_, Message> {
    let registry = ThemeRegistry::with_builtins();
    let theme = UiTheme::from(registry.resolve(&app.shell.theme_id, app.shell.theme_mode));
    let tokens = theme.tokens;
    let responsive = ResponsivePolicy::default().compose(
        app.viewport_width,
        app.shell.sidebar.visible,
        app.shell.context.visible,
    );
    let _reduced_motion = ResponsivePolicy::reduced_motion();
    let _transition_duration_ms = ResponsivePolicy::transition_duration_ms();
    let sidebar = match app.shell.active_activity {
        Activity::Connections => connections::sidebar(app, &theme),
        Activity::Search => search::sidebar(app, &theme),
        Activity::SourceControl => source_control::sidebar(app, &theme),
        Activity::Settings => settings::sidebar(app, &theme),
        Activity::Sessions => sessions::sidebar(app, &theme),
        _ => files::sidebar(app, &theme),
    };
    let sidebar = if app.remote.status != RemoteStatus::Disconnected
        && matches!(
            app.shell.active_activity,
            Activity::Files | Activity::Search | Activity::SourceControl | Activity::Tasks
        ) {
        remote_workspace::sidebar(app, &theme)
    } else {
        sidebar
    };
    let sidebar: Element<'_, Message> = if responsive.sidebar {
        sidebar
    } else {
        container(Space::new()).width(Length::Shrink).into()
    };
    let context_panel: Element<'_, Message> = if responsive.context {
        context::panel(app, &theme)
    } else {
        container(Space::new()).width(Length::Shrink).into()
    };
    let canvas = match &app.shell.canvas {
        CanvasLayout::Split { ratio, .. } if terminal::is_secondary_canvas(app) => {
            let primary_width = if *ratio <= 0.5 { 50 } else { 62 };
            let secondary_width = 100_u16.saturating_sub(primary_width).max(1);
            row![
                container(primary_canvas(app, tokens, &theme))
                    .width(Length::FillPortion(primary_width)),
                container(terminal::canvas(app, tokens, &theme))
                    .width(Length::FillPortion(secondary_width)),
            ]
            .height(Fill)
            .into()
        }
        _ => primary_canvas(app, tokens, &theme),
    };
    let body = row![activity::rail(app, &theme), sidebar, canvas, context_panel,].height(Fill);
    let workspace = column![
        workspace_bar(app, &theme),
        body,
        terminal::drawer(app, tokens, &theme),
        status::strip(app, &theme),
    ]
    .height(Fill);
    let content: Element<'_, Message> = if app.command_center_visible {
        stack![workspace, command_center::overlay(app, &theme)].into()
    } else {
        workspace.into()
    };

    chrome(content, &theme, ChromeRole::Canvas)
        .width(Fill)
        .height(Fill)
        .into()
}

fn workspace_bar(app: &StruktApp, theme: &UiTheme) -> Element<'static, Message> {
    let (scope, boundary, boundary_detail, kind) =
        if app.remote.status == RemoteStatus::Disconnected {
            let scope = app.workspace.as_ref().map_or_else(
                || "No folder open".to_owned(),
                |workspace| {
                    format!(
                        "{} / {}",
                        workspace.root.display_name(),
                        workspace.root.path().display()
                    )
                },
            );
            (scope, "LOCAL", String::new(), BadgeKind::Neutral)
        } else {
            (
                app.remote
                    .root_label
                    .clone()
                    .unwrap_or_else(|| "Remote workspace".to_owned()),
                "REMOTE",
                app.remote
                    .host_label
                    .clone()
                    .unwrap_or_else(|| "REMOTE".to_owned()),
                BadgeKind::Remote,
            )
        };
    let content = row![
        text("strukt").size(14),
        badge(boundary, kind, theme),
        text(boundary_detail).size(12),
        text(scope).size(12),
        Space::new().width(Fill),
        quiet_button(command_prompt(), Some(Message::ToggleCommandCenter), theme,)
            .width(Length::Fixed(360.0)),
    ]
    .align_y(Alignment::Center)
    .spacing(theme.metrics.space_2);
    container(chrome(content, theme, ChromeRole::Panel))
        .height(41)
        .padding([0.0, theme.metrics.space_3])
        .into()
}

const fn command_prompt() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Search files, commands, sessions…                 ⌘K"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Search files, commands, sessions…             Ctrl+K"
    }
}
