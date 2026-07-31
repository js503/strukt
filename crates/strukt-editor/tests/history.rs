use strukt_editor::{
    EditKind, EditTransaction, History, HistoryBudget, HistoryEntry, HistoryError, Revision,
    TextBuffer,
};

fn apply_and_record(
    buffer: &mut TextBuffer,
    history: &mut History,
    transaction: EditTransaction,
    kind: EditKind,
    cursor_before: usize,
    cursor_after: usize,
) {
    let forward = transaction.clone();
    let applied = buffer.apply(transaction).unwrap();
    history.record(HistoryEntry::from_applied(
        forward,
        applied,
        kind,
        cursor_before,
        cursor_after,
    ));
}

#[test]
fn adjacent_typing_coalesces_into_one_undo_entry() {
    let mut buffer = TextBuffer::new("abc");
    let mut history = History::default();
    apply_and_record(
        &mut buffer,
        &mut history,
        EditTransaction::insert(Revision::INITIAL, 3, "d"),
        EditKind::Typing,
        3,
        4,
    );
    apply_and_record(
        &mut buffer,
        &mut history,
        EditTransaction::insert(Revision::new(1), 4, "e"),
        EditKind::Typing,
        4,
        5,
    );

    assert_eq!(history.undo_len(), 1);
    buffer
        .apply(history.undo(buffer.revision()).unwrap())
        .unwrap();
    assert_eq!(buffer.to_string(), "abc");
    buffer
        .apply(history.redo(buffer.revision()).unwrap())
        .unwrap();
    assert_eq!(buffer.to_string(), "abcde");
}

#[test]
fn cursor_discontinuity_breaks_typing_coalescing() {
    let mut buffer = TextBuffer::new("abc");
    let mut history = History::default();
    apply_and_record(
        &mut buffer,
        &mut history,
        EditTransaction::insert(Revision::INITIAL, 3, "d"),
        EditKind::Typing,
        3,
        4,
    );
    apply_and_record(
        &mut buffer,
        &mut history,
        EditTransaction::insert(Revision::new(1), 0, "x"),
        EditKind::Typing,
        0,
        1,
    );
    assert_eq!(history.undo_len(), 2);
}

#[test]
fn new_edit_after_undo_clears_redo() {
    let mut buffer = TextBuffer::new("abc");
    let mut history = History::default();
    apply_and_record(
        &mut buffer,
        &mut history,
        EditTransaction::insert(Revision::INITIAL, 3, "d"),
        EditKind::Other,
        3,
        4,
    );
    buffer
        .apply(history.undo(buffer.revision()).unwrap())
        .unwrap();
    let revision = buffer.revision();
    apply_and_record(
        &mut buffer,
        &mut history,
        EditTransaction::insert(revision, 3, "x"),
        EditKind::Other,
        3,
        4,
    );
    assert_eq!(
        history.redo(buffer.revision()),
        Err(HistoryError::NothingToRedo)
    );
}

#[test]
fn entry_and_byte_budgets_evict_oldest_complete_entries() {
    let mut buffer = TextBuffer::new("");
    let mut history = History::new(HistoryBudget::new(2, 2));
    for (index, text) in ["a", "b", "cc"].into_iter().enumerate() {
        let revision = buffer.revision();
        apply_and_record(
            &mut buffer,
            &mut history,
            EditTransaction::insert(revision, index, text),
            EditKind::Other,
            index,
            index + text.chars().count(),
        );
    }
    assert_eq!(history.undo_len(), 1);
    buffer
        .apply(history.undo(buffer.revision()).unwrap())
        .unwrap();
    assert_eq!(buffer.to_string(), "ab");
}
