//! UI-independent terminal domain and local process transport.

mod cell;
mod id;

pub use cell::{Cell, CellAttributes, CellError, CellWidth, Color};
pub use id::{TerminalPaneId, TerminalTabId};
