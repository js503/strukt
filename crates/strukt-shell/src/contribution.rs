use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{Activity, CommandContribution, CommandId, ShellState, SurfaceId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityContribution {
    pub activity: Activity,
    pub label: String,
    pub sidebar: Option<SurfaceId>,
    pub canvas: SurfaceId,
    pub order: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceContribution {
    pub id: SurfaceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellContribution {
    pub id: String,
    pub activity: Option<ActivityContribution>,
    pub surfaces: Vec<SurfaceContribution>,
    pub commands: Vec<CommandContribution>,
}

#[derive(Default)]
pub struct ContributionRegistry {
    contributions: BTreeMap<String, ShellContribution>,
}

impl ContributionRegistry {
    /// Registers one feature's complete shell contribution atomically.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid or duplicate contribution, activity,
    /// surface, or command identifiers.
    pub fn register(&mut self, contribution: ShellContribution) -> Result<(), ContributionError> {
        if contribution.id.is_empty() || contribution.id.len() > 128 {
            return Err(ContributionError::InvalidContribution(contribution.id));
        }
        if self.contributions.contains_key(&contribution.id) {
            return Err(ContributionError::DuplicateContribution(contribution.id));
        }
        if let Some(activity) = &contribution.activity
            && self
                .contributions
                .values()
                .filter_map(|entry| entry.activity.as_ref())
                .any(|entry| entry.activity == activity.activity)
        {
            return Err(ContributionError::DuplicateActivity(activity.activity));
        }
        let existing_surfaces = self.surface_ids();
        let mut incoming_surfaces = BTreeSet::new();
        for surface in &contribution.surfaces {
            if existing_surfaces.contains(&surface.id)
                || !incoming_surfaces.insert(surface.id.clone())
            {
                return Err(ContributionError::DuplicateSurface(surface.id.clone()));
            }
        }
        let existing_commands = self.command_ids();
        let mut incoming_commands = BTreeSet::new();
        for command in &contribution.commands {
            if !command.id.is_valid()
                || existing_commands.contains(&command.id)
                || !incoming_commands.insert(command.id.clone())
            {
                return Err(ContributionError::DuplicateCommand(command.id.clone()));
            }
        }
        self.contributions
            .insert(contribution.id.clone(), contribution);
        Ok(())
    }

    /// Removes a feature contribution and sanitizes all shell references.
    ///
    /// # Errors
    ///
    /// Returns an error when the contribution is not registered.
    pub fn unregister(
        &mut self,
        id: &str,
        state: &mut ShellState,
    ) -> Result<ShellContribution, ContributionError> {
        let removed = self
            .contributions
            .remove(id)
            .ok_or_else(|| ContributionError::UnknownContribution(id.to_owned()))?;
        let removed_surfaces = removed
            .surfaces
            .iter()
            .map(|surface| surface.id.clone())
            .chain(
                removed
                    .activity
                    .iter()
                    .flat_map(|activity| [Some(activity.canvas.clone()), activity.sidebar.clone()])
                    .flatten(),
            )
            .collect::<BTreeSet<_>>();
        state.remove_surfaces(&removed_surfaces);
        if removed
            .activity
            .as_ref()
            .is_some_and(|activity| activity.activity == state.active_activity)
            && let Some(fallback) = self.fallback_activity()
        {
            state.select_contribution(fallback);
        }
        Ok(removed)
    }

    fn fallback_activity(&self) -> Option<&ActivityContribution> {
        let mut activities = self
            .contributions
            .values()
            .filter_map(|entry| entry.activity.as_ref())
            .collect::<Vec<_>>();
        activities.sort_by_key(|activity| (activity.activity != Activity::Files, activity.order));
        activities.into_iter().next()
    }

    fn surface_ids(&self) -> BTreeSet<SurfaceId> {
        self.contributions
            .values()
            .flat_map(|entry| entry.surfaces.iter().map(|surface| surface.id.clone()))
            .collect()
    }

    fn command_ids(&self) -> BTreeSet<CommandId> {
        self.contributions
            .values()
            .flat_map(|entry| entry.commands.iter().map(|command| command.id.clone()))
            .collect()
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContributionError {
    #[error("shell contribution `{0}` is invalid")]
    InvalidContribution(String),
    #[error("shell contribution `{0}` is already registered")]
    DuplicateContribution(String),
    #[error("activity {0:?} is already registered")]
    DuplicateActivity(Activity),
    #[error("surface `{0:?}` is already registered")]
    DuplicateSurface(SurfaceId),
    #[error("command `{0}` is already registered")]
    DuplicateCommand(CommandId),
    #[error("shell contribution `{0}` is not registered")]
    UnknownContribution(String),
}
