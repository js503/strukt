#![forbid(unsafe_code)]

mod activity;
mod composition;
mod contribution;
mod state;

pub use activity::Activity;
pub use composition::{
    CanvasLayout, DrawerState, FocusRegion, PanelState, SurfaceId, SurfaceIdError,
};
pub use contribution::{
    ActivityContribution, CommandContribution, ContributionError, ContributionRegistry,
    ShellContribution, SurfaceContribution,
};
pub use state::{ShellAction, ShellState};

pub(crate) use composition::{PromotionState, clamp_split_ratio};
