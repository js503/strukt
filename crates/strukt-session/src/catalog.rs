use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use strukt_terminal::SplitAxis;
use thiserror::Error;

use crate::id::{IdError, PaneId, SessionId, WindowId};

pub const MAX_SESSIONS: usize = 64;
pub const MAX_WINDOWS_PER_SESSION: usize = 32;
pub const MAX_PANES_PER_WINDOW: usize = 32;
pub const MAX_TOTAL_PANES: usize = 256;
const MAX_LAYOUT_DEPTH: usize = 16;
const MIN_SPLIT_RATIO: u16 = 1_000;
const MAX_SPLIT_RATIO: u16 = 9_000;
const DEFAULT_SPLIT_RATIO: u16 = 5_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PaneLifecycle {
    Stopped,
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { message: String },
    Backpressured,
}

impl PaneLifecycle {
    #[must_use]
    pub const fn is_live(&self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Backpressured)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionLayoutNode {
    Pane(PaneId),
    Split {
        axis: SplitAxis,
        ratio_basis_points: u16,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl SessionLayoutNode {
    fn contains(&self, pane: PaneId) -> bool {
        match self {
            Self::Pane(candidate) => *candidate == pane,
            Self::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    fn split(&mut self, focused: PaneId, new_pane: PaneId, axis: SplitAxis) -> bool {
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

    fn set_parent_ratio(&mut self, pane: PaneId, ratio: u16) -> bool {
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

    fn remove(self, pane: PaneId) -> Option<Self> {
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

    fn remap(&self, ids: &BTreeMap<PaneId, PaneId>) -> Result<Self, CatalogError> {
        match self {
            Self::Pane(pane) => Ok(Self::Pane(
                ids.get(pane).copied().ok_or(CatalogError::PaneNotFound)?,
            )),
            Self::Split {
                axis,
                ratio_basis_points,
                first,
                second,
            } => Ok(Self::Split {
                axis: *axis,
                ratio_basis_points: *ratio_basis_points,
                first: Box::new(first.remap(ids)?),
                second: Box::new(second.remap(ids)?),
            }),
        }
    }

    fn validate(&self, panes: &BTreeMap<PaneId, SessionPane>) -> bool {
        let mut found = BTreeSet::new();
        self.validate_at(panes, 1, &mut found) && found.len() == panes.len()
    }

    fn validate_at(
        &self,
        panes: &BTreeMap<PaneId, SessionPane>,
        depth: usize,
        found: &mut BTreeSet<PaneId>,
    ) -> bool {
        if depth > MAX_LAYOUT_DEPTH {
            return false;
        }
        match self {
            Self::Pane(pane) => panes.contains_key(pane) && found.insert(*pane),
            Self::Split {
                ratio_basis_points,
                first,
                second,
                ..
            } => {
                (MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(ratio_basis_points)
                    && first.validate_at(panes, depth + 1, found)
                    && second.validate_at(panes, depth + 1, found)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionPane {
    id: PaneId,
    working_directory: PathBuf,
    generation: u64,
    lifecycle: PaneLifecycle,
}

impl SessionPane {
    #[must_use]
    pub const fn id(&self) -> PaneId {
        self.id
    }

    #[must_use]
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn lifecycle(&self) -> &PaneLifecycle {
        &self.lifecycle
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionWindow {
    id: WindowId,
    name: String,
    revision: u64,
    root: SessionLayoutNode,
    focused_pane: PaneId,
    panes: BTreeMap<PaneId, SessionPane>,
}

impl SessionWindow {
    #[must_use]
    pub const fn id(&self) -> WindowId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn root(&self) -> &SessionLayoutNode {
        &self.root
    }

    #[must_use]
    pub fn focused_pane(&self) -> &SessionPane {
        &self.panes[&self.focused_pane]
    }

    #[must_use]
    pub fn panes(&self) -> impl ExactSizeIterator<Item = &SessionPane> {
        self.panes.values()
    }

    #[must_use]
    pub fn pane(&self, pane: PaneId) -> Option<&SessionPane> {
        self.panes.get(&pane)
    }

    #[must_use]
    pub fn validate(&self) -> bool {
        !self.panes.is_empty()
            && self.panes.len() <= MAX_PANES_PER_WINDOW
            && self.panes.contains_key(&self.focused_pane)
            && self.root.validate(&self.panes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Session {
    id: SessionId,
    name: String,
    revision: u64,
    windows: Vec<SessionWindow>,
    active_window: WindowId,
}

impl Session {
    #[must_use]
    pub const fn id(&self) -> SessionId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn windows(&self) -> &[SessionWindow] {
        &self.windows
    }

    #[must_use]
    pub fn active_window(&self) -> Option<&SessionWindow> {
        self.windows
            .iter()
            .find(|window| window.id == self.active_window)
    }

    fn active_window_mut(&mut self) -> Result<&mut SessionWindow, CatalogError> {
        self.windows
            .iter_mut()
            .find(|window| window.id == self.active_window)
            .ok_or(CatalogError::WindowNotFound)
    }

    fn has_live_panes(&self) -> bool {
        self.windows
            .iter()
            .flat_map(SessionWindow::panes)
            .any(|pane| pane.lifecycle.is_live())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionCatalog {
    revision: u64,
    sessions: BTreeMap<SessionId, Session>,
    active_session: Option<SessionId>,
}

impl SessionCatalog {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            revision: 0,
            sessions: BTreeMap::new(),
            active_session: None,
        }
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn active_session_id(&self) -> Option<SessionId> {
        self.active_session
    }

    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    #[must_use]
    pub fn session(&self, session: SessionId) -> Option<&Session> {
        self.sessions.get(&session)
    }

    /// Validates a complete deserialized catalog.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidCatalog`] when any hierarchy, identifier,
    /// capacity, name, path, focus, or revision invariant is invalid.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.sessions.len() > MAX_SESSIONS {
            return Err(CatalogError::InvalidCatalog);
        }
        if self.sessions.is_empty() != self.active_session.is_none()
            || self
                .active_session
                .is_some_and(|active| !self.sessions.contains_key(&active))
        {
            return Err(CatalogError::InvalidCatalog);
        }
        let mut pane_ids = BTreeSet::new();
        let mut total_panes = 0_usize;
        for (session_id, session) in &self.sessions {
            if *session_id != session.id
                || session.revision == 0
                || validated_name(&session.name).is_err()
                || session.windows.is_empty()
                || session.windows.len() > MAX_WINDOWS_PER_SESSION
                || !session
                    .windows
                    .iter()
                    .any(|window| window.id == session.active_window)
            {
                return Err(CatalogError::InvalidCatalog);
            }
            let mut window_ids = BTreeSet::new();
            for window in &session.windows {
                if !window_ids.insert(window.id)
                    || window.revision == 0
                    || validated_name(&window.name).is_err()
                    || !window.validate()
                {
                    return Err(CatalogError::InvalidCatalog);
                }
                total_panes = total_panes
                    .checked_add(window.panes.len())
                    .ok_or(CatalogError::InvalidCatalog)?;
                for (pane_id, pane) in &window.panes {
                    if *pane_id != pane.id
                        || pane.working_directory.as_os_str().is_empty()
                        || !pane_ids.insert(*pane_id)
                    {
                        return Err(CatalogError::InvalidCatalog);
                    }
                }
            }
        }
        if total_panes > MAX_TOTAL_PANES {
            return Err(CatalogError::InvalidCatalog);
        }
        Ok(())
    }

    /// Returns a validated clone suitable for service-restart persistence.
    ///
    /// Every pane is normalized to generation zero and stopped.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidCatalog`] for invalid source state.
    pub fn stopped_clone(&self) -> Result<Self, CatalogError> {
        self.validate()?;
        let mut stopped = self.clone();
        for pane in stopped
            .sessions
            .values_mut()
            .flat_map(|session| &mut session.windows)
            .flat_map(|window| window.panes.values_mut())
        {
            pane.generation = 0;
            pane.lifecycle = PaneLifecycle::Stopped;
        }
        Ok(stopped)
    }

    #[must_use]
    pub fn contains_pane(&self, pane: PaneId) -> bool {
        self.sessions
            .values()
            .flat_map(|session| &session.windows)
            .any(|window| window.panes.contains_key(&pane))
    }

    /// Creates one stopped session with one window and pane.
    ///
    /// # Errors
    ///
    /// Returns validation, capacity, revision, or random-source errors.
    pub fn create_session(
        &mut self,
        expected_revision: u64,
        name: impl Into<String>,
        working_directory: impl AsRef<Path>,
    ) -> Result<SessionId, CatalogError> {
        self.expect_revision(expected_revision)?;
        if self.sessions.len() >= MAX_SESSIONS || self.total_panes() >= MAX_TOTAL_PANES {
            return Err(CatalogError::CapacityReached);
        }
        let name = name.into();
        let name = validated_name(&name)?;
        let directory = validated_directory(working_directory.as_ref())?;
        let session_id = SessionId::new()?;
        let window_id = WindowId::new()?;
        let pane_id = PaneId::new()?;
        let pane = SessionPane {
            id: pane_id,
            working_directory: directory,
            generation: 0,
            lifecycle: PaneLifecycle::Stopped,
        };
        self.sessions.insert(
            session_id,
            Session {
                id: session_id,
                name,
                revision: 1,
                windows: vec![SessionWindow {
                    id: window_id,
                    name: "shell".to_owned(),
                    revision: 1,
                    root: SessionLayoutNode::Pane(pane_id),
                    focused_pane: pane_id,
                    panes: BTreeMap::from([(pane_id, pane)]),
                }],
                active_window: window_id,
            },
        );
        self.active_session = Some(session_id);
        self.bump_revision();
        Ok(session_id)
    }

    /// Renames a session.
    ///
    /// # Errors
    ///
    /// Returns stale revision, invalid name, or unknown session errors.
    pub fn rename_session(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        name: impl Into<String>,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let name = name.into();
        let name = validated_name(&name)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        target.name = name;
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Activates an existing session.
    ///
    /// # Errors
    ///
    /// Returns stale revision or unknown session errors.
    pub fn activate_session(
        &mut self,
        expected_revision: u64,
        session: SessionId,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        if !self.sessions.contains_key(&session) {
            return Err(CatalogError::SessionNotFound);
        }
        self.active_session = Some(session);
        self.bump_revision();
        Ok(())
    }

    /// Creates and activates a stopped window with one pane.
    ///
    /// # Errors
    ///
    /// Returns stale revision, validation, capacity, hierarchy, or ID errors.
    pub fn create_window(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        name: impl Into<String>,
        working_directory: impl AsRef<Path>,
    ) -> Result<WindowId, CatalogError> {
        self.expect_revision(expected_revision)?;
        if self.total_panes() >= MAX_TOTAL_PANES {
            return Err(CatalogError::CapacityReached);
        }
        let name = name.into();
        let name = validated_name(&name)?;
        let directory = validated_directory(working_directory.as_ref())?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        if target.windows.len() >= MAX_WINDOWS_PER_SESSION {
            return Err(CatalogError::CapacityReached);
        }
        let window_id = WindowId::new()?;
        let pane_id = PaneId::new()?;
        target.windows.push(SessionWindow {
            id: window_id,
            name,
            revision: 1,
            root: SessionLayoutNode::Pane(pane_id),
            focused_pane: pane_id,
            panes: BTreeMap::from([(
                pane_id,
                SessionPane {
                    id: pane_id,
                    working_directory: directory,
                    generation: 0,
                    lifecycle: PaneLifecycle::Stopped,
                },
            )]),
        });
        target.active_window = window_id;
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(window_id)
    }

    /// Renames one window.
    ///
    /// # Errors
    ///
    /// Returns stale revision, invalid name, or hierarchy errors.
    pub fn rename_window(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        window: WindowId,
        name: impl Into<String>,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let name = name.into();
        let name = validated_name(&name)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let window = target
            .windows
            .iter_mut()
            .find(|candidate| candidate.id == window)
            .ok_or(CatalogError::WindowNotFound)?;
        window.name = name;
        window.revision = window.revision.saturating_add(1);
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Activates one window.
    ///
    /// # Errors
    ///
    /// Returns stale revision or hierarchy errors.
    pub fn activate_window(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        window: WindowId,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        if !target
            .windows
            .iter()
            .any(|candidate| candidate.id == window)
        {
            return Err(CatalogError::WindowNotFound);
        }
        target.active_window = window;
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Closes a stopped non-final window.
    ///
    /// # Errors
    ///
    /// Returns stale revision, hierarchy, live-pane, or final-window errors.
    pub fn close_window(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        window: WindowId,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let index = target
            .windows
            .iter()
            .position(|candidate| candidate.id == window)
            .ok_or(CatalogError::WindowNotFound)?;
        if target.windows[index]
            .panes()
            .any(|pane| pane.lifecycle.is_live())
        {
            return Err(CatalogError::WindowRunning);
        }
        if target.windows.len() == 1 {
            return Err(CatalogError::LastWindow);
        }
        target.windows.remove(index);
        if target.active_window == window {
            target.active_window = target
                .windows
                .get(index)
                .or_else(|| target.windows.last())
                .ok_or(CatalogError::LastWindow)?
                .id;
        }
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Duplicates presentation definitions into a new stopped session.
    ///
    /// # Errors
    ///
    /// Returns validation, capacity, revision, or random-source errors.
    pub fn duplicate_session(
        &mut self,
        expected_revision: u64,
        source: SessionId,
    ) -> Result<SessionId, CatalogError> {
        self.expect_revision(expected_revision)?;
        let source = self
            .sessions
            .get(&source)
            .cloned()
            .ok_or(CatalogError::SessionNotFound)?;
        let source_panes = source
            .windows
            .iter()
            .map(|window| window.panes.len())
            .sum::<usize>();
        if self.sessions.len() >= MAX_SESSIONS
            || self.total_panes().saturating_add(source_panes) > MAX_TOTAL_PANES
        {
            return Err(CatalogError::CapacityReached);
        }
        let session_id = SessionId::new()?;
        let mut windows = Vec::with_capacity(source.windows.len());
        let mut active_window = None;
        for source_window in &source.windows {
            let window_id = WindowId::new()?;
            let mut ids = BTreeMap::new();
            let mut panes = BTreeMap::new();
            for source_pane in source_window.panes.values() {
                let pane_id = PaneId::new()?;
                ids.insert(source_pane.id, pane_id);
                panes.insert(
                    pane_id,
                    SessionPane {
                        id: pane_id,
                        working_directory: source_pane.working_directory.clone(),
                        generation: 0,
                        lifecycle: PaneLifecycle::Stopped,
                    },
                );
            }
            let focused_pane = ids
                .get(&source_window.focused_pane)
                .copied()
                .ok_or(CatalogError::PaneNotFound)?;
            if source_window.id == source.active_window {
                active_window = Some(window_id);
            }
            windows.push(SessionWindow {
                id: window_id,
                name: source_window.name.clone(),
                revision: 1,
                root: source_window.root.remap(&ids)?,
                focused_pane,
                panes,
            });
        }
        let duplicate_name = validated_name(&format!("{} copy", source.name))?;
        self.sessions.insert(
            session_id,
            Session {
                id: session_id,
                name: duplicate_name,
                revision: 1,
                windows,
                active_window: active_window.ok_or(CatalogError::WindowNotFound)?,
            },
        );
        self.active_session = Some(session_id);
        self.bump_revision();
        Ok(session_id)
    }

    /// Splits the active window's focused pane.
    ///
    /// # Errors
    ///
    /// Returns stale revision, capacity, hierarchy, or random-source errors.
    pub fn split_focused(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        axis: SplitAxis,
    ) -> Result<PaneId, CatalogError> {
        self.expect_revision(expected_revision)?;
        if self.total_panes() >= MAX_TOTAL_PANES {
            return Err(CatalogError::CapacityReached);
        }
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let window = target.active_window_mut()?;
        if window.panes.len() >= MAX_PANES_PER_WINDOW {
            return Err(CatalogError::CapacityReached);
        }
        let focused = window.focused_pane;
        let directory = window
            .panes
            .get(&focused)
            .ok_or(CatalogError::PaneNotFound)?
            .working_directory
            .clone();
        let new_pane = PaneId::new()?;
        let mut next_root = window.root.clone();
        if !next_root.split(focused, new_pane, axis) {
            return Err(CatalogError::PaneNotFound);
        }
        let mut next_panes = window.panes.clone();
        next_panes.insert(
            new_pane,
            SessionPane {
                id: new_pane,
                working_directory: directory,
                generation: 0,
                lifecycle: PaneLifecycle::Stopped,
            },
        );
        if !next_root.validate(&next_panes) {
            return Err(CatalogError::LayoutTooDeep);
        }
        window.root = next_root;
        window.panes = next_panes;
        window.focused_pane = new_pane;
        window.revision = window.revision.saturating_add(1);
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(new_pane)
    }

    /// Changes the split immediately containing the focused pane.
    ///
    /// # Errors
    ///
    /// Returns stale revision, invalid ratio, or hierarchy errors.
    pub fn set_focused_split_ratio(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        ratio_basis_points: u16,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        if !(MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(&ratio_basis_points) {
            return Err(CatalogError::InvalidSplitRatio);
        }
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let window = target.active_window_mut()?;
        if !window
            .root
            .set_parent_ratio(window.focused_pane, ratio_basis_points)
        {
            return Err(CatalogError::NoFocusedSplit);
        }
        window.revision = window.revision.saturating_add(1);
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Focuses a pane in the active window.
    ///
    /// # Errors
    ///
    /// Returns stale revision or hierarchy errors.
    pub fn focus_pane(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        pane: PaneId,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let window = target.active_window_mut()?;
        if !window.root.contains(pane) || !window.panes.contains_key(&pane) {
            return Err(CatalogError::PaneNotFound);
        }
        window.focused_pane = pane;
        window.revision = window.revision.saturating_add(1);
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Closes a stopped non-final pane and collapses its split branch.
    ///
    /// # Errors
    ///
    /// Returns stale revision, hierarchy, live-pane, or final-pane errors.
    pub fn close_pane(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        pane: PaneId,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let window = target
            .windows
            .iter_mut()
            .find(|window| window.panes.contains_key(&pane))
            .ok_or(CatalogError::PaneNotFound)?;
        if window
            .panes
            .get(&pane)
            .ok_or(CatalogError::PaneNotFound)?
            .lifecycle
            .is_live()
        {
            return Err(CatalogError::PaneRunning);
        }
        if window.panes.len() == 1 {
            return Err(CatalogError::LastPane);
        }
        let root = window
            .root
            .clone()
            .remove(pane)
            .ok_or(CatalogError::LastPane)?;
        window.panes.remove(&pane);
        window.root = root;
        if window.focused_pane == pane {
            window.focused_pane = window
                .panes
                .keys()
                .next()
                .copied()
                .ok_or(CatalogError::LastPane)?;
        }
        if !window.validate() {
            return Err(CatalogError::InvalidLayout);
        }
        window.revision = window.revision.saturating_add(1);
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Applies a generation-independent lifecycle projection to one pane.
    ///
    /// # Errors
    ///
    /// Returns stale revision or hierarchy errors.
    pub fn set_pane_lifecycle(
        &mut self,
        expected_revision: u64,
        session: SessionId,
        pane: PaneId,
        lifecycle: PaneLifecycle,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let target = self
            .sessions
            .get_mut(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        let window = target
            .windows
            .iter_mut()
            .find(|window| window.panes.contains_key(&pane))
            .ok_or(CatalogError::PaneNotFound)?;
        window
            .panes
            .get_mut(&pane)
            .ok_or(CatalogError::PaneNotFound)?
            .lifecycle = lifecycle;
        window.revision = window.revision.saturating_add(1);
        target.revision = target.revision.saturating_add(1);
        self.bump_revision();
        Ok(())
    }

    /// Removes a stopped session.
    ///
    /// # Errors
    ///
    /// Returns stale revision, unknown session, or live-pane errors.
    pub fn remove_session(
        &mut self,
        expected_revision: u64,
        session: SessionId,
    ) -> Result<(), CatalogError> {
        self.expect_revision(expected_revision)?;
        let target = self
            .sessions
            .get(&session)
            .ok_or(CatalogError::SessionNotFound)?;
        if target.has_live_panes() {
            return Err(CatalogError::SessionRunning);
        }
        self.sessions.remove(&session);
        if self.active_session == Some(session) {
            self.active_session = self.sessions.keys().next().copied();
        }
        self.bump_revision();
        Ok(())
    }

    fn expect_revision(&self, expected: u64) -> Result<(), CatalogError> {
        if expected == self.revision {
            Ok(())
        } else {
            Err(CatalogError::StaleRevision {
                expected,
                actual: self.revision,
            })
        }
    }

    fn total_panes(&self) -> usize {
        self.sessions
            .values()
            .flat_map(|session| &session.windows)
            .map(|window| window.panes.len())
            .sum()
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn validated_name(name: &str) -> Result<String, CatalogError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(CatalogError::InvalidName);
    }
    Ok(name.to_owned())
}

fn validated_directory(directory: &Path) -> Result<PathBuf, CatalogError> {
    if directory.as_os_str().is_empty() {
        return Err(CatalogError::InvalidWorkingDirectory);
    }
    Ok(directory.to_path_buf())
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CatalogError {
    #[error("stale catalog revision: expected {expected}, actual {actual}")]
    StaleRevision { expected: u64, actual: u64 },
    #[error("session name is invalid")]
    InvalidName,
    #[error("working directory is invalid")]
    InvalidWorkingDirectory,
    #[error("session capacity reached")]
    CapacityReached,
    #[error("session not found")]
    SessionNotFound,
    #[error("window not found")]
    WindowNotFound,
    #[error("pane not found")]
    PaneNotFound,
    #[error("split ratio must be between 10 and 90 percent")]
    InvalidSplitRatio,
    #[error("focused pane has no parent split")]
    NoFocusedSplit,
    #[error("session layout is too deep")]
    LayoutTooDeep,
    #[error("session layout is invalid")]
    InvalidLayout,
    #[error("session catalog is invalid")]
    InvalidCatalog,
    #[error("the last pane must be closed with its window")]
    LastPane,
    #[error("the last window must be closed with its session")]
    LastWindow,
    #[error("running pane must be terminated before close")]
    PaneRunning,
    #[error("window contains running panes")]
    WindowRunning,
    #[error("running session must be terminated before removal")]
    SessionRunning,
    #[error(transparent)]
    Id(#[from] IdError),
}
