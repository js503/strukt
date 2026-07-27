use std::collections::BTreeMap;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CapabilityId(&'static str);

impl CapabilityId {
    pub const AI: Self = Self("ai");
    pub const CONNECTIONS: Self = Self("connections");
    pub const FILES: Self = Self("files");
    pub const TERMINAL: Self = Self("terminal");
    pub const THEMES: Self = Self("themes");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub enabled_by_default: bool,
}

impl CapabilityDescriptor {
    #[must_use]
    pub const fn new(id: CapabilityId, enabled_by_default: bool) -> Self {
        Self {
            id,
            enabled_by_default,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CapabilityState {
    descriptor: CapabilityDescriptor,
    override_enabled: Option<bool>,
}

#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    capabilities: BTreeMap<CapabilityId, CapabilityState>,
}

impl CapabilityRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a capability and its default enablement.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::Duplicate`] when the identifier is already
    /// registered.
    pub fn register(&mut self, descriptor: CapabilityDescriptor) -> Result<(), RegistryError> {
        if self.capabilities.contains_key(&descriptor.id) {
            return Err(RegistryError::Duplicate(descriptor.id));
        }

        self.capabilities.insert(
            descriptor.id,
            CapabilityState {
                descriptor,
                override_enabled: None,
            },
        );
        Ok(())
    }

    /// Overrides the enablement state of a registered capability.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::Unknown`] when the identifier is not registered.
    pub fn set_enabled(&mut self, id: CapabilityId, enabled: bool) -> Result<(), RegistryError> {
        let state = self
            .capabilities
            .get_mut(&id)
            .ok_or(RegistryError::Unknown(id))?;
        state.override_enabled = Some(enabled);
        Ok(())
    }

    #[must_use]
    pub fn is_enabled(&self, id: CapabilityId) -> bool {
        self.capabilities.get(&id).is_some_and(|state| {
            state
                .override_enabled
                .unwrap_or(state.descriptor.enabled_by_default)
        })
    }

    #[must_use]
    pub fn enabled_count(&self) -> usize {
        self.capabilities
            .values()
            .filter(|state| {
                state
                    .override_enabled
                    .unwrap_or(state.descriptor.enabled_by_default)
            })
            .count()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RegistryError {
    #[error("capability already registered: {0:?}")]
    Duplicate(CapabilityId),
    #[error("unknown capability: {0:?}")]
    Unknown(CapabilityId),
}
