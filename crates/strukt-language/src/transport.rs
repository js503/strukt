use std::collections::VecDeque;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{ResolvedCommand, bounded_error_text};

const STREAM_CHUNK_BYTES: usize = 64 * 1024;
const STDOUT_CHANNEL_CHUNKS: usize = 4 * 1024 * 1024 / STREAM_CHUNK_BYTES;
const STDERR_LIMIT_BYTES: usize = 1024 * 1024;
const MAX_TERMINATION_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnRequest {
    command: ResolvedCommand,
    current_dir: PathBuf,
}

impl SpawnRequest {
    /// Creates a no-shell spawn request.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidSpawn`] when the working directory is
    /// not absolute.
    pub fn new(command: ResolvedCommand, current_dir: PathBuf) -> Result<Self, TransportError> {
        if !current_dir.is_absolute() {
            return Err(TransportError::InvalidSpawn);
        }
        Ok(Self {
            command,
            current_dir,
        })
    }

    #[must_use]
    pub const fn command(&self) -> &ResolvedCommand {
        &self.command
    }

    #[must_use]
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessExit {
    code: Option<i32>,
    forced: bool,
}

impl ProcessExit {
    #[must_use]
    pub const fn new(code: Option<i32>, forced: bool) -> Self {
        Self { code, forced }
    }

    #[must_use]
    pub const fn code(self) -> Option<i32> {
        self.code
    }

    #[must_use]
    pub const fn success(self) -> bool {
        matches!(self.code, Some(0)) && !self.forced
    }

    #[must_use]
    pub const fn forced(self) -> bool {
        self.forced
    }
}

pub trait LanguageTransport: Send + Sync {
    /// Spawns one language-server process without a shell.
    ///
    /// # Errors
    ///
    /// Returns a transport error when process or reader setup fails.
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn LanguageProcess>, TransportError>;
}

pub trait LanguageProcess: Send {
    /// Writes one already-framed message.
    ///
    /// # Errors
    ///
    /// Returns a transport error when stdin is unavailable or the write fails.
    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError>;

    /// Returns the next bounded stdout chunk without blocking.
    ///
    /// # Errors
    ///
    /// Returns a transport error when the reader failed.
    fn try_read(&mut self) -> Result<Option<Vec<u8>>, TransportError>;

    /// Drains currently retained stderr bytes without blocking.
    ///
    /// # Errors
    ///
    /// Returns a transport error if bounded stderr state is unavailable.
    fn try_read_stderr(&mut self) -> Result<Option<Vec<u8>>, TransportError>;

    /// Checks process exit without blocking.
    ///
    /// # Errors
    ///
    /// Returns a transport error if the process cannot be inspected.
    fn try_wait(&mut self) -> Result<Option<ProcessExit>, TransportError>;

    /// Requests termination, waits for at most two seconds, then forces exit.
    ///
    /// # Errors
    ///
    /// Returns a transport error if termination or waiting fails.
    fn terminate(&mut self, grace: Duration) -> Result<(), TransportError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StdioTransport;

impl LanguageTransport for StdioTransport {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn LanguageProcess>, TransportError> {
        let mut child = Command::new(request.command().executable())
            .args(request.command().arguments())
            .current_dir(request.current_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(TransportError::from)?;
        let stdin = child.stdin.take().ok_or(TransportError::InvalidSpawn)?;
        let stdout = child.stdout.take().ok_or(TransportError::InvalidSpawn)?;
        let stderr = child.stderr.take().ok_or(TransportError::InvalidSpawn)?;

        let (stdout_sender, stdout_receiver) = mpsc::sync_channel(STDOUT_CHANNEL_CHUNKS);
        spawn_stdout_reader(stdout, stdout_sender);
        let stderr_ring = Arc::new(Mutex::new(BoundedByteRing::new(STDERR_LIMIT_BYTES)));
        spawn_stderr_reader(stderr, Arc::clone(&stderr_ring));

        Ok(Box::new(StdioProcess {
            child,
            stdin: Some(stdin),
            stdout: stdout_receiver,
            stderr: stderr_ring,
            forced: false,
        }))
    }
}

struct StdioProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<Result<Vec<u8>, String>>,
    stderr: Arc<Mutex<BoundedByteRing>>,
    forced: bool,
}

impl LanguageProcess for StdioProcess {
    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        let stdin = self.stdin.as_mut().ok_or(TransportError::ClosedStdin)?;
        stdin.write_all(frame).map_err(TransportError::from)?;
        stdin.flush().map_err(TransportError::from)
    }

    fn try_read(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        match self.stdout.try_recv() {
            Ok(Ok(bytes)) => Ok(Some(bytes)),
            Ok(Err(error)) => Err(TransportError::Io(error)),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn try_read_stderr(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        let mut ring = self
            .stderr
            .lock()
            .map_err(|_| TransportError::StateUnavailable)?;
        let bytes = ring.take();
        Ok((!bytes.is_empty()).then_some(bytes))
    }

    fn try_wait(&mut self) -> Result<Option<ProcessExit>, TransportError> {
        self.child
            .try_wait()
            .map(|status| status.map(|status| ProcessExit::new(status.code(), self.forced)))
            .map_err(TransportError::from)
    }

    fn terminate(&mut self, grace: Duration) -> Result<(), TransportError> {
        self.stdin.take();
        let deadline = Instant::now() + grace.min(MAX_TERMINATION_GRACE);
        loop {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            std::thread::sleep((deadline - now).min(Duration::from_millis(10)));
        }
        self.forced = true;
        self.child.kill().map_err(TransportError::from)?;
        self.child.wait().map_err(TransportError::from)?;
        Ok(())
    }
}

impl Drop for StdioProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn spawn_stdout_reader(
    mut stdout: std::process::ChildStdout,
    sender: SyncSender<Result<Vec<u8>, String>>,
) {
    std::thread::spawn(move || {
        loop {
            let mut chunk = vec![0; STREAM_CHUNK_BYTES];
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    chunk.truncate(read);
                    if sender.send(Ok(chunk)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(bounded_error_text(&error.to_string())));
                    break;
                }
            }
        }
    });
}

fn spawn_stderr_reader(mut stderr: std::process::ChildStderr, ring: Arc<Mutex<BoundedByteRing>>) {
    std::thread::spawn(move || {
        loop {
            let mut chunk = vec![0; STREAM_CHUNK_BYTES];
            match stderr.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let Ok(mut ring) = ring.lock() else {
                        break;
                    };
                    ring.extend(&chunk[..read]);
                }
            }
        }
    });
}

#[derive(Debug)]
struct BoundedByteRing {
    bytes: VecDeque<u8>,
    limit: usize,
}

impl BoundedByteRing {
    fn new(limit: usize) -> Self {
        Self {
            bytes: VecDeque::with_capacity(limit),
            limit,
        }
    }

    fn extend(&mut self, incoming: &[u8]) {
        let skip = incoming.len().saturating_sub(self.limit);
        self.bytes.extend(incoming[skip..].iter().copied());
        while self.bytes.len() > self.limit {
            self.bytes.pop_front();
        }
    }

    fn take(&mut self) -> Vec<u8> {
        self.bytes.drain(..).collect()
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TransportError {
    #[error("language process spawn request is invalid")]
    InvalidSpawn,
    #[error("language process stdin is closed")]
    ClosedStdin,
    #[error("language process transport state is unavailable")]
    StateUnavailable,
    #[error("language protocol failed: {0}")]
    Protocol(String),
    #[error("language process I/O failed: {0}")]
    Io(String),
}

impl TransportError {
    #[must_use]
    pub fn protocol(message: impl AsRef<str>) -> Self {
        Self::Protocol(bounded_error_text(message.as_ref()))
    }
}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(bounded_error_text(&error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedByteRing;

    #[test]
    fn stderr_ring_keeps_only_the_newest_bytes() {
        let mut ring = BoundedByteRing::new(4);
        ring.extend(b"abc");
        ring.extend(b"def");
        assert_eq!(ring.take(), b"cdef");
    }
}
