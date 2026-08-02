use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use strukt_language::ResolvedCommand;
use strukt_workspace::WorkspaceState;
use thiserror::Error;

pub const LANGUAGE_CONTRIBUTION_ID: &str = "strukt.language";
const LANGUAGE_SCHEMA_VERSION: u32 = 1;
const ENTRY_LIMIT: usize = 128;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageSelectionSnapshot {
    language_id: String,
    descriptor_id: String,
    enabled: bool,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl LanguageSelectionSnapshot {
    /// Creates an enabled language-to-descriptor selection.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageStoreError::InvalidSelection`] for invalid IDs.
    pub fn enabled(
        language_id: impl Into<String>,
        descriptor_id: impl Into<String>,
    ) -> Result<Self, LanguageStoreError> {
        Self::new(language_id, descriptor_id, true)
    }

    /// Creates a validated language-to-descriptor selection.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageStoreError::InvalidSelection`] for invalid IDs.
    pub fn new(
        language_id: impl Into<String>,
        descriptor_id: impl Into<String>,
        enabled: bool,
    ) -> Result<Self, LanguageStoreError> {
        let language_id = language_id.into();
        let descriptor_id = descriptor_id.into();
        if !valid_identifier(&language_id) || !valid_identifier(&descriptor_id) {
            return Err(LanguageStoreError::InvalidSelection);
        }
        Ok(Self {
            language_id,
            descriptor_id,
            enabled,
            extensions: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn language_id(&self) -> &str {
        &self.language_id
    }

    #[must_use]
    pub fn descriptor_id(&self) -> &str {
        &self.descriptor_id
    }

    #[must_use]
    pub const fn enabled_state(&self) -> bool {
        self.enabled
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApprovalSnapshot {
    language_id: String,
    command_fingerprint: [u8; 32],
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl ApprovalSnapshot {
    /// Creates an exact-command approval fingerprint for one language.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageStoreError::InvalidApproval`] for an invalid language ID.
    pub fn new(
        language_id: impl Into<String>,
        command_fingerprint: [u8; 32],
    ) -> Result<Self, LanguageStoreError> {
        let language_id = language_id.into();
        if !valid_identifier(&language_id) {
            return Err(LanguageStoreError::InvalidApproval);
        }
        Ok(Self {
            language_id,
            command_fingerprint,
            extensions: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn language_id(&self) -> &str {
        &self.language_id
    }

    #[must_use]
    pub fn matches(&self, command: &ResolvedCommand) -> bool {
        self.command_fingerprint == command.fingerprint()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageSessionSnapshot {
    pub schema_version: u32,
    #[serde(default)]
    selections: Vec<LanguageSelectionSnapshot>,
    #[serde(default)]
    approvals: Vec<ApprovalSnapshot>,
    #[serde(default = "default_problems_visible")]
    pub problems_visible: bool,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl LanguageSessionSnapshot {
    /// Creates a validated, runtime-free language contribution.
    ///
    /// # Errors
    ///
    /// Returns a validation error for duplicate or excessive entries.
    pub fn new(
        selections: Vec<LanguageSelectionSnapshot>,
        approvals: Vec<ApprovalSnapshot>,
        problems_visible: bool,
    ) -> Result<Self, LanguageStoreError> {
        let snapshot = Self {
            schema_version: LANGUAGE_SCHEMA_VERSION,
            selections,
            approvals,
            problems_visible,
            extensions: BTreeMap::new(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Restores configuration and presentation only. No runtime server state is
    /// represented by the returned value.
    ///
    /// # Errors
    ///
    /// Returns a validation error for unsupported or malformed snapshots.
    pub fn restore(&self) -> Result<RestoredLanguageSession, LanguageStoreError> {
        self.validate()?;
        Ok(RestoredLanguageSession {
            selections: self.selections.clone(),
            approvals: self.approvals.clone(),
            problems_visible: self.problems_visible,
        })
    }

    fn validate(&self) -> Result<(), LanguageStoreError> {
        if self.schema_version != LANGUAGE_SCHEMA_VERSION {
            return Err(LanguageStoreError::UnsupportedSchema(self.schema_version));
        }
        if self.selections.len() > ENTRY_LIMIT || self.approvals.len() > ENTRY_LIMIT {
            return Err(LanguageStoreError::TooManyEntries);
        }
        if self.selections.iter().any(|entry| {
            !valid_identifier(&entry.language_id) || !valid_identifier(&entry.descriptor_id)
        }) {
            return Err(LanguageStoreError::InvalidSelection);
        }
        if self
            .approvals
            .iter()
            .any(|entry| !valid_identifier(&entry.language_id))
        {
            return Err(LanguageStoreError::InvalidApproval);
        }
        if !unique(
            self.selections
                .iter()
                .map(|entry| entry.language_id.as_str()),
        ) || !unique(
            self.approvals
                .iter()
                .map(|entry| entry.language_id.as_str()),
        ) {
            return Err(LanguageStoreError::DuplicateLanguage);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoredLanguageSession {
    selections: Vec<LanguageSelectionSnapshot>,
    approvals: Vec<ApprovalSnapshot>,
    problems_visible: bool,
}

impl RestoredLanguageSession {
    #[must_use]
    pub const fn running_servers(&self) -> usize {
        0
    }

    #[must_use]
    pub fn selections(&self) -> &[LanguageSelectionSnapshot] {
        &self.selections
    }

    #[must_use]
    pub fn approvals(&self) -> &[ApprovalSnapshot] {
        &self.approvals
    }

    #[must_use]
    pub const fn problems_visible(&self) -> bool {
        self.problems_visible
    }
}

/// Stores the validated language contribution without altering opaque siblings.
///
/// # Errors
///
/// Returns a validation or serialization error.
pub fn set_language_contribution(
    state: &mut WorkspaceState,
    snapshot: &LanguageSessionSnapshot,
) -> Result<(), LanguageStoreError> {
    snapshot.validate()?;
    state
        .set_contribution(LANGUAGE_CONTRIBUTION_ID, snapshot)
        .map_err(|error| LanguageStoreError::Serialization(error.to_string()))
}

/// Decodes and validates the optional language workspace contribution.
///
/// # Errors
///
/// Returns a typed error for malformed or invalid state.
pub fn language_contribution(
    state: &WorkspaceState,
) -> Result<Option<LanguageSessionSnapshot>, LanguageStoreError> {
    let Some(value) = state.contributions.get(LANGUAGE_CONTRIBUTION_ID) else {
        return Ok(None);
    };
    let snapshot = serde_json::from_value::<LanguageSessionSnapshot>(value.clone())
        .map_err(|error| LanguageStoreError::MalformedContribution(error.to_string()))?;
    snapshot.validate()?;
    Ok(Some(snapshot))
}

pub(crate) fn contribution_is_valid(state: &WorkspaceState) -> bool {
    language_contribution(state).is_ok()
}

fn unique<'a>(values: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();
    values.into_iter().all(|value| seen.insert(value))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

const fn default_problems_visible() -> bool {
    true
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LanguageStoreError {
    #[error("unsupported language schema version {0}")]
    UnsupportedSchema(u32),
    #[error("language contribution has too many entries")]
    TooManyEntries,
    #[error("language selection is invalid")]
    InvalidSelection,
    #[error("language approval is invalid")]
    InvalidApproval,
    #[error("language IDs must be unique within each contribution list")]
    DuplicateLanguage,
    #[error("language contribution serialization failed: {0}")]
    Serialization(String),
    #[error("language contribution is malformed: {0}")]
    MalformedContribution(String),
}
