use strukt_persistence::ShellSnapshotV1;
use strukt_shell::{Activity, CanvasLayout};
use strukt_theme::{ThemeId, ThemeMode, ThemeRegistry};

use crate::app::{LaunchMode, Message, StruktApp, built_in_shell_surfaces};

pub(crate) const SUCCESS: &str = "M5.5 visual foundation smoke passed";

pub(crate) fn run() -> Result<(), String> {
    let registry = ThemeRegistry::with_builtins();
    let theme_id = ThemeId::quiet_precision();
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        let resolved = registry.resolve(&theme_id, mode);
        if resolved.tokens.canvas == resolved.tokens.text_primary {
            return Err("theme resolution produced indistinguishable canvas and text".to_owned());
        }
    }

    let mut app = StruktApp::new_with_store(LaunchMode::M5_5VisualFoundationSmoke, None);
    for activity in [
        Activity::Files,
        Activity::Search,
        Activity::SourceControl,
        Activity::Sessions,
        Activity::Tasks,
        Activity::Connections,
        Activity::Extensions,
        Activity::Settings,
    ] {
        drop(app.update(Message::SelectActivity(activity)));
        if app.shell.active_activity != activity {
            return Err(format!("activity {activity:?} did not become active"));
        }
    }

    drop(app.update(Message::ToggleContext));
    drop(app.update(Message::ToggleContext));
    drop(app.update(Message::ToggleDrawer));
    drop(app.update(Message::PromoteTerminalToSplit));
    if !matches!(app.shell.canvas, CanvasLayout::Split { .. }) {
        return Err("terminal did not promote to split".to_owned());
    }
    drop(app.update(Message::DemoteTerminalToDrawer));
    drop(app.update(Message::PromoteTerminalToFull));
    drop(app.update(Message::DemoteTerminalToDrawer));
    drop(app.update(Message::ToggleCommandCenter));
    drop(app.update(Message::CommandQueryChanged("terminal".to_owned())));
    if app.command_query != "terminal" {
        return Err("command center search did not retain its query".to_owned());
    }
    drop(app.update(Message::ToggleCommandCenter));

    let available = built_in_shell_surfaces();
    let snapshot = ShellSnapshotV1::from_state(&app.shell);
    snapshot
        .restore(&available)
        .map_err(|error| format!("valid shell snapshot failed: {error}"))?;
    let mut invalid = snapshot;
    invalid.schema_version = u16::MAX;
    if invalid.restore(&available).is_ok() {
        return Err("invalid shell snapshot unexpectedly restored".to_owned());
    }

    Ok(())
}
