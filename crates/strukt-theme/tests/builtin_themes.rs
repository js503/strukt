use strukt_theme::{ThemeMode, ThemeTokens};

#[test]
fn light_and_dark_themes_have_distinct_surfaces() {
    let light = ThemeTokens::builtin(ThemeMode::Light);
    let dark = ThemeTokens::builtin(ThemeMode::Dark);

    assert_ne!(light.canvas, dark.canvas);
    assert_ne!(light.text_primary, dark.text_primary);
}

#[test]
fn terminal_and_connection_tokens_are_semantic() {
    let theme = ThemeTokens::builtin(ThemeMode::Dark);

    assert_ne!(theme.terminal_background, theme.panel);
    assert_ne!(theme.connection_remote, theme.status_warning);
}
