use iced::Color;
use strukt_theme::{ThemeMode, ThemeRegistry, ThemeTokens};
use strukt_ui::{
    BadgeKind, ChromeRole, ComponentState, Emphasis, Icon, SelectionState, UiTheme,
    activity_rail_item, badge, button_appearance, chrome_appearance, divider, list_row_appearance,
    state_appearance, toolbar_group,
};

fn color(red: u8, green: u8, blue: u8) -> Color {
    Color::from_rgb8(red, green, blue)
}

#[derive(Clone)]
enum Message {
    Select,
}

#[test]
fn navigation_and_supporting_builders_share_the_component_contract() {
    let resolved = ThemeRegistry::with_builtins()
        .resolve(&strukt_theme::ThemeId::quiet_precision(), ThemeMode::Dark);
    let ui = UiTheme::from(resolved);

    let _activity = activity_rail_item(Icon::Files, "Files", true, Some(Message::Select), &ui);
    let _toolbar = toolbar_group::<Message>(iced::widget::text("Tools"), &ui);
    let _badge = badge::<Message>("Remote", BadgeKind::Remote, &ui);
    let _divider = divider::<Message>(&ui);
}

#[test]
fn component_states_resolve_from_semantic_tokens_in_both_modes() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let resolved =
            ThemeRegistry::with_builtins().resolve(&strukt_theme::ThemeId::quiet_precision(), mode);
        let ui = UiTheme::from(resolved);
        let tokens = ThemeTokens::builtin(mode);

        let quiet = button_appearance(&ui, Emphasis::Quiet, ComponentState::Resting);
        assert_eq!(
            quiet.text,
            color(
                tokens.text_primary.red,
                tokens.text_primary.green,
                tokens.text_primary.blue
            )
        );
        assert_eq!(quiet.border, Color::TRANSPARENT);

        let hovered = button_appearance(&ui, Emphasis::Standard, ComponentState::Hovered);
        assert_eq!(
            hovered.background,
            color(
                tokens.panel_active.red,
                tokens.panel_active.green,
                tokens.panel_active.blue
            )
        );

        let focused = button_appearance(&ui, Emphasis::Standard, ComponentState::Focused);
        assert_eq!(
            focused.focus,
            color(tokens.focus.red, tokens.focus.green, tokens.focus.blue)
        );

        let destructive = button_appearance(&ui, Emphasis::Destructive, ComponentState::Pressed);
        assert_eq!(
            destructive.text,
            color(
                tokens.diagnostic_error.red,
                tokens.diagnostic_error.green,
                tokens.diagnostic_error.blue
            )
        );

        let disabled = button_appearance(&ui, Emphasis::Standard, ComponentState::Disabled);
        assert_eq!(
            disabled.text,
            color(
                tokens.text_muted.red,
                tokens.text_muted.green,
                tokens.text_muted.blue
            )
        );

        let selected = list_row_appearance(&ui, SelectionState::Selected);
        assert_eq!(
            selected.background,
            color(
                tokens.panel_active.red,
                tokens.panel_active.green,
                tokens.panel_active.blue
            )
        );
        assert_eq!(
            selected.focus,
            color(tokens.accent.red, tokens.accent.green, tokens.accent.blue)
        );

        let panel = chrome_appearance(&ui, ChromeRole::Panel);
        assert_eq!(
            panel.background,
            color(tokens.panel.red, tokens.panel.green, tokens.panel.blue)
        );

        let warning = state_appearance(&ui, strukt_ui::StateKind::Warning);
        assert_eq!(
            warning.accent,
            color(
                tokens.status_warning.red,
                tokens.status_warning.green,
                tokens.status_warning.blue
            )
        );

        let error = state_appearance(&ui, strukt_ui::StateKind::Error);
        assert_eq!(
            error.accent,
            color(
                tokens.diagnostic_error.red,
                tokens.diagnostic_error.green,
                tokens.diagnostic_error.blue
            )
        );
    }
}

#[test]
fn quiet_precision_metrics_drive_component_geometry() {
    let resolved = ThemeRegistry::with_builtins()
        .resolve(&strukt_theme::ThemeId::quiet_precision(), ThemeMode::Dark);
    let ui = UiTheme::from(resolved);

    assert!((ui.metrics.control_height - 28.0).abs() < f32::EPSILON);
    assert!((ui.metrics.row_height - 27.0).abs() < f32::EPSILON);
    assert!(ui.metrics.radius_small <= ui.metrics.radius_medium);
}
