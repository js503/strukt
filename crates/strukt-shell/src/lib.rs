#![forbid(unsafe_code)]

mod activity;
mod command;
mod composition;
mod contribution;
mod state;

pub use activity::Activity;
pub use command::{
    CommandCatalog, CommandContribution, CommandError, CommandId, CommandMatch, ExecutionBoundary,
};
pub use composition::{
    CanvasLayout, DrawerState, FocusRegion, PanelState, SurfaceId, SurfaceIdError,
};
pub use contribution::{
    ActivityContribution, ContributionError, ContributionRegistry, ShellContribution,
    SurfaceContribution,
};
pub use state::{ShellAction, ShellState};

pub(crate) use composition::{PromotionState, clamp_split_ratio};
