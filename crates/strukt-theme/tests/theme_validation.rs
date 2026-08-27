use strukt_theme::{
    THEME_SCHEMA_VERSION, ThemeId, ThemeMode, ThemeRegistry, ThemeRole, ThemeValidationError,
    quiet_precision_definition,
};

#[test]
fn definitions_reject_unsupported_schema_and_incomplete_variants() {
    let mut unsupported = quiet_precision_definition();
    unsupported.schema_version = THEME_SCHEMA_VERSION + 1;
    assert!(matches!(
        unsupported.validate(),
        Err(ThemeValidationError::UnsupportedSchema { .. })
    ));

    let mut missing_dark = quiet_precision_definition();
    missing_dark.variants.remove(&ThemeMode::Dark);
    assert!(matches!(
        missing_dark.validate(),
        Err(ThemeValidationError::MissingVariant(ThemeMode::Dark))
    ));
}

#[test]
fn definitions_reject_missing_roles_and_palette_references() {
    let mut missing_role = quiet_precision_definition();
    missing_role
        .variants
        .get_mut(&ThemeMode::Dark)
        .expect("dark variant")
        .roles
        .remove(&ThemeRole::Canvas);
    assert!(matches!(
        missing_role.validate(),
        Err(ThemeValidationError::MissingRole {
            mode: ThemeMode::Dark,
            role: ThemeRole::Canvas,
        })
    ));

    let mut missing_palette_value = quiet_precision_definition();
    missing_palette_value
        .variants
        .get_mut(&ThemeMode::Light)
        .expect("light variant")
        .roles
        .insert(ThemeRole::Canvas, "absent".to_owned());
    assert!(matches!(
        missing_palette_value.validate(),
        Err(ThemeValidationError::UnknownPaletteKey {
            mode: ThemeMode::Light,
            role: ThemeRole::Canvas,
            ..
        })
    ));
}

#[test]
fn definitions_reject_invalid_identifiers_metrics_and_contrast() {
    assert!(ThemeId::new("Quiet Precision").is_err());

    let mut invalid_metric = quiet_precision_definition();
    invalid_metric.metrics.control_height = f32::NAN;
    assert!(matches!(
        invalid_metric.validate(),
        Err(ThemeValidationError::InvalidMetric { .. })
    ));

    let mut low_contrast = quiet_precision_definition();
    let light = low_contrast
        .variants
        .get_mut(&ThemeMode::Light)
        .expect("light variant");
    let canvas_key = light.roles[&ThemeRole::Canvas].clone();
    light.roles.insert(ThemeRole::TextPrimary, canvas_key);
    assert!(matches!(
        low_contrast.validate(),
        Err(ThemeValidationError::InsufficientContrast {
            mode: ThemeMode::Light,
            foreground: ThemeRole::TextPrimary,
            background: ThemeRole::Canvas,
            ..
        })
    ));
}

#[test]
fn registry_falls_back_atomically_for_an_unknown_selection() {
    let registry = ThemeRegistry::with_builtins();
    let requested = ThemeId::new("missing-theme").expect("valid theme id");
    let resolved = registry.resolve(&requested, ThemeMode::Dark);

    assert_eq!(resolved.id.as_str(), "quiet-precision");
    assert_eq!(resolved.requested_id, requested);
    assert!(resolved.diagnostic.is_some());
    assert_eq!(resolved.mode, ThemeMode::Dark);
}
