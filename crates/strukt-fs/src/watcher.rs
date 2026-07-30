use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

const EVENT_QUEUE_CAPACITY: usize = 1024;
const EVENT_LOSS_MESSAGE: &str = "filesystem watcher events were lost";
const EVENT_CHANNEL_DISCONNECTED_MESSAGE: &str = "filesystem watcher event channel disconnected";
const NATIVE_RESCAN_MESSAGE: &str = "filesystem watcher requested a full rescan";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileEvent {
    Changed(Vec<PathBuf>),
    Stale(String),
}

impl FileEvent {
    #[must_use]
    pub fn watch_error(message: impl Into<String>) -> Self {
        Self::Stale(message.into())
    }
}

#[must_use]
pub fn normalize_notify_event(event: Event) -> FileEvent {
    if event.need_rescan() {
        return FileEvent::watch_error(NATIVE_RESCAN_MESSAGE);
    }

    let mut paths = event.paths;
    paths.sort();
    paths.dedup();
    FileEvent::Changed(paths)
}

struct EventHandoff {
    sender: SyncSender<FileEvent>,
    stale: Arc<AtomicBool>,
}

impl EventHandoff {
    fn try_send(&self, event: FileEvent) -> bool {
        match self.sender.try_send(event) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.stale.store(true, Ordering::Release);
                false
            }
        }
    }
}

struct EventInbox {
    events: Receiver<FileEvent>,
    stale: Arc<AtomicBool>,
    disconnected_reported: AtomicBool,
    capacity: usize,
}

#[cfg(test)]
impl EventInbox {
    fn try_recv(&self) -> Option<FileEvent> {
        try_recv_event(
            &self.events,
            &self.stale,
            &self.disconnected_reported,
            self.capacity,
        )
    }
}

fn event_channel(capacity: usize) -> (EventHandoff, EventInbox) {
    let (sender, events) = mpsc::sync_channel(capacity);
    let stale = Arc::new(AtomicBool::new(false));

    (
        EventHandoff {
            sender,
            stale: Arc::clone(&stale),
        },
        EventInbox {
            events,
            stale,
            disconnected_reported: AtomicBool::new(false),
            capacity,
        },
    )
}

fn try_recv_event(
    events: &Receiver<FileEvent>,
    stale: &AtomicBool,
    disconnected_reported: &AtomicBool,
    capacity: usize,
) -> Option<FileEvent> {
    if stale.swap(false, Ordering::AcqRel) {
        drain_obsolete_events(events, disconnected_reported, capacity);
        stale.store(false, Ordering::Release);
        return Some(FileEvent::watch_error(EVENT_LOSS_MESSAGE));
    }

    match events.try_recv() {
        Ok(event) => Some(event),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            if disconnected_reported.swap(true, Ordering::AcqRel) {
                None
            } else {
                Some(FileEvent::watch_error(EVENT_CHANNEL_DISCONNECTED_MESSAGE))
            }
        }
    }
}

fn drain_obsolete_events(
    events: &Receiver<FileEvent>,
    disconnected_reported: &AtomicBool,
    capacity: usize,
) {
    for _ in 0..capacity {
        match events.try_recv() {
            Ok(_) => {}
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                disconnected_reported.store(true, Ordering::Release);
                break;
            }
        }
    }
}

pub struct WorkspaceWatcher {
    _watcher: RecommendedWatcher,
    events: Receiver<FileEvent>,
    stale: Arc<AtomicBool>,
    disconnected_reported: AtomicBool,
    event_queue_capacity: usize,
}

impl WorkspaceWatcher {
    /// Starts recursively watching `root` for filesystem changes.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError`] when the platform watcher cannot be created or
    /// when `root` cannot be registered for recursive watching.
    pub fn start(root: impl AsRef<Path>) -> Result<Self, WatcherError> {
        let (handoff, inbox) = event_channel(EVENT_QUEUE_CAPACITY);
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            let event = match result {
                Ok(event) => normalize_notify_event(event),
                Err(error) => FileEvent::watch_error(error.to_string()),
            };
            let _ = handoff.try_send(event);
        })
        .map_err(WatcherError::Notify)?;
        watcher
            .watch(root.as_ref(), RecursiveMode::Recursive)
            .map_err(WatcherError::Notify)?;

        Ok(Self {
            _watcher: watcher,
            events: inbox.events,
            stale: inbox.stale,
            disconnected_reported: inbox.disconnected_reported,
            event_queue_capacity: inbox.capacity,
        })
    }

    #[must_use]
    pub fn try_recv(&self) -> Option<FileEvent> {
        try_recv_event(
            &self.events,
            &self.stale,
            &self.disconnected_reported,
            self.event_queue_capacity,
        )
    }
}

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("filesystem watcher failed: {0}")]
    Notify(#[source] notify::Error),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{FileEvent, event_channel};

    #[test]
    fn full_event_queue_drains_before_new_changes_are_received() {
        let (handoff, inbox) = event_channel(1);
        let first = FileEvent::Changed(vec![PathBuf::from("a")]);
        let dropped = FileEvent::Changed(vec![PathBuf::from("b")]);
        let next = FileEvent::Changed(vec![PathBuf::from("c")]);

        assert!(handoff.try_send(first));
        assert!(!handoff.try_send(dropped));
        assert_eq!(
            inbox.try_recv(),
            Some(FileEvent::watch_error(super::EVENT_LOSS_MESSAGE))
        );
        assert!(handoff.try_send(next.clone()));
        assert_eq!(inbox.try_recv(), Some(next));
        assert_eq!(inbox.try_recv(), None);
    }

    #[test]
    fn disconnected_event_queue_reports_stale_once() {
        let (handoff, inbox) = event_channel(1);
        drop(handoff);

        assert_eq!(
            inbox.try_recv(),
            Some(FileEvent::watch_error(
                super::EVENT_CHANNEL_DISCONNECTED_MESSAGE
            ))
        );
        assert_eq!(inbox.try_recv(), None);
    }
}
