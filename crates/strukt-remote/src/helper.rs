use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::PathBuf;

use thiserror::Error;

use crate::{
    Capability, ClientHello, FramingError, ProtocolError, ProtocolLimits, RemoteBuildTarget,
    RemoteError, RemoteErrorKind, RemoteFilesystem, RemoteFilesystemError, RemotePath, RequestBody,
    RequestEnvelope, ResponseBody, ResponseEnvelope, ServerHello, StreamChunk, negotiate,
    read_frame, read_preface, write_frame, write_preface,
};

const PROTOCOL_MAJOR: u16 = 1;
const PROTOCOL_MINOR: u16 = 0;

pub struct HelperServer {
    filesystem: RemoteFilesystem,
}

impl HelperServer {
    /// Opens a helper server confined to one canonical workspace root.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem error when the root cannot be retained.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, HelperError> {
        Ok(Self {
            filesystem: RemoteFilesystem::open(path.into())?,
        })
    }

    #[must_use]
    pub fn capabilities() -> BTreeSet<Capability> {
        BTreeSet::from([Capability::Files, Capability::Search])
    }

    #[must_use]
    pub fn canonical_root(&self) -> String {
        self.filesystem
            .canonical_root()
            .to_string_lossy()
            .into_owned()
    }

    #[must_use]
    pub fn handle(&self, request: &RequestEnvelope) -> ResponseEnvelope {
        let body = self
            .handle_body(&request.body)
            .unwrap_or_else(|error| ResponseBody::Error(remote_error(&error)));
        ResponseEnvelope {
            request_id: request.request_id,
            generation: request.generation,
            body,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive bounded protocol dispatch remains centralized and auditable"
    )]
    fn handle_body(&self, request: &RequestBody) -> Result<ResponseBody, RemoteFilesystemError> {
        match request {
            RequestBody::Stat { path } => {
                let document = self.filesystem.read(&RemotePath::new(path.clone())?)?;
                Ok(ResponseBody::Metadata {
                    revision: document.revision,
                    kind: format!("{:?}", document.kind).to_ascii_lowercase(),
                    size: document.size,
                })
            }
            RequestBody::ListDirectory {
                path,
                cursor,
                limit,
            } => {
                let path = if path.is_empty() {
                    RemotePath::root()
                } else {
                    RemotePath::new(path.clone())?
                };
                let page = self.filesystem.list(
                    &path,
                    cursor.as_deref(),
                    usize::try_from(*limit).unwrap_or(usize::MAX),
                )?;
                Ok(ResponseBody::DirectoryPage {
                    entries: page.entries.into_iter().map(|entry| entry.path).collect(),
                    next_cursor: page.next_cursor,
                })
            }
            RequestBody::ReadFile {
                path,
                offset,
                length,
            } => {
                let document = self.filesystem.read(&RemotePath::new(path.clone())?)?;
                let start = usize::try_from(*offset).unwrap_or(usize::MAX);
                let requested = usize::try_from(*length).unwrap_or(usize::MAX);
                let end = start.saturating_add(requested).min(document.bytes.len());
                let bytes = document.bytes.get(start..end).unwrap_or(&[]).to_vec();
                Ok(ResponseBody::Stream(StreamChunk {
                    request_id: crate::RequestId::new(1).expect("constant request ID is valid"),
                    sequence: 0,
                    bytes,
                }))
            }
            RequestBody::WriteFile {
                path,
                expected_revision,
                bytes,
            } => {
                let outcome = self.filesystem.save(
                    &RemotePath::new(path.clone())?,
                    bytes,
                    expected_revision,
                    false,
                )?;
                Ok(ResponseBody::Metadata {
                    revision: outcome.revision,
                    kind: "file".into(),
                    size: u64::try_from(outcome.bytes_written).unwrap_or(u64::MAX),
                })
            }
            RequestBody::EnumerateFiles { include_ignored } => {
                let report =
                    self.filesystem
                        .enumerate(*include_ignored, *include_ignored, 100_000)?;
                Ok(ResponseBody::DirectoryPage {
                    entries: report.paths,
                    next_cursor: None,
                })
            }
            RequestBody::Search {
                query,
                include_ignored,
                limit,
            } => {
                let result = self.filesystem.search(
                    query,
                    *include_ignored,
                    usize::try_from(*limit).unwrap_or(usize::MAX),
                )?;
                Ok(ResponseBody::DirectoryPage {
                    entries: result
                        .matches
                        .into_iter()
                        .map(|item| format!("{}:{}:{}", item.path, item.line, item.preview))
                        .collect(),
                    next_cursor: None,
                })
            }
            RequestBody::Watch { .. }
            | RequestBody::Cancel { .. }
            | RequestBody::GrantCredit { .. } => Ok(ResponseBody::Acknowledged),
            RequestBody::GitSummary
            | RequestBody::Spawn { .. }
            | RequestBody::ProcessInput { .. }
            | RequestBody::Resize { .. }
            | RequestBody::LanguageInput { .. } => Ok(ResponseBody::Error(RemoteError::new(
                RemoteErrorKind::Unsupported,
                "helper capability is not implemented in this version",
            ))),
        }
    }
}

/// Runs one helper protocol session over protected SSH stdio.
///
/// # Errors
///
/// Returns a typed framing, protocol, root, or output error. Per-request filesystem
/// failures are returned as typed response frames without terminating the session.
pub fn run_helper_stdio(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<(), HelperError> {
    read_preface(reader)?;
    let client: ClientHello = read_frame(reader, crate::DEFAULT_FRAME_LIMIT)?;
    if client.protocol_major != PROTOCOL_MAJOR {
        return Err(HelperError::Protocol(ProtocolError::IncompatibleMajor));
    }
    let server = HelperServer::open(expand_root(&client.workspace_root)?)?;
    let server_hello = ServerHello {
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        nonce: client.nonce,
        helper_version: env!("CARGO_PKG_VERSION").into(),
        build_target: build_target(),
        workspace_root: server.canonical_root(),
        limits: ProtocolLimits::default(),
        capabilities: HelperServer::capabilities(),
    };
    let negotiated = negotiate(&client, &server_hello, &HelperServer::capabilities())?;
    write_preface(writer)?;
    write_frame(writer, &server_hello, negotiated.limits.max_frame_bytes)?;

    loop {
        let request =
            match read_frame::<_, RequestEnvelope>(reader, negotiated.limits.max_frame_bytes) {
                Ok(request) => request,
                Err(FramingError::EndOfStream) => break,
                Err(error) => return Err(HelperError::Framing(error)),
            };
        let mut response = server.handle(&request);
        if let ResponseBody::Stream(chunk) = &mut response.body {
            chunk.request_id = request.request_id;
        }
        write_frame(writer, &response, negotiated.limits.max_frame_bytes)?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum HelperError {
    #[error(transparent)]
    Framing(#[from] FramingError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Filesystem(#[from] RemoteFilesystemError),
    #[error("remote helper workspace root is invalid")]
    InvalidRoot,
}

fn expand_root(root: &str) -> Result<PathBuf, HelperError> {
    if root == "~" || root.starts_with("~/") {
        let home = std::env::var_os("HOME").ok_or(HelperError::InvalidRoot)?;
        let suffix = root.strip_prefix("~/").unwrap_or("");
        return Ok(PathBuf::from(home).join(suffix));
    }
    let path = PathBuf::from(root);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(HelperError::InvalidRoot)
    }
}

const fn build_target() -> RemoteBuildTarget {
    #[cfg(target_arch = "aarch64")]
    {
        RemoteBuildTarget::LinuxAarch64
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        RemoteBuildTarget::LinuxX86_64
    }
}

fn remote_error(error: &RemoteFilesystemError) -> RemoteError {
    let kind = match error {
        RemoteFilesystemError::Conflict { .. } => RemoteErrorKind::Conflict,
        RemoteFilesystemError::InvalidPath(_)
        | RemoteFilesystemError::InvalidCursor
        | RemoteFilesystemError::InvalidLimit => RemoteErrorKind::InvalidRequest,
        RemoteFilesystemError::Confined(_) => RemoteErrorKind::PermissionDenied,
        RemoteFilesystemError::FileTooLarge => RemoteErrorKind::CapacityReached,
        RemoteFilesystemError::OpenRoot(_)
        | RemoteFilesystemError::WorkspaceChanged
        | RemoteFilesystemError::Discovery(_)
        | RemoteFilesystemError::Search(_)
        | RemoteFilesystemError::Io(_) => RemoteErrorKind::Internal,
    };
    RemoteError::new(kind, error.to_string())
}
