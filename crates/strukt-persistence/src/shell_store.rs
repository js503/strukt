use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use strukt_shell::{Activity, CanvasLayout, ShellState, SurfaceId, SurfaceIdError};
use strukt_theme::{ThemeId, ThemeMode, ThemeValidationError};
use strukt_workspace::WorkspaceState;
use thiserror::Error;

pub const SHELL_CONTRIBUTION_ID: &str = "shell";
pub const SHELL_SCHEMA_VERSION: u16 = 4;

const SIDEBAR_MIN: u16 = 180;
const SIDEBAR_MAX: u16 = 640;
const CONTEXT_MIN: u16 = 180;
const CONTEXT_MAX: u16 = 640;
const DRAWER_MIN: u16 = 120;
const DRAWER_MAX: u16 = 720;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PersistedCanvasLayout {
    Single {
        primary: String,
    },
    Split {
        primary: String,
        secondary: String,
        ratio: f32,
    },
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "the snapshot preserves independent user presentation preferences"
)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellSnapshotV1 {
    pub schema_version: u16,
    pub active_activity: String,
    pub sidebar_visible: bool,
    pub sidebar_width: u16,
    pub context_visible: bool,
    pub context_width: u16,
    pub canvas: PersistedCanvasLayout,
    pub drawer_surface: Option<String>,
    pub drawer_visible: bool,
    pub drawer_height: u16,
    pub theme_id: String,
    pub theme_mode: ThemeMode,
    #[serde(default = "legacy_reduced_motion_default")]
    pub reduced_motion: bool,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

const fn legacy_reduced_motion_default() -> bool {
    true
}

impl ShellSnapshotV1 {
    #[must_use]
    pub fn from_state(state: &ShellState) -> Self {
        let canvas = match &state.canvas {
            CanvasLayout::Single { primary } => PersistedCanvasLayout::Single {
                primary: primary.as_str().to_owned(),
            },
            CanvasLayout::Split {
                primary,
                secondary,
                ratio,
            } => PersistedCanvasLayout::Split {
                primary: primary.as_str().to_owned(),
                secondary: secondary.as_str().to_owned(),
                ratio: *ratio,
            },
        };
        Self {
            schema_version: SHELL_SCHEMA_VERSION,
            active_activity: activity_id(state.active_activity).to_owned(),
            sidebar_visible: state.sidebar.visible,
            sidebar_width: state.sidebar.width.clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            context_visible: state.context.visible,
            context_width: state.context.width.clamp(CONTEXT_MIN, CONTEXT_MAX),
            canvas,
            drawer_surface: state
                .drawer
                .surface
                .as_ref()
                .map(|surface| surface.as_str().to_owned()),
            drawer_visible: state.drawer.visible,
            drawer_height: state.drawer.height.clamp(DRAWER_MIN, DRAWER_MAX),
            theme_id: state.theme_id.as_str().to_owned(),
            theme_mode: state.theme_mode,
            reduced_motion: state.reduced_motion,
            extensions: BTreeMap::new(),
        }
    }

    /// Restores bounded presentation state against currently available surfaces.
    ///
    /// Unknown but valid surface IDs fall back without failing workspace startup.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported schemas or malformed stable identifiers.
    pub fn restore(
        &self,
        available_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<ShellState, ShellStoreError> {
        if !matches!(self.schema_version, 1 | 2 | 3 | SHELL_SCHEMA_VERSION) {
            return Err(ShellStoreError::UnsupportedSchema(self.schema_version));
        }
        let activity = parse_activity(&self.active_activity)?;
        let theme_id = ThemeId::new(self.theme_id.clone())?;
        let canvas = restore_canvas(&self.canvas, activity, available_surfaces)?;
        let drawer_surface = self
            .drawer_surface
            .as_deref()
            .map(SurfaceId::new)
            .transpose()?;
        let drawer_available = drawer_surface
            .as_ref()
            .is_some_and(|surface| available_surfaces.contains(surface));

        let mut state = ShellState::default();
        state.active_activity = activity;
        state.theme_id = theme_id;
        state.theme_mode = self.theme_mode;
        state.reduced_motion = self.reduced_motion;
        state.canvas = canvas;
        let legacy = self.schema_version == 1;
        state.sidebar.width = migrate_legacy_metric(legacy, self.sidebar_width, 256, 218)
            .clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        state.sidebar.surface = activity.sidebar_surface().filter(|surface| {
            available_surfaces.is_empty() || available_surfaces.contains(surface)
        });
        state.sidebar.visible = self.sidebar_visible && state.sidebar.surface.is_some();
        state.explorer_visible = state.sidebar.visible;
        state.context.width = migrate_legacy_metric(legacy, self.context_width, 320, 235)
            .clamp(CONTEXT_MIN, CONTEXT_MAX);
        state.context.visible = self.context_visible;
        state.context_visible = self.context_visible;
        state.drawer.height = migrate_legacy_metric(legacy, self.drawer_height, 280, 205)
            .clamp(DRAWER_MIN, DRAWER_MAX);
        state.drawer.surface = drawer_available.then_some(drawer_surface).flatten();
        state.drawer.visible = self.drawer_visible && drawer_available;
        state.drawer_visible = state.drawer.visible;
        Ok(state)
    }
}

const fn migrate_legacy_metric(
    legacy: bool,
    value: u16,
    old_default: u16,
    new_default: u16,
) -> u16 {
    if legacy && value == old_default {
        new_default
    } else {
        value
    }
}

/// Stores shell presentation without altering opaque sibling contributions.
///
/// # Errors
///
/// Returns a serialization error when the snapshot cannot be represented as JSON.
pub fn set_shell_contribution(
    state: &mut WorkspaceState,
    snapshot: &ShellSnapshotV1,
) -> Result<(), ShellStoreError> {
    snapshot.restore(&BTreeSet::new())?;
    state
        .set_contribution(SHELL_CONTRIBUTION_ID, snapshot)
        .map_err(|error| ShellStoreError::Serialization(error.to_string()))
}

/// Decodes and validates the optional shell contribution.
///
/// # Errors
///
/// Returns a malformed-contribution or validation error.
pub fn shell_contribution(
    state: &WorkspaceState,
) -> Result<Option<ShellSnapshotV1>, ShellStoreError> {
    let Some(value) = state.contributions.get(SHELL_CONTRIBUTION_ID) else {
        return Ok(None);
    };
    let snapshot = serde_json::from_value::<ShellSnapshotV1>(value.clone())
        .map_err(|error| ShellStoreError::MalformedContribution(error.to_string()))?;
    snapshot.restore(&BTreeSet::new())?;
    Ok(Some(snapshot))
}

pub(crate) fn contribution_is_valid(state: &WorkspaceState) -> bool {
    shell_contribution(state).is_ok()
}

fn restore_canvas(
    persisted: &PersistedCanvasLayout,
    activity: Activity,
    available: &BTreeSet<SurfaceId>,
) -> Result<CanvasLayout, ShellStoreError> {
    let fallback = activity.canvas_surface();
    let is_available = |surface: &SurfaceId| available.is_empty() || available.contains(surface);
    match persisted {
        PersistedCanvasLayout::Single { primary } => {
            let primary = SurfaceId::new(primary.clone())?;
            Ok(CanvasLayout::Single {
                primary: if is_available(&primary) {
                    primary
                } else {
                    fallback
                },
            })
        }
        PersistedCanvasLayout::Split {
            primary,
            secondary,
            ratio,
        } => {
            let primary = SurfaceId::new(primary.clone())?;
            let secondary = SurfaceId::new(secondary.clone())?;
            if primary != secondary && is_available(&primary) && is_available(&secondary) {
                Ok(CanvasLayout::Split {
                    primary,
                    secondary,
                    ratio: if ratio.is_finite() {
                        ratio.clamp(0.2, 0.8)
                    } else {
                        0.5
                    },
                })
            } else {
                Ok(CanvasLayout::Single { primary: fallback })
            }
        }
    }
}

const fn activity_id(activity: Activity) -> &'static str {
    match activity {
        Activity::Files => "files",
        Activity::Search => "search",
        Activity::SourceControl => "source-control",
        Activity::Sessions => "sessions",
        Activity::Tasks => "tasks",
        Activity::Connections => "connections",
        Activity::Extensions => "extensions",
        Activity::Settings => "settings",
    }
}

fn parse_activity(value: &str) -> Result<Activity, ShellStoreError> {
    match value {
        "files" => Ok(Activity::Files),
        "search" => Ok(Activity::Search),
        "source-control" => Ok(Activity::SourceControl),
        "sessions" => Ok(Activity::Sessions),
        "tasks" => Ok(Activity::Tasks),
        "connections" => Ok(Activity::Connections),
        "extensions" => Ok(Activity::Extensions),
        "settings" => Ok(Activity::Settings),
        other => Err(ShellStoreError::InvalidActivity(other.to_owned())),
    }
}

#[derive(Debug, Error)]
pub enum ShellStoreError {
    #[error("unsupported shell schema version {0}")]
    UnsupportedSchema(u16),
    #[error("shell activity `{0}` is invalid")]
    InvalidActivity(String),
    #[error(transparent)]
    InvalidSurface(#[from] SurfaceIdError),
    #[error(transparent)]
    InvalidTheme(#[from] ThemeValidationError),
    #[error("shell contribution serialization failed: {0}")]
    Serialization(String),
    #[error("shell contribution is malformed: {0}")]
    MalformedContribution(String),
}
