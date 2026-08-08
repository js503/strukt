use std::io::Read as _;
use std::path::Path;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};
use cap_std::fs::OpenOptions;
use serde::{Deserialize, Serialize};
use strukt_editor::DiskRevision;
use strukt_fs::{
    CancellationToken, DiscoveryError, DiscoveryOptions, DocumentIoError, DocumentKind,
    ReadOptions, SaveMode, SaveRequest, SearchError, SearchOptions,
    discover_report_for_root_cancellable, read_document, save_document, search_content_cancellable,
};
use strukt_workspace::{WorkspaceError, WorkspaceRoot};
use thiserror::Error;

use crate::{RemotePath, RemotePathError};

const MAX_READ_BYTES: u64 = 4 * 1_024 * 1_024;
const MAX_PAGE_ENTRIES: usize = 1_000;
const MAX_ENUMERATED_ENTRIES: usize = 100_000;
const MAX_SEARCH_RESULTS: usize = 10_000;

pub struct RemoteFilesystem {
    root: WorkspaceRoot,
}

impl RemoteFilesystem {
    /// Opens and retains a capability to the canonical helper workspace root.
    ///
    /// # Errors
    ///
    /// Returns a typed root error when the path is unavailable or not a directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RemoteFilesystemError> {
        Ok(Self {
            root: WorkspaceRoot::open(path).map_err(RemoteFilesystemError::OpenRoot)?,
        })
    }

    #[must_use]
    pub fn canonical_root(&self) -> &Path {
        self.root.path()
    }

    /// Lists one deterministic page of direct children.
    ///
    /// # Errors
    ///
    /// Returns a confinement, cursor, limit, root-change, or I/O error.
    pub fn list(
        &self,
        path: &RemotePath,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DirectoryPage, RemoteFilesystemError> {
        self.validate_root()?;
        if limit == 0 || limit > MAX_PAGE_ENTRIES {
            return Err(RemoteFilesystemError::InvalidLimit);
        }
        let offset = cursor
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| RemoteFilesystemError::InvalidCursor)?
            .unwrap_or(0);
        let directory = self
            .root
            .try_clone_capability()
            .map_err(|_| RemoteFilesystemError::WorkspaceChanged)?;
        let mut entries = directory
            .read_dir(path.as_path())
            .map_err(RemoteFilesystemError::Io)?
            .filter_map(Result::ok)
            .map(|entry| directory_entry(path, &entry))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let end = offset.saturating_add(limit).min(entries.len());
        let page = entries.get(offset..end).unwrap_or(&[]).to_vec();
        let next_cursor = (end < entries.len()).then(|| end.to_string());
        self.validate_root()?;
        Ok(DirectoryPage {
            entries: page,
            next_cursor,
        })
    }

    /// Reads and classifies a bounded regular file through the retained root.
    ///
    /// # Errors
    ///
    /// Returns a typed confinement, root-change, file-kind, size, or I/O error.
    pub fn read(&self, path: &RemotePath) -> Result<RemoteDocument, RemoteFilesystemError> {
        self.validate_root()?;
        let document = read_document(
            &self.root,
            path.as_path(),
            ReadOptions {
                max_editable_bytes: MAX_READ_BYTES,
                preview_bytes: usize::try_from(MAX_READ_BYTES).unwrap_or(usize::MAX),
                force_full: false,
            },
        )
        .map_err(map_document_error)?;
        let kind = match document.kind {
            DocumentKind::Text { truncated, .. } => {
                if truncated {
                    RemoteDocumentKind::TruncatedText
                } else {
                    RemoteDocumentKind::Text
                }
            }
            DocumentKind::Binary => RemoteDocumentKind::Binary,
            DocumentKind::InvalidUtf8 => RemoteDocumentKind::InvalidUtf8,
        };
        let bytes = if let Some(text) = document.text {
            text.into_bytes()
        } else {
            self.read_raw(path, MAX_READ_BYTES)?
        };
        self.validate_root()?;
        Ok(RemoteDocument {
            kind,
            bytes,
            size: document.size,
            revision: document.disk_revision.as_str().to_owned(),
        })
    }

    /// Conditionally atomically saves a regular file.
    ///
    /// # Errors
    ///
    /// Returns a typed conflict, confinement, root-change, or I/O error.
    pub fn save(
        &self,
        path: &RemotePath,
        bytes: &[u8],
        expected_revision: &str,
        force: bool,
    ) -> Result<RemoteSaveOutcome, RemoteFilesystemError> {
        self.validate_root()?;
        if bytes.len() > usize::try_from(MAX_READ_BYTES).unwrap_or(usize::MAX) {
            return Err(RemoteFilesystemError::FileTooLarge);
        }
        let mut request = SaveRequest::new(
            path.as_path(),
            bytes.to_vec(),
            DiskRevision::new(expected_revision),
        );
        if force {
            request = request.with_mode(SaveMode::Force);
        }
        let outcome = save_document(&self.root, &request).map_err(map_document_error)?;
        self.validate_root()?;
        Ok(RemoteSaveOutcome {
            revision: outcome.disk_revision.as_str().to_owned(),
            bytes_written: outcome.bytes_written,
        })
    }

    /// Enumerates bounded workspace files using the local ignored/hidden policy.
    ///
    /// # Errors
    ///
    /// Returns a root-change, discovery, representation, or limit error.
    pub fn enumerate(
        &self,
        show_hidden: bool,
        show_ignored: bool,
        limit: usize,
    ) -> Result<RemoteEnumeration, RemoteFilesystemError> {
        self.enumerate_cancellable(show_hidden, show_ignored, limit, &CancellationToken::new())
    }

    /// Enumerates workspace files with cooperative cancellation.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::enumerate`] plus cancellation.
    pub fn enumerate_cancellable(
        &self,
        show_hidden: bool,
        show_ignored: bool,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<RemoteEnumeration, RemoteFilesystemError> {
        if limit == 0 || limit > MAX_ENUMERATED_ENTRIES {
            return Err(RemoteFilesystemError::InvalidLimit);
        }
        let report = discover_report_for_root_cancellable(
            &self.root,
            DiscoveryOptions {
                show_hidden,
                show_ignored,
                max_entries: limit,
            },
            cancellation,
        )
        .map_err(|error| match error {
            DiscoveryError::Cancelled => RemoteFilesystemError::Cancelled,
            error => RemoteFilesystemError::Discovery(error.to_string()),
        })?;
        let paths = report
            .entries
            .into_iter()
            .filter(|entry| entry.kind == strukt_fs::FileKind::File)
            .map(|entry| display_path(&entry.relative_path))
            .collect();
        Ok(RemoteEnumeration {
            paths,
            truncated: report.truncated,
            warnings: report.warnings,
        })
    }

    /// Searches bounded workspace text using the local search policy.
    ///
    /// # Errors
    ///
    /// Returns a root-change, discovery, search, or limit error.
    pub fn search(
        &self,
        query: &str,
        include_ignored: bool,
        limit: usize,
    ) -> Result<RemoteSearchResult, RemoteFilesystemError> {
        self.search_cancellable(query, include_ignored, limit, &CancellationToken::new())
    }

    /// Searches workspace text with cooperative cancellation.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::search`] plus cancellation.
    pub fn search_cancellable(
        &self,
        query: &str,
        include_ignored: bool,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<RemoteSearchResult, RemoteFilesystemError> {
        if limit == 0 || limit > MAX_SEARCH_RESULTS {
            return Err(RemoteFilesystemError::InvalidLimit);
        }
        let result = search_content_cancellable(
            &self.root,
            query,
            SearchOptions {
                max_results: limit,
                max_file_bytes: 2 * 1_024 * 1_024,
                discovery: DiscoveryOptions {
                    show_hidden: include_ignored,
                    show_ignored: include_ignored,
                    max_entries: MAX_ENUMERATED_ENTRIES,
                },
            },
            cancellation,
        )
        .map_err(|error| match error {
            SearchError::Cancelled => RemoteFilesystemError::Cancelled,
            error => RemoteFilesystemError::Search(error.to_string()),
        })?;
        Ok(RemoteSearchResult {
            matches: result
                .matches
                .into_iter()
                .map(|item| RemoteSearchMatch {
                    path: display_path(&item.relative_path),
                    line: item.line,
                    preview: item.preview,
                })
                .collect(),
            truncated: result.truncated,
        })
    }

    fn read_raw(&self, path: &RemotePath, maximum: u64) -> Result<Vec<u8>, RemoteFilesystemError> {
        let directory = self
            .root
            .try_clone_capability()
            .map_err(|_| RemoteFilesystemError::WorkspaceChanged)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = directory
            .open_with(path.as_path(), &options)
            .map_err(RemoteFilesystemError::Io)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(RemoteFilesystemError::Io)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
            return Err(RemoteFilesystemError::FileTooLarge);
        }
        Ok(bytes)
    }

    fn validate_root(&self) -> Result<(), RemoteFilesystemError> {
        self.root
            .validate_location()
            .map_err(|_| RemoteFilesystemError::WorkspaceChanged)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub kind: RemoteEntryKind,
    pub size: u64,
    pub editable_name: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirectoryPage {
    pub entries: Vec<DirectoryEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteDocumentKind {
    Text,
    TruncatedText,
    Binary,
    InvalidUtf8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteDocument {
    pub kind: RemoteDocumentKind,
    pub bytes: Vec<u8>,
    pub size: u64,
    pub revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteSaveOutcome {
    pub revision: String,
    pub bytes_written: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteEnumeration {
    pub paths: Vec<String>,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteSearchMatch {
    pub path: String,
    pub line: usize,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteSearchResult {
    pub matches: Vec<RemoteSearchMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteWatchInput {
    Changed(Vec<String>),
    Stale(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteWatchEvent {
    pub sequence: u64,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteWatchBatch {
    pub generation: u64,
    pub events: Vec<RemoteWatchEvent>,
    pub stale: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RemoteWatchSequencer {
    generation: u64,
    next_sequence: u64,
    max_paths_per_event: usize,
}

impl RemoteWatchSequencer {
    /// Creates a bounded watch-event sequencer.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteFilesystemError::InvalidLimit`] for a zero path bound.
    pub const fn new(max_paths_per_event: usize) -> Result<Self, RemoteFilesystemError> {
        if max_paths_per_event == 0 {
            return Err(RemoteFilesystemError::InvalidLimit);
        }
        Ok(Self {
            generation: 1,
            next_sequence: 0,
            max_paths_per_event,
        })
    }

    #[must_use]
    pub fn accept(&mut self, input: RemoteWatchInput) -> RemoteWatchBatch {
        match input {
            RemoteWatchInput::Changed(mut paths) if paths.len() <= self.max_paths_per_event => {
                paths.sort();
                paths.dedup();
                let event = RemoteWatchEvent {
                    sequence: self.next_sequence,
                    paths,
                };
                self.next_sequence = self.next_sequence.saturating_add(1);
                RemoteWatchBatch {
                    generation: self.generation,
                    events: vec![event],
                    stale: false,
                    detail: None,
                }
            }
            RemoteWatchInput::Changed(_) => self.mark_stale("remote watch event overflow"),
            RemoteWatchInput::Stale(detail) => self.mark_stale(&detail),
        }
    }

    #[must_use]
    pub const fn cursor(&self) -> (u64, u64) {
        (self.generation, self.next_sequence)
    }

    fn mark_stale(&mut self, detail: &str) -> RemoteWatchBatch {
        self.generation = self.generation.saturating_add(1);
        self.next_sequence = 0;
        RemoteWatchBatch {
            generation: self.generation,
            events: Vec::new(),
            stale: true,
            detail: Some(bounded_watch_detail(detail)),
        }
    }
}

#[derive(Debug, Error)]
pub enum RemoteFilesystemError {
    #[error("remote workspace root could not be opened: {0}")]
    OpenRoot(WorkspaceError),
    #[error("remote workspace root changed")]
    WorkspaceChanged,
    #[error("remote path is not confined: {0}")]
    Confined(String),
    #[error("remote file changed (expected {expected}, actual {actual})")]
    Conflict { expected: String, actual: String },
    #[error("remote file exceeds the transfer bound")]
    FileTooLarge,
    #[error("remote page cursor is invalid")]
    InvalidCursor,
    #[error("remote operation limit is invalid")]
    InvalidLimit,
    #[error("remote filesystem operation was cancelled")]
    Cancelled,
    #[error("remote discovery failed: {0}")]
    Discovery(String),
    #[error("remote search failed: {0}")]
    Search(String),
    #[error("remote filesystem I/O failed: {0}")]
    Io(std::io::Error),
    #[error(transparent)]
    InvalidPath(#[from] RemotePathError),
}

fn map_document_error(error: DocumentIoError) -> RemoteFilesystemError {
    match error {
        DocumentIoError::OutsideRoot(path)
        | DocumentIoError::Symlink(path)
        | DocumentIoError::NotRegularFile(path) => {
            RemoteFilesystemError::Confined(path.display().to_string())
        }
        DocumentIoError::WorkspaceChanged => RemoteFilesystemError::WorkspaceChanged,
        DocumentIoError::SaveConflict { expected, actual } => RemoteFilesystemError::Conflict {
            expected: expected.as_str().to_owned(),
            actual: actual.as_str().to_owned(),
        },
        DocumentIoError::AtomicReplaceUnavailable { platform } => {
            RemoteFilesystemError::Confined(format!("atomic replace unavailable on {platform}"))
        }
        DocumentIoError::Io(error) => RemoteFilesystemError::Io(error),
    }
}

fn directory_entry(
    parent: &RemotePath,
    entry: &cap_std::fs::DirEntry,
) -> Result<DirectoryEntry, RemoteFilesystemError> {
    let (name, editable_name) = display_name(&entry.file_name());
    let path = if parent.is_root() {
        name.clone()
    } else {
        format!("{parent}/{name}")
    };
    let file_type = entry.file_type().map_err(RemoteFilesystemError::Io)?;
    let kind = if file_type.is_dir() {
        RemoteEntryKind::Directory
    } else if file_type.is_file() {
        RemoteEntryKind::File
    } else if file_type.is_symlink() {
        RemoteEntryKind::Symlink
    } else {
        RemoteEntryKind::Other
    };
    let size = entry.metadata().map_or(0, |metadata| metadata.len());
    Ok(DirectoryEntry {
        name,
        path,
        kind,
        size,
        editable_name,
    })
}

fn display_path(path: &Path) -> String {
    path.components()
        .map(|component| display_name(component.as_os_str()).0)
        .collect::<Vec<_>>()
        .join("/")
}

fn bounded_watch_detail(detail: &str) -> String {
    const MAXIMUM: usize = 512;
    let sanitized = detail.replace('\0', "�");
    if sanitized.len() <= MAXIMUM {
        return sanitized;
    }
    let mut end = MAXIMUM;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_owned()
}

#[cfg(unix)]
fn display_name(name: &std::ffi::OsStr) -> (String, bool) {
    use std::os::unix::ffi::OsStrExt as _;

    if let Some(name) = name.to_str() {
        return (name.to_owned(), true);
    }
    let mut escaped = String::new();
    for byte in name.as_bytes() {
        if byte.is_ascii_graphic() && *byte != b'%' {
            escaped.push(char::from(*byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(escaped, "%{byte:02X}");
        }
    }
    (escaped, false)
}

#[cfg(not(unix))]
fn display_name(name: &std::ffi::OsStr) -> (String, bool) {
    name.to_str().map_or_else(
        || (name.to_string_lossy().into_owned(), false),
        |name| (name.to_owned(), true),
    )
}
