use strukt_terminal::{
    FocusEvent, GridSize, MouseButton, MouseEvent, PasteDecision, Selection, TerminalKey,
    TerminalModel,
};

#[test]
fn selection_extracts_wrapped_wide_text_and_links_are_explicit() {
    let mut terminal = TerminalModel::new(GridSize::new(3, 20).unwrap(), 100);
    terminal.advance("go https://example.com/界".as_bytes());
    let selection = Selection::linear((0, 0), (1, 3));

    assert!(terminal.copy_text(&selection).unwrap().contains('界'));
    let link = terminal.links().next().unwrap();
    assert_eq!(link.target(), "https://example.com/界");
    assert!(!link.opened());
}

#[test]
fn paste_removes_nul_frames_bracketed_mode_and_requires_large_confirmation() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 10).unwrap(), 10);
    terminal.advance(b"\x1b[?2004h");

    assert_eq!(
        terminal.prepare_paste("a\0b", false),
        PasteDecision::Send(b"\x1b[200~ab\x1b[201~".to_vec())
    );
    assert!(matches!(
        terminal.prepare_paste(&"x".repeat(1_048_577), false),
        PasteDecision::Confirm { .. }
    ));
}

#[test]
fn mode_aware_input_encoding_is_deterministic() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 10).unwrap(), 10);
    assert_eq!(terminal.encode_key(TerminalKey::ArrowUp), b"\x1b[A");

    terminal.advance(b"\x1b[?1;1004;1006h");
    assert_eq!(terminal.encode_key(TerminalKey::ArrowUp), b"\x1bOA");
    assert_eq!(
        terminal.encode_focus(FocusEvent::In),
        Some(b"\x1b[I".to_vec())
    );
    assert_eq!(
        terminal.encode_mouse(MouseEvent::press(2, 3, MouseButton::Left)),
        Some(b"\x1b[<0;3;4M".to_vec())
    );
}

#[test]
fn unsupported_links_and_out_of_bounds_selections_are_rejected() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 30).unwrap(), 10);
    terminal.advance(b"javascript:alert(1) file:///tmp/log");

    let links = terminal.links().collect::<Vec<_>>();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target(), "file:///tmp/log");
    assert!(
        terminal
            .copy_text(&Selection::linear((0, 0), (99, 0)))
            .is_err()
    );
}

#[test]
fn copy_uses_the_selected_scrollback_viewport() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 8).unwrap(), 10);
    terminal.advance(b"one\r\ntwo\r\nthree");
    let selection = Selection::linear((0, 0), (0, 2));

    assert_eq!(terminal.copy_text(&selection).unwrap(), "two");
    assert_eq!(terminal.copy_text_at(1, &selection).unwrap(), "one");
}
