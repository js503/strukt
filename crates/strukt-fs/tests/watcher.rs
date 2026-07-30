use std::path::PathBuf;

use notify::event::Flag;
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

#[test]
fn native_rescan_events_mark_the_workspace_stale() {
    let event = Event::new(EventKind::Any)
        .add_path(PathBuf::from("ignored"))
        .set_flag(Flag::Rescan);

    assert_eq!(
        normalize_notify_event(event),
        FileEvent::Stale("filesystem watcher requested a full rescan".into())
    );
}
