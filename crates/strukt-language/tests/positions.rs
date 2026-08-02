use strukt_language::{
    LspPosition, PositionEncoding, PositionError, ScalarPosition, from_lsp_position,
    to_lsp_position,
};

#[test]
fn utf16_positions_round_trip_astral_and_combining_text() {
    let text = "a😀e\u{301}\r\n界";
    let scalar = ScalarPosition::new(0, 2);
    let lsp = to_lsp_position(text, scalar, PositionEncoding::Utf16).unwrap();

    assert_eq!(lsp, LspPosition::new(0, 3));
    assert_eq!(
        from_lsp_position(text, lsp, PositionEncoding::Utf16).unwrap(),
        scalar
    );
}

#[test]
fn utf8_positions_count_bytes_and_crlf_is_not_document_content() {
    let text = "λx\r\n界";
    assert_eq!(
        to_lsp_position(text, ScalarPosition::new(0, 1), PositionEncoding::Utf8).unwrap(),
        LspPosition::new(0, 2)
    );
    assert_eq!(
        to_lsp_position(text, ScalarPosition::new(1, 1), PositionEncoding::Utf8).unwrap(),
        LspPosition::new(1, 3)
    );
}

#[test]
fn invalid_lines_columns_and_surrogate_interiors_are_rejected() {
    assert_eq!(
        from_lsp_position("😀", LspPosition::new(0, 1), PositionEncoding::Utf16),
        Err(PositionError::InvalidCharacter)
    );
    assert_eq!(
        to_lsp_position("text", ScalarPosition::new(1, 0), PositionEncoding::Utf8),
        Err(PositionError::InvalidLine)
    );
}
