use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cap_fs_ext::{
    FollowSymlinks, MetadataExt as IdentityMetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt,
};
#[cfg(windows)]
use cap_std::fs::MetadataExt as WindowsMetadataExt;
#[cfg(unix)]
use cap_std::fs::PermissionsExt;
use cap_std::fs::{Dir, File, Metadata, OpenOptions, Permissions};
#[cfg(unix)]
use rustix::fs::{RenameFlags, renameat_with};
use strukt_workspace::WorkspaceRoot;
use thiserror::Error;

#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const STAGING_PREFIX: &str = ".strukt-duplicate-stage-";
const STAGING_ATTEMPTS: usize = 32;
const STAGING_PAYLOAD: &str = "payload";
static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileOperation {
    CreateFile(PathBuf),
    CreateDirectory(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Duplicate { from: PathBuf, to: PathBuf },
    MoveToTrash(PathBuf),
    DeletePermanently(PathBuf),
}

/// Applies one file operation beneath a retained local workspace capability.
///
/// Paths used for filesystem mutations are resolved relative to the capability
/// retained when [`WorkspaceRoot`] was opened. The canonical display path is
/// checked before execution, and a renamed, replaced, symlinked, or reparse
/// workspace root is rejected.
///
/// # Errors
///
/// Returns [`OperationError::OutsideRoot`] when a path lexically escapes,
/// [`OperationError::SymlinkCopy`] when a duplicate encounters a symbolic
/// link, [`OperationError::TrashUnavailable`] when the platform cannot trash
/// through the retained capability, and the corresponding IO error when the
/// requested operation fails.
pub fn apply_operation(
    root: &WorkspaceRoot,
    operation: FileOperation,
) -> Result<(), OperationError> {
    root.validate_location()
        .map_err(|_| OperationError::WorkspaceChanged)?;
    let root = root
        .try_clone_capability()
        .map_err(|_| OperationError::WorkspaceChanged)?;

    match operation {
        FileOperation::CreateFile(path) => {
            let path = scoped(&path)?;
            root.open_with(path, OpenOptions::new().write(true).create_new(true))
                .map_err(OperationError::Io)?;
        }
        FileOperation::CreateDirectory(path) => {
            root.create_dir(scoped(&path)?)
                .map_err(OperationError::Io)?;
        }
        FileOperation::Rename { from, to } | FileOperation::Move { from, to } => {
            let source = scoped(&from)?;
            let destination = scoped(&to)?;
            rename_noreplace(&root, &source, &destination)?;
        }
        FileOperation::Duplicate { from, to } => {
            let source = scoped(&from)?;
            let destination = scoped(&to)?;
            duplicate(&root, &source, &destination)?;
        }
        FileOperation::MoveToTrash(path) => {
            let path = scoped(&path)?;
            trash_confined(&root, &path)?;
        }
        FileOperation::DeletePermanently(path) => {
            let path = scoped(&path)?;
            delete_permanently(&root, &path)?;
        }
    }

    Ok(())
}

fn scoped(relative: &Path) -> Result<PathBuf, OperationError> {
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(OperationError::OutsideRoot(relative.to_path_buf()));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(invalid_input(
            "operation target must name an entry inside the workspace",
        ));
    }
    Ok(normalized)
}

fn rename_noreplace(root: &Dir, source: &Path, destination: &Path) -> Result<(), OperationError> {
    rename_noreplace_with_hook(root, source, destination, || Ok(()))
}

fn rename_noreplace_with_hook(
    root: &Dir,
    source: &Path,
    destination: &Path,
    before_publication: impl FnOnce() -> Result<(), OperationError>,
) -> Result<(), OperationError> {
    let (source_parent, source_name) = destination_parts(source)?;
    let (destination_parent, destination_name) = destination_parts(destination)?;
    let source_parent = open_destination_parent(root, source_parent)?;
    let destination_parent = open_destination_parent(root, destination_parent)?;
    before_publication()?;
    atomic_rename_noreplace(
        &source_parent,
        Path::new(source_name),
        &destination_parent,
        Path::new(destination_name),
    )
}

#[cfg(unix)]
fn atomic_rename_noreplace(
    source_parent: &Dir,
    source_name: &Path,
    destination_parent: &Dir,
    destination_name: &Path,
) -> Result<(), OperationError> {
    renameat_with(
        source_parent,
        source_name,
        destination_parent,
        destination_name,
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| OperationError::Io(error.into()))
}

#[cfg(windows)]
fn atomic_rename_noreplace(
    source_parent: &Dir,
    source_name: &Path,
    destination_parent: &Dir,
    destination_name: &Path,
) -> Result<(), OperationError> {
    // MoveFileExW without MOVEFILE_REPLACE_EXISTING is an atomic no-replace
    // publication on Windows. cap-std performs this operation relative to the
    // two already-open parent capabilities.
    source_parent
        .rename(source_name, destination_parent, destination_name)
        .map_err(OperationError::Io)
}

fn trash_confined(_root: &Dir, path: &Path) -> Result<(), OperationError> {
    Err(OperationError::TrashUnavailable {
        path: path.to_path_buf(),
        reason: "the platform Trash API cannot consume a retained directory capability",
    })
}

fn delete_permanently(root: &Dir, path: &Path) -> Result<(), OperationError> {
    let metadata = root.symlink_metadata(path).map_err(OperationError::Io)?;
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        root.remove_dir_all(path).map_err(OperationError::Io)?;
    } else {
        #[cfg(windows)]
        // FILE_ATTRIBUTE_DIRECTORY marks directory reparse points, including
        // directory symlinks, which Windows removes with directory semantics.
        if file_type.is_symlink()
            && metadata.file_attributes() & WINDOWS_FILE_ATTRIBUTE_DIRECTORY != 0
        {
            root.remove_dir(path).map_err(OperationError::Io)?;
            return Ok(());
        }
        root.remove_file(path).map_err(OperationError::Io)?;
    }
    Ok(())
}

fn duplicate(root: &Dir, source: &Path, destination: &Path) -> Result<(), OperationError> {
    duplicate_with_hook(root, source, destination, &mut || Ok(()))
}

fn duplicate_with_hook(
    root: &Dir,
    source: &Path,
    destination: &Path,
    after_file_copy: &mut impl FnMut() -> Result<(), OperationError>,
) -> Result<(), OperationError> {
    let source_metadata = checked_copy_metadata(root, source)?;
    reject_resolved_descendant(root, source, destination, source_metadata.is_dir())?;
    let plan = preflight_copy(root, source)?;
    let (destination_parent, destination_name) = destination_parts(destination)?;
    let parent = open_destination_parent(root, destination_parent)?;
    let staging = create_staging(&parent)?;

    if let Err(error) = execute_copy_plan(
        root,
        &staging,
        Path::new(STAGING_PAYLOAD),
        &plan,
        after_file_copy,
    ) {
        return Err(cleanup_failed_staging(staging, error));
    }

    if let Err(error) = atomic_rename_noreplace(
        &staging,
        Path::new(STAGING_PAYLOAD),
        &parent,
        Path::new(destination_name),
    ) {
        return Err(cleanup_failed_staging(staging, error));
    }

    // cap-std cannot publish a read-only directory payload on every supported
    // platform. The staged root is restrictive (0700 on Unix), then its saved
    // final permissions are applied immediately through the held parent
    // capability after publication. Child permissions are finalized in stage.
    if let Some(permissions) = top_directory_permissions(&plan)
        && let Err(error) = parent
            .set_permissions(Path::new(destination_name), permissions)
            .map_err(|error| {
                OperationError::Io(io::Error::new(
                    error.kind(),
                    format!("published duplicate final permission update failed: {error}"),
                ))
            })
    {
        return Err(cleanup_published_stage(staging, error));
    }

    staging.remove_open_dir_all().map_err(|error| {
        OperationError::Io(io::Error::other(format!(
            "duplicate published but staging cleanup failed: {error}"
        )))
    })
}

fn top_directory_permissions(plan: &[CopyEntry]) -> Option<Permissions> {
    plan.first().and_then(|entry| {
        matches!(entry.kind, CopyKind::Directory).then(|| entry.permissions.clone())
    })
}

fn reject_resolved_descendant(
    root: &Dir,
    source: &Path,
    destination: &Path,
    source_is_directory: bool,
) -> Result<(), OperationError> {
    if !source_is_directory {
        return Ok(());
    }

    let canonical_source = root.canonicalize(source).map_err(OperationError::Io)?;
    let (destination_parent, destination_name) = destination_parts(destination)?;
    let canonical_parent = root
        .canonicalize(nonempty_parent(destination_parent))
        .map_err(OperationError::Io)?;
    let canonical_destination = canonical_parent.join(destination_name);
    if canonical_destination.starts_with(&canonical_source)
        || destination_ancestor_matches_source(root, &canonical_source, &canonical_parent)?
    {
        return Err(invalid_input(
            "cannot duplicate a directory into itself or its descendant",
        ));
    }
    Ok(())
}

fn destination_ancestor_matches_source(
    root: &Dir,
    canonical_source: &Path,
    canonical_parent: &Path,
) -> Result<bool, OperationError> {
    let source = root
        .open_dir(canonical_source)
        .map_err(OperationError::Io)?
        .dir_metadata()
        .map_err(OperationError::Io)?;
    let mut ancestor = Some(canonical_parent);

    while let Some(path) = ancestor {
        let metadata = open_destination_parent(root, path)?
            .dir_metadata()
            .map_err(OperationError::Io)?;
        if source.dev() == metadata.dev() && source.ino() == metadata.ino() {
            return Ok(true);
        }
        ancestor = path.parent().filter(|parent| *parent != path);
    }
    Ok(false)
}

fn destination_parts(destination: &Path) -> Result<(&Path, &std::ffi::OsStr), OperationError> {
    let parent = destination
        .parent()
        .ok_or_else(|| OperationError::OutsideRoot(destination.to_path_buf()))?;
    let name = destination
        .file_name()
        .ok_or_else(|| invalid_input("duplicate destination must name a workspace entry"))?;
    Ok((parent, name))
}

fn nonempty_parent(parent: &Path) -> &Path {
    if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    }
}

fn open_destination_parent(root: &Dir, parent: &Path) -> Result<Dir, OperationError> {
    if parent.as_os_str().is_empty() {
        root.try_clone().map_err(OperationError::Io)
    } else {
        root.open_dir(parent).map_err(OperationError::Io)
    }
}

fn create_staging(parent: &Dir) -> Result<Dir, OperationError> {
    for _ in 0..STAGING_ATTEMPTS {
        let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!("{STAGING_PREFIX}{}-{id}", std::process::id());
        match parent.create_dir(&name) {
            Ok(()) => {
                if let Err(error) = make_staging_private(parent, Path::new(&name)) {
                    let cleanup_result = parent.remove_dir_all(&name);
                    return Err(match cleanup_result {
                        Ok(()) => error,
                        Err(cleanup_error) => combined_cleanup_error(&error, &cleanup_error),
                    });
                }
                return parent.open_dir(&name).map_err(|open_error| {
                    let cleanup_result = parent.remove_dir_all(&name);
                    match cleanup_result {
                        Ok(()) => OperationError::Io(open_error),
                        Err(cleanup_error) => {
                            combined_cleanup_error(&OperationError::Io(open_error), &cleanup_error)
                        }
                    }
                });
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(OperationError::Io(error)),
        }
    }
    Err(OperationError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique duplicate staging directory",
    )))
}

fn make_staging_private(parent: &Dir, name: &Path) -> Result<(), OperationError> {
    #[cfg(unix)]
    parent
        .set_permissions(name, Permissions::from_mode(0o700))
        .map_err(OperationError::Io)?;
    #[cfg(not(unix))]
    let _ = (parent, name);
    Ok(())
}

#[derive(Clone, Copy)]
enum CopyKind {
    Directory,
    File,
}

struct CopyEntry {
    source: PathBuf,
    relative: PathBuf,
    kind: CopyKind,
    permissions: Permissions,
}

fn preflight_copy(root: &Dir, source: &Path) -> Result<Vec<CopyEntry>, OperationError> {
    let mut plan = Vec::new();
    let mut pending = vec![(source.to_path_buf(), PathBuf::new())];

    while let Some((path, relative)) = pending.pop() {
        let metadata = checked_copy_metadata(root, &path)?;
        let kind = if metadata.is_dir() {
            CopyKind::Directory
        } else {
            CopyKind::File
        };
        plan.push(CopyEntry {
            source: path.clone(),
            relative,
            kind,
            permissions: metadata.permissions(),
        });

        if matches!(kind, CopyKind::Directory) {
            let entry_index = plan.len() - 1;
            let relative = plan[entry_index].relative.clone();
            let mut children = root
                .read_dir(&path)
                .map_err(OperationError::Io)?
                .map(|entry| entry.map_err(OperationError::Io))
                .collect::<Result<Vec<_>, _>>()?;
            children.sort_by_key(cap_std::fs::DirEntry::file_name);
            for child in children.into_iter().rev() {
                let name = child.file_name();
                pending.push((path.join(&name), relative.join(name)));
            }
        }
    }

    Ok(plan)
}

fn checked_copy_metadata(root: &Dir, source: &Path) -> Result<Metadata, OperationError> {
    let metadata = root.symlink_metadata(source).map_err(OperationError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(OperationError::SymlinkCopy(source.to_path_buf()));
    }
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(invalid_input(
            "duplicate source must contain only regular files and directories",
        ));
    }
    Ok(metadata)
}

fn execute_copy_plan(
    source_root: &Dir,
    destination_root: &Dir,
    destination: &Path,
    plan: &[CopyEntry],
    after_file_copy: &mut impl FnMut() -> Result<(), OperationError>,
) -> Result<(), OperationError> {
    if plan.is_empty() {
        return Err(invalid_input(
            "duplicate source produced an empty copy plan",
        ));
    }

    for entry in plan {
        let target = copy_target(destination, &entry.relative);
        match entry.kind {
            CopyKind::Directory => {
                destination_root
                    .create_dir(&target)
                    .map_err(OperationError::Io)?;
            }
            CopyKind::File => {
                let mut source_file = open_regular_source(source_root, &entry.source)?;
                let mut destination_file = destination_root
                    .open_with(&target, OpenOptions::new().write(true).create_new(true))
                    .map_err(OperationError::Io)?;
                io::copy(&mut source_file, &mut destination_file).map_err(OperationError::Io)?;
                destination_file
                    .set_permissions(entry.permissions.clone())
                    .map_err(OperationError::Io)?;
                after_file_copy()?;
            }
        }
    }

    for entry in plan.iter().rev() {
        if matches!(entry.kind, CopyKind::Directory) && !entry.relative.as_os_str().is_empty() {
            destination_root
                .set_permissions(
                    copy_target(destination, &entry.relative),
                    entry.permissions.clone(),
                )
                .map_err(OperationError::Io)?;
        }
    }
    set_restrictive_staging_root(destination_root, destination, plan)?;
    Ok(())
}

fn set_restrictive_staging_root(
    destination_root: &Dir,
    destination: &Path,
    plan: &[CopyEntry],
) -> Result<(), OperationError> {
    if !matches!(
        plan.first().map(|entry| entry.kind),
        Some(CopyKind::Directory)
    ) {
        return Ok(());
    }

    #[cfg(unix)]
    destination_root
        .set_permissions(destination, Permissions::from_mode(0o700))
        .map_err(OperationError::Io)?;
    #[cfg(not(unix))]
    let _ = (destination_root, destination);
    Ok(())
}

fn open_regular_source(root: &Dir, source: &Path) -> Result<File, OperationError> {
    let mut options = OpenOptions::new();
    options.read(true).nonblock(true).follow(FollowSymlinks::No);
    let file = root
        .open_with(source, &options)
        .map_err(OperationError::Io)?;
    let opened_metadata = file.metadata().map_err(OperationError::Io)?;
    if !opened_metadata.is_file() {
        return Err(invalid_input(
            "duplicate source changed to a non-regular file",
        ));
    }
    checked_copy_metadata(root, source)?;
    Ok(file)
}

fn copy_target(destination: &Path, relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        destination.to_path_buf()
    } else {
        destination.join(relative)
    }
}

fn cleanup_failed_staging(staging: Dir, original: OperationError) -> OperationError {
    match staging.remove_open_dir_all() {
        Ok(()) => original,
        Err(cleanup) => combined_cleanup_error(&original, &cleanup),
    }
}

fn cleanup_published_stage(staging: Dir, original: OperationError) -> OperationError {
    match staging.remove_open_dir_all() {
        Ok(()) => original,
        Err(cleanup) => OperationError::Io(io::Error::other(format!(
            "duplicate published but final permission update failed: {original}; staging cleanup \
             failed: {cleanup}"
        ))),
    }
}

fn combined_cleanup_error(original: &OperationError, cleanup: &io::Error) -> OperationError {
    OperationError::Io(io::Error::other(format!(
        "duplicate failed: {original}; staging cleanup failed: {cleanup}"
    )))
}

fn invalid_input(message: &'static str) -> OperationError {
    OperationError::Io(io::Error::new(ErrorKind::InvalidInput, message))
}

#[derive(Debug, Error)]
pub enum OperationError {
    #[error("path escapes workspace root: {0}")]
    OutsideRoot(PathBuf),
    #[error("duplicating symbolic links is not supported: {0}")]
    SymlinkCopy(PathBuf),
    #[error("workspace root changed after it was opened")]
    WorkspaceChanged,
    #[error("file operation failed: {0}")]
    Io(#[source] io::Error),
    #[error("cannot safely move {path} to Trash: {reason}")]
    TrashUnavailable { path: PathBuf, reason: &'static str },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use strukt_workspace::WorkspaceRoot;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn injected_failure_after_a_staged_child_cleans_stage_and_allows_retry() {
        let workspace = tempdir().unwrap();
        fs::create_dir(workspace.path().join("source")).unwrap();
        fs::write(workspace.path().join("source/child.txt"), "child").unwrap();
        let workspace_root = WorkspaceRoot::open(workspace.path()).unwrap();
        let root = workspace_root.try_clone_capability().unwrap();
        let mut copied_children = 0;

        let result =
            duplicate_with_hook(&root, Path::new("source"), Path::new("copy"), &mut || {
                copied_children += 1;
                Err(invalid_input("injected copy failure"))
            });

        assert!(result.is_err());
        assert_eq!(copied_children, 1);
        assert!(root.symlink_metadata("copy").is_err());
        assert_no_staging_entries(&root);

        duplicate(&root, Path::new("source"), Path::new("copy")).unwrap();
        assert_eq!(root.read_to_string("copy/child.txt").unwrap(), "child");
        assert_no_staging_entries(&root);
    }

    #[test]
    fn destination_created_during_duplicate_is_never_overwritten() {
        let workspace = tempdir().unwrap();
        fs::write(workspace.path().join("source.txt"), "source").unwrap();
        let workspace_root = WorkspaceRoot::open(workspace.path()).unwrap();
        let root = workspace_root.try_clone_capability().unwrap();
        let mut published_racer = false;

        let result = duplicate_with_hook(
            &root,
            Path::new("source.txt"),
            Path::new("copy.txt"),
            &mut || {
                root.write("copy.txt", "racer")
                    .map_err(OperationError::Io)?;
                published_racer = true;
                Ok(())
            },
        );

        assert!(published_racer);
        assert!(matches!(
            result,
            Err(OperationError::Io(ref error)) if error.kind() == ErrorKind::AlreadyExists
        ));
        assert_eq!(root.read_to_string("copy.txt").unwrap(), "racer");
        assert_eq!(root.read_to_string("source.txt").unwrap(), "source");
        assert_no_staging_entries(&root);
    }

    #[test]
    fn destination_created_at_rename_publication_is_never_overwritten() {
        let workspace = tempdir().unwrap();
        fs::write(workspace.path().join("source.txt"), "source").unwrap();
        let workspace_root = WorkspaceRoot::open(workspace.path()).unwrap();
        let root = workspace_root.try_clone_capability().unwrap();

        let result = rename_noreplace_with_hook(
            &root,
            Path::new("source.txt"),
            Path::new("destination.txt"),
            || {
                root.write("destination.txt", "racer")
                    .map_err(OperationError::Io)
            },
        );

        assert!(matches!(
            result,
            Err(OperationError::Io(ref error)) if error.kind() == ErrorKind::AlreadyExists
        ));
        assert_eq!(root.read_to_string("source.txt").unwrap(), "source");
        assert_eq!(root.read_to_string("destination.txt").unwrap(), "racer");
    }

    fn assert_no_staging_entries(root: &Dir) {
        let entries = root
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(STAGING_PREFIX))
            .collect::<Vec<_>>();
        assert!(
            entries.is_empty(),
            "unexpected staging entries: {entries:?}"
        );
    }
}
