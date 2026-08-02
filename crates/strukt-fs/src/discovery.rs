use std::cmp::Ordering;
#[cfg(any(windows, test))]
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;
#[cfg(windows)]
use cap_std::fs::MetadataExt as _;
use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use strukt_workspace::WorkspaceRoot;
use thiserror::Error;

use crate::CancellationToken;

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
    discover_report_cancellable(root, options, &CancellationToken::new())
}

/// Discovers entries beneath a path and cooperatively stops when cancelled.
///
/// # Errors
///
/// Returns [`DiscoveryError::Cancelled`] when cancellation is observed, and
/// otherwise returns the same errors as [`discover_report`].
pub fn discover_report_cancellable(
    root: impl AsRef<Path>,
    options: DiscoveryOptions,
    cancellation: &CancellationToken,
) -> Result<DiscoveryReport, DiscoveryError> {
    if cancellation.is_cancelled() {
        return Err(DiscoveryError::Cancelled);
    }
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
        if cancellation.is_cancelled() {
            return Err(DiscoveryError::Cancelled);
        }
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
        let ignored_by_rules = if let Some(accepted) = &mut accepted {
            !accepted.contains(&relative_path, &mut warnings, cancellation)?
        } else {
            false
        };
        let effectively_ignored = ignored_by_rules || has_heavy_component(&relative_path);
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
            ignored: effectively_ignored,
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

/// Discovers entries through a retained workspace directory capability.
///
/// # Errors
///
/// Returns [`DiscoveryError::WorkspaceChanged`] when the display path no longer
/// names the retained root, plus IO and representation errors encountered while
/// enumerating the retained directory.
pub fn discover_report_for_root(
    root: &WorkspaceRoot,
    options: DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    discover_report_for_root_cancellable(root, options, &CancellationToken::new())
}

/// Capability-confined discovery with cooperative cancellation.
///
/// # Errors
///
/// Returns [`DiscoveryError::Cancelled`] when cancellation is observed,
/// [`DiscoveryError::WorkspaceChanged`] when the retained root moves or is
/// replaced, plus IO and representation errors encountered during enumeration.
pub fn discover_report_for_root_cancellable(
    root: &WorkspaceRoot,
    options: DiscoveryOptions,
    cancellation: &CancellationToken,
) -> Result<DiscoveryReport, DiscoveryError> {
    discover_report_for_root_inner(root, options, cancellation, || {})
}

fn discover_report_for_root_inner(
    root: &WorkspaceRoot,
    options: DiscoveryOptions,
    cancellation: &CancellationToken,
    after_capability_clone: impl FnOnce(),
) -> Result<DiscoveryReport, DiscoveryError> {
    check_cancellation(cancellation)?;
    root.validate_location()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;
    let capability = root
        .try_clone_capability()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;
    after_capability_clone();
    check_cancellation(cancellation)?;

    let mut context = CapabilityDiscovery {
        options,
        cancellation,
        entries: Vec::new(),
        warnings: Warnings::default(),
        truncated: false,
    };
    let mut ignores = load_root_ignores(&capability, root.path(), &mut context.warnings);
    walk_capability(
        &capability,
        Path::new(""),
        root.path(),
        0,
        &mut ignores,
        &mut context,
    )?;
    root.validate_location()
        .map_err(|_| DiscoveryError::WorkspaceChanged)?;

    Ok(DiscoveryReport {
        entries: context.entries,
        warnings: context.warnings.values,
        truncated: context.truncated,
    })
}

struct CapabilityDiscovery<'a> {
    options: DiscoveryOptions,
    cancellation: &'a CancellationToken,
    entries: Vec<FileEntry>,
    warnings: Warnings,
    truncated: bool,
}

fn walk_capability(
    directory: &Dir,
    relative_directory: &Path,
    logical_root: &Path,
    depth: usize,
    ignores: &mut Vec<Gitignore>,
    context: &mut CapabilityDiscovery<'_>,
) -> Result<(), DiscoveryError> {
    check_cancellation(context.cancellation)?;
    let mut entries = directory
        .entries()
        .map_err(DiscoveryError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(DiscoveryError::Io)?;
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);

    for entry in entries {
        check_cancellation(context.cancellation)?;
        if context.entries.len() == context.options.max_entries {
            context.truncated = true;
            return Ok(());
        }

        let name = entry.file_name();
        let relative_path = relative_directory.join(&name);
        let file_type = entry.file_type().map_err(DiscoveryError::Io)?;
        let is_directory = file_type.is_dir();
        let hidden = has_dot_component(&relative_path) || capability_hidden(&entry)?;
        let ignored_by_rules =
            ignored_by(ignores, &logical_root.join(&relative_path), is_directory);
        let effectively_ignored = ignored_by_rules || has_heavy_component(&relative_path);
        if (!context.options.show_hidden && hidden)
            || (!context.options.show_ignored && effectively_ignored)
        {
            continue;
        }

        let kind = if is_directory {
            FileKind::Directory
        } else if file_type.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::File
        };
        context.entries.push(FileEntry {
            relative_path: relative_path.clone(),
            kind,
            depth: depth + 1,
            hidden,
            ignored: effectively_ignored,
        });

        if is_directory {
            let child = entry.open_dir().map_err(DiscoveryError::Io)?;
            let added = load_directory_ignores(
                &child,
                &logical_root.join(&relative_path),
                &mut context.warnings,
            );
            let previous_len = ignores.len();
            ignores.extend(added);
            walk_capability(
                &child,
                &relative_path,
                logical_root,
                depth + 1,
                ignores,
                context,
            )?;
            ignores.truncate(previous_len);
            if context.truncated {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn load_root_ignores(
    directory: &Dir,
    logical_root: &Path,
    warnings: &mut Warnings,
) -> Vec<Gitignore> {
    let mut ignores = Vec::new();
    match directory.symlink_metadata(".git") {
        Ok(metadata) if metadata.is_dir() => {
            if let Some(exclude) = load_ignore_file(
                directory,
                Path::new(".git/info/exclude"),
                logical_root,
                warnings,
            ) {
                ignores.push(exclude);
            }
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => warnings.push_message(format!(
            "could not inspect {}: {error}",
            logical_root.join(".git").display()
        )),
    }
    ignores.extend(load_directory_ignores(directory, logical_root, warnings));
    ignores
}

fn load_directory_ignores(
    directory: &Dir,
    logical_directory: &Path,
    warnings: &mut Warnings,
) -> Vec<Gitignore> {
    [".gitignore", ".ignore"]
        .into_iter()
        .filter_map(|name| {
            load_ignore_file(directory, Path::new(name), logical_directory, warnings)
        })
        .collect()
}

fn load_ignore_file(
    directory: &Dir,
    relative_path: &Path,
    logical_directory: &Path,
    warnings: &mut Warnings,
) -> Option<Gitignore> {
    let contents = match directory.read_to_string(relative_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            warnings.push_message(format!(
                "could not read {}: {error}",
                logical_directory.join(relative_path).display()
            ));
            return None;
        }
    };
    let source = logical_directory.join(relative_path);
    let mut builder = GitignoreBuilder::new(logical_directory);
    for line in contents.lines() {
        if let Err(error) = builder.add_line(Some(source.clone()), line) {
            warnings.push_message(error.to_string());
        }
    }
    match builder.build() {
        Ok(ignore) => Some(ignore),
        Err(error) => {
            warnings.push_message(error.to_string());
            None
        }
    }
}

fn ignored_by(ignores: &[Gitignore], path: &Path, is_directory: bool) -> bool {
    for ignore in ignores.iter().rev() {
        let matched = ignore.matched_path_or_any_parents(path, is_directory);
        if matched.is_ignore() {
            return true;
        }
        if matched.is_whitelist() {
            return false;
        }
    }
    false
}

#[cfg(windows)]
fn capability_hidden(entry: &cap_std::fs::DirEntry) -> Result<bool, DiscoveryError> {
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    Ok(entry
        .metadata()
        .map_err(DiscoveryError::Io)?
        .file_attributes()
        & FILE_ATTRIBUTE_HIDDEN
        != 0)
}

#[cfg(not(windows))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the cross-platform helper can fail when Windows metadata is inspected"
)]
fn capability_hidden(_entry: &cap_std::fs::DirEntry) -> Result<bool, DiscoveryError> {
    Ok(false)
}

fn check_cancellation(cancellation: &CancellationToken) -> Result<(), DiscoveryError> {
    if cancellation.is_cancelled() {
        Err(DiscoveryError::Cancelled)
    } else {
        Ok(())
    }
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

    fn contains(
        &mut self,
        target: &Path,
        warnings: &mut Warnings,
        cancellation: &CancellationToken,
    ) -> Result<bool, DiscoveryError> {
        loop {
            if cancellation.is_cancelled() {
                return Err(DiscoveryError::Cancelled);
            }
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
        self.push_message(error.to_string());
    }

    fn push_message(&mut self, warning: String) {
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

fn has_heavy_component(relative_path: &Path) -> bool {
    relative_path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | "node_modules" | "target")
        )
    })
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
    #[error("filesystem discovery was cancelled")]
    Cancelled,
    #[error("workspace root changed after it was opened")]
    WorkspaceChanged,
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

    #[cfg(unix)]
    use super::{DiscoveryError, DiscoveryOptions, discover_report_for_root_inner};
    use super::{HiddenAttributeCache, Warnings};
    #[cfg(unix)]
    use crate::CancellationToken;
    #[cfg(unix)]
    use strukt_workspace::WorkspaceRoot;
    #[cfg(unix)]
    use tempfile::tempdir;

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

    #[cfg(unix)]
    #[test]
    fn capability_discovery_rejects_a_root_replaced_during_enumeration() {
        let parent = tempdir().unwrap();
        let root_path = parent.path().join("workspace");
        let moved_path = parent.path().join("moved");
        std::fs::create_dir(&root_path).unwrap();
        std::fs::write(root_path.join("safe.txt"), "safe").unwrap();
        let root = WorkspaceRoot::open(&root_path).unwrap();

        let result = discover_report_for_root_inner(
            &root,
            DiscoveryOptions::default(),
            &CancellationToken::new(),
            || {
                std::fs::rename(&root_path, &moved_path).unwrap();
                std::fs::create_dir(&root_path).unwrap();
                std::fs::write(root_path.join("secret.txt"), "secret").unwrap();
            },
        );

        assert!(matches!(result, Err(DiscoveryError::WorkspaceChanged)));
    }
}
