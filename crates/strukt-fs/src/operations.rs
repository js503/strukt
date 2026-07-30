use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

use cap_std::ambient_authority;
#[cfg(windows)]
use cap_std::fs::MetadataExt;
use cap_std::fs::{Dir, File, Metadata, OpenOptions, Permissions};
use thiserror::Error;

#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

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

/// Applies one file operation beneath an open local workspace capability.
///
/// Paths used for filesystem mutations are resolved relative to the capability
/// rather than through ambient process authority. The operating-system Trash
/// API only accepts ambient paths, so [`FileOperation::MoveToTrash`] validates
/// the target's parent through the capability immediately before its ambient
/// handoff, but remains best-effort under adversarial concurrent parent
/// replacement.
///
/// # Errors
///
/// Returns [`OperationError::OutsideRoot`] when a path lexically escapes,
/// [`OperationError::SymlinkCopy`] when a duplicate encounters a symbolic
/// link, and the corresponding IO or trash error when the requested operation
/// fails.
pub fn apply_operation(
    root: impl AsRef<Path>,
    operation: FileOperation,
) -> Result<(), OperationError> {
    let root_path = root.as_ref();
    let root = Dir::open_ambient_dir(root_path, ambient_authority()).map_err(OperationError::Io)?;

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
            ensure_vacant(&root, &destination)?;

            // Neither `std` nor cap-std offers a portable atomic no-replace
            // rename. This capability-scoped preflight minimizes the
            // publication race for normal local-workspace use.
            root.rename(source, &root, destination)
                .map_err(OperationError::Io)?;
        }
        FileOperation::Duplicate { from, to } => {
            let source = scoped(&from)?;
            let destination = scoped(&to)?;
            ensure_vacant(&root, &destination)?;
            duplicate(&root, &source, &destination)?;
        }
        FileOperation::MoveToTrash(path) => {
            let path = scoped(&path)?;
            validate_parent(&root, &path)?;
            trash::delete(root_path.join(path)).map_err(OperationError::Trash)?;
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

fn validate_parent(root: &Dir, path: &Path) -> Result<(), OperationError> {
    let parent = path
        .parent()
        .ok_or_else(|| OperationError::OutsideRoot(path.to_path_buf()))?;
    if parent.as_os_str().is_empty() {
        root.dir_metadata().map_err(OperationError::Io)?;
    } else {
        root.open_dir(parent).map_err(OperationError::Io)?;
    }
    Ok(())
}

fn ensure_vacant(root: &Dir, path: &Path) -> Result<(), OperationError> {
    match root.symlink_metadata(path) {
        Ok(_) => Err(OperationError::Io(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("destination already exists: {}", path.display()),
        ))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(OperationError::Io(error)),
    }
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
    let plan = preflight_copy(root, source)?;
    if matches!(
        plan.first().map(|entry| entry.kind),
        Some(CopyKind::Directory)
    ) && destination.starts_with(source)
    {
        return Err(invalid_input(
            "cannot duplicate a directory into itself or its descendant",
        ));
    }

    execute_copy_plan(root, destination, &plan)
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
    root: &Dir,
    destination: &Path,
    plan: &[CopyEntry],
) -> Result<(), OperationError> {
    let root_kind = plan
        .first()
        .map(|entry| entry.kind)
        .ok_or_else(|| invalid_input("duplicate source produced an empty copy plan"))?;
    let mut created_destination = false;

    let result = (|| {
        for entry in plan {
            let target = copy_target(destination, &entry.relative);
            match entry.kind {
                CopyKind::Directory => {
                    root.create_dir(&target).map_err(OperationError::Io)?;
                    if entry.relative.as_os_str().is_empty() {
                        created_destination = true;
                    }
                }
                CopyKind::File => {
                    let mut source_file = open_regular_source(root, &entry.source)?;
                    let mut destination_file = root
                        .open_with(&target, OpenOptions::new().write(true).create_new(true))
                        .map_err(OperationError::Io)?;
                    if entry.relative.as_os_str().is_empty() {
                        created_destination = true;
                    }
                    io::copy(&mut source_file, &mut destination_file)
                        .map_err(OperationError::Io)?;
                    destination_file
                        .set_permissions(entry.permissions.clone())
                        .map_err(OperationError::Io)?;
                }
            }
        }

        for entry in plan.iter().rev() {
            if matches!(entry.kind, CopyKind::Directory) {
                root.set_permissions(
                    copy_target(destination, &entry.relative),
                    entry.permissions.clone(),
                )
                .map_err(OperationError::Io)?;
            }
        }
        Ok(())
    })();

    if result.is_err() && created_destination {
        cleanup_duplicate(root, destination, root_kind);
    }
    result
}

fn open_regular_source(root: &Dir, source: &Path) -> Result<File, OperationError> {
    let mut options = OpenOptions::new();
    options.read(true)._cap_fs_ext_nonblock(true);
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

fn cleanup_duplicate(root: &Dir, destination: &Path, kind: CopyKind) {
    let _ = match kind {
        CopyKind::Directory => root.remove_dir_all(destination),
        CopyKind::File => root.remove_file(destination),
    };
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
    #[error("file operation failed: {0}")]
    Io(#[source] io::Error),
    #[error("trash operation failed: {0}")]
    Trash(#[source] trash::Error),
}
