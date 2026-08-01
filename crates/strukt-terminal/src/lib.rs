//! UI-independent terminal domain and local process transport.

mod cell;
mod grid;
mod id;

pub use cell::{Cell, CellAttributes, CellError, CellWidth, Color};
pub use grid::{
    Cursor, EraseDisplay, EraseLine, Grid, GridError, GridSize, ResizeOutcome, Row,
    TerminalSnapshot,
};
pub use id::{TerminalPaneId, TerminalTabId};
