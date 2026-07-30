use std::io::{self, Read};
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, OpenOptions};
use serde::{Deserialize, Serialize};
use strukt_workspace::WorkspaceRoot;
use thiserror::Error;

use crate::{DiscoveryError, DiscoveryOptions, FileEntry, FileKind, discover_report};

const MAX_TOTAL_SEARCH_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 512;
const ELLIPSIS: &str = "…";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchOptions {
    pub max_results: usize,
    pub max_file_bytes: u64,
    pub discovery: DiscoveryOptions,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 500,
            max_file_bytes: 2 * 1024 * 1024,
            discovery: DiscoveryOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SearchMatch {
    pub relative_path: PathBuf,
    pub line: usize,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickOpenCandidate {
    pub relative_path: PathBuf,
    pub score: usize,
}

#[must_use]
pub fn quick_open_candidates(
    entries: &[FileEntry],
    query: &str,
    max_results: usize,
) -> Vec<QuickOpenCandidate> {
    quick_open_candidates_with_ignored(entries, query, max_results, true)
}

#[must_use]
pub fn quick_open_candidates_with_ignored(
    entries: &[FileEntry],
    query: &str,
    max_results: usize,
    include_ignored: bool,
) -> Vec<QuickOpenCandidate> {
    let query = query.to_lowercase();
    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == FileKind::File)
        .filter(|entry| include_ignored || !entry.ignored)
        .filter_map(|entry| {
            let path = entry.relative_path.to_string_lossy().to_lowercase();
            subsequence_score(&path, &query).map(|score| QuickOpenCandidate {
                relative_path: entry.relative_path.clone(),
                score,
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    candidates.truncate(max_results);
    candidates
}

fn subsequence_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(candidate.len());
    }

    let mut next_index = 0;
    let mut score = 0;
    for query_char in query.chars() {
        let offset = candidate[next_index..].find(query_char)?;
        score += offset;
        next_index += offset + query_char.len_utf8();
    }
    Some(score)
}

/// Searches visible workspace files for case-sensitive line matches.
///
/// # Errors
///
/// Returns [`SearchError`] when discovery of the workspace root fails or file IO
/// fails for a reason other than disappearance churn. Missing, oversized, binary,
/// and invalid UTF-8 files produce an incomplete result without discarding matches
/// already found. A NUL byte is the deliberate binary-file heuristic.
pub fn search_content(
    root: &WorkspaceRoot,
    needle: &str,
    options: SearchOptions,
) -> Result<SearchResult, SearchError> {
    search_content_with_budget(root, needle, options, MAX_TOTAL_SEARCH_BYTES)
}

fn search_content_with_budget(
    root: &WorkspaceRoot,
    needle: &str,
    options: SearchOptions,
    total_budget: u64,
) -> Result<SearchResult, SearchError> {
    search_content_inner(root, needle, options, total_budget, || {})
}

#[cfg(test)]
fn search_content_with_hook(
    root: &WorkspaceRoot,
    needle: &str,
    options: SearchOptions,
    after_discovery: impl FnOnce(),
) -> Result<SearchResult, SearchError> {
    search_content_inner(
        root,
        needle,
        options,
        MAX_TOTAL_SEARCH_BYTES,
        after_discovery,
    )
}

fn search_content_inner(
    workspace: &WorkspaceRoot,
    needle: &str,
    options: SearchOptions,
    total_budget: u64,
    after_discovery: impl FnOnce(),
) -> Result<SearchResult, SearchError> {
    workspace
        .validate_location()
        .map_err(|_| SearchError::WorkspaceChanged)?;
    let report =
        discover_report(workspace.path(), options.discovery).map_err(SearchError::Discovery)?;
    after_discovery();
    workspace
        .validate_location()
        .map_err(|_| SearchError::WorkspaceChanged)?;
    let root = workspace
        .try_clone_capability()
        .map_err(|_| SearchError::WorkspaceChanged)?;
    let mut incomplete = report.truncated || !report.warnings.is_empty();
    let mut remaining_budget = total_budget.min(MAX_TOTAL_SEARCH_BYTES);
    let mut matches = Vec::new();
    let entries = report
        .entries
        .into_iter()
        .filter(|entry| entry.kind == FileKind::File);

    for entry in entries {
        if remaining_budget == 0 {
            incomplete = true;
            break;
        }

        let effective_limit = options.max_file_bytes.min(remaining_budget);
        let Some(metadata) = classify_entry_io(root.symlink_metadata(&entry.relative_path))? else {
            incomplete = true;
            continue;
        };
        if !metadata.file_type().is_file() || metadata.len() > effective_limit {
            incomplete = true;
            continue;
        }

        let Some(file) = classify_entry_io(open_for_search(&root, &entry.relative_path))? else {
            incomplete = true;
            continue;
        };
        let Some(metadata) = classify_entry_io(file.metadata())? else {
            incomplete = true;
            continue;
        };
        if !metadata.file_type().is_file() || metadata.len() > effective_limit {
            incomplete = true;
            continue;
        }

        let mut bytes = Vec::new();
        let read_result = file
            .take(effective_limit.saturating_add(1))
            .read_to_end(&mut bytes);
        let bytes_read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        remaining_budget = remaining_budget.saturating_sub(bytes_read);
        if classify_entry_io(read_result)?.is_none() || bytes_read > effective_limit {
            incomplete = true;
            continue;
        }
        // NUL detection is an intentionally simple binary-file heuristic.
        if bytes.contains(&0) {
            incomplete = true;
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            incomplete = true;
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            if line.contains(needle) {
                if matches.len() == options.max_results {
                    return Ok(SearchResult {
                        matches,
                        truncated: true,
                    });
                }
                matches.push(SearchMatch {
                    relative_path: entry.relative_path.clone(),
                    line: index + 1,
                    preview: bounded_preview(line.trim()),
                });
            }
        }
    }

    Ok(SearchResult {
        matches,
        truncated: incomplete || remaining_budget == 0,
    })
}

fn open_for_search(root: &Dir, path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).nonblock(true).follow(FollowSymlinks::No);
    root.open_with(path, &options)
}

fn classify_entry_io<T>(result: io::Result<T>) -> Result<Option<T>, SearchError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SearchError::Io(error)),
    }
}

fn bounded_preview(line: &str) -> String {
    if line.len() <= MAX_PREVIEW_BYTES {
        return line.to_owned();
    }

    let mut end = MAX_PREVIEW_BYTES - ELLIPSIS.len();
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ELLIPSIS}", &line[..end])
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("workspace root changed after it was opened")]
    WorkspaceChanged,
    #[error("file discovery failed: {0}")]
    Discovery(#[source] DiscoveryError),
    #[error("content search IO failed: {0}")]
    Io(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, ErrorKind};
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        SearchError, SearchOptions, classify_entry_io, search_content_with_budget,
        search_content_with_hook,
    };
    use strukt_workspace::WorkspaceRoot;

    #[test]
    fn not_found_entry_io_is_recoverable_churn() {
        let result = classify_entry_io::<()>(Err(io::Error::from(ErrorKind::NotFound)));

        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn permission_denied_entry_io_is_fatal() {
        let result = classify_entry_io::<()>(Err(io::Error::from(ErrorKind::PermissionDenied)));

        assert!(matches!(
            result,
            Err(SearchError::Io(error)) if error.kind() == ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn other_entry_io_is_fatal() {
        let result = classify_entry_io::<()>(Err(io::Error::other("read failed")));

        assert!(matches!(
            result,
            Err(SearchError::Io(error)) if error.kind() == ErrorKind::Other
        ));
    }

    #[test]
    fn aggregate_budget_preserves_matches_and_marks_remaining_work_incomplete() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("a.txt"), "needle").unwrap();
        fs::write(root.path().join("b.txt"), "needle").unwrap();
        let workspace = WorkspaceRoot::open(root.path()).unwrap();

        let result =
            search_content_with_budget(&workspace, "needle", SearchOptions::default(), 6).unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].relative_path, Path::new("a.txt"));
        assert!(result.truncated);
    }

    #[test]
    fn exhausting_the_aggregate_budget_marks_the_result_incomplete() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("only.txt"), "needle").unwrap();
        let workspace = WorkspaceRoot::open(root.path()).unwrap();

        let result =
            search_content_with_budget(&workspace, "needle", SearchOptions::default(), 6).unwrap();

        assert_eq!(result.matches.len(), 1);
        assert!(result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_swap_after_discovery_cannot_redirect_a_search_read() {
        use std::os::unix::fs::symlink;

        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir(workspace.path().join("nested")).unwrap();
        fs::write(workspace.path().join("nested/file.txt"), "safe").unwrap();
        fs::write(outside.path().join("file.txt"), "needle secret").unwrap();
        let root = WorkspaceRoot::open(workspace.path()).unwrap();

        let result = search_content_with_hook(&root, "needle", SearchOptions::default(), || {
            fs::rename(
                workspace.path().join("nested"),
                workspace.path().join("moved"),
            )
            .unwrap();
            symlink(outside.path(), workspace.path().join("nested")).unwrap();
        });

        assert!(matches!(
            result,
            Err(SearchError::Io(ref error)) if error.kind() == ErrorKind::PermissionDenied
        ));
    }
}
