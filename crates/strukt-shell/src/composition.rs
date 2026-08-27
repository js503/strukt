use thiserror::Error;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SurfaceId(String);

impl SurfaceId {
    /// Creates a stable shell surface identifier.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, or non-portable identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, SurfaceIdError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
            });
        if !valid {
            return Err(SurfaceIdError(value));
        }
        Ok(Self(value))
    }

    pub(crate) fn trusted(value: &str) -> Self {
        Self::new(value).expect("built-in surface identifiers are valid")
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("surface id `{0}` is invalid")]
pub struct SurfaceIdError(String);

#[derive(Clone, Debug, PartialEq)]
pub enum CanvasLayout {
    Single {
        primary: SurfaceId,
    },
    Split {
        primary: SurfaceId,
        secondary: SurfaceId,
        ratio: f32,
    },
}

impl CanvasLayout {
    #[must_use]
    pub const fn primary(&self) -> &SurfaceId {
        match self {
            Self::Single { primary } | Self::Split { primary, .. } => primary,
        }
    }

    #[must_use]
    pub fn contains(&self, surface: &SurfaceId) -> bool {
        match self {
            Self::Single { primary } => primary == surface,
            Self::Split {
                primary, secondary, ..
            } => primary == surface || secondary == surface,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PanelState {
    pub visible: bool,
    pub surface: Option<SurfaceId>,
    pub width: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrawerState {
    pub surface: Option<SurfaceId>,
    pub visible: bool,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusRegion {
    ActivityRail,
    Sidebar,
    Canvas,
    ContextPanel,
    Drawer,
    CommandCenter,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PromotionState {
    pub prior_canvas: CanvasLayout,
    pub surface: SurfaceId,
}

pub(crate) fn clamp_split_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.2, 0.8)
    } else {
        0.5
    }
}
