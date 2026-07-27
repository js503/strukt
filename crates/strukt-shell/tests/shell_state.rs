use strukt_shell::{Activity, ShellAction, ShellState};
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

    assert!(!state.context_visible);
    assert!(state.drawer_visible);
}

#[test]
fn theme_toggle_switches_between_builtin_modes() {
    let mut state = ShellState::default();
    assert_eq!(state.theme_mode, ThemeMode::Dark);

    state.apply(ShellAction::ToggleTheme);

    assert_eq!(state.theme_mode, ThemeMode::Light);
}
