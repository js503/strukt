use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use strukt_session::{
    PaneId, SessionCatalog, SessionId, SessionLayoutNode, StoppedWindowDefinition, WindowId,
};
use strukt_terminal::{LayoutNode, TerminalPaneId};
use strukt_workspace::WorkspaceState;
use thiserror::Error;

use crate::{TERMINAL_CONTRIBUTION_ID, terminal_contribution};

pub const SESSION_CONTRIBUTION_ID: &str = "strukt.session";
const SESSION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionContribution {
    pub schema_version: u32,
    pub provider: String,
    pub selected_session: Option<SessionId>,
    pub selected_window: Option<WindowId>,
    pub migrated_terminal_schema: Option<u32>,
}

impl SessionContribution {
    fn validate(&self) -> Result<(), SessionMigrationError> {
        if self.schema_version != SESSION_SCHEMA_VERSION || self.provider != "native-local" {
            return Err(SessionMigrationError::UnsupportedContribution);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMigrationPlan {
    pub catalog: SessionCatalog,
    pub contribution: SessionContribution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionMigrationOutcome {
    None,
    M3Authoritative,
    Planned(SessionMigrationPlan),
}

/// Plans a stopped-only M2 terminal migration without touching disk or starting a process.
///
/// # Errors
///
/// Returns malformed contribution, hierarchy, or random-identifier errors.
pub fn plan_session_migration(
    state: &WorkspaceState,
    existing: Option<&SessionCatalog>,
) -> Result<SessionMigrationOutcome, SessionMigrationError> {
    if session_contribution(state)?.is_some()
        || existing.is_some_and(|catalog| catalog.sessions().next().is_some())
    {
        return Ok(SessionMigrationOutcome::M3Authoritative);
    }
    let Some(terminal) = terminal_contribution(state)? else {
        return Ok(SessionMigrationOutcome::None);
    };
    if terminal.tabs.is_empty() {
        return Ok(SessionMigrationOutcome::None);
    }

    let active_window_index = terminal
        .active_tab
        .and_then(|active| terminal.tabs.iter().position(|tab| tab.id == active))
        .unwrap_or(0);
    let mut windows = Vec::with_capacity(terminal.tabs.len());
    for tab in &terminal.tabs {
        let mut ids = BTreeMap::new();
        for pane in &tab.panes {
            ids.insert(pane.id, PaneId::new()?);
        }
        let panes = tab
            .panes
            .iter()
            .map(|pane| {
                Ok((
                    *ids.get(&pane.id)
                        .ok_or(SessionMigrationError::InvalidLayout)?,
                    pane.working_directory.clone(),
                ))
            })
            .collect::<Result<Vec<_>, SessionMigrationError>>()?;
        windows.push(StoppedWindowDefinition {
            name: tab.name.clone(),
            root: migrate_layout(&tab.root, &ids)?,
            focused_pane: *ids
                .get(&tab.focused_pane)
                .ok_or(SessionMigrationError::InvalidLayout)?,
            panes,
        });
    }
    let mut catalog = SessionCatalog::new();
    let session = catalog.import_stopped_session(0, "Local", windows, active_window_index)?;
    let window = catalog
        .session(session)
        .and_then(strukt_session::Session::active_window)
        .ok_or(SessionMigrationError::InvalidLayout)?
        .id();
    Ok(SessionMigrationOutcome::Planned(SessionMigrationPlan {
        catalog,
        contribution: SessionContribution {
            schema_version: SESSION_SCHEMA_VERSION,
            provider: "native-local".to_owned(),
            selected_session: Some(session),
            selected_window: Some(window),
            migrated_terminal_schema: Some(terminal.schema_version),
        },
    }))
}

/// Applies metadata only after the caller has durably saved the service catalog.
///
/// Unknown sibling contributions are preserved and only the obsolete terminal
/// contribution is removed.
///
/// # Errors
///
/// Returns a serialization or invalid-contribution error.
pub fn apply_session_migration_metadata(
    state: &mut WorkspaceState,
    plan: &SessionMigrationPlan,
) -> Result<(), SessionMigrationError> {
    set_session_contribution(state, &plan.contribution)?;
    state.contributions.remove(TERMINAL_CONTRIBUTION_ID);
    Ok(())
}

/// Stores validated M3 presentation linkage.
///
/// # Errors
///
/// Returns validation or JSON serialization errors.
pub fn set_session_contribution(
    state: &mut WorkspaceState,
    contribution: &SessionContribution,
) -> Result<(), SessionMigrationError> {
    contribution.validate()?;
    state.set_contribution(SESSION_CONTRIBUTION_ID, contribution)?;
    Ok(())
}

/// Reads validated M3 presentation linkage.
///
/// # Errors
///
/// Returns malformed JSON or unsupported contribution errors.
pub fn session_contribution(
    state: &WorkspaceState,
) -> Result<Option<SessionContribution>, SessionMigrationError> {
    let Some(value) = state.contributions.get(SESSION_CONTRIBUTION_ID) else {
        return Ok(None);
    };
    let contribution: SessionContribution = serde_json::from_value(value.clone())?;
    contribution.validate()?;
    Ok(Some(contribution))
}

pub(crate) fn contribution_is_valid(state: &WorkspaceState) -> bool {
    session_contribution(state).is_ok()
}

fn migrate_layout(
    node: &LayoutNode,
    ids: &BTreeMap<TerminalPaneId, PaneId>,
) -> Result<SessionLayoutNode, SessionMigrationError> {
    match node {
        LayoutNode::Pane(pane) => Ok(SessionLayoutNode::Pane(
            *ids.get(pane).ok_or(SessionMigrationError::InvalidLayout)?,
        )),
        LayoutNode::Split {
            axis,
            ratio_basis_points,
            first,
            second,
        } => Ok(SessionLayoutNode::Split {
            axis: *axis,
            ratio_basis_points: *ratio_basis_points,
            first: Box::new(migrate_layout(first, ids)?),
            second: Box::new(migrate_layout(second, ids)?),
        }),
    }
}

#[derive(Debug, Error)]
pub enum SessionMigrationError {
    #[error("session contribution is unsupported")]
    UnsupportedContribution,
    #[error("terminal layout cannot be migrated")]
    InvalidLayout,
    #[error(transparent)]
    Terminal(#[from] crate::TerminalStoreError),
    #[error(transparent)]
    Catalog(#[from] strukt_session::CatalogError),
    #[error(transparent)]
    Id(#[from] strukt_session::IdError),
    #[error("session contribution serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}
