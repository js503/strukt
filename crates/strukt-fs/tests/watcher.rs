use std::path::PathBuf;

use notify::{Event, EventKind};
use strukt_fs::{FileEvent, normalize_notify_event};

#[test]
fn notify_paths_are_deduplicated_and_sorted() {
    let event = Event {
        kind: EventKind::Any,
        paths: vec![PathBuf::from("b"), PathBuf::from("a"), PathBuf::from("a")],
        attrs: notify::event::EventAttributes::default(),
    };

    assert_eq!(
        normalize_notify_event(event),
        FileEvent::Changed(vec![PathBuf::from("a"), PathBuf::from("b")])
    );
}

#[test]
fn watcher_errors_mark_the_workspace_stale() {
    assert_eq!(
        FileEvent::watch_error("overflow"),
        FileEvent::Stale("overflow".into())
    );
}
