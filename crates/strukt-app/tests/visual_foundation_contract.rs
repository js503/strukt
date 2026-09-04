#[path = "../src/view/accessibility.rs"]
mod accessibility;
#[path = "../src/view/layout.rs"]
mod layout;
#[path = "../src/view/responsive.rs"]
mod responsive;

use accessibility::{LogicalShortcut, PlatformShortcut};
use layout::{
    ACTIVITY_RAIL_WIDTH, CANVAS_OUTER_PADDING, EDITOR_BREADCRUMB_HEIGHT,
    EDITOR_PERMANENT_TOOLBAR_ROWS, EDITOR_TAB_GAP, EDITOR_TAB_HEIGHT,
    EXPLORER_PERMANENT_ACTION_ROWS, STATUS_STRIP_HEIGHT, TERMINAL_DRAWER_HEADER_HEIGHT,
    WORKSPACE_BAR_BOUNDARY_BADGES, WORKSPACE_BAR_HEIGHT,
};
use responsive::{ResponsiveComposition, ResponsivePolicy};

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
    assert!(ResponsivePolicy::reduced_motion());
    assert_eq!(ResponsivePolicy::transition_duration_ms(), 0);
}

#[test]
fn shell_geometry_matches_the_promoteable_drawer_north_star() {
    assert!((WORKSPACE_BAR_HEIGHT - 40.0).abs() <= 2.0);
    assert!((ACTIVITY_RAIL_WIDTH - 48.0).abs() <= 2.0);
    assert!((STATUS_STRIP_HEIGHT - 25.0).abs() < f32::EPSILON);
    assert!((EDITOR_TAB_HEIGHT - 33.0).abs() < f32::EPSILON);
    assert!(EDITOR_TAB_GAP.abs() < f32::EPSILON);
    assert!((EDITOR_BREADCRUMB_HEIGHT - 28.0).abs() < f32::EPSILON);
    assert_eq!(EDITOR_PERMANENT_TOOLBAR_ROWS, 0);
    assert!((TERMINAL_DRAWER_HEADER_HEIGHT - 33.0).abs() < f32::EPSILON);
    assert!(CANVAS_OUTER_PADDING.abs() < f32::EPSILON);
    assert_eq!(EXPLORER_PERMANENT_ACTION_ROWS, 0);
    assert_eq!(WORKSPACE_BAR_BOUNDARY_BADGES, 0);
}
