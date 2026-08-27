use std::collections::BTreeSet;

use strukt_theme::{
    ThemeDefinitionV1, ThemeMode, ThemeRole, ThemeTokens, quiet_precision_definition,
};

#[test]
fn quiet_precision_round_trips_through_the_versioned_definition() {
    let definition = quiet_precision_definition();
    let json = serde_json::to_string_pretty(&definition).expect("serialize theme");
    let restored: ThemeDefinitionV1 = serde_json::from_str(&json).expect("deserialize theme");

    assert_eq!(restored, definition);
    assert_eq!(restored.schema_version, 1);
    assert_eq!(restored.id.as_str(), "quiet-precision");
    assert!(json.contains("\"terminal_ansi_15\""));
}

#[test]
fn both_variants_resolve_every_semantic_role() {
    let definition = quiet_precision_definition();
    let expected_roles = ThemeRole::ALL.into_iter().collect::<BTreeSet<_>>();

    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let variant = definition.variants.get(&mode).expect("required variant");
        assert_eq!(
            variant.roles.keys().copied().collect::<BTreeSet<_>>(),
            expected_roles
        );
        assert!(
            variant
                .roles
                .values()
                .all(|palette_key| variant.palette.contains_key(palette_key))
        );

        let resolved = definition.resolve(mode).expect("resolve built-in theme");
        assert_eq!(resolved.tokens, ThemeTokens::builtin(mode));
        assert_eq!(resolved.mode, mode);
        assert_eq!(resolved.id.as_str(), "quiet-precision");
    }
}
