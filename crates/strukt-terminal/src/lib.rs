//! UI-independent terminal domain and local process transport.

mod cell;
mod grid;
mod id;
mod layout;
mod parser;
mod selection;

pub use cell::{Cell, CellAttributes, CellError, CellWidth, Color, HyperlinkId};
pub use grid::{
    Cursor, EraseDisplay, EraseLine, Grid, GridError, GridSize, ResizeOutcome, Row, TerminalModes,
    TerminalSnapshot,
};
pub use id::{TerminalPaneId, TerminalTabId};
pub use layout::{
    LayoutNode, PaneState, SplitAxis, TerminalPane, TerminalPaneSnapshot, TerminalTab,
    TerminalTabSnapshot, TerminalWorkspace, TerminalWorkspaceError, TerminalWorkspaceSnapshot,
};
pub use parser::{ParserDiagnostics, TerminalModel};
pub use selection::{
    FocusEvent, LinkId, MouseButton, MouseEvent, PasteDecision, Selection, SelectionError,
    TerminalCoordinate, TerminalKey, TerminalLink,
};
