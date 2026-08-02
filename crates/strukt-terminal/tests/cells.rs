use strukt_terminal::{Cell, CellAttributes, CellWidth, Color, TerminalPaneId};

#[test]
fn pane_ids_are_unique_and_cells_reset_to_semantic_defaults() {
    assert_ne!(TerminalPaneId::new(), TerminalPaneId::new());

    let mut cell = Cell::default();
    cell.set_text("界", CellWidth::Wide).unwrap();
    cell.attributes = CellAttributes {
        bold: true,
        ..CellAttributes::default()
    };
    cell.foreground = Color::Indexed(42);
    cell.reset();

    assert_eq!(cell, Cell::default());
}

#[test]
fn cells_bound_combining_text_and_reject_continuation_content() {
    let mut cell = Cell::default();
    cell.set_text("e\u{301}", CellWidth::Single).unwrap();

    assert_eq!(cell.text(), "e\u{301}");
    assert!(cell.set_text("x", CellWidth::Continuation).is_err());
}
