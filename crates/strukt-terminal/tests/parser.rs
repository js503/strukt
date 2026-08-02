use strukt_terminal::{Color, GridSize, TerminalModel};

#[test]
fn parser_applies_unicode_sgr_cursor_and_alternate_screen() {
    let mut terminal = TerminalModel::new(GridSize::new(3, 12).unwrap(), 100);
    terminal.advance(b"plain \x1b[1;38;2;1;2;3mred\x1b[0m");

    let snapshot = terminal.snapshot(0);
    assert_eq!(snapshot.cell(0, 6).unwrap().foreground, Color::Rgb(1, 2, 3));
    assert!(snapshot.cell(0, 6).unwrap().attributes.bold);

    terminal.advance(b"\x1b[?1049halt\x1b[?1049l");
    assert!(terminal.snapshot(0).plain_text().contains("plain"));
}

#[test]
fn oversized_osc_is_discarded_and_counted_without_growth() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 8).unwrap(), 10);
    let oversized = format!("\x1b]2;{}\x07", "x".repeat(9_000));

    terminal.advance(oversized.as_bytes());

    assert_eq!(terminal.snapshot(0).title(), None);
    assert_eq!(terminal.diagnostics().discarded_sequences, 1);
}

#[test]
fn parser_applies_palette_colors_and_semantic_attributes() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 12).unwrap(), 10);
    terminal.advance(b"\x1b[31;44;3;4;7;9mA\x1b[38;5;201mB");
    let snapshot = terminal.snapshot(0);

    let first = snapshot.cell(0, 0).unwrap();
    assert_eq!(first.foreground, Color::Indexed(1));
    assert_eq!(first.background, Color::Indexed(4));
    assert!(first.attributes.italic);
    assert!(first.attributes.underline);
    assert!(first.attributes.inverse);
    assert!(first.attributes.strikethrough);
    assert_eq!(snapshot.cell(0, 1).unwrap().foreground, Color::Indexed(201));
}

#[test]
fn parser_moves_saves_restores_and_obeys_scrolling_margins() {
    let mut terminal = TerminalModel::new(GridSize::new(4, 4).unwrap(), 10);
    terminal.advance(b"1111\r\n2222\r\n3333\r\n4444");
    terminal.advance(b"\x1b[2;3H\x1b7\x1b[4;4HX\x1b8Y");
    assert_eq!(terminal.snapshot(0).cell(1, 2).unwrap().text(), "Y");

    terminal.advance(b"\x1b[2;3r\x1b[3;1H\n");
    assert_eq!(terminal.snapshot(0).plain_text(), "1111\n3333\n    \n444X");
}

#[test]
fn parser_tracks_private_input_modes_and_cursor_visibility() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 4).unwrap(), 10);
    terminal.advance(b"\x1b[?1;25;1004;1006;2004h");
    let snapshot = terminal.snapshot(0);
    assert!(snapshot.modes().application_cursor_keys);
    assert!(snapshot.modes().focus_reporting);
    assert!(snapshot.modes().mouse_reporting);
    assert!(snapshot.modes().bracketed_paste);
    assert!(snapshot.cursor().visible);

    terminal.advance(b"\x1b[?25;2004l");
    let snapshot = terminal.snapshot(0);
    assert!(!snapshot.modes().bracketed_paste);
    assert!(!snapshot.cursor().visible);
}

#[test]
fn parser_records_titles_and_explicit_osc8_hyperlinks() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 12).unwrap(), 10);
    terminal.advance(b"\x1b]2;build logs\x07\x1b]8;;https://example.com\x07link\x1b]8;;\x07");
    let snapshot = terminal.snapshot(0);

    assert_eq!(snapshot.title(), Some("build logs"));
    let link_id = snapshot.cell(0, 0).unwrap().hyperlink.unwrap();
    assert_eq!(
        snapshot.hyperlink_target(link_id),
        Some("https://example.com")
    );
    assert_eq!(snapshot.cell(0, 4).unwrap().hyperlink, None);
}

#[test]
fn parser_handles_malformed_utf8_and_escape_sequences_split_across_chunks() {
    let mut terminal = TerminalModel::new(GridSize::new(2, 12).unwrap(), 10);
    for chunk in [
        b"ok \x1b[38;2;9".as_slice(),
        b";8;7m".as_slice(),
        &[0xf0, 0x28, 0x8c, 0x28],
    ] {
        terminal.advance(chunk);
    }

    let snapshot = terminal.snapshot(0);
    assert_eq!(snapshot.cell(0, 3).unwrap().foreground, Color::Rgb(9, 8, 7));
    assert!(snapshot.plain_text().starts_with("ok "));
}
