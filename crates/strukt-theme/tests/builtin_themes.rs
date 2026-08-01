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
    assert_eq!(theme.terminal_ansi.len(), 16);
    assert_ne!(theme.terminal_foreground, theme.terminal_background);
    assert_ne!(theme.terminal_selection, theme.terminal_cursor);
    assert_ne!(theme.terminal_link, theme.terminal_foreground);
    assert_ne!(theme.terminal_exited, theme.terminal_backpressure);
    assert_ne!(theme.connection_remote, theme.status_warning);
}

#[test]
fn editor_and_syntax_tokens_are_semantic_and_mode_specific() {
    let light = ThemeTokens::builtin(ThemeMode::Light);
    let dark = ThemeTokens::builtin(ThemeMode::Dark);

    assert_ne!(light.editor_background, dark.editor_background);
    assert_ne!(light.syntax_keyword, dark.syntax_keyword);
    assert_ne!(dark.editor_selection, dark.editor_active_line);
    assert_ne!(dark.editor_gutter, dark.editor_foreground);
    assert_ne!(dark.editor_dirty, dark.editor_conflict);
    assert_ne!(dark.editor_conflict, dark.editor_missing);
    assert_ne!(dark.syntax_comment, dark.syntax_string);
    assert_ne!(dark.syntax_type, dark.syntax_function);
}
