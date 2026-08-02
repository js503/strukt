use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::{
    ExitStatus, OutputChunk, SpawnRequest, TerminalProcess, TerminalSize, TerminalTransport,
    TransportError,
};

const OUTPUT_QUEUE_CHUNKS: usize = 1024;
const OUTPUT_QUEUE_BYTES: usize = 4 * 1024 * 1024;
const OUTPUT_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct PortableTransport;

impl PortableTransport {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TerminalTransport for PortableTransport {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn TerminalProcess>, TransportError> {
        validate_request(&request)?;
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(to_pty_size(request.size))
            .map_err(adapter_error)?;
        let mut command = CommandBuilder::new(&request.executable);
        command.args(&request.arguments);
        command.cwd(&request.working_directory);
        for (key, value) in &request.environment {
            command.env(key, value);
        }
        let reader = pair.master.try_clone_reader().map_err(adapter_error)?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(adapter_error)?,
        ));
        let (sender, receiver) = sync_channel(OUTPUT_QUEUE_CHUNKS);
        let queue = Arc::new(QueueBudget::default());
        let cursor_bootstrap = Arc::new(AtomicBool::new(cfg!(windows)));
        let reader_thread = spawn_reader(
            reader,
            sender,
            Arc::clone(&queue),
            Arc::clone(&writer),
            Arc::clone(&cursor_bootstrap),
        )?;
        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                cursor_bootstrap.store(false, Ordering::Release);
                queue.close();
                return Err(adapter_error(error));
            }
        };
        drop(pair.slave);

        Ok(Box::new(PortableProcess {
            master: pair.master,
            writer,
            child,
            receiver,
            queue,
            reader_thread: Some(reader_thread),
            cached_exit: None,
            termination_requested: false,
        }))
    }
}

struct PortableProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    receiver: Receiver<ReaderEvent>,
    queue: Arc<QueueBudget>,
    reader_thread: Option<JoinHandle<()>>,
    cached_exit: Option<ExitStatus>,
    termination_requested: bool,
}

impl TerminalProcess for PortableProcess {
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|error| io_error(error.to_string()))?;
        writer.write_all(bytes).map_err(io_error)?;
        writer.flush().map_err(io_error)
    }

    fn resize(&mut self, size: TerminalSize) -> Result<(), TransportError> {
        self.master.resize(to_pty_size(size)).map_err(adapter_error)
    }

    fn try_read(&mut self) -> Result<Option<OutputChunk>, TransportError> {
        match self.receiver.try_recv() {
            Ok(ReaderEvent::Output(chunk)) => {
                self.queue.release(chunk.bytes().len());
                Ok(Some(chunk))
            }
            Ok(ReaderEvent::Failure(message)) => Err(TransportError::Io(message)),
            Ok(ReaderEvent::Closed) | Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                Ok(None)
            }
        }
    }

    fn output_backpressured(&self) -> bool {
        self.queue.blocked.load(Ordering::Acquire)
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, TransportError> {
        if let Some(status) = &self.cached_exit {
            return Ok(Some(status.clone()));
        }
        let status = self.child.try_wait().map_err(io_error)?.map(|status| {
            ExitStatus::new(
                i32::try_from(status.exit_code()).ok(),
                status.signal().map(str::to_owned),
                self.termination_requested,
            )
        });
        if let Some(status) = &status {
            self.cached_exit = Some(status.clone());
            self.finish_reader_if_ready();
        }
        Ok(status)
    }

    fn wait(&mut self, timeout: Duration) -> Result<ExitStatus, TransportError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(TransportError::WaitTimeout);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn terminate(&mut self, grace: Duration) -> Result<(), TransportError> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        self.termination_requested = true;
        self.child.kill().map_err(io_error)?;
        let deadline = Instant::now() + grace;
        loop {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(TransportError::TerminationTimeout);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl PortableProcess {
    fn finish_reader_if_ready(&mut self) {
        if self
            .reader_thread
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(reader_thread) = self.reader_thread.take()
        {
            let _ = reader_thread.join();
        }
    }
}

impl Drop for PortableProcess {
    fn drop(&mut self) {
        self.queue.close();
        if self.cached_exit.is_none() {
            self.termination_requested = true;
            let _ = self.child.kill();
        }
        self.finish_reader_if_ready();
    }
}

enum ReaderEvent {
    Output(OutputChunk),
    Failure(String),
    Closed,
}

#[derive(Debug, Default)]
struct QueueBudget {
    bytes: Mutex<usize>,
    available: Condvar,
    closed: AtomicBool,
    blocked: AtomicBool,
}

impl QueueBudget {
    fn reserve(&self, bytes: usize) -> bool {
        let mut used = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while used.saturating_add(bytes) > OUTPUT_QUEUE_BYTES
            && !self.closed.load(Ordering::Acquire)
        {
            self.blocked.store(true, Ordering::Release);
            used = self
                .available
                .wait(used)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        self.blocked.store(false, Ordering::Release);
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        *used += bytes;
        true
    }

    fn release(&self, bytes: usize) {
        let mut used = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *used = used.saturating_sub(bytes);
        self.available.notify_one();
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.available.notify_all();
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    sender: SyncSender<ReaderEvent>,
    queue: Arc<QueueBudget>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    cursor_bootstrap: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, TransportError> {
    thread::Builder::new()
        .name("strukt-terminal-reader".to_owned())
        .spawn(move || {
            let sequence = AtomicU64::new(0);
            let mut buffer = vec![0; OUTPUT_CHUNK_BYTES];
            let mut cursor_probe = CursorQueryProbe::default();
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = sender.send(ReaderEvent::Closed);
                        break;
                    }
                    Ok(count) => {
                        if cursor_bootstrap.load(Ordering::Acquire)
                            && cursor_probe.observe(&buffer[..count])
                        {
                            let response = writer
                                .lock()
                                .map_err(|error| error.to_string())
                                .and_then(|mut writer| {
                                    writer
                                        .write_all(b"\x1b[1;1R")
                                        .and_then(|()| writer.flush())
                                        .map_err(|error| error.to_string())
                                });
                            cursor_bootstrap.store(false, Ordering::Release);
                            if let Err(message) = response {
                                let _ = sender.send(ReaderEvent::Failure(message));
                                break;
                            }
                        }
                        if !queue.reserve(count) {
                            break;
                        }
                        let chunk = OutputChunk::new(
                            sequence.fetch_add(1, Ordering::Relaxed),
                            buffer[..count].to_vec(),
                        );
                        if sender.send(ReaderEvent::Output(chunk)).is_err() {
                            queue.release(count);
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(ReaderEvent::Failure(error.to_string()));
                        break;
                    }
                }
            }
        })
        .map_err(io_error)
}

#[derive(Debug, Default)]
struct CursorQueryProbe {
    tail: Vec<u8>,
    answered: bool,
}

impl CursorQueryProbe {
    fn observe(&mut self, bytes: &[u8]) -> bool {
        if self.answered {
            return false;
        }
        for byte in bytes {
            self.tail.push(*byte);
            if self.tail.len() > 5 {
                self.tail.remove(0);
            }
            if self.tail.ends_with(b"\x1b[6n") || self.tail.ends_with(b"\x1b[?6n") {
                self.answered = true;
                return true;
            }
        }
        false
    }
}

fn validate_request(request: &SpawnRequest) -> Result<(), TransportError> {
    if request.executable.as_os_str().is_empty() {
        return Err(TransportError::InvalidExecutable);
    }
    if !request.working_directory.is_absolute() || !request.working_directory.is_dir() {
        return Err(TransportError::InvalidWorkingDirectory);
    }
    if request
        .environment
        .iter()
        .any(|(key, _)| key.is_empty() || key.to_string_lossy().contains('='))
    {
        return Err(TransportError::InvalidEnvironmentKey);
    }
    Ok(())
}

const fn to_pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows(),
        cols: size.columns(),
        pixel_width: size.pixel_width(),
        pixel_height: size.pixel_height(),
    }
}

fn adapter_error(error: impl std::fmt::Display) -> TransportError {
    TransportError::Adapter(error.to_string())
}

fn io_error(error: impl std::fmt::Display) -> TransportError {
    TransportError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::CursorQueryProbe;

    #[test]
    fn cursor_query_probe_detects_conpty_handshakes_across_chunks_once() {
        let mut probe = CursorQueryProbe::default();

        assert!(!probe.observe(b"prefix\x1b["));
        assert!(probe.observe(b"6n"));
        assert!(!probe.observe(b"\x1b[6n"));
    }

    #[test]
    fn cursor_query_probe_accepts_private_cursor_queries() {
        let mut probe = CursorQueryProbe::default();

        assert!(probe.observe(b"\x1b[?6n"));
    }
}
