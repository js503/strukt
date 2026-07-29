use strukt_core::{CapabilityDescriptor, CapabilityId, CapabilityRegistry, RegistryError};

#[test]
fn registered_capabilities_use_their_default_state() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::AI, false))
        .unwrap();

    assert!(registry.is_enabled(CapabilityId::FILES));
    assert!(!registry.is_enabled(CapabilityId::AI));
}

#[test]
fn explicit_enablement_overrides_the_default() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::AI, false))
        .unwrap();

    registry.set_enabled(CapabilityId::AI, true).unwrap();

    assert!(registry.is_enabled(CapabilityId::AI));
}

#[test]
fn duplicate_registration_is_rejected() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap();

    let error = registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap_err();

    assert_eq!(error, RegistryError::Duplicate(CapabilityId::FILES));
}

#[test]
fn enabled_count_reflects_defaults_and_overrides() {
    let mut registry = CapabilityRegistry::new();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::FILES, true))
        .unwrap();
    registry
        .register(CapabilityDescriptor::new(CapabilityId::AI, false))
        .unwrap();

    assert_eq!(registry.enabled_count(), 1);

    registry.set_enabled(CapabilityId::AI, true).unwrap();

    assert_eq!(registry.enabled_count(), 2);
}
