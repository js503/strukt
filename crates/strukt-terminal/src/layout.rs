use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{TerminalPaneId, TerminalTabId};

const MIN_SPLIT_RATIO: u16 = 1000;
const MAX_SPLIT_RATIO: u16 = 9000;
const DEFAULT_SPLIT_RATIO: u16 = 5000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneState {
    Stopped,
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { message: String },
    Backpressured,
}

impl PaneState {
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LayoutNode {
    Pane(TerminalPaneId),
    Split {
        axis: SplitAxis,
        ratio_basis_points: u16,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl LayoutNode {
    #[must_use]
    pub const fn is_pane(&self) -> bool {
        matches!(self, Self::Pane(_))
    }

    fn contains(&self, pane: TerminalPaneId) -> bool {
        match self {
            Self::Pane(candidate) => *candidate == pane,
            Self::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    fn collect_panes(&self, panes: &mut Vec<TerminalPaneId>) {
        match self {
            Self::Pane(pane) => panes.push(*pane),
            Self::Split { first, second, .. } => {
                first.collect_panes(panes);
                second.collect_panes(panes);
            }
        }
    }

    fn split(
        &mut self,
        focused: TerminalPaneId,
        new_pane: TerminalPaneId,
        axis: SplitAxis,
    ) -> bool {
        match self {
            Self::Pane(pane) if *pane == focused => {
                *self = Self::Split {
                    axis,
                    ratio_basis_points: DEFAULT_SPLIT_RATIO,
                    first: Box::new(Self::Pane(focused)),
                    second: Box::new(Self::Pane(new_pane)),
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                first.split(focused, new_pane, axis) || second.split(focused, new_pane, axis)
            }
        }
    }

    fn remove(self, pane: TerminalPaneId) -> Option<Self> {
        match self {
            Self::Pane(candidate) => (candidate != pane).then_some(Self::Pane(candidate)),
            Self::Split {
                axis,
                ratio_basis_points,
                first,
                second,
            } => match (first.remove(pane), second.remove(pane)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis,
                    ratio_basis_points,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                (None, None) => None,
            },
        }
    }

    fn set_parent_ratio(&mut self, pane: TerminalPaneId, ratio: u16) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split {
                ratio_basis_points,
                first,
                second,
                ..
            } => {
                if matches!(first.as_ref(), Self::Pane(candidate) if *candidate == pane)
                    || matches!(second.as_ref(), Self::Pane(candidate) if *candidate == pane)
                {
                    *ratio_basis_points = ratio;
                    true
                } else {
                    first.set_parent_ratio(pane, ratio) || second.set_parent_ratio(pane, ratio)
                }
            }
        }
    }

    fn validate(&self) -> bool {
        match self {
            Self::Pane(_) => true,
            Self::Split {
                ratio_basis_points,
                first,
                second,
                ..
            } => {
                (MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(ratio_basis_points)
                    && first.validate()
                    && second.validate()
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalPane {
    id: TerminalPaneId,
    working_directory: PathBuf,
    state: PaneState,
}

impl TerminalPane {
    #[must_use]
    pub const fn id(&self) -> TerminalPaneId {
        self.id
    }

    #[must_use]
    pub fn working_directory(&self) -> &PathBuf {
        &self.working_directory
    }

    #[must_use]
    pub const fn state(&self) -> &PaneState {
        &self.state
    }

    #[must_use]
    pub const fn command(&self) -> Option<&str> {
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalTab {
    id: TerminalTabId,
    name: String,
    root: LayoutNode,
    focused_pane: TerminalPaneId,
}

impl TerminalTab {
    #[must_use]
    pub const fn id(&self) -> TerminalTabId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn root(&self) -> &LayoutNode {
        &self.root
    }

    #[must_use]
    pub const fn focused_pane(&self) -> TerminalPaneId {
        self.focused_pane
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalPaneSnapshot {
    pub id: TerminalPaneId,
    pub working_directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalTabSnapshot {
    pub id: TerminalTabId,
    pub name: String,
    pub root: LayoutNode,
    pub focused_pane: TerminalPaneId,
    pub panes: Vec<TerminalPaneSnapshot>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalWorkspaceSnapshot {
    pub tabs: Vec<TerminalTabSnapshot>,
    pub active_tab: Option<TerminalTabId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalWorkspace {
    tabs: Vec<TerminalTab>,
    panes: BTreeMap<TerminalPaneId, TerminalPane>,
    active_tab: Option<TerminalTabId>,
}

impl TerminalWorkspace {
    /// Creates and activates a terminal tab with one stopped pane.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::InvalidName`] for an empty name or
    /// [`TerminalWorkspaceError::InvalidWorkingDirectory`] for an empty path.
    pub fn create_tab(
        &mut self,
        name: impl Into<String>,
        working_directory: impl Into<PathBuf>,
    ) -> Result<TerminalPaneId, TerminalWorkspaceError> {
        let name = validated_name(name.into())?;
        let working_directory = validated_directory(working_directory.into())?;
        let pane_id = TerminalPaneId::new();
        let tab_id = TerminalTabId::new();
        self.panes.insert(
            pane_id,
            TerminalPane {
                id: pane_id,
                working_directory,
                state: PaneState::Stopped,
            },
        );
        self.tabs.push(TerminalTab {
            id: tab_id,
            name,
            root: LayoutNode::Pane(pane_id),
            focused_pane: pane_id,
        });
        self.active_tab = Some(tab_id);
        Ok(pane_id)
    }

    /// Splits the focused pane and focuses the new stopped sibling.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::NoActiveTab`] when no tab is active,
    /// or [`TerminalWorkspaceError::PaneNotFound`] for inconsistent state.
    pub fn split_focused(
        &mut self,
        axis: SplitAxis,
    ) -> Result<TerminalPaneId, TerminalWorkspaceError> {
        let active = self.active_tab.ok_or(TerminalWorkspaceError::NoActiveTab)?;
        let tab_index = self
            .tab_index(active)
            .ok_or(TerminalWorkspaceError::TabNotFound)?;
        let focused = self.tabs[tab_index].focused_pane;
        let directory = self
            .panes
            .get(&focused)
            .ok_or(TerminalWorkspaceError::PaneNotFound)?
            .working_directory
            .clone();
        let new_pane = TerminalPaneId::new();
        if !self.tabs[tab_index].root.split(focused, new_pane, axis) {
            return Err(TerminalWorkspaceError::PaneNotFound);
        }
        self.tabs[tab_index].focused_pane = new_pane;
        self.panes.insert(
            new_pane,
            TerminalPane {
                id: new_pane,
                working_directory: directory,
                state: PaneState::Stopped,
            },
        );
        Ok(new_pane)
    }

    /// Closes a pane and deterministically collapses its empty split branch.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::PaneNotFound`] when the pane does not
    /// belong to any tab.
    pub fn close_pane(&mut self, pane: TerminalPaneId) -> Result<(), TerminalWorkspaceError> {
        let tab_index = self
            .tabs
            .iter()
            .position(|tab| tab.root.contains(pane))
            .ok_or(TerminalWorkspaceError::PaneNotFound)?;
        let root = self.tabs[tab_index].root.clone().remove(pane);
        self.panes.remove(&pane);
        if let Some(root) = root {
            let mut remaining = Vec::new();
            root.collect_panes(&mut remaining);
            self.tabs[tab_index].root = root;
            if self.tabs[tab_index].focused_pane == pane {
                self.tabs[tab_index].focused_pane = remaining[0];
            }
        } else {
            let removed_id = self.tabs.remove(tab_index).id;
            if self.active_tab == Some(removed_id) {
                self.active_tab = self
                    .tabs
                    .get(tab_index)
                    .or_else(|| self.tabs.last())
                    .map(|tab| tab.id);
            }
        }
        Ok(())
    }

    /// Activates an existing terminal tab.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::TabNotFound`] for an unknown ID.
    pub fn activate_tab(&mut self, tab: TerminalTabId) -> Result<(), TerminalWorkspaceError> {
        if self.tab_index(tab).is_none() {
            return Err(TerminalWorkspaceError::TabNotFound);
        }
        self.active_tab = Some(tab);
        Ok(())
    }

    /// Focuses a pane within the active terminal tab.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::PaneNotFound`] when the pane does not
    /// belong to the active tab.
    pub fn focus_pane(&mut self, pane: TerminalPaneId) -> Result<(), TerminalWorkspaceError> {
        let tab = self.active_tab_mut()?;
        if !tab.root.contains(pane) {
            return Err(TerminalWorkspaceError::PaneNotFound);
        }
        tab.focused_pane = pane;
        Ok(())
    }

    /// Renames an existing terminal tab.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::TabNotFound`] for an unknown ID or
    /// [`TerminalWorkspaceError::InvalidName`] for an empty name.
    pub fn rename_tab(
        &mut self,
        tab: TerminalTabId,
        name: impl Into<String>,
    ) -> Result<(), TerminalWorkspaceError> {
        let name = validated_name(name.into())?;
        let index = self
            .tab_index(tab)
            .ok_or(TerminalWorkspaceError::TabNotFound)?;
        self.tabs[index].name = name;
        Ok(())
    }

    /// Renames the active tab.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::NoActiveTab`] when empty and the same
    /// validation errors as [`Self::rename_tab`].
    pub fn rename_active_tab(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(), TerminalWorkspaceError> {
        let active = self.active_tab.ok_or(TerminalWorkspaceError::NoActiveTab)?;
        self.rename_tab(active, name)
    }

    /// Updates the split immediately containing the focused pane.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::InvalidSplitRatio`] outside 10%-90%,
    /// or [`TerminalWorkspaceError::NoFocusedSplit`] when the pane is a root.
    pub fn set_focused_split_ratio(
        &mut self,
        ratio_basis_points: u16,
    ) -> Result<(), TerminalWorkspaceError> {
        if !(MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(&ratio_basis_points) {
            return Err(TerminalWorkspaceError::InvalidSplitRatio);
        }
        let tab = self.active_tab_mut()?;
        if !tab
            .root
            .set_parent_ratio(tab.focused_pane, ratio_basis_points)
        {
            return Err(TerminalWorkspaceError::NoFocusedSplit);
        }
        Ok(())
    }

    /// Changes one pane's runtime lifecycle state.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::PaneNotFound`] for an unknown pane.
    pub fn set_pane_state(
        &mut self,
        pane: TerminalPaneId,
        state: PaneState,
    ) -> Result<(), TerminalWorkspaceError> {
        self.panes
            .get_mut(&pane)
            .ok_or(TerminalWorkspaceError::PaneNotFound)?
            .state = state;
        Ok(())
    }

    /// Moves a stopped or completed pane into its explicit starting state.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalWorkspaceError::PaneNotFound`] for an unknown pane or
    /// [`TerminalWorkspaceError::InvalidPaneTransition`] when a process is
    /// already starting, running, or backpressured.
    pub fn restart_pane(&mut self, pane: TerminalPaneId) -> Result<(), TerminalWorkspaceError> {
        let pane = self
            .panes
            .get_mut(&pane)
            .ok_or(TerminalWorkspaceError::PaneNotFound)?;
        match pane.state {
            PaneState::Stopped | PaneState::Exited { .. } | PaneState::Failed { .. } => {
                pane.state = PaneState::Starting;
                Ok(())
            }
            PaneState::Starting | PaneState::Running | PaneState::Backpressured => {
                Err(TerminalWorkspaceError::InvalidPaneTransition)
            }
        }
    }

    #[must_use]
    pub fn focused_pane(&self) -> Option<TerminalPaneId> {
        self.active_tab().map(TerminalTab::focused_pane)
    }

    #[must_use]
    pub fn active_tab(&self) -> Option<&TerminalTab> {
        self.active_tab
            .and_then(|active| self.tabs.iter().find(|tab| tab.id == active))
    }

    #[must_use]
    pub fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    pub fn panes(&self) -> impl Iterator<Item = &TerminalPane> {
        self.panes.values()
    }

    #[must_use]
    pub fn pane(&self, pane: TerminalPaneId) -> Option<&TerminalPane> {
        self.panes.get(&pane)
    }

    #[must_use]
    pub fn snapshot(&self) -> TerminalWorkspaceSnapshot {
        TerminalWorkspaceSnapshot {
            tabs: self
                .tabs
                .iter()
                .map(|tab| {
                    let mut pane_ids = Vec::new();
                    tab.root.collect_panes(&mut pane_ids);
                    TerminalTabSnapshot {
                        id: tab.id,
                        name: tab.name.clone(),
                        root: tab.root.clone(),
                        focused_pane: tab.focused_pane,
                        panes: pane_ids
                            .into_iter()
                            .filter_map(|id| self.panes.get(&id))
                            .map(|pane| TerminalPaneSnapshot {
                                id: pane.id,
                                working_directory: pane.working_directory.clone(),
                            })
                            .collect(),
                    }
                })
                .collect(),
            active_tab: self.active_tab,
        }
    }

    /// Restores a validated presentation snapshot with every pane stopped.
    ///
    /// # Errors
    ///
    /// Returns a specific [`TerminalWorkspaceError`] for malformed identifiers,
    /// trees, names, paths, focus, ratios, or active-tab references.
    pub fn restore(snapshot: TerminalWorkspaceSnapshot) -> Result<Self, TerminalWorkspaceError> {
        validate_snapshot(&snapshot)?;
        let mut workspace = Self {
            tabs: Vec::with_capacity(snapshot.tabs.len()),
            panes: BTreeMap::new(),
            active_tab: snapshot.active_tab,
        };
        for tab in snapshot.tabs {
            tab.id.reserve_after();
            for pane in tab.panes {
                pane.id.reserve_after();
                workspace.panes.insert(
                    pane.id,
                    TerminalPane {
                        id: pane.id,
                        working_directory: pane.working_directory,
                        state: PaneState::Stopped,
                    },
                );
            }
            workspace.tabs.push(TerminalTab {
                id: tab.id,
                name: tab.name,
                root: tab.root,
                focused_pane: tab.focused_pane,
            });
        }
        Ok(workspace)
    }

    fn tab_index(&self, tab: TerminalTabId) -> Option<usize> {
        self.tabs.iter().position(|candidate| candidate.id == tab)
    }

    fn active_tab_mut(&mut self) -> Result<&mut TerminalTab, TerminalWorkspaceError> {
        let active = self.active_tab.ok_or(TerminalWorkspaceError::NoActiveTab)?;
        let index = self
            .tab_index(active)
            .ok_or(TerminalWorkspaceError::TabNotFound)?;
        Ok(&mut self.tabs[index])
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TerminalWorkspaceError {
    #[error("terminal tab name must not be empty")]
    InvalidName,
    #[error("terminal working directory must not be empty")]
    InvalidWorkingDirectory,
    #[error("no terminal tab is active")]
    NoActiveTab,
    #[error("terminal tab was not found")]
    TabNotFound,
    #[error("terminal pane was not found")]
    PaneNotFound,
    #[error("split ratio must be between 1000 and 9000 basis points")]
    InvalidSplitRatio,
    #[error("focused pane has no parent split")]
    NoFocusedSplit,
    #[error("terminal pane cannot restart from its current lifecycle state")]
    InvalidPaneTransition,
    #[error("terminal snapshot contains duplicate identifiers")]
    DuplicateId,
    #[error("terminal snapshot layout is malformed")]
    InvalidLayout,
    #[error("terminal snapshot focused pane is invalid")]
    InvalidFocusedPane,
    #[error("terminal snapshot active tab is invalid")]
    InvalidActiveTab,
}

fn validate_snapshot(snapshot: &TerminalWorkspaceSnapshot) -> Result<(), TerminalWorkspaceError> {
    if snapshot.tabs.is_empty() {
        return if snapshot.active_tab.is_none() {
            Ok(())
        } else {
            Err(TerminalWorkspaceError::InvalidActiveTab)
        };
    }
    if !snapshot
        .active_tab
        .is_some_and(|active| snapshot.tabs.iter().any(|tab| tab.id == active))
    {
        return Err(TerminalWorkspaceError::InvalidActiveTab);
    }

    let mut tab_ids = BTreeSet::new();
    let mut pane_ids = BTreeSet::new();
    for tab in &snapshot.tabs {
        validated_name(tab.name.clone())?;
        if !tab_ids.insert(tab.id) {
            return Err(TerminalWorkspaceError::DuplicateId);
        }
        if !tab.root.validate() {
            return Err(TerminalWorkspaceError::InvalidSplitRatio);
        }
        let mut layout_panes = Vec::new();
        tab.root.collect_panes(&mut layout_panes);
        let layout_set = layout_panes.iter().copied().collect::<BTreeSet<_>>();
        if layout_set.len() != layout_panes.len() {
            return Err(TerminalWorkspaceError::DuplicateId);
        }
        if !layout_set.contains(&tab.focused_pane) {
            return Err(TerminalWorkspaceError::InvalidFocusedPane);
        }
        let declared_set = tab
            .panes
            .iter()
            .map(|pane| pane.id)
            .collect::<BTreeSet<_>>();
        if declared_set.len() != tab.panes.len() || declared_set != layout_set {
            return Err(TerminalWorkspaceError::InvalidLayout);
        }
        for pane in &tab.panes {
            validated_directory(pane.working_directory.clone())?;
            if !pane_ids.insert(pane.id) {
                return Err(TerminalWorkspaceError::DuplicateId);
            }
        }
    }
    Ok(())
}

fn validated_name(name: String) -> Result<String, TerminalWorkspaceError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err(TerminalWorkspaceError::InvalidName)
    } else if trimmed.len() == name.len() {
        Ok(name)
    } else {
        Ok(trimmed.to_owned())
    }
}

fn validated_directory(directory: PathBuf) -> Result<PathBuf, TerminalWorkspaceError> {
    if directory.as_os_str().is_empty() {
        Err(TerminalWorkspaceError::InvalidWorkingDirectory)
    } else {
        Ok(directory)
    }
}
