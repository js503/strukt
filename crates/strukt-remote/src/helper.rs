use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strukt_fs::CancellationToken;
use strukt_terminal::TerminalSize;
use thiserror::Error;

use crate::{
    Capability, ClientHello, FramingError, ProtocolError, ProtocolLimits, RemoteBuildTarget,
    RemoteError, RemoteErrorKind, RemoteFilesystem, RemoteFilesystemError, RemoteGitSummary,
    RemoteLanguageManager, RemotePath, RemoteProcessManager, RemoteProcessRequest, RequestBody,
    RequestEnvelope, ResponseBody, ResponseEnvelope, ServerHello, StreamChunk, negotiate,
    read_frame, read_preface, write_frame, write_preface,
};

const PROTOCOL_MAJOR: u16 = 1;
const PROTOCOL_MINOR: u16 = 0;

pub struct HelperServer {
    filesystem: RemoteFilesystem,
    git_root: PathBuf,
    processes: Mutex<RemoteProcessManager>,
    languages: Mutex<RemoteLanguageManager>,
}

impl HelperServer {
    /// Opens a helper server confined to one canonical workspace root.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem error when the root cannot be retained.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, HelperError> {
        let path = path.into();
        let filesystem = RemoteFilesystem::open(&path)?;
        let canonical_root = filesystem.canonical_root().to_path_buf();
        Ok(Self {
            filesystem,
            git_root: canonical_root.clone(),
            processes: Mutex::new(
                RemoteProcessManager::new(&canonical_root)
                    .map_err(|error| HelperError::Subsystem(error.to_string()))?,
            ),
            languages: Mutex::new(
                RemoteLanguageManager::new(&canonical_root)
                    .map_err(|error| HelperError::Subsystem(error.to_string()))?,
            ),
        })
    }

    #[must_use]
    pub fn capabilities() -> BTreeSet<Capability> {
        BTreeSet::from([
            Capability::Files,
            Capability::Search,
            Capability::Git,
            Capability::Processes,
            Capability::Language,
        ])
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
        self.handle_cancellable(request, &CancellationToken::new())
    }

    #[must_use]
    pub fn handle_cancellable(
        &self,
        request: &RequestEnvelope,
        cancellation: &CancellationToken,
    ) -> ResponseEnvelope {
        let body = self.handle_body(&request.body, cancellation);
        ResponseEnvelope {
            request_id: request.request_id,
            generation: request.generation,
            body,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive process protocol dispatch remains centralized and auditable"
    )]
    fn handle_body(&self, request: &RequestBody, cancellation: &CancellationToken) -> ResponseBody {
        if cancellation.is_cancelled() {
            return ResponseBody::Error(RemoteError::new(
                RemoteErrorKind::Cancelled,
                "remote helper request was cancelled",
            ));
        }
        if matches!(
            request,
            RequestBody::Stat { .. }
                | RequestBody::ListDirectory { .. }
                | RequestBody::ReadFile { .. }
                | RequestBody::WriteFile { .. }
                | RequestBody::EnumerateFiles { .. }
                | RequestBody::Search { .. }
        ) {
            return self
                .handle_filesystem(request, cancellation)
                .unwrap_or_else(|error| ResponseBody::Error(remote_error(&error)));
        }

        match request {
            RequestBody::GitSummary => match RemoteGitSummary::read(&self.git_root) {
                Ok(summary) => ResponseBody::GitSummary {
                    branch: summary.branch,
                    detached: summary.detached,
                    staged: summary.staged,
                    modified: summary.modified,
                    untracked: summary.untracked,
                },
                Err(error) => subsystem_error(&error),
            },
            RequestBody::Spawn {
                executable,
                args,
                cwd,
                shell,
            } => {
                if *shell {
                    return unsupported("shell processes require a separate exact approval path");
                }
                let cwd = protocol_path(cwd);
                let size = TerminalSize::new(24, 80);
                let request = cwd.and_then(|cwd| {
                    size.map_err(|error| error.to_string()).and_then(|size| {
                        RemoteProcessRequest::new(
                            PathBuf::from(executable),
                            args.iter().map(OsString::from).collect(),
                            cwd,
                            Vec::new(),
                            size,
                        )
                        .map_err(|error| error.to_string())
                    })
                });
                match request.and_then(|request| {
                    self.processes
                        .lock()
                        .map_err(|_| "remote process state is unavailable".to_owned())?
                        .spawn(request)
                        .map_err(|error| error.to_string())
                }) {
                    Ok(process_id) => ResponseBody::ProcessStarted { process_id },
                    Err(error) => internal_error(error),
                }
            }
            RequestBody::ProcessInput { process_id, bytes } => self.with_processes(|manager| {
                manager
                    .write(*process_id, bytes)
                    .map(|()| ResponseBody::Acknowledged)
            }),
            RequestBody::Resize {
                process_id,
                rows,
                columns,
            } => match TerminalSize::new(*rows, *columns) {
                Ok(size) => self.with_processes(|manager| {
                    manager
                        .resize(*process_id, size)
                        .map(|()| ResponseBody::Acknowledged)
                }),
                Err(error) => internal_error(error.to_string()),
            },
            RequestBody::DrainProcess {
                process_id,
                max_bytes,
            } => self.with_processes(|manager| {
                manager
                    .drain(
                        *process_id,
                        64,
                        usize::try_from(*max_bytes).unwrap_or(usize::MAX),
                    )
                    .map(|output| {
                        ResponseBody::Stream(StreamChunk {
                            request_id: crate::RequestId::new(1)
                                .expect("constant request ID is valid"),
                            sequence: 0,
                            bytes: output.bytes,
                        })
                    })
            }),
            RequestBody::PollProcess { process_id } => self.with_processes(|manager| {
                manager.try_wait(*process_id).map(|status| {
                    status.map_or(ResponseBody::Acknowledged, |status| {
                        ResponseBody::Completed {
                            exit_code: status.code(),
                        }
                    })
                })
            }),
            RequestBody::TerminateProcess { process_id } => self.with_processes(|manager| {
                manager
                    .terminate(*process_id, Duration::from_secs(2))
                    .map(|()| ResponseBody::Acknowledged)
            }),
            RequestBody::SpawnLanguage {
                executable,
                args,
                cwd,
            } => match protocol_path(cwd).and_then(|cwd| {
                self.languages
                    .lock()
                    .map_err(|_| "remote language state is unavailable".to_owned())?
                    .spawn(
                        PathBuf::from(executable),
                        args.iter().map(OsString::from).collect(),
                        &cwd,
                    )
                    .map_err(|error| error.to_string())
            }) {
                Ok(process_id) => ResponseBody::ProcessStarted { process_id },
                Err(error) => internal_error(error),
            },
            RequestBody::LanguageInput { process_id, bytes } => self.with_languages(|manager| {
                manager
                    .write(*process_id, bytes)
                    .map(|()| ResponseBody::Acknowledged)
            }),
            RequestBody::ReadLanguage { process_id } => self.with_languages(|manager| {
                manager.try_read(*process_id).map(|bytes| {
                    bytes.map_or(ResponseBody::Acknowledged, |bytes| {
                        ResponseBody::Stream(StreamChunk {
                            request_id: crate::RequestId::new(1)
                                .expect("constant request ID is valid"),
                            sequence: 0,
                            bytes,
                        })
                    })
                })
            }),
            RequestBody::TerminateLanguage { process_id } => self.with_languages(|manager| {
                manager
                    .terminate(*process_id, Duration::from_secs(2))
                    .map(|()| ResponseBody::Acknowledged)
            }),
            RequestBody::Watch { .. } => unsupported("filesystem watch transport is unavailable"),
            RequestBody::Cancel { .. } | RequestBody::GrantCredit { .. } => {
                ResponseBody::Acknowledged
            }
            RequestBody::Stat { .. }
            | RequestBody::ListDirectory { .. }
            | RequestBody::ReadFile { .. }
            | RequestBody::WriteFile { .. }
            | RequestBody::EnumerateFiles { .. }
            | RequestBody::Search { .. } => unreachable!("filesystem requests returned above"),
        }
    }

    fn handle_filesystem(
        &self,
        request: &RequestBody,
        cancellation: &CancellationToken,
    ) -> Result<ResponseBody, RemoteFilesystemError> {
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
                let report = self.filesystem.enumerate_cancellable(
                    *include_ignored,
                    *include_ignored,
                    100_000,
                    cancellation,
                )?;
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
                let result = self.filesystem.search_cancellable(
                    query,
                    *include_ignored,
                    usize::try_from(*limit).unwrap_or(usize::MAX),
                    cancellation,
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
            _ => unreachable!("non-filesystem request routed to filesystem dispatcher"),
        }
    }

    fn with_processes(
        &self,
        operation: impl FnOnce(
            &mut RemoteProcessManager,
        ) -> Result<ResponseBody, crate::RemoteProcessError>,
    ) -> ResponseBody {
        match self.processes.lock() {
            Ok(mut manager) => {
                operation(&mut manager).unwrap_or_else(|error| internal_error(error.to_string()))
            }
            Err(_) => internal_error("remote process state is unavailable"),
        }
    }

    fn with_languages(
        &self,
        operation: impl FnOnce(
            &mut RemoteLanguageManager,
        ) -> Result<ResponseBody, crate::RemoteLanguageError>,
    ) -> ResponseBody {
        match self.languages.lock() {
            Ok(mut manager) => {
                operation(&mut manager).unwrap_or_else(|error| internal_error(error.to_string()))
            }
            Err(_) => internal_error("remote language state is unavailable"),
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
    reader: &mut (impl Read + Send),
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
    writer.flush().map_err(FramingError::from)?;

    run_request_loop(server, reader, writer, negotiated.limits)
}

#[expect(
    clippy::too_many_lines,
    reason = "the concurrent request lifecycle stays centralized so cancellation and cleanup remain auditable"
)]
fn run_request_loop<R: Read + Send, W: Write>(
    server: HelperServer,
    reader: &mut R,
    writer: &mut W,
    limits: ProtocolLimits,
) -> Result<(), HelperError> {
    let server = Arc::new(server);
    let cancellations = Arc::new(Mutex::new(
        HashMap::<crate::RequestId, CancellationToken>::new(),
    ));
    let (input_sender, input_receiver) = mpsc::sync_channel(limits.max_in_flight);
    let (completion_sender, completion_receiver) =
        mpsc::sync_channel::<Completion>(limits.max_in_flight);

    std::thread::scope(|scope| -> Result<(), HelperError> {
        scope.spawn(move || read_requests(reader, limits.max_frame_bytes, &input_sender));
        let mut active = 0_usize;
        let mut input_closed = false;
        loop {
            loop {
                match completion_receiver.try_recv() {
                    Ok(completion) => {
                        active = active.saturating_sub(1);
                        cancellations
                            .lock()
                            .map_err(|_| {
                                HelperError::Subsystem("cancellation state unavailable".into())
                            })?
                            .remove(&completion.request_id);
                        write_response(writer, completion.response, limits.max_frame_bytes)?;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if active != 0 {
                            return Err(HelperError::Subsystem(
                                "request worker channel disconnected".into(),
                            ));
                        }
                        break;
                    }
                }
            }
            if input_closed && active == 0 {
                return Ok(());
            }

            match input_receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(InputEvent::Request(request)) => {
                    if let RequestBody::Cancel { request_id } = &request.body {
                        let cancelled = cancellations
                            .lock()
                            .map_err(|_| {
                                HelperError::Subsystem("cancellation state unavailable".into())
                            })?
                            .get(request_id)
                            .cloned();
                        let body = cancelled.map_or_else(
                            || {
                                ResponseBody::Error(RemoteError::new(
                                    RemoteErrorKind::NotFound,
                                    "remote helper request is not active",
                                ))
                            },
                            |cancellation| {
                                cancellation.cancel();
                                ResponseBody::Acknowledged
                            },
                        );
                        write_response(
                            writer,
                            ResponseEnvelope {
                                request_id: request.request_id,
                                generation: request.generation,
                                body,
                            },
                            limits.max_frame_bytes,
                        )?;
                        continue;
                    }
                    if active >= limits.max_in_flight {
                        write_response(
                            writer,
                            ResponseEnvelope {
                                request_id: request.request_id,
                                generation: request.generation,
                                body: ResponseBody::Error(RemoteError::new(
                                    RemoteErrorKind::CapacityReached,
                                    "remote helper in-flight capacity reached",
                                )),
                            },
                            limits.max_frame_bytes,
                        )?;
                        continue;
                    }
                    let cancellation = CancellationToken::new();
                    let duplicate = {
                        let mut active_cancellations = cancellations.lock().map_err(|_| {
                            HelperError::Subsystem("cancellation state unavailable".into())
                        })?;
                        if let std::collections::hash_map::Entry::Vacant(entry) =
                            active_cancellations.entry(request.request_id)
                        {
                            entry.insert(cancellation.clone());
                            false
                        } else {
                            true
                        }
                    };
                    if duplicate {
                        write_response(
                            writer,
                            ResponseEnvelope {
                                request_id: request.request_id,
                                generation: request.generation,
                                body: ResponseBody::Error(RemoteError::new(
                                    RemoteErrorKind::InvalidRequest,
                                    "duplicate remote helper request ID",
                                )),
                            },
                            limits.max_frame_bytes,
                        )?;
                        continue;
                    }
                    active += 1;
                    let server = Arc::clone(&server);
                    let completion_sender = completion_sender.clone();
                    scope.spawn(move || {
                        let request_id = request.request_id;
                        let response = server.handle_cancellable(&request, &cancellation);
                        let _ = completion_sender.send(Completion {
                            request_id,
                            response,
                        });
                    });
                }
                Ok(InputEvent::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    input_closed = true;
                }
                Ok(InputEvent::Failed(error)) => return Err(HelperError::Framing(error)),
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    })
}

fn read_requests(reader: &mut impl Read, maximum: usize, sender: &mpsc::SyncSender<InputEvent>) {
    loop {
        match read_frame::<_, RequestEnvelope>(reader, maximum) {
            Ok(request) => {
                if sender.send(InputEvent::Request(request)).is_err() {
                    break;
                }
            }
            Err(FramingError::EndOfStream) => {
                let _ = sender.send(InputEvent::Closed);
                break;
            }
            Err(error) => {
                let _ = sender.send(InputEvent::Failed(error));
                break;
            }
        }
    }
}

fn write_response(
    writer: &mut impl Write,
    mut response: ResponseEnvelope,
    maximum: usize,
) -> Result<(), HelperError> {
    if let ResponseBody::Stream(chunk) = &mut response.body {
        chunk.request_id = response.request_id;
    }
    write_frame(writer, &response, maximum)?;
    writer.flush().map_err(FramingError::from)?;
    Ok(())
}

enum InputEvent {
    Request(RequestEnvelope),
    Closed,
    Failed(FramingError),
}

struct Completion {
    request_id: crate::RequestId,
    response: ResponseEnvelope,
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
    #[error("remote helper subsystem failed: {0}")]
    Subsystem(String),
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
        RemoteFilesystemError::Cancelled => RemoteErrorKind::Cancelled,
        RemoteFilesystemError::OpenRoot(_)
        | RemoteFilesystemError::WorkspaceChanged
        | RemoteFilesystemError::Discovery(_)
        | RemoteFilesystemError::Search(_)
        | RemoteFilesystemError::Io(_) => RemoteErrorKind::Internal,
    };
    RemoteError::new(kind, error.to_string())
}

fn protocol_path(path: &str) -> Result<RemotePath, String> {
    if path.is_empty() {
        Ok(RemotePath::root())
    } else {
        RemotePath::new(path.to_owned()).map_err(|error| error.to_string())
    }
}

fn unsupported(detail: &str) -> ResponseBody {
    ResponseBody::Error(RemoteError::new(RemoteErrorKind::Unsupported, detail))
}

fn internal_error(detail: impl AsRef<str>) -> ResponseBody {
    ResponseBody::Error(RemoteError::new(RemoteErrorKind::Internal, detail))
}

fn subsystem_error(error: &impl std::fmt::Display) -> ResponseBody {
    internal_error(error.to_string())
}
