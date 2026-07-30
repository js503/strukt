#![forbid(unsafe_code)]

mod discovery;
mod operations;
mod search;
mod watcher;

pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, FileEntry, FileKind, discover,
    discover_report,
};
pub use operations::{FileOperation, OperationError, apply_operation};
pub use search::{
    QuickOpenCandidate, SearchError, SearchMatch, SearchOptions, SearchResult,
    quick_open_candidates, quick_open_candidates_with_ignored, search_content,
};
pub use watcher::{FileEvent, WatcherError, WorkspaceWatcher, normalize_notify_event};
