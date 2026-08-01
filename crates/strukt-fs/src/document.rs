use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, Permissions};
#[cfg(unix)]
use rustix::fs::renameat;
use strukt_editor::DiskRevision;
use strukt_workspace::WorkspaceRoot;
use thiserror::Error;

const DEFAULT_EDITABLE_BYTES: u64 = 4 * 1024 * 1024;
const DEFAULT_PREVIEW_BYTES: usize = 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
const STAGING_ATTEMPTS: usize = 32;
static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadOptions {
    pub max_editable_bytes: u64,
    pub preview_bytes: usize,
    pub force_full: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_editable_bytes: DEFAULT_EDITABLE_BYTES,
            preview_bytes: DEFAULT_PREVIEW_BYTES,
            force_full: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentKind {
    Text { read_only: bool, truncated: bool },
    Binary,
    InvalidUtf8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentRead {
    pub kind: DocumentKind,
    pub text: Option<String>,
    pub size: u64,
    pub disk_revision: DiskRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveMode {
    IfUnchanged,
    Force,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveRequest {
    path: PathBuf,
    contents: Vec<u8>,
    expected_revision: DiskRevision,
    mode: SaveMode,
}

impl SaveRequest {
    #[must_use]
    pub fn new(
        path: impl Into<PathBuf>,
        contents: Vec<u8>,
        expected_revision: DiskRevision,
    ) -> Self {
        Self {
            path: path.into(),
            contents,
            expected_revision,
            mode: SaveMode::IfUnchanged,
        }
    }

    #[must_use]
    pub const fn with_mode(mut self, mode: SaveMode) -> Self {
        self.mode = mode;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveOutcome {
    pub disk_revision: DiskRevision,
    pub bytes_written: usize,
}

#[derive(Debug, Error)]
pub enum DocumentIoError {
    #[error("document path escapes the workspace: {0}")]
    OutsideRoot(PathBuf),
    #[error("document path is a symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("document is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("the workspace location changed")]
    WorkspaceChanged,
    #[error("document changed on disk (expected {expected:?}, actual {actual:?})")]
    SaveConflict {
        expected: DiskRevision,
        actual: DiskRevision,
    },
    #[error("atomic confined replacement is unavailable on {platform}")]
    AtomicReplaceUnavailable { platform: &'static str },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Reads and classifies a regular file through a retained workspace capability.
///
/// # Errors
///
/// Returns a typed error when the path escapes, resolves to a link or non-file,
/// the workspace identity changed, or the read fails.
pub fn read_document(
    root: &WorkspaceRoot,
    path: impl AsRef<Path>,
    options: ReadOptions,
) -> Result<DocumentRead, DocumentIoError> {
    let path = scoped(path.as_ref())?;
    let directory = validated_capability(root)?;
    let (mut file, _permissions, metadata_size) = open_regular(&directory, &path)?;
    let default_capture_limit = usize::try_from(options.max_editable_bytes)
        .unwrap_or(usize::MAX)
        .saturating_add(1)
        .max(options.preview_bytes.saturating_add(4));
    let capture_limit = if options.force_full {
        usize::MAX
    } else if metadata_size > options.max_editable_bytes {
        options.preview_bytes.saturating_add(4)
    } else {
        default_capture_limit
    };
    let mut captured = Vec::with_capacity(
        usize::try_from(metadata_size)
            .unwrap_or(capture_limit)
            .min(capture_limit),
    );
    let mut hasher = blake3::Hasher::new();
    let mut utf8_carry = Vec::with_capacity(4);
    let mut valid_utf8 = true;
    let mut binary = false;
    let mut size = 0_u64;
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).map_err(DocumentIoError::Io)?;
        if read == 0 {
            break;
        }
        let bytes = &chunk[..read];
        size = size.saturating_add(read as u64);
        hasher.update(bytes);
        if size <= BINARY_SNIFF_BYTES as u64
            || size.saturating_sub(read as u64) < BINARY_SNIFF_BYTES as u64
        {
            let sniff_start =
                usize::try_from(size.saturating_sub(read as u64)).unwrap_or(usize::MAX);
            let sniff_len = BINARY_SNIFF_BYTES.saturating_sub(sniff_start).min(read);
            binary |= bytes[..sniff_len].contains(&0);
        }
        if captured.len() < capture_limit {
            let remaining = capture_limit - captured.len();
            captured.extend_from_slice(&bytes[..remaining.min(read)]);
        }
        if valid_utf8 {
            utf8_carry.extend_from_slice(bytes);
            match std::str::from_utf8(&utf8_carry) {
                Ok(_) => utf8_carry.clear(),
                Err(error) if error.error_len().is_none() => {
                    let incomplete = utf8_carry.split_off(error.valid_up_to());
                    utf8_carry = incomplete;
                }
                Err(_) => valid_utf8 = false,
            }
        }
    }
    valid_utf8 &= utf8_carry.is_empty();
    let disk_revision = DiskRevision::new(hasher.finalize().to_hex().to_string());

    if binary {
        return Ok(DocumentRead {
            kind: DocumentKind::Binary,
            text: None,
            size,
            disk_revision,
        });
    }

    if !valid_utf8 {
        return Ok(DocumentRead {
            kind: DocumentKind::InvalidUtf8,
            text: None,
            size,
            disk_revision,
        });
    }
    let oversized = size > options.max_editable_bytes && !options.force_full;
    let text = if oversized {
        let end = options.preview_bytes.min(captured.len());
        let valid_end = std::str::from_utf8(&captured[..end])
            .map_or_else(|error| error.valid_up_to(), str::len);
        String::from_utf8(captured[..valid_end].to_vec()).map_err(|error| {
            DocumentIoError::Io(io::Error::new(io::ErrorKind::InvalidData, error))
        })?
    } else {
        String::from_utf8(captured).map_err(|error| {
            DocumentIoError::Io(io::Error::new(io::ErrorKind::InvalidData, error))
        })?
    };
    Ok(DocumentRead {
        kind: DocumentKind::Text {
            read_only: oversized,
            truncated: oversized,
        },
        text: Some(text),
        size,
        disk_revision,
    })
}

/// Publishes a complete staged document only when its expected revision still
/// matches, unless the caller explicitly selects [`SaveMode::Force`].
///
/// # Errors
///
/// Returns [`DocumentIoError::SaveConflict`] before publication when the disk
/// revision changed, or another typed confinement/publication error.
pub fn save_document(
    root: &WorkspaceRoot,
    request: &SaveRequest,
) -> Result<SaveOutcome, DocumentIoError> {
    save_document_with_hook(root, request, || Ok(()))
}

fn save_document_with_hook(
    root: &WorkspaceRoot,
    request: &SaveRequest,
    before_publication: impl FnOnce() -> Result<(), DocumentIoError>,
) -> Result<SaveOutcome, DocumentIoError> {
    let path = scoped(&request.path)?;
    let directory = validated_capability(root)?;
    let (current, permissions) = read_regular(&directory, &path)?;
    ensure_expected(request, revision(&current))?;
    let (parent_path, name) = destination_parts(&path)?;
    let parent = open_parent(&directory, parent_path)?;
    let staging = create_staged_file(&parent, &request.contents, permissions)?;

    let publication = (|| {
        before_publication()?;
        if request.mode == SaveMode::IfUnchanged {
            let (latest, _) = read_regular(&directory, &path)?;
            ensure_expected(request, revision(&latest))?;
        }
        atomic_replace(&parent, &staging, name)
    })();
    if let Err(error) = publication {
        let _ = parent.remove_file(&staging.name);
        return Err(error);
    }

    let expected_published = revision(&request.contents);
    let (published, _) = read_regular(&directory, &path)?;
    let actual_published = revision(&published);
    if actual_published != expected_published {
        return Err(DocumentIoError::SaveConflict {
            expected: expected_published,
            actual: actual_published,
        });
    }

    Ok(SaveOutcome {
        disk_revision: expected_published,
        bytes_written: request.contents.len(),
    })
}

fn validated_capability(root: &WorkspaceRoot) -> Result<Dir, DocumentIoError> {
    root.validate_location()
        .map_err(|_| DocumentIoError::WorkspaceChanged)?;
    root.try_clone_capability()
        .map_err(|_| DocumentIoError::WorkspaceChanged)
}

fn scoped(path: &Path) -> Result<PathBuf, DocumentIoError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(DocumentIoError::OutsideRoot(path.to_path_buf()));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(DocumentIoError::OutsideRoot(path.to_path_buf()));
    }
    Ok(normalized)
}

fn read_regular(directory: &Dir, path: &Path) -> Result<(Vec<u8>, Permissions), DocumentIoError> {
    let (mut file, permissions, size) = open_regular(directory, path)?;
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.read_to_end(&mut bytes).map_err(DocumentIoError::Io)?;
    Ok((bytes, permissions))
}

fn open_regular(directory: &Dir, path: &Path) -> Result<(File, Permissions, u64), DocumentIoError> {
    let link_metadata = directory
        .symlink_metadata(path)
        .map_err(DocumentIoError::Io)?;
    if link_metadata.file_type().is_symlink() {
        return Err(DocumentIoError::Symlink(path.to_path_buf()));
    }
    if !link_metadata.is_file() {
        return Err(DocumentIoError::NotRegularFile(path.to_path_buf()));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(path, &options)
        .map_err(DocumentIoError::Io)?;
    let metadata = file.metadata().map_err(DocumentIoError::Io)?;
    if !metadata.is_file() {
        return Err(DocumentIoError::NotRegularFile(path.to_path_buf()));
    }
    let permissions = metadata.permissions();
    Ok((file, permissions, metadata.len()))
}

fn ensure_expected(request: &SaveRequest, actual: DiskRevision) -> Result<(), DocumentIoError> {
    if request.mode == SaveMode::IfUnchanged && request.expected_revision != actual {
        return Err(DocumentIoError::SaveConflict {
            expected: request.expected_revision.clone(),
            actual,
        });
    }
    Ok(())
}

fn destination_parts(path: &Path) -> Result<(&Path, &std::ffi::OsStr), DocumentIoError> {
    let parent = path
        .parent()
        .ok_or_else(|| DocumentIoError::OutsideRoot(path.to_path_buf()))?;
    let name = path
        .file_name()
        .ok_or_else(|| DocumentIoError::OutsideRoot(path.to_path_buf()))?;
    Ok((parent, name))
}

fn open_parent(root: &Dir, parent: &Path) -> Result<Dir, DocumentIoError> {
    if parent.as_os_str().is_empty() {
        root.try_clone().map_err(DocumentIoError::Io)
    } else {
        root.open_dir(parent).map_err(DocumentIoError::Io)
    }
}

fn create_staged_file(
    parent: &Dir,
    contents: &[u8],
    permissions: Permissions,
) -> Result<StagedFile, DocumentIoError> {
    for _ in 0..STAGING_ATTEMPTS {
        let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!(".strukt-save-{}-{id}", std::process::id());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(windows)]
        strukt_platform::prepare_rename_source(&mut options);
        match parent.open_with(&name, &options) {
            Ok(mut file) => {
                let result = (|| {
                    file.set_permissions(permissions)?;
                    file.write_all(contents)?;
                    file.sync_all()
                })();
                if let Err(error) = result {
                    drop(file);
                    let _ = parent.remove_file(&name);
                    return Err(DocumentIoError::Io(error));
                }
                return Ok(StagedFile {
                    name,
                    #[cfg(windows)]
                    file,
                    #[cfg(not(windows))]
                    _file: file,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(DocumentIoError::Io(error)),
        }
    }
    Err(DocumentIoError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique save staging file",
    )))
}

struct StagedFile {
    name: String,
    #[cfg(windows)]
    file: File,
    #[cfg(not(windows))]
    _file: File,
}

#[cfg(unix)]
fn atomic_replace(
    parent: &Dir,
    source: &StagedFile,
    destination: &std::ffi::OsStr,
) -> Result<(), DocumentIoError> {
    renameat(
        parent,
        Path::new(&source.name),
        parent,
        Path::new(destination),
    )
    .map_err(|error| DocumentIoError::Io(error.into()))
}

#[cfg(windows)]
fn atomic_replace(
    parent: &Dir,
    source: &StagedFile,
    destination: &std::ffi::OsStr,
) -> Result<(), DocumentIoError> {
    strukt_platform::atomic_replace(&source.file, parent, destination).map_err(DocumentIoError::Io)
}

fn revision(bytes: &[u8]) -> DiskRevision {
    DiskRevision::new(blake3::hash(bytes).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn staged_failure_keeps_the_complete_old_file_and_cleans_up() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("file.txt"), b"old complete").unwrap();
        let root = WorkspaceRoot::open(directory.path()).unwrap();
        let opened = read_document(&root, "file.txt", ReadOptions::default()).unwrap();

        let result = save_document_with_hook(
            &root,
            &SaveRequest::new("file.txt", b"new complete".to_vec(), opened.disk_revision),
            || Err(DocumentIoError::Io(io::Error::other("injected failure"))),
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read(directory.path().join("file.txt")).unwrap(),
            b"old complete"
        );
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".strukt-save-")
        }));
    }
}
