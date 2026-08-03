use serde::{Deserialize, Serialize};
use strukt_terminal::{Cell, TerminalSnapshot};
use thiserror::Error;

use crate::{PaneLifecycle, ProviderCapabilities, ProviderKind, ServiceInstanceId, SessionCatalog};

const MAX_ROWS: usize = 2_048;
const MAX_COLUMNS: usize = 512;
const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
const MAX_TITLE_BYTES: usize = 512;
const MAX_FAILURE_BYTES: usize = 1_024;
const CELL_OVERHEAD_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum AttentionState {
    #[default]
    None,
    Unread,
    Attention,
}

impl AttentionState {
    #[must_use]
    pub const fn on_output(self, active_at_newest_revision: bool) -> Self {
        if active_at_newest_revision {
            self
        } else {
            match self {
                Self::None => Self::Unread,
                Self::Unread | Self::Attention => self,
            }
        }
    }

    #[must_use]
    pub const fn on_bell(self) -> Self {
        Self::Attention
    }

    #[must_use]
    pub const fn on_viewed(self) -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CursorSnapshot {
    pub row: usize,
    pub column: usize,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ModesSnapshot {
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub mouse_reporting: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PaneScreenSnapshot {
    rows: Vec<Vec<Cell>>,
    cursor: CursorSnapshot,
    modes: ModesSnapshot,
    terminal_revision: u64,
    output_revision: u64,
    viewport_offset: usize,
    title: Option<String>,
    generation: u64,
    lifecycle: PaneLifecycle,
    unread_count: u64,
    attention: AttentionState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderCatalogSnapshot {
    service_instance: ServiceInstanceId,
    provider_kind: ProviderKind,
    capabilities: ProviderCapabilities,
    catalog: SessionCatalog,
}

impl ProviderCatalogSnapshot {
    #[must_use]
    pub const fn new(
        service_instance: ServiceInstanceId,
        provider_kind: ProviderKind,
        capabilities: ProviderCapabilities,
        catalog: SessionCatalog,
    ) -> Self {
        Self {
            service_instance,
            provider_kind,
            capabilities,
            catalog,
        }
    }

    #[must_use]
    pub const fn service_instance(&self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn provider_kind(&self) -> ProviderKind {
        self.provider_kind
    }

    #[must_use]
    pub const fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }

    #[must_use]
    pub const fn catalog(&self) -> &SessionCatalog {
        &self.catalog
    }
}

impl PaneScreenSnapshot {
    /// Creates an owned, bounded provider snapshot from the terminal model.
    ///
    /// # Errors
    ///
    /// Returns a dimension or aggregate-size error for unsafe snapshots.
    pub fn from_terminal(
        terminal: &TerminalSnapshot,
        output_revision: u64,
        generation: u64,
        lifecycle: PaneLifecycle,
        unread_count: u64,
        attention: AttentionState,
    ) -> Result<Self, SnapshotError> {
        if terminal.rows().len() > MAX_ROWS {
            return Err(SnapshotError::TooManyRows);
        }
        if terminal.rows().iter().any(|row| row.len() > MAX_COLUMNS) {
            return Err(SnapshotError::TooManyColumns);
        }
        let estimated_bytes = terminal.rows().iter().flat_map(|row| row.iter()).try_fold(
            0_usize,
            |total, cell| {
                total
                    .checked_add(CELL_OVERHEAD_BYTES + cell.text().len())
                    .ok_or(SnapshotError::TooLarge)
            },
        )?;
        if estimated_bytes > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::TooLarge);
        }
        let cursor = terminal.cursor();
        let modes = terminal.modes();
        Ok(Self {
            rows: terminal.rows().to_vec(),
            cursor: CursorSnapshot {
                row: cursor.row,
                column: cursor.column,
                visible: cursor.visible,
            },
            modes: ModesSnapshot {
                application_cursor_keys: modes.application_cursor_keys,
                bracketed_paste: modes.bracketed_paste,
                focus_reporting: modes.focus_reporting,
                mouse_reporting: modes.mouse_reporting,
            },
            terminal_revision: terminal.revision(),
            output_revision,
            viewport_offset: terminal.viewport_offset(),
            title: terminal
                .title()
                .map(|title| bounded(title, MAX_TITLE_BYTES)),
            generation,
            lifecycle: bounded_lifecycle(lifecycle),
            unread_count,
            attention,
        })
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }

    #[must_use]
    pub const fn cursor(&self) -> CursorSnapshot {
        self.cursor
    }

    #[must_use]
    pub const fn modes(&self) -> ModesSnapshot {
        self.modes
    }

    #[must_use]
    pub const fn terminal_revision(&self) -> u64 {
        self.terminal_revision
    }

    #[must_use]
    pub const fn output_revision(&self) -> u64 {
        self.output_revision
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn lifecycle(&self) -> &PaneLifecycle {
        &self.lifecycle
    }

    #[must_use]
    pub const fn unread_count(&self) -> u64 {
        self.unread_count
    }

    #[must_use]
    pub const fn attention(&self) -> AttentionState {
        self.attention
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

fn bounded_lifecycle(lifecycle: PaneLifecycle) -> PaneLifecycle {
    match lifecycle {
        PaneLifecycle::Failed { message } => PaneLifecycle::Failed {
            message: bounded(&message.replace('\0', "�"), MAX_FAILURE_BYTES),
        },
        other => other,
    }
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum SnapshotError {
    #[error("terminal snapshot exceeds the row limit")]
    TooManyRows,
    #[error("terminal snapshot exceeds the column limit")]
    TooManyColumns,
    #[error("terminal snapshot exceeds the aggregate byte limit")]
    TooLarge,
}
