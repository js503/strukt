#[path = "../src/view/accessibility.rs"]
mod accessibility;
#[path = "../src/view/layout.rs"]
mod layout;
#[path = "../src/view/responsive.rs"]
mod responsive;

use accessibility::{LogicalShortcut, PlatformShortcut};
use layout::{
    ACTIVITY_RAIL_WIDTH, CANVAS_OUTER_PADDING, COMMAND_CONTROL_WIDTH, EDITOR_BREADCRUMB_HEIGHT,
    EDITOR_PERMANENT_TOOLBAR_ROWS, EDITOR_SUPPORTING_DRAWER_HEIGHT, EDITOR_TAB_GAP,
    EDITOR_TAB_HEIGHT, EXPLORER_PERMANENT_ACTION_ROWS, INITIAL_WINDOW_HEIGHT, INITIAL_WINDOW_WIDTH,
    STATUS_STRIP_BOUNDARY_BADGES, STATUS_STRIP_HEIGHT, TERMINAL_DRAWER_HEADER_HEIGHT,
    WORKSPACE_BAR_BOUNDARY_BADGES, WORKSPACE_BAR_HEIGHT,
};
use responsive::{ResponsiveComposition, ResponsivePolicy};
use strukt_ui::{
    ACTIVITY_ITEM_SIZE, ACTIVITY_SELECTION_INDICATOR_WIDTH, BRAND_IDENTITY_SLOT_WIDTH,
    BRAND_MARK_EXTENT,
};

#[test]
fn logical_shortcuts_are_platform_neutral_and_have_text_alternatives() {
    let mac = PlatformShortcut::for_target("macos");
    let windows = PlatformShortcut::for_target("windows");

    assert_eq!(mac.display(LogicalShortcut::CommandCenter), "⌘K");
    assert_eq!(windows.display(LogicalShortcut::CommandCenter), "Ctrl+K");
    assert_eq!(
        LogicalShortcut::TerminalDrawer.text_alternative(),
        "Focus terminal drawer"
    );
    assert_eq!(
        LogicalShortcut::Escape.text_alternative(),
        "Close the topmost interface layer"
    );
}

#[test]
fn responsive_composition_preserves_canvas_and_collapses_supporting_regions_first() {
    let policy = ResponsivePolicy::default();

    assert_eq!(
        policy.compose(960, true, true),
        ResponsiveComposition {
            activity_rail: true,
            sidebar: false,
            canvas: true,
            context: false,
        }
    );
    assert_eq!(
        policy.compose(1280, true, true),
        ResponsiveComposition {
            activity_rail: true,
            sidebar: true,
            canvas: true,
            context: false,
        }
    );
    assert_eq!(
        policy.compose(1728, true, true),
        ResponsiveComposition {
            activity_rail: true,
            sidebar: true,
            canvas: true,
            context: true,
        }
    );
}

#[test]
fn shell_geometry_matches_the_promoteable_drawer_north_star() {
    assert_eq!(
        (INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT),
        (1_440.0, 900.0)
    );
    assert!((WORKSPACE_BAR_HEIGHT - 40.0).abs() <= 2.0);
    assert!((BRAND_MARK_EXTENT - 29.0).abs() < f32::EPSILON);
    assert!((BRAND_IDENTITY_SLOT_WIDTH - 42.0).abs() < f32::EPSILON);
    assert!((COMMAND_CONTROL_WIDTH - 300.0).abs() < f32::EPSILON);
    assert!((ACTIVITY_RAIL_WIDTH - 48.0).abs() <= 2.0);
    assert!(ACTIVITY_SELECTION_INDICATOR_WIDTH.abs() < f32::EPSILON);
    assert!((ACTIVITY_ITEM_SIZE - ACTIVITY_RAIL_WIDTH).abs() < f32::EPSILON);
    assert!((STATUS_STRIP_HEIGHT - 25.0).abs() < f32::EPSILON);
    assert_eq!(STATUS_STRIP_BOUNDARY_BADGES, 0);
    assert!((EDITOR_TAB_HEIGHT - 33.0).abs() < f32::EPSILON);
    assert!(EDITOR_TAB_GAP.abs() < f32::EPSILON);
    assert!((EDITOR_BREADCRUMB_HEIGHT - 28.0).abs() < f32::EPSILON);
    assert_eq!(EDITOR_PERMANENT_TOOLBAR_ROWS, 0);
    assert!((TERMINAL_DRAWER_HEADER_HEIGHT - 33.0).abs() < f32::EPSILON);
    assert!((EDITOR_SUPPORTING_DRAWER_HEIGHT - 148.0).abs() < f32::EPSILON);
    assert!(CANVAS_OUTER_PADDING.abs() < f32::EPSILON);
    assert_eq!(EXPLORER_PERMANENT_ACTION_ROWS, 0);
    assert_eq!(WORKSPACE_BAR_BOUNDARY_BADGES, 0);
}
#[test]
fn workspace_header_background_owns_its_full_height_and_width() {
    let source = include_str!("../src/view/shell.rs");
    assert!(
        !source.contains("container(chrome(content, theme, ChromeRole::Panel))"),
        "a shrink-height chrome inside an outer fixed-height container leaves header gutters"
    );
}
#[test]
fn explorer_content_reserves_its_border() {
    let source = include_str!("../src/view/mod.rs");
    let explorer = source
        .split("fn explorer<'a>")
        .nth(1)
        .unwrap()
        .split("pub(crate) fn file_entry_label")
        .next()
        .unwrap();
    assert!(
        explorer.contains(".padding(1)"),
        "rows must not paint over the panel hairline"
    );
}
#[test]
fn quiet_deck_uses_explicit_type_and_opt_in_tools() {
    let main = include_str!("../src/main.rs");
    let view = include_str!("../src/view/mod.rs");
    assert!(main.contains("default_text_size: iced::Pixels(13.0)"));
    assert!(view.contains(".font(iced::Font::MONOSPACE)\n        .size(13)"));
    assert!(view.contains("if app.session_tools_visible"));
}
