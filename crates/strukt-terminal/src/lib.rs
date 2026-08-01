//! UI-independent terminal domain and local process transport.

mod cell;
mod grid;
mod id;
mod parser;

pub use cell::{Cell, CellAttributes, CellError, CellWidth, Color, HyperlinkId};
pub use grid::{
    Cursor, EraseDisplay, EraseLine, Grid, GridError, GridSize, ResizeOutcome, Row, TerminalModes,
    TerminalSnapshot,
};
pub use id::{TerminalPaneId, TerminalTabId};
pub use parser::{ParserDiagnostics, TerminalModel};
