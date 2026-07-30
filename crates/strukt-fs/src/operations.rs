use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

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

/// Applies one file operation beneath a canonical local workspace root.
///
/// # Errors
///
/// Returns [`OperationError::OutsideRoot`] when a path escapes through its
/// components or a resolved parent, [`OperationError::SymlinkCopy`] when a
/// duplicate would follow a symbolic link, and the corresponding IO or trash
/// error when the requested filesystem operation fails.
pub fn apply_operation(
    root: impl AsRef<Path>,
    operation: FileOperation,
) -> Result<(), OperationError> {
    let root = canonical_directory(root.as_ref())?;

    match operation {
        FileOperation::CreateFile(path) => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(scoped(&root, &path)?)
                .map_err(OperationError::Io)?;
        }
        FileOperation::CreateDirectory(path) => {
            fs::create_dir(scoped(&root, &path)?).map_err(OperationError::Io)?;
        }
        FileOperation::Rename { from, to } | FileOperation::Move { from, to } => {
            let source = scoped(&root, &from)?;
            let destination = scoped(&root, &to)?;
            ensure_vacant(&destination)?;

            // `std` has no portable atomic no-replace rename. This preflight
            // minimizes the race while keeping normal local-workspace behavior
            // consistent across supported platforms.
            fs::rename(source, destination).map_err(OperationError::Io)?;
        }
        FileOperation::Duplicate { from, to } => {
            let source = scoped(&root, &from)?;
            let destination = scoped(&root, &to)?;
            ensure_vacant(&destination)?;
            duplicate(&root, &source, &destination)?;
        }
        FileOperation::MoveToTrash(path) => {
            trash::delete(scoped(&root, &path)?).map_err(OperationError::Trash)?;
        }
        FileOperation::DeletePermanently(path) => {
            let target = scoped(&root, &path)?;
            let metadata = fs::symlink_metadata(&target).map_err(OperationError::Io)?;
            if metadata.file_type().is_dir() {
                fs::remove_dir_all(target).map_err(OperationError::Io)?;
            } else {
                fs::remove_file(target).map_err(OperationError::Io)?;
            }
        }
    }

    Ok(())
}

fn canonical_directory(root: &Path) -> Result<PathBuf, OperationError> {
    let root = root.canonicalize().map_err(OperationError::Io)?;
    if !root.is_dir() {
        return Err(invalid_input("workspace root must be a directory"));
    }
    Ok(root)
}

fn scoped(root: &Path, relative: &Path) -> Result<PathBuf, OperationError> {
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

    let target = root.join(normalized);
    let parent = target
        .parent()
        .ok_or_else(|| OperationError::OutsideRoot(relative.to_path_buf()))?;
    let resolved_parent = parent.canonicalize().map_err(OperationError::Io)?;
    if !resolved_parent.starts_with(root) {
        return Err(OperationError::OutsideRoot(relative.to_path_buf()));
    }

    let file_name = target
        .file_name()
        .ok_or_else(|| invalid_input("operation target must name a workspace entry"))?;
    Ok(resolved_parent.join(file_name))
}

fn ensure_vacant(path: &Path) -> Result<(), OperationError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(OperationError::Io(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("destination already exists: {}", path.display()),
        ))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(OperationError::Io(error)),
    }
}

fn duplicate(root: &Path, source: &Path, destination: &Path) -> Result<(), OperationError> {
    let metadata = checked_copy_metadata(root, source)?;
    let resolved_source = source.canonicalize().map_err(OperationError::Io)?;
    if metadata.is_dir() && destination.starts_with(&resolved_source) {
        return Err(invalid_input(
            "cannot duplicate a directory into itself or its descendant",
        ));
    }

    let mut plan = Vec::new();
    preflight_copy(root, source, Path::new(""), &mut plan)?;
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
}

fn preflight_copy(
    root: &Path,
    source: &Path,
    relative: &Path,
    plan: &mut Vec<CopyEntry>,
) -> Result<(), OperationError> {
    let metadata = checked_copy_metadata(root, source)?;
    if metadata.is_dir() {
        plan.push(CopyEntry {
            source: source.to_path_buf(),
            relative: relative.to_path_buf(),
            kind: CopyKind::Directory,
        });
        for child in fs::read_dir(source).map_err(OperationError::Io)? {
            let child = child.map_err(OperationError::Io)?;
            preflight_copy(root, &child.path(), &relative.join(child.file_name()), plan)?;
        }
    } else {
        plan.push(CopyEntry {
            source: source.to_path_buf(),
            relative: relative.to_path_buf(),
            kind: CopyKind::File,
        });
    }
    Ok(())
}

fn checked_copy_metadata(root: &Path, source: &Path) -> Result<fs::Metadata, OperationError> {
    let metadata = fs::symlink_metadata(source).map_err(OperationError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(OperationError::SymlinkCopy(source.to_path_buf()));
    }

    let resolved = source.canonicalize().map_err(OperationError::Io)?;
    if !resolved.starts_with(root) {
        return Err(OperationError::OutsideRoot(source.to_path_buf()));
    }
    Ok(metadata)
}

fn execute_copy_plan(
    root: &Path,
    destination: &Path,
    plan: &[CopyEntry],
) -> Result<(), OperationError> {
    for entry in plan {
        checked_copy_metadata(root, &entry.source)?;
        let target = if entry.relative.as_os_str().is_empty() {
            destination.to_path_buf()
        } else {
            destination.join(&entry.relative)
        };
        match entry.kind {
            CopyKind::Directory => {
                fs::create_dir(target).map_err(OperationError::Io)?;
            }
            CopyKind::File => {
                let mut source_file = File::open(&entry.source).map_err(OperationError::Io)?;
                let mut destination_file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .map_err(OperationError::Io)?;
                io::copy(&mut source_file, &mut destination_file).map_err(OperationError::Io)?;
            }
        }
    }
    Ok(())
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
