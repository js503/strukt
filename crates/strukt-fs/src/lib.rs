#![forbid(unsafe_code)]

mod cancellation;
mod discovery;
mod operations;
mod search;
mod watcher;

pub use cancellation::CancellationToken;
pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, FileEntry, FileKind, discover,
    discover_report, discover_report_cancellable, discover_report_for_root,
    discover_report_for_root_cancellable,
};
pub use operations::{FileOperation, OperationError, apply_operation};
pub use search::{
    QuickOpenCandidate, SearchError, SearchMatch, SearchOptions, SearchResult,
    quick_open_candidates, quick_open_candidates_with_ignored, search_content,
    search_content_cancellable,
};
pub use watcher::{FileEvent, WatcherError, WorkspaceWatcher, normalize_notify_event};
