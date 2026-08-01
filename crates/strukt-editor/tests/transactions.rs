use strukt_editor::{
    CharRange, EditTransaction, LineEnding, Replacement, Revision, TextBuffer, TransactionError,
};

#[test]
fn transaction_rejects_overlapping_ranges() {
    let overlapping = EditTransaction::new(
        Revision::INITIAL,
        vec![
            Replacement::new(CharRange::new(0, 5).unwrap(), "one"),
            Replacement::new(CharRange::new(4, 8).unwrap(), "two"),
        ],
    );
    assert_eq!(
        overlapping.unwrap_err(),
        TransactionError::OverlappingRanges
    );
}

#[test]
fn transaction_rejects_a_stale_revision() {
    let mut buffer = TextBuffer::new("alpha beta");
    buffer
        .apply(EditTransaction::insert(Revision::INITIAL, 0, "x"))
        .unwrap();
    assert_eq!(
        buffer.apply(EditTransaction::insert(Revision::INITIAL, 0, "y")),
        Err(TransactionError::StaleRevision {
            expected: Revision::new(1),
            actual: Revision::INITIAL,
        })
    );
}

#[test]
fn multiple_unicode_replacements_apply_atomically_and_invert() {
    let mut buffer = TextBuffer::new("a😀c and café");
    let transaction = EditTransaction::new(
        Revision::INITIAL,
        vec![
            Replacement::new(CharRange::new(1, 2).unwrap(), "界"),
            Replacement::new(CharRange::new(8, 12).unwrap(), "tea"),
        ],
    )
    .unwrap();
    let applied = buffer.apply(transaction).unwrap();
    assert_eq!(buffer.to_string(), "a界c and tea");
    assert_eq!(buffer.revision(), Revision::new(1));
    buffer.apply(applied.inverse).unwrap();
    assert_eq!(buffer.to_string(), "a😀c and café");
}

#[test]
fn invalid_ranges_do_not_partially_mutate_the_buffer() {
    let mut buffer = TextBuffer::new("short");
    let transaction = EditTransaction::new(
        Revision::INITIAL,
        vec![Replacement::new(CharRange::new(4, 8).unwrap(), "long")],
    )
    .unwrap();
    assert_eq!(
        buffer.apply(transaction),
        Err(TransactionError::RangeOutOfBounds {
            end: 8,
            char_len: 5,
        })
    );
    assert_eq!(buffer.to_string(), "short");
    assert_eq!(buffer.revision(), Revision::INITIAL);
}

#[test]
fn line_endings_are_detected_without_normalizing_content() {
    let crlf = TextBuffer::new("one\r\ntwo\r\n");
    let lf = TextBuffer::new("one\ntwo\n");
    assert_eq!(crlf.line_ending(), LineEnding::CrLf);
    assert_eq!(crlf.to_string(), "one\r\ntwo\r\n");
    assert_eq!(lf.line_ending(), LineEnding::Lf);
}
