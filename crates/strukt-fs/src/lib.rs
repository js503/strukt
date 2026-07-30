#![forbid(unsafe_code)]

mod discovery;
mod operations;
mod search;
mod watcher;

pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, FileEntry, FileKind, discover,
    discover_report,
};
