use std::collections::BTreeMap;

use crate::{
    ResolvedTheme, ThemeDefinitionV1, ThemeDiagnostic, ThemeId, ThemeMode, ThemeValidationError,
    invalid_diagnostic, quiet_precision_definition,
};

pub struct ThemeRegistry {
    definitions: BTreeMap<ThemeId, ThemeDefinitionV1>,
    fallback: ThemeId,
}

impl ThemeRegistry {
    #[must_use]
    pub fn with_builtins() -> Self {
        let definition = quiet_precision_definition();
        let fallback = definition.id.clone();
        Self {
            definitions: BTreeMap::from([(fallback.clone(), definition)]),
            fallback,
        }
    }

    /// Registers one complete valid theme definition.
    ///
    /// # Errors
    ///
    /// Returns an error when validation fails. A matching ID replaces the prior
    /// definition atomically only after the new value is valid.
    pub fn register(&mut self, definition: ThemeDefinitionV1) -> Result<(), ThemeValidationError> {
        definition.validate()?;
        self.definitions.insert(definition.id.clone(), definition);
        Ok(())
    }

    #[must_use]
    pub fn resolve(&self, requested: &ThemeId, mode: ThemeMode) -> ResolvedTheme {
        if let Some(definition) = self.definitions.get(requested) {
            match definition.resolve(mode) {
                Ok(mut resolved) => {
                    resolved.requested_id = requested.clone();
                    return resolved;
                }
                Err(error) => {
                    return self.fallback_resolution(
                        requested.clone(),
                        Some(invalid_diagnostic(requested.clone(), &error)),
                        mode,
                    );
                }
            }
        }
        self.fallback_resolution(
            requested.clone(),
            Some(ThemeDiagnostic::UnknownTheme {
                requested: requested.clone(),
            }),
            mode,
        )
    }

    fn fallback_resolution(
        &self,
        requested: ThemeId,
        diagnostic: Option<ThemeDiagnostic>,
        mode: ThemeMode,
    ) -> ResolvedTheme {
        let fallback = self
            .definitions
            .get(&self.fallback)
            .expect("theme registry always retains its built-in fallback");
        let mut resolved = fallback
            .resolve(mode)
            .expect("the built-in Quiet Precision definition must remain valid");
        resolved.requested_id = requested;
        resolved.diagnostic = diagnostic;
        resolved
    }
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}
