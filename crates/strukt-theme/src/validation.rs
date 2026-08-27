use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    ResolvedTheme, Rgb, THEME_SCHEMA_VERSION, ThemeDefinitionV1, ThemeDiagnostic, ThemeId,
    ThemeMode, ThemeRole, tokens_from_roles,
};

const REQUIRED_CONTRAST: [(ThemeRole, ThemeRole, f32); 5] = [
    (ThemeRole::TextPrimary, ThemeRole::Canvas, 4.5),
    (ThemeRole::TextPrimary, ThemeRole::Panel, 4.5),
    (ThemeRole::TextMuted, ThemeRole::Canvas, 3.0),
    (ThemeRole::Focus, ThemeRole::Canvas, 3.0),
    (ThemeRole::DiagnosticError, ThemeRole::Canvas, 3.0),
];

impl ThemeDefinitionV1 {
    /// Validates the complete, versioned theme definition.
    ///
    /// # Errors
    ///
    /// Returns the first deterministic schema, identity, metric, completeness,
    /// palette, or contrast violation.
    pub fn validate(&self) -> Result<(), ThemeValidationError> {
        if self.schema_version != THEME_SCHEMA_VERSION {
            return Err(ThemeValidationError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        ThemeId::new(self.id.as_str())?;
        if self.display_name.trim().is_empty() || self.display_name.len() > 80 {
            return Err(ThemeValidationError::InvalidMetadata("display_name"));
        }
        if self.author.trim().is_empty() || self.author.len() > 120 {
            return Err(ThemeValidationError::InvalidMetadata("author"));
        }
        validate_metrics(self.metrics)?;
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let variant = self
                .variants
                .get(&mode)
                .ok_or(ThemeValidationError::MissingVariant(mode))?;
            let values = resolve_values(mode, variant)?;
            validate_contrast(mode, &values)?;
        }
        Ok(())
    }

    /// Resolves one complete immutable variant.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::validate`].
    pub fn resolve(&self, mode: ThemeMode) -> Result<ResolvedTheme, ThemeValidationError> {
        self.validate()?;
        let variant = self
            .variants
            .get(&mode)
            .ok_or(ThemeValidationError::MissingVariant(mode))?;
        let values = resolve_values(mode, variant)?;
        Ok(ResolvedTheme {
            requested_id: self.id.clone(),
            id: self.id.clone(),
            mode,
            tokens: tokens_from_roles(&values),
            metrics: self.metrics,
            diagnostic: None,
        })
    }
}

fn resolve_values(
    mode: ThemeMode,
    variant: &crate::ThemeVariantV1,
) -> Result<BTreeMap<ThemeRole, Rgb>, ThemeValidationError> {
    ThemeRole::ALL
        .into_iter()
        .map(|role| {
            let palette_key = variant
                .roles
                .get(&role)
                .ok_or(ThemeValidationError::MissingRole { mode, role })?;
            let color = variant.palette.get(palette_key).copied().ok_or_else(|| {
                ThemeValidationError::UnknownPaletteKey {
                    mode,
                    role,
                    key: palette_key.clone(),
                }
            })?;
            Ok((role, color))
        })
        .collect()
}

fn validate_metrics(metrics: crate::ThemeMetricsV1) -> Result<(), ThemeValidationError> {
    let bounded = [
        ("space_1", metrics.space_1, 2.0, 8.0),
        ("space_2", metrics.space_2, 4.0, 16.0),
        ("space_3", metrics.space_3, 8.0, 24.0),
        ("space_4", metrics.space_4, 12.0, 32.0),
        ("radius_small", metrics.radius_small, 0.0, 8.0),
        ("radius_medium", metrics.radius_medium, 0.0, 12.0),
        ("control_height", metrics.control_height, 28.0, 56.0),
        ("row_height", metrics.row_height, 24.0, 52.0),
        ("sidebar_width", metrics.sidebar_width, 180.0, 640.0),
        ("context_width", metrics.context_width, 180.0, 640.0),
        ("drawer_height", metrics.drawer_height, 120.0, 720.0),
    ];
    for (name, value, minimum, maximum) in bounded {
        if !value.is_finite() || !(minimum..=maximum).contains(&value) {
            return Err(ThemeValidationError::InvalidMetric {
                name,
                value,
                minimum,
                maximum,
            });
        }
    }
    if !(metrics.space_1 <= metrics.space_2
        && metrics.space_2 <= metrics.space_3
        && metrics.space_3 <= metrics.space_4)
    {
        return Err(ThemeValidationError::InvalidMetricOrder("spacing"));
    }
    if metrics.radius_small > metrics.radius_medium {
        return Err(ThemeValidationError::InvalidMetricOrder("radius"));
    }
    Ok(())
}

fn validate_contrast(
    mode: ThemeMode,
    values: &BTreeMap<ThemeRole, Rgb>,
) -> Result<(), ThemeValidationError> {
    for (foreground, background, minimum) in REQUIRED_CONTRAST {
        let ratio = contrast_ratio(values[&foreground], values[&background]);
        if ratio < minimum {
            return Err(ThemeValidationError::InsufficientContrast {
                mode,
                foreground,
                background,
                minimum,
                actual: ratio,
            });
        }
    }
    Ok(())
}

fn contrast_ratio(first: Rgb, second: Rgb) -> f32 {
    let first = relative_luminance(first);
    let second = relative_luminance(second);
    let lighter = first.max(second);
    let darker = first.min(second);
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Rgb) -> f32 {
    0.2126 * linear_channel(color.red)
        + 0.7152 * linear_channel(color.green)
        + 0.0722 * linear_channel(color.blue)
}

fn linear_channel(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[derive(Debug, Error)]
pub enum ThemeValidationError {
    #[error("theme schema version {found} is unsupported")]
    UnsupportedSchema { found: u16 },
    #[error("theme id `{0}` is invalid")]
    InvalidThemeId(String),
    #[error("theme metadata field `{0}` is invalid")]
    InvalidMetadata(&'static str),
    #[error("theme is missing its {0:?} variant")]
    MissingVariant(ThemeMode),
    #[error("theme {mode:?} variant is missing role {role:?}")]
    MissingRole { mode: ThemeMode, role: ThemeRole },
    #[error("theme {mode:?} role {role:?} references unknown palette key `{key}`")]
    UnknownPaletteKey {
        mode: ThemeMode,
        role: ThemeRole,
        key: String,
    },
    #[error("theme metric `{name}` value {value} is outside {minimum}..={maximum}")]
    InvalidMetric {
        name: &'static str,
        value: f32,
        minimum: f32,
        maximum: f32,
    },
    #[error("theme metric group `{0}` is not monotonically ordered")]
    InvalidMetricOrder(&'static str),
    #[error(
        "theme {mode:?} contrast between {foreground:?} and {background:?} is {actual:.2}, below {minimum:.2}"
    )]
    InsufficientContrast {
        mode: ThemeMode,
        foreground: ThemeRole,
        background: ThemeRole,
        minimum: f32,
        actual: f32,
    },
}

pub(crate) fn invalid_diagnostic(
    requested: ThemeId,
    error: &ThemeValidationError,
) -> ThemeDiagnostic {
    ThemeDiagnostic::InvalidTheme {
        requested,
        detail: error.to_string(),
    }
}
