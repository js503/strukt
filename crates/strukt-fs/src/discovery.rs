use std::cmp::Ordering;
#[cfg(any(windows, test))]
use std::collections::HashMap;
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
/// Returns [`DiscoveryError::Io`] with [`std::io::ErrorKind::InvalidInput`] when
/// the canonicalized root is not a directory. Other errors are returned when
/// the root cannot be canonicalized or an entry cannot be represented relative
/// to that root.
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
    #[cfg(windows)]
    let mut hidden_attributes =
        HiddenAttributeCache::new(options.show_hidden, windows_hidden_attribute);
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
        let hidden = has_dot_component(&relative_path);
        #[cfg(windows)]
        let hidden = hidden || hidden_attributes.is_hidden(&root, &relative_path);
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

fn has_dot_component(relative_path: &Path) -> bool {
    relative_path
        .components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
}

#[cfg(any(windows, test))]
struct HiddenAttributeCache<F> {
    enabled: bool,
    classifier: F,
    attributes: HashMap<PathBuf, bool>,
}

#[cfg(any(windows, test))]
impl<F> HiddenAttributeCache<F>
where
    F: FnMut(&Path) -> bool,
{
    fn new(enabled: bool, classifier: F) -> Self {
        Self {
            enabled,
            classifier,
            attributes: HashMap::new(),
        }
    }

    fn is_hidden(&mut self, root: &Path, relative_path: &Path) -> bool {
        if !self.enabled {
            return false;
        }

        let mut path = root.to_path_buf();
        for component in relative_path.components() {
            path.push(component.as_os_str());
            let hidden = *self
                .attributes
                .entry(path.clone())
                .or_insert_with(|| (self.classifier)(&path));
            if hidden {
                return true;
            }
        }
        false
    }
}

#[cfg(windows)]
fn windows_hidden_attribute(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
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
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use super::{HiddenAttributeCache, Warnings};

    #[test]
    fn duplicate_warning_messages_are_reported_once() {
        let mut warnings = Warnings::default();

        let warning = ignore::Error::UnrecognizedFileType("duplicate".to_owned());
        warnings.push(&warning);
        warnings.push(&warning);

        assert_eq!(warnings.values.len(), 1);
    }

    #[test]
    fn hidden_attribute_cache_inspects_shared_ancestors_once() {
        let calls = Rc::new(RefCell::new(HashMap::<PathBuf, usize>::new()));
        let classifier_calls = Rc::clone(&calls);
        let mut cache = HiddenAttributeCache::new(true, move |path: &Path| {
            *classifier_calls
                .borrow_mut()
                .entry(path.to_path_buf())
                .or_default() += 1;
            path.ends_with("shared")
        });
        let root = Path::new("/workspace");

        assert!(cache.is_hidden(root, Path::new("shared/first.txt")));
        assert!(cache.is_hidden(root, Path::new("shared/second.txt")));

        assert_eq!(calls.borrow()[&root.join("shared")], 1);
        assert_eq!(calls.borrow().len(), 1);
    }

    #[test]
    fn hidden_attribute_cache_bypasses_classifier_when_hidden_files_are_not_shown() {
        let calls = Rc::new(RefCell::new(0));
        let classifier_calls = Rc::clone(&calls);
        let mut cache = HiddenAttributeCache::new(false, move |_path: &Path| {
            *classifier_calls.borrow_mut() += 1;
            true
        });

        assert!(!cache.is_hidden(Path::new("/workspace"), Path::new("hidden/file.txt")));
        assert_eq!(*calls.borrow(), 0);
    }
}
