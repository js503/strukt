use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DiscoveryError, DiscoveryOptions, FileEntry, FileKind, discover};

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
    let query = query.to_lowercase();
    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == FileKind::File)
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
/// Returns [`SearchError`] when discovery or file IO fails. Oversized, binary,
/// and invalid UTF-8 files are skipped.
pub fn search_content(
    root: impl AsRef<Path>,
    needle: &str,
    options: SearchOptions,
) -> Result<SearchResult, SearchError> {
    let root = root.as_ref();
    let entries = discover(root, options.discovery).map_err(SearchError::Discovery)?;
    let mut matches = Vec::new();

    for entry in entries {
        if entry.kind != FileKind::File {
            continue;
        }

        let path = root.join(&entry.relative_path);
        if fs::metadata(&path).map_err(SearchError::Io)?.len() > options.max_file_bytes {
            continue;
        }

        let file = File::open(&path).map_err(SearchError::Io)?;
        let mut bytes = Vec::new();
        file.take(options.max_file_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(SearchError::Io)?;
        let oversized = u64::try_from(bytes.len())
            .map_or(true, |bytes_read| bytes_read > options.max_file_bytes);
        if oversized {
            continue;
        }
        if bytes.contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
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
                    preview: line.trim().to_owned(),
                });
            }
        }
    }

    Ok(SearchResult {
        matches,
        truncated: false,
    })
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("file discovery failed: {0}")]
    Discovery(#[source] DiscoveryError),
    #[error("content search IO failed: {0}")]
    Io(#[source] std::io::Error),
}
