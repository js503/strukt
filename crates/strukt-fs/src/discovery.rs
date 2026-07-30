use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscoveryOptions {
    pub show_hidden: bool,
    pub show_ignored: bool,
    pub max_entries: usize,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_ignored: false,
            max_entries: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileEntry {
    pub relative_path: PathBuf,
    pub kind: FileKind,
    pub depth: usize,
    pub hidden: bool,
    pub ignored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryReport {
    pub entries: Vec<FileEntry>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

/// Discovers entries beneath a local workspace root.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when the root cannot be canonicalized or an entry
/// cannot be represented relative to that root.
pub fn discover(
    root: impl AsRef<Path>,
    options: DiscoveryOptions,
) -> Result<Vec<FileEntry>, DiscoveryError> {
    Ok(discover_report(root, options)?.entries)
}

/// Discovers entries and reports warnings and result truncation.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when the root cannot be canonicalized or an entry
/// cannot be represented relative to that root.
pub fn discover_report(
    root: impl AsRef<Path>,
    options: DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    let root = root.as_ref().canonicalize().map_err(DiscoveryError::Io)?;
    let mut warnings = Vec::new();
    let accepted_paths = if options.show_ignored {
        collect_relative_paths(
            &root,
            DiscoveryOptions {
                show_ignored: false,
                ..options
            },
            &mut warnings,
        )?
    } else {
        HashSet::new()
    };

    let mut entries = Vec::new();
    let mut truncated = false;
    for result in walker(&root, options) {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(error.to_string());
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        if entries.len() == options.max_entries {
            truncated = true;
            break;
        }

        let relative_path = relative_path(&root, entry.path())?;
        let file_type = entry
            .file_type()
            .ok_or_else(|| DiscoveryError::MissingType(entry.path().to_path_buf()))?;
        let kind = if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::File
        };
        let hidden = relative_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'));
        entries.push(FileEntry {
            ignored: options.show_ignored && !accepted_paths.contains(&relative_path),
            relative_path,
            kind,
            depth: entry.depth(),
            hidden,
        });
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(DiscoveryReport {
        entries,
        warnings,
        truncated,
    })
}

fn collect_relative_paths(
    root: &Path,
    options: DiscoveryOptions,
    warnings: &mut Vec<String>,
) -> Result<HashSet<PathBuf>, DiscoveryError> {
    let mut paths = HashSet::new();
    for result in walker(root, options) {
        match result {
            Ok(entry) if entry.depth() > 0 => {
                paths.insert(relative_path(root, entry.path())?);
            }
            Ok(_) => {}
            Err(error) => warnings.push(error.to_string()),
        }
    }
    Ok(paths)
}

fn relative_path(root: &Path, path: &Path) -> Result<PathBuf, DiscoveryError> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| DiscoveryError::OutsideRoot(path.to_path_buf()))
}

fn walker(root: &Path, options: DiscoveryOptions) -> ignore::Walk {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!options.show_hidden)
        .git_ignore(!options.show_ignored)
        .git_global(!options.show_ignored)
        .git_exclude(!options.show_ignored)
        .parents(true);
    let exclude_heavy = !options.show_ignored;
    builder.filter_entry(move |entry| {
        !exclude_heavy
            || !matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target")
            )
    });
    builder.build()
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("filesystem IO failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("filesystem walk failed: {0}")]
    Walk(#[source] ignore::Error),
    #[error("entry escaped workspace root: {0}")]
    OutsideRoot(PathBuf),
    #[error("entry has no file type: {0}")]
    MissingType(PathBuf),
}
