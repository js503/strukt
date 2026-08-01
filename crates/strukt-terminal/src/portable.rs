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
        let child = pair.slave.spawn_command(command).map_err(adapter_error)?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(adapter_error)?;
        let writer = pair.master.take_writer().map_err(adapter_error)?;
        let (sender, receiver) = sync_channel(OUTPUT_QUEUE_CHUNKS);
        let queue = Arc::new(QueueBudget::default());
        let reader_thread = spawn_reader(reader, sender, Arc::clone(&queue))?;

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
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    receiver: Receiver<ReaderEvent>,
    queue: Arc<QueueBudget>,
    reader_thread: Option<JoinHandle<()>>,
    cached_exit: Option<ExitStatus>,
    termination_requested: bool,
}

impl TerminalProcess for PortableProcess {
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.writer.write_all(bytes).map_err(io_error)?;
        self.writer.flush().map_err(io_error)
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
            used = self
                .available
                .wait(used)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
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
) -> Result<JoinHandle<()>, TransportError> {
    thread::Builder::new()
        .name("strukt-terminal-reader".to_owned())
        .spawn(move || {
            let sequence = AtomicU64::new(0);
            let mut buffer = vec![0; OUTPUT_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = sender.send(ReaderEvent::Closed);
                        break;
                    }
                    Ok(count) => {
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
