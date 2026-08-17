use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use thiserror::Error;

use crate::{
    Capability, ClientHello, FramingError, NegotiatedProtocol, OpenSsh, OpenSshError,
    ProtocolError, ProtocolLimits, RequestBody, RequestEnvelope, RequestId, ResponseBody,
    ResponseEnvelope, ServerHello, SshAlias, negotiate, read_frame, read_preface, write_frame,
    write_preface,
};

const MAX_HELPER_STDERR_BYTES: usize = 64 * 1_024;
const STDERR_READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);
const CHILD_EXIT_POLL_ATTEMPTS: usize = 200;
const CHILD_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    stderr_reader: Option<StderrReader>,
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
        let stderr_reader = Some(read_stderr(stderr, Arc::clone(&diagnostics)));
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
            &BTreeSet::from([
                Capability::Files,
                Capability::Search,
                Capability::Git,
                Capability::Processes,
                Capability::Language,
                Capability::Watches,
                Capability::Sessions,
                Capability::Tmux,
            ]),
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
            stderr_reader,
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
        if !poll_child_exit(&mut self.child) {
            let _ = self.child.kill();
            // Windows job or descendant handle cleanup must not turn an explicit
            // SSH disconnect into an unbounded wait. `try_wait` still reaps the
            // direct child whenever termination completes inside the bound.
            let _ = poll_child_exit(&mut self.child);
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = finish_stderr_reader(reader, STDERR_READER_SHUTDOWN_TIMEOUT);
        }
    }
}

fn poll_child_exit(child: &mut Child) -> bool {
    for _ in 0..CHILD_EXIT_POLL_ATTEMPTS {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(CHILD_EXIT_POLL_INTERVAL),
            Err(_) => return false,
        }
    }
    false
}

impl Drop for OpenSshClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn read_stderr(
    mut stderr: std::process::ChildStderr,
    diagnostics: Arc<Mutex<Vec<u8>>>,
) -> StderrReader {
    let (finished_sender, finished) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
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
        let _ = finished_sender.send(());
    });
    StderrReader { thread, finished }
}

struct StderrReader {
    thread: JoinHandle<()>,
    finished: Receiver<()>,
}

fn finish_stderr_reader(reader: StderrReader, timeout: Duration) -> bool {
    match reader.finished.recv_timeout(timeout) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = reader.thread.join();
            true
        }
        Err(mpsc::RecvTimeoutError::Timeout) => false,
    }
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::{StderrReader, finish_stderr_reader};

    #[test]
    fn diagnostic_reader_shutdown_is_bounded_when_a_descendant_retains_the_pipe() {
        let (finished_sender, finished_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let _ = release_receiver.recv();
            let _ = finished_sender.send(());
        });
        let reader = StderrReader {
            thread,
            finished: finished_receiver,
        };

        let started = Instant::now();
        assert!(!finish_stderr_reader(reader, Duration::from_millis(25)));
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = release_sender.send(());
    }
}
