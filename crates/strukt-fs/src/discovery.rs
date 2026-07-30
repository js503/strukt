use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
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
    let metadata = fs::metadata(&root).map_err(DiscoveryError::Io)?;
    if !metadata.is_dir() {
        return Err(DiscoveryError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("discovery root is not a directory: {}", root.display()),
        )));
    }

    let mut entries = Vec::new();
    let mut warnings = Warnings::default();
    let mut truncated = false;
    let mut accepted = options
        .show_ignored
        .then(|| AcceptedCursor::new(&root, options));
    for result in walker(&root, options) {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(&error);
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
        let ignored = if let Some(accepted) = &mut accepted {
            !accepted.contains(&relative_path, &mut warnings)?
        } else {
            false
        };
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
        let hidden = is_hidden(&root, &relative_path);
        entries.push(FileEntry {
            ignored,
            relative_path,
            kind,
            depth: entry.depth(),
            hidden,
        });
    }

    Ok(DiscoveryReport {
        entries,
        warnings: warnings.values,
        truncated,
    })
}

struct AcceptedCursor<'a> {
    root: &'a Path,
    walk: ignore::Walk,
    next: Option<PathBuf>,
}

impl<'a> AcceptedCursor<'a> {
    fn new(root: &'a Path, options: DiscoveryOptions) -> Self {
        Self {
            root,
            walk: walker(
                root,
                DiscoveryOptions {
                    show_ignored: false,
                    ..options
                },
            ),
            next: None,
        }
    }

    fn contains(&mut self, target: &Path, warnings: &mut Warnings) -> Result<bool, DiscoveryError> {
        loop {
            if let Some(next) = &self.next {
                match next.as_path().cmp(target) {
                    Ordering::Less => self.next = None,
                    Ordering::Equal => return Ok(true),
                    Ordering::Greater => return Ok(false),
                }
            }

            match self.walk.next() {
                Some(Ok(entry)) if entry.depth() > 0 => {
                    self.next = Some(relative_path(self.root, entry.path())?);
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => warnings.push(&error),
                None => return Ok(false),
            }
        }
    }
}

#[derive(Default)]
struct Warnings {
    values: Vec<String>,
    seen: HashSet<String>,
}

impl Warnings {
    fn push(&mut self, error: &ignore::Error) {
        let warning = error.to_string();
        if self.seen.insert(warning.clone()) {
            self.values.push(warning);
        }
    }
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
        .ignore(!options.show_ignored)
        .git_ignore(!options.show_ignored)
        .git_global(!options.show_ignored)
        .git_exclude(!options.show_ignored)
        .parents(true)
        .sort_by_file_path(Path::cmp);
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

fn is_hidden(root: &Path, relative_path: &Path) -> bool {
    if relative_path
        .components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
    {
        return true;
    }

    has_windows_hidden_attribute(root, relative_path)
}

#[cfg(not(windows))]
fn has_windows_hidden_attribute(_root: &Path, _relative_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn has_windows_hidden_attribute(root: &Path, relative_path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

    let mut path = root.to_path_buf();
    for component in relative_path.components() {
        path.push(component.as_os_str());
        if fs::symlink_metadata(&path)
            .is_ok_and(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        {
            return true;
        }
    }
    false
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

#[cfg(test)]
mod tests {
    use super::Warnings;

    #[test]
    fn duplicate_warning_messages_are_reported_once() {
        let mut warnings = Warnings::default();

        let warning = ignore::Error::UnrecognizedFileType("duplicate".to_owned());
        warnings.push(&warning);
        warnings.push(&warning);

        assert_eq!(warnings.values.len(), 1);
    }
}
