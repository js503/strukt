use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use thiserror::Error;

use crate::{
    Capability, ClientHello, FramingError, NegotiatedProtocol, OpenSsh, OpenSshError,
    ProtocolError, ProtocolLimits, RequestBody, RequestEnvelope, RequestId, ResponseBody,
    ResponseEnvelope, ServerHello, SshAlias, negotiate, read_frame, read_preface, write_frame,
    write_preface,
};

const MAX_HELPER_STDERR_BYTES: usize = 64 * 1_024;

pub struct HelperClient<R, W> {
    reader: R,
    writer: W,
    negotiated: NegotiatedProtocol,
    generation: u64,
    next_request_id: u64,
    workspace_root: String,
}

impl<R: Read, W: Write> HelperClient<R, W> {
    /// Performs the helper handshake over an already-authenticated SSH stdio
    /// transport.
    ///
    /// # Errors
    ///
    /// Returns a framing or protocol error when the helper response is invalid.
    pub fn connect(
        mut reader: R,
        mut writer: W,
        hello: &ClientHello,
        capabilities: &BTreeSet<Capability>,
        generation: u64,
    ) -> Result<Self, RemoteClientError> {
        if generation == 0 {
            return Err(RemoteClientError::InvalidGeneration);
        }
        write_preface(&mut writer)?;
        write_frame(&mut writer, &hello, hello.limits.max_frame_bytes)?;
        read_preface(&mut reader)?;
        let server: ServerHello = read_frame(&mut reader, hello.limits.max_frame_bytes)?;
        let negotiated = negotiate(hello, &server, capabilities)?;
        Ok(Self {
            reader,
            writer,
            negotiated,
            generation,
            next_request_id: 1,
            workspace_root: server.workspace_root,
        })
    }

    #[must_use]
    pub const fn capabilities(&self) -> &BTreeSet<Capability> {
        &self.negotiated.capabilities
    }

    #[must_use]
    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    /// Sends one bounded typed request and rejects stale or mismatched results.
    ///
    /// # Errors
    ///
    /// Returns a framing error, request-ID mismatch, stale generation, or request
    /// identifier exhaustion.
    pub fn request(&mut self, body: RequestBody) -> Result<ResponseBody, RemoteClientError> {
        let request_id = RequestId::new(self.next_request_id)?;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(RemoteClientError::RequestIdExhausted)?;
        let request = RequestEnvelope {
            request_id,
            generation: self.generation,
            body,
        };
        write_frame(
            &mut self.writer,
            &request,
            self.negotiated.limits.max_frame_bytes,
        )?;
        let response: ResponseEnvelope =
            read_frame(&mut self.reader, self.negotiated.limits.max_frame_bytes)?;
        if response.request_id != request_id {
            return Err(RemoteClientError::MismatchedRequestId);
        }
        if response.generation != self.generation {
            return Err(RemoteClientError::StaleGeneration {
                expected: self.generation,
                actual: response.generation,
            });
        }
        Ok(response.body)
    }

    #[must_use]
    pub fn into_writer(self) -> W {
        self.writer
    }
}

pub struct OpenSshClient {
    helper: Option<HelperClient<ChildStdout, ChildStdin>>,
    child: Child,
    diagnostics: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl OpenSshClient {
    /// Spawns the versioned helper through OpenSSH and negotiates its protocol.
    ///
    /// # Errors
    ///
    /// Returns a command, spawn, framing, randomness, or protocol error without
    /// falling back to a shell.
    pub fn connect(
        openssh: &OpenSsh,
        alias: &SshAlias,
        version: &str,
        workspace_root: &str,
        generation: u64,
    ) -> Result<Self, RemoteClientError> {
        let spec = openssh.open_helper(alias, version)?;
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let writer = child
            .stdin
            .take()
            .ok_or(RemoteClientError::MissingChildPipe)?;
        let reader = child
            .stdout
            .take()
            .ok_or(RemoteClientError::MissingChildPipe)?;
        let stderr = child
            .stderr
            .take()
            .ok_or(RemoteClientError::MissingChildPipe)?;
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let stderr_thread = Some(read_stderr(stderr, Arc::clone(&diagnostics)));
        let mut nonce = [0_u8; 32];
        if getrandom::fill(&mut nonce).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RemoteClientError::RandomUnavailable);
        }
        let hello = ClientHello {
            protocol_major: 1,
            protocol_minor: 0,
            nonce,
            workspace_root: workspace_root.to_owned(),
            limits: ProtocolLimits::default(),
        };
        let helper = match HelperClient::connect(
            reader,
            writer,
            &hello,
            &crate::HelperServer::capabilities(),
            generation,
        ) {
            Ok(helper) => helper,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            helper: Some(helper),
            child,
            diagnostics,
            stderr_thread,
        })
    }

    #[must_use]
    pub fn capabilities(&self) -> Option<&BTreeSet<Capability>> {
        self.helper.as_ref().map(HelperClient::capabilities)
    }

    #[must_use]
    pub fn workspace_root(&self) -> Option<&str> {
        self.helper.as_ref().map(HelperClient::workspace_root)
    }

    /// Sends one helper request.
    ///
    /// # Errors
    ///
    /// Returns a bounded protocol or transport error.
    pub fn request(&mut self, body: RequestBody) -> Result<ResponseBody, RemoteClientError> {
        self.helper
            .as_mut()
            .ok_or(RemoteClientError::Disconnected)?
            .request(body)
    }

    #[must_use]
    pub fn diagnostics(&self) -> Vec<u8> {
        self.diagnostics
            .lock()
            .map_or_else(|_| Vec::new(), |bytes| bytes.clone())
    }

    pub fn disconnect(&mut self) {
        drop(self.helper.take());
        let mut exited = false;
        for _ in 0..200 {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    exited = true;
                    break;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        if !exited {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for OpenSshClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn read_stderr(
    mut stderr: std::process::ChildStderr,
    diagnostics: Arc<Mutex<Vec<u8>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4_096];
        while let Ok(read) = stderr.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let Ok(mut destination) = diagnostics.lock() else {
                break;
            };
            let remaining = MAX_HELPER_STDERR_BYTES.saturating_sub(destination.len());
            destination.extend_from_slice(&buffer[..read.min(remaining)]);
            if destination.len() == MAX_HELPER_STDERR_BYTES {
                break;
            }
        }
    })
}

#[derive(Debug, Error)]
pub enum RemoteClientError {
    #[error(transparent)]
    Framing(#[from] FramingError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    OpenSsh(#[from] OpenSshError),
    #[error("remote helper process could not be started: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote helper child process did not expose its stdio pipe")]
    MissingChildPipe,
    #[error("secure randomness is unavailable")]
    RandomUnavailable,
    #[error("remote helper client is disconnected")]
    Disconnected,
    #[error("remote helper generation must be nonzero")]
    InvalidGeneration,
    #[error("remote helper request identifiers were exhausted")]
    RequestIdExhausted,
    #[error("remote helper returned a response for a different request")]
    MismatchedRequestId,
    #[error("remote helper returned stale generation {actual}; expected {expected}")]
    StaleGeneration { expected: u64, actual: u64 },
}
