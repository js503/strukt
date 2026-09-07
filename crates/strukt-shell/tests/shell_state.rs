use strukt_shell::{Activity, CanvasLayout, ShellAction, ShellState, SurfaceId};
use strukt_theme::ThemeMode;

#[test]
fn selecting_files_keeps_the_explorer_visible() {
    let mut state = ShellState::default();
    state.apply(ShellAction::SelectActivity(Activity::Files));

    assert_eq!(state.active_activity, Activity::Files);
    assert!(state.explorer_visible);
}

#[test]
fn panels_toggle_independently() {
    let mut state = ShellState::default();

    state.apply(ShellAction::ToggleContext);
    state.apply(ShellAction::ToggleDrawer);

    assert!(state.context_visible);
    assert!(state.drawer_visible);
}

#[test]
fn theme_toggle_switches_between_builtin_modes() {
    let mut state = ShellState::default();
    assert_eq!(state.theme_mode, ThemeMode::Dark);

    state.apply(ShellAction::ToggleTheme);

    assert_eq!(state.theme_mode, ThemeMode::Light);
}

#[test]
fn default_composition_uses_the_north_star_geometry() {
    let state = ShellState::default();

    assert_eq!(state.active_activity, Activity::Sessions);
    assert_eq!(
        state.sidebar.surface,
        Some(SurfaceId::new("sessions.sidebar").expect("valid surface"))
    );
    assert_eq!(
        state.canvas,
        CanvasLayout::Single {
            primary: SurfaceId::new("sessions").expect("valid surface"),
        }
    );
    assert_eq!(state.sidebar.width, 218);
    assert_eq!(state.context.width, 235);
    assert_eq!(state.drawer.height, 205);
    assert!(!state.context.visible);
    assert!(!state.drawer.visible);
    assert!(!state.reduced_motion);
}
