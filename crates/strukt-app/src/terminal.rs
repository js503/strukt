use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use strukt_persistence::TerminalSessionSnapshot;
use strukt_terminal::{
    DrainBudget, FocusEvent, GridSize, MouseEvent, PaneState, PasteDecision, PortableTransport,
    RuntimeBatch, RuntimePaneHealth, RuntimePaneState, RuntimeStartJob, Selection, SpawnRequest,
    SplitAxis, TerminalKey, TerminalLink, TerminalPaneId, TerminalProcess, TerminalRuntime,
    TerminalSize, TerminalSnapshot, TerminalTabId, TerminalWorkspace, TerminalWorkspaceError,
    TransportError,
};
use thiserror::Error;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLUMNS: u16 = 80;
const DEFAULT_SCROLLBACK: usize = 10_000;

pub(crate) struct TerminalSurfaces {
    workspace: TerminalWorkspace,
    runtime: TerminalRuntime,
    selections: BTreeMap<TerminalPaneId, Selection>,
    viewport_offsets: BTreeMap<TerminalPaneId, usize>,
}

type TerminalSpawnResult = Result<Box<dyn TerminalProcess>, String>;

#[derive(Clone)]
pub(crate) struct TerminalSpawnCompletion(Arc<Mutex<Option<TerminalSpawnResult>>>);

impl std::fmt::Debug for TerminalSpawnCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("TerminalSpawnCompletion").finish()
    }
}

impl TerminalSpawnCompletion {
    pub(crate) fn new(result: TerminalSpawnResult) -> Self {
        Self(Arc::new(Mutex::new(Some(result))))
    }

    fn take(&self) -> Option<TerminalSpawnResult> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

impl Default for TerminalSurfaces {
    fn default() -> Self {
        Self {
            workspace: TerminalWorkspace::default(),
            runtime: TerminalRuntime::new(Arc::new(PortableTransport::new()), DEFAULT_SCROLLBACK),
            selections: BTreeMap::new(),
            viewport_offsets: BTreeMap::new(),
        }
    }
}

impl TerminalSurfaces {
    pub(crate) fn restore(
        snapshot: Option<&TerminalSessionSnapshot>,
    ) -> Result<Self, TerminalSurfaceError> {
        let workspace = snapshot.map_or_else(
            || Ok(TerminalWorkspace::default()),
            TerminalSessionSnapshot::restore,
        )?;
        let mut surfaces = Self {
            workspace,
            ..Self::default()
        };
        for pane in surfaces.workspace.panes() {
            surfaces.runtime.prepare(pane.id(), default_grid_size());
        }
        Ok(surfaces)
    }

    pub(crate) const fn workspace(&self) -> &TerminalWorkspace {
        &self.workspace
    }

    pub(crate) fn new_tab(&mut self, root: &Path) -> Result<TerminalPaneId, TerminalSurfaceError> {
        let number = self.workspace.tabs().len() + 1;
        let pane = self
            .workspace
            .create_tab(format!("Terminal {number}"), root)?;
        self.runtime.prepare(pane, default_grid_size());
        Ok(pane)
    }

    pub(crate) fn split_focused(
        &mut self,
        axis: SplitAxis,
    ) -> Result<TerminalPaneId, TerminalSurfaceError> {
        let pane = self.workspace.split_focused(axis)?;
        self.runtime.prepare(pane, default_grid_size());
        Ok(pane)
    }

    pub(crate) fn activate_tab(&mut self, tab: TerminalTabId) -> Result<(), TerminalSurfaceError> {
        let previous = self.workspace.focused_pane();
        self.workspace.activate_tab(tab)?;
        let current = self.workspace.focused_pane();
        self.report_focus_change(previous, current)?;
        Ok(())
    }

    pub(crate) fn focus_pane(&mut self, pane: TerminalPaneId) -> Result<(), TerminalSurfaceError> {
        let previous = self.workspace.focused_pane();
        self.workspace.focus_pane(pane)?;
        self.report_focus_change(previous, Some(pane))?;
        Ok(())
    }

    pub(crate) fn activate_relative_tab(
        &mut self,
        reverse: bool,
    ) -> Result<(), TerminalSurfaceError> {
        let tabs = self.workspace.tabs();
        let active = self
            .workspace
            .active_tab()
            .ok_or(TerminalWorkspaceError::NoActiveTab)?
            .id();
        let index = tabs
            .iter()
            .position(|tab| tab.id() == active)
            .ok_or(TerminalWorkspaceError::TabNotFound)?;
        let next = if reverse {
            index.checked_sub(1).unwrap_or(tabs.len() - 1)
        } else {
            (index + 1) % tabs.len()
        };
        self.activate_tab(tabs[next].id())
    }

    pub(crate) fn focus_relative_pane(
        &mut self,
        reverse: bool,
    ) -> Result<(), TerminalSurfaceError> {
        let active = self
            .workspace
            .active_tab()
            .ok_or(TerminalWorkspaceError::NoActiveTab)?;
        let snapshot = self.workspace.snapshot();
        let tab = snapshot
            .tabs
            .iter()
            .find(|tab| tab.id == active.id())
            .ok_or(TerminalWorkspaceError::TabNotFound)?;
        let index = tab
            .panes
            .iter()
            .position(|pane| pane.id == active.focused_pane())
            .ok_or(TerminalWorkspaceError::PaneNotFound)?;
        let next = if reverse {
            index.checked_sub(1).unwrap_or(tab.panes.len() - 1)
        } else {
            (index + 1) % tab.panes.len()
        };
        self.focus_pane(tab.panes[next].id)
    }

    pub(crate) fn rename_active_tab(&mut self, name: String) -> Result<(), TerminalSurfaceError> {
        self.workspace.rename_active_tab(name)?;
        Ok(())
    }

    pub(crate) fn begin_start(
        &mut self,
        pane: TerminalPaneId,
    ) -> Result<RuntimeStartJob, TerminalSurfaceError> {
        let working_directory = self
            .workspace
            .pane(pane)
            .ok_or(TerminalWorkspaceError::PaneNotFound)?
            .working_directory()
            .clone();
        let request = shell_request(working_directory)?;
        self.workspace.restart_pane(pane)?;
        match self.runtime.begin_restart(pane, request) {
            Ok(job) => Ok(job),
            Err(error) => {
                let message = error.to_string();
                self.workspace
                    .set_pane_state(pane, PaneState::Failed { message })?;
                Err(error.into())
            }
        }
    }

    pub(crate) fn finish_start(
        &mut self,
        pane: TerminalPaneId,
        generation: u64,
        completion: &TerminalSpawnCompletion,
    ) -> Result<(), TerminalSurfaceError> {
        let Some(result) = completion.take() else {
            return Ok(());
        };
        let current = self.runtime.generation(pane) == Some(generation);
        match self.runtime.finish_restart(pane, generation, result) {
            Ok(()) if !current => Ok(()),
            Ok(()) => {
                self.workspace.set_pane_state(pane, PaneState::Running)?;
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                self.workspace
                    .set_pane_state(pane, PaneState::Failed { message })?;
                Err(error.into())
            }
        }
    }

    pub(crate) fn poll(&mut self) -> RuntimeBatch {
        let batch = self.runtime.drain(DrainBudget::default());
        for pane in batch.changed_panes() {
            if let Some(state) = self.runtime.state(*pane) {
                let state = match state {
                    RuntimePaneState::Stopped => PaneState::Stopped,
                    RuntimePaneState::Starting => PaneState::Starting,
                    RuntimePaneState::Running => PaneState::Running,
                    RuntimePaneState::Exited { code } => PaneState::Exited { code: *code },
                    RuntimePaneState::Failed { message } => PaneState::Failed {
                        message: message.clone(),
                    },
                    RuntimePaneState::Backpressured => PaneState::Backpressured,
                };
                let _ = self.workspace.set_pane_state(*pane, state);
            }
        }
        batch
    }

    #[must_use]
    pub(crate) fn snapshot(&self, pane: TerminalPaneId) -> Option<TerminalSnapshot> {
        self.runtime
            .snapshot_at(pane, self.viewport_offsets.get(&pane).copied().unwrap_or(0))
    }

    pub(crate) fn resize(
        &mut self,
        pane: TerminalPaneId,
        size: TerminalSize,
    ) -> Result<(), TerminalSurfaceError> {
        if self.workspace.pane(pane).is_some_and(|pane| {
            matches!(
                pane.state(),
                PaneState::Running | PaneState::Backpressured | PaneState::Starting
            )
        }) {
            self.runtime.resize(pane, size)?;
        }
        Ok(())
    }

    pub(crate) fn select(
        &mut self,
        pane: TerminalPaneId,
        selection: Selection,
    ) -> Result<(), TerminalSurfaceError> {
        if self.workspace.pane(pane).is_none() {
            return Err(TerminalWorkspaceError::PaneNotFound.into());
        }
        self.selections.insert(pane, selection);
        Ok(())
    }

    pub(crate) fn scroll(
        &mut self,
        pane: TerminalPaneId,
        lines: i32,
    ) -> Result<(), TerminalSurfaceError> {
        if self.workspace.pane(pane).is_none() {
            return Err(TerminalWorkspaceError::PaneNotFound.into());
        }
        let offset = self.viewport_offsets.entry(pane).or_default();
        if lines > 0 {
            if let Ok(lines) = usize::try_from(lines) {
                *offset = offset.saturating_add(lines).min(100_000);
            }
        } else {
            *offset = offset.saturating_sub(lines.unsigned_abs() as usize);
        }
        Ok(())
    }

    pub(crate) fn write(
        &mut self,
        pane: TerminalPaneId,
        bytes: &[u8],
    ) -> Result<(), TerminalSurfaceError> {
        self.viewport_offsets.insert(pane, 0);
        self.runtime.write(pane, bytes)?;
        Ok(())
    }

    pub(crate) fn write_key(
        &mut self,
        pane: TerminalPaneId,
        key: TerminalKey,
    ) -> Result<(), TerminalSurfaceError> {
        let bytes = self.runtime.encode_key(pane, key)?;
        self.write(pane, &bytes)
    }

    pub(crate) fn write_mouse(
        &mut self,
        pane: TerminalPaneId,
        event: MouseEvent,
    ) -> Result<(), TerminalSurfaceError> {
        if let Some(bytes) = self.runtime.encode_mouse(pane, event)? {
            self.write(pane, &bytes)?;
        }
        Ok(())
    }

    pub(crate) fn prepare_paste(
        &self,
        pane: TerminalPaneId,
        text: &str,
        confirmed: bool,
    ) -> Result<PasteDecision, TerminalSurfaceError> {
        Ok(self.runtime.prepare_paste(pane, text, confirmed)?)
    }

    pub(crate) fn copy_selection(
        &self,
        pane: TerminalPaneId,
    ) -> Result<Option<String>, TerminalSurfaceError> {
        self.selections
            .get(&pane)
            .map(|selection| {
                self.runtime.copy_text_at(
                    pane,
                    self.viewport_offsets.get(&pane).copied().unwrap_or(0),
                    selection,
                )
            })
            .transpose()
            .map_err(Into::into)
    }

    #[must_use]
    pub(crate) fn selection(&self, pane: TerminalPaneId) -> Option<Selection> {
        self.selections.get(&pane).copied()
    }

    pub(crate) fn links(
        &self,
        pane: TerminalPaneId,
    ) -> Result<Vec<TerminalLink>, TerminalSurfaceError> {
        Ok(self.runtime.links(pane)?)
    }

    #[must_use]
    pub(crate) fn health(&self, pane: TerminalPaneId) -> Option<RuntimePaneHealth> {
        self.runtime.health(pane)
    }

    pub(crate) fn close_and_take_process(
        &mut self,
        pane: TerminalPaneId,
    ) -> Result<Option<Box<dyn TerminalProcess>>, TerminalSurfaceError> {
        if self.workspace.pane(pane).is_none() {
            return Err(TerminalWorkspaceError::PaneNotFound.into());
        }
        let process = self.runtime.take_process_and_discard(pane);
        self.selections.remove(&pane);
        self.viewport_offsets.remove(&pane);
        self.workspace.close_pane(pane)?;
        Ok(process)
    }

    #[must_use]
    pub(crate) fn running(&self, pane: TerminalPaneId) -> bool {
        self.workspace.pane(pane).is_some_and(|pane| {
            matches!(
                pane.state(),
                PaneState::Starting | PaneState::Running | PaneState::Backpressured
            )
        })
    }

    #[must_use]
    pub(crate) fn running_processes(&self) -> usize {
        self.runtime.running_processes()
    }

    fn report_focus_change(
        &mut self,
        previous: Option<TerminalPaneId>,
        current: Option<TerminalPaneId>,
    ) -> Result<(), TerminalSurfaceError> {
        if previous == current {
            return Ok(());
        }
        if let Some(previous) = previous
            && self.running(previous)
            && let Some(bytes) = self.runtime.encode_focus(previous, FocusEvent::Out)?
        {
            self.runtime.write(previous, &bytes)?;
        }
        if let Some(current) = current
            && self.running(current)
            && let Some(bytes) = self.runtime.encode_focus(current, FocusEvent::In)?
        {
            self.runtime.write(current, &bytes)?;
        }
        Ok(())
    }

    #[must_use]
    pub(crate) fn session_snapshot(&self) -> TerminalSessionSnapshot {
        TerminalSessionSnapshot::from_workspace(&self.workspace)
    }
}

fn default_grid_size() -> GridSize {
    GridSize::new(usize::from(DEFAULT_ROWS), usize::from(DEFAULT_COLUMNS))
        .expect("default terminal grid is nonempty")
}

fn shell_request(working_directory: PathBuf) -> Result<SpawnRequest, TerminalSurfaceError> {
    Ok(SpawnRequest {
        executable: default_shell()?,
        arguments: Vec::new(),
        working_directory,
        environment: vec![
            (OsString::from("TERM"), OsString::from("xterm-256color")),
            (OsString::from("COLORTERM"), OsString::from("truecolor")),
        ],
        size: TerminalSize::new(DEFAULT_ROWS, DEFAULT_COLUMNS)?,
    })
}

#[cfg(windows)]
fn default_shell() -> Result<PathBuf, TerminalSurfaceError> {
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let powershell = PathBuf::from(program_files).join("PowerShell/7/pwsh.exe");
        if powershell.is_file() {
            return Ok(powershell);
        }
    }
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        let powershell =
            PathBuf::from(system_root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        if powershell.is_file() {
            return Ok(powershell);
        }
    }
    std::env::var_os("COMSPEC")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("cmd.exe")))
        .ok_or(TerminalSurfaceError::ShellUnavailable)
}

#[cfg(not(windows))]
fn default_shell() -> Result<PathBuf, TerminalSurfaceError> {
    if let Some(shell) = std::env::var_os("SHELL")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
    {
        return Ok(shell);
    }
    for fallback in [PathBuf::from("/bin/zsh"), PathBuf::from("/bin/sh")] {
        if fallback.is_file() {
            return Ok(fallback);
        }
    }
    Err(TerminalSurfaceError::ShellUnavailable)
}

#[derive(Debug, Error)]
pub(crate) enum TerminalSurfaceError {
    #[error("no usable default shell was found")]
    ShellUnavailable,
    #[error(transparent)]
    Layout(#[from] TerminalWorkspaceError),
    #[error(transparent)]
    Persistence(#[from] strukt_persistence::TerminalStoreError),
    #[error(transparent)]
    Runtime(#[from] strukt_terminal::RuntimeError),
    #[error(transparent)]
    Transport(#[from] TransportError),
}
