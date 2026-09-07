#![forbid(unsafe_code)]

mod brand;
mod components;
mod icon;
mod theme;

pub use brand::{
    BRAND_IDENTITY_SLOT_WIDTH, BRAND_MARK_EXTENT, BRAND_MOTION_CYCLE_MS, BrandCommand, BrandLine,
    BrandMotion, brand_geometry, brand_mark, brand_motion_sample,
};
pub use components::{
    ACTIVITY_ITEM_SIZE, ACTIVITY_SELECTION_INDICATOR_WIDTH, BadgeKind, ChromeRole, StateKind,
    activity_rail_item, badge, chrome, command_button, divider, icon_button, list_row,
    list_row_content, list_row_owned, panel_header, primary_button, quiet_button, state_panel,
    toolbar_group,
};
pub use icon::{
    ACTIVITY_ICON_TRANSITION_MS, Icon, IconCommand, activity_transition_duration_ms, icon_view,
};
pub use theme::{
    ChromeAppearance, ComponentAppearance, ComponentState, Emphasis, SelectionState,
    StateAppearance, UiTheme, button_appearance, chrome_appearance, list_row_appearance,
    pick_list_appearance, quiet_pick_list_appearance, quiet_text_input_appearance, semantic_color,
    state_appearance, text_input_appearance,
};
