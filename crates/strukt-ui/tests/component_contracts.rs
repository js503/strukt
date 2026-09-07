use iced::Color;
use strukt_theme::{ThemeMode, ThemeRegistry, ThemeTokens};
use strukt_ui::{
    ACTIVITY_ICON_TRANSITION_MS, ACTIVITY_ITEM_SIZE, ACTIVITY_SELECTION_INDICATOR_WIDTH,
    BRAND_IDENTITY_SLOT_WIDTH, BRAND_MARK_EXTENT, BRAND_MOTION_CYCLE_MS, BadgeKind, BrandCommand,
    ChromeRole, ComponentState, Emphasis, Icon, SelectionState, UiTheme, activity_rail_item, badge,
    brand_geometry, brand_motion_sample, button_appearance, chrome_appearance, command_button,
    divider, list_row_appearance, list_row_content, pick_list_appearance,
    quiet_pick_list_appearance, quiet_text_input_appearance, state_appearance,
    text_input_appearance, toolbar_group,
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

    let _activity = activity_rail_item(
        Icon::Files,
        "Files",
        true,
        false,
        Some(Message::Select),
        &ui,
    );
    let _command = command_button(
        "Search files, commands, sessions…",
        "⌘K",
        Some(Message::Select),
        &ui,
    );
    let _rich_row = list_row_content(
        iced::widget::row![iced::widget::text("Command"), iced::widget::text("⌘K")],
        false,
        Some(Message::Select),
        &ui,
    );
    let _toolbar = toolbar_group::<Message>(iced::widget::text("Tools"), &ui);
    let _badge = badge::<Message>("Remote", BadgeKind::Remote, &ui);
    let _divider = divider::<Message>(&ui);
}

#[test]
fn activity_icons_use_one_vector_family_and_edge_to_edge_geometry() {
    for icon in Icon::ALL {
        assert!(
            !icon.geometry().is_empty(),
            "{icon:?} must expose canonical vector geometry"
        );
    }
    assert!((ACTIVITY_ITEM_SIZE - 48.0).abs() < f32::EPSILON);
    assert!(ACTIVITY_SELECTION_INDICATOR_WIDTH.abs() < f32::EPSILON);
    assert_eq!(ACTIVITY_ICON_TRANSITION_MS, 180);
}

#[test]
fn bolt_structure_brand_has_canonical_geometry_and_calm_motion() {
    let geometry = brand_geometry();
    assert_eq!(geometry.len(), 16);
    assert!(geometry.iter().any(|command| matches!(command,
        BrandCommand::Node { x, y, radius } if (*x - 22.0).abs() < f32::EPSILON && (*y - 19.0).abs() < f32::EPSILON && (*radius - 0.3).abs() < f32::EPSILON
    )));
    assert_eq!(
        geometry
            .iter()
            .filter(|command| matches!(command, BrandCommand::Outer(_)))
            .count(),
        6
    );
    assert_eq!(
        geometry
            .iter()
            .filter(|command| matches!(command, BrandCommand::Plane(_)))
            .count(),
        3
    );
    assert_eq!(
        geometry
            .iter()
            .filter(|command| matches!(command, BrandCommand::Bolt(_)))
            .count(),
        6
    );
    assert_eq!(
        geometry
            .iter()
            .filter(|command| matches!(command, BrandCommand::Node { .. }))
            .count(),
        1
    );
    assert!(
        geometry
            .iter()
            .any(|command| matches!(command, BrandCommand::Outer(_)))
    );
    assert!(
        geometry
            .iter()
            .any(|command| matches!(command, BrandCommand::Plane(_)))
    );
    assert!(
        geometry
            .iter()
            .any(|command| matches!(command, BrandCommand::Bolt(_)))
    );
    assert!(
        geometry
            .iter()
            .any(|command| matches!(command, BrandCommand::Node { .. }))
    );
    assert!((BRAND_MARK_EXTENT - 29.0).abs() < f32::EPSILON);
    assert!((BRAND_IDENTITY_SLOT_WIDTH - 42.0).abs() < f32::EPSILON);
    assert_eq!(BRAND_MOTION_CYCLE_MS, 8_400);
    assert_eq!(
        brand_motion_sample(0, false, true),
        brand_motion_sample(BRAND_MOTION_CYCLE_MS, false, true)
    );

    let resting = brand_motion_sample(1_000, false, true);
    assert!(!resting.active);
    assert!(resting.current_progress.abs() < f32::EPSILON);

    let active = brand_motion_sample(4_000, false, true);
    assert!(active.active);
    assert!(active.current_progress > 0.0 && active.current_progress < 1.0);
    assert!(active.node_scale > 1.0);

    let unfocused = brand_motion_sample(4_000, false, false);
    let reduced = brand_motion_sample(4_000, true, true);
    assert!(!unfocused.active);
    assert!(!reduced.active);
    assert_eq!(unfocused.next_redraw_ms, None);
    assert_eq!(reduced.next_redraw_ms, None);
}

#[test]
fn component_states_resolve_from_semantic_tokens_in_both_modes() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let resolved =
            ThemeRegistry::with_builtins().resolve(&strukt_theme::ThemeId::quiet_precision(), mode);
        let ui = UiTheme::from(resolved);
        let tokens = ThemeTokens::builtin(mode);

        let quiet = button_appearance(&ui, Emphasis::Quiet, ComponentState::Resting);
        assert_eq!(quiet.background, Color::TRANSPARENT);
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
            focused.background,
            color(
                tokens.panel_active.red,
                tokens.panel_active.green,
                tokens.panel_active.blue
            )
        );
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
        assert_eq!(
            selected.text,
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

#[test]
fn compact_inputs_use_quiet_surfaces_until_focused() {
    let resolved = ThemeRegistry::with_builtins()
        .resolve(&strukt_theme::ThemeId::quiet_precision(), ThemeMode::Dark);
    let ui = UiTheme::from(resolved);

    let input = text_input_appearance(&ui, iced::widget::text_input::Status::Active);
    assert_eq!(input.background, iced::Background::Color(color(23, 26, 29)));
    assert!((input.border.width - 1.0).abs() < f32::EPSILON);
    assert_eq!(input.border.color, color(48, 54, 59));

    let focused = text_input_appearance(
        &ui,
        iced::widget::text_input::Status::Focused { is_hovered: false },
    );
    assert_eq!(focused.border.color, color(128, 183, 170));

    let pick = pick_list_appearance(&ui, iced::widget::pick_list::Status::Active);
    assert_eq!(pick.background, iced::Background::Color(color(23, 26, 29)));
    assert_eq!(pick.border.color, color(48, 54, 59));
}

#[test]
fn embedded_chrome_inputs_have_no_resting_box() {
    let resolved = ThemeRegistry::with_builtins()
        .resolve(&strukt_theme::ThemeId::quiet_precision(), ThemeMode::Dark);
    let ui = UiTheme::from(resolved);

    let input = quiet_text_input_appearance(&ui, iced::widget::text_input::Status::Active);
    assert_eq!(
        input.background,
        iced::Background::Color(Color::TRANSPARENT)
    );
    assert!(input.border.width.abs() < f32::EPSILON);

    let pick = quiet_pick_list_appearance(&ui, iced::widget::pick_list::Status::Active);
    assert_eq!(pick.background, iced::Background::Color(Color::TRANSPARENT));
    assert!(pick.border.width.abs() < f32::EPSILON);
}
