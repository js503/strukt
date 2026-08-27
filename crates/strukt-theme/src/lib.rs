#![forbid(unsafe_code)]

mod definition;
mod registry;
mod tokens;
mod validation;

pub use definition::{
    QUIET_PRECISION_THEME_ID, ResolvedTheme, THEME_SCHEMA_VERSION, ThemeDefinitionV1,
    ThemeDiagnostic, ThemeId, ThemeMetricsV1, ThemeRole, ThemeVariantV1,
    quiet_precision_definition,
};
pub use registry::ThemeRegistry;
pub use tokens::{Rgb, ThemeMode, ThemeTokens};
pub use validation::ThemeValidationError;

pub(crate) use definition::tokens_from_roles;
pub(crate) use validation::invalid_diagnostic;
