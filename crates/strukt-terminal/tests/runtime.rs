use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strukt_terminal::{
    DrainBudget, ExitStatus, GridSize, OutputChunk, PasteDecision, RuntimePaneState, Selection,
    SpawnRequest, TerminalKey, TerminalPaneId, TerminalProcess, TerminalRuntime, TerminalSize,
    TerminalTransport, TransportError,
};

#[test]
fn ready_panes_drain_round_robin_with_per_pane_and_aggregate_budgets() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let noisy = TerminalPaneId::new();
    let quiet = TerminalPaneId::new();
    let noisy_generation = runtime.prepare(noisy, GridSize::new(2, 80).unwrap());
    let quiet_generation = runtime.prepare(quiet, GridSize::new(2, 80).unwrap());
    for sequence in 0..32 {
        runtime
            .apply_output(
                noisy,
                noisy_generation,
                OutputChunk::new(sequence, vec![b'x'; 64 * 1024]),
            )
            .unwrap();
    }
    runtime
        .apply_output(
            quiet,
            quiet_generation,
            OutputChunk::new(0, b"quiet".to_vec()),
        )
        .unwrap();

    let batch = runtime.drain(DrainBudget::new(256 * 1024, 1024 * 1024));

    assert!(batch.bytes_for(noisy) <= 256 * 1024);
    assert_eq!(batch.bytes_for(quiet), 5);
    assert!(batch.changed_panes().contains(&quiet));
}

#[test]
fn sustained_producer_advances_64_mib_without_starving_a_quiet_pane() {
    const TOTAL_BYTES: usize = 64 * 1024 * 1024;
    const CHUNK_BYTES: usize = 64 * 1024;

    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let noisy = TerminalPaneId::new();
    let quiet = TerminalPaneId::new();
    let noisy_generation = runtime.prepare(noisy, GridSize::new(2, 80).unwrap());
    let quiet_generation = runtime.prepare(quiet, GridSize::new(2, 80).unwrap());
    runtime
        .apply_output(
            quiet,
            quiet_generation,
            OutputChunk::new(0, b"quiet-progress".to_vec()),
        )
        .unwrap();

    let mut progressed = 0;
    for sequence in 0..u64::try_from(TOTAL_BYTES / CHUNK_BYTES).unwrap() {
        runtime
            .apply_output(
                noisy,
                noisy_generation,
                OutputChunk::new(sequence, vec![b'\r'; CHUNK_BYTES]),
            )
            .unwrap();
        let batch = runtime.drain(DrainBudget::default());
        progressed += batch.bytes_for(noisy);
    }
    while progressed < TOTAL_BYTES {
        progressed += runtime.drain(DrainBudget::default()).bytes_for(noisy);
    }

    assert_eq!(progressed, TOTAL_BYTES);
    assert!(
        runtime
            .snapshot(quiet)
            .unwrap()
            .plain_text()
            .contains("quiet-progress")
    );
}

#[test]
fn stale_output_cannot_cross_a_restart_generation() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let pane = TerminalPaneId::new();
    let old = runtime.prepare(pane, GridSize::new(2, 20).unwrap());

    let new = runtime.restart(pane, spawn_request()).unwrap();
    assert!(new > old);
    runtime
        .apply_output(pane, old, OutputChunk::new(0, b"stale".to_vec()))
        .unwrap();
    let _ = runtime.drain(DrainBudget::default());

    assert!(
        !runtime
            .snapshot(pane)
            .unwrap()
            .plain_text()
            .contains("stale")
    );
}

#[test]
fn spawn_resize_exit_and_termination_failures_are_pane_local() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport.clone(), 100);
    let failed = TerminalPaneId::new();
    let healthy = TerminalPaneId::new();
    transport.fail_next_spawn();
    assert!(runtime.restart(failed, spawn_request()).is_err());
    runtime.restart(healthy, spawn_request()).unwrap();
    transport.push_output(healthy, OutputChunk::new(0, b"ok".to_vec()));
    let _ = runtime.drain(DrainBudget::default());

    assert!(matches!(
        runtime.state(failed),
        Some(RuntimePaneState::Failed { .. })
    ));
    assert!(
        runtime
            .snapshot(healthy)
            .unwrap()
            .plain_text()
            .contains("ok")
    );

    transport.fail_resize(healthy);
    assert!(
        runtime
            .resize(healthy, TerminalSize::new(40, 120).unwrap())
            .is_err()
    );
    assert!(runtime.last_error(healthy).unwrap().contains("resize"));
    assert!(
        runtime
            .snapshot(healthy)
            .unwrap()
            .plain_text()
            .contains("ok")
    );

    transport.exit(healthy, 7);
    let _ = runtime.drain(DrainBudget::default());
    assert!(matches!(
        runtime.state(healthy),
        Some(RuntimePaneState::Exited { code: Some(7) })
    ));
}

#[test]
fn snapshots_change_revision_only_after_output_is_applied() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let pane = TerminalPaneId::new();
    let generation = runtime.prepare(pane, GridSize::new(2, 20).unwrap());
    let before = runtime.snapshot(pane).unwrap().revision();
    runtime
        .apply_output(pane, generation, OutputChunk::new(0, b"x".to_vec()))
        .unwrap();
    assert_eq!(runtime.snapshot(pane).unwrap().revision(), before);
    let _ = runtime.drain(DrainBudget::default());
    assert!(runtime.snapshot(pane).unwrap().revision() > before);
}

#[test]
fn explicit_termination_reduces_to_an_exited_pane() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let pane = TerminalPaneId::new();
    runtime.restart(pane, spawn_request()).unwrap();

    runtime.terminate(pane, Duration::from_secs(1)).unwrap();
    let _ = runtime.drain(DrainBudget::default());

    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Exited { code: None })
    ));
}

#[test]
fn runtime_exposes_model_interactions_without_leaking_mutable_model_access() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let pane = TerminalPaneId::new();
    let generation = runtime.prepare(pane, GridSize::new(2, 20).unwrap());
    runtime
        .apply_output(pane, generation, OutputChunk::new(0, b"hello".to_vec()))
        .unwrap();
    let _ = runtime.drain(DrainBudget::default());

    assert_eq!(
        runtime
            .copy_text(pane, &Selection::linear((0, 0), (0, 4)))
            .unwrap(),
        "hello"
    );
    assert_eq!(runtime.encode_key(pane, TerminalKey::Enter).unwrap(), b"\r");
    assert!(matches!(
        runtime.prepare_paste(pane, "safe", false).unwrap(),
        PasteDecision::Send(bytes) if bytes == b"safe"
    ));
    assert_eq!(runtime.snapshot_at(pane, 1).unwrap().viewport_offset(), 0);
}

#[test]
fn start_jobs_keep_blocking_spawn_outside_runtime_mutation() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport.clone(), 100);
    let pane = TerminalPaneId::new();

    let job = runtime.begin_restart(pane, spawn_request()).unwrap();
    assert_eq!(transport.process_count(), 0);
    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Starting)
    ));
    let generation = job.generation();
    let result = job.run();
    assert_eq!(transport.process_count(), 1);
    runtime.finish_restart(pane, generation, result).unwrap();
    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Running)
    ));
}

#[test]
fn native_transport_pressure_becomes_a_visible_pane_state() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport.clone(), 100);
    let pane = TerminalPaneId::new();
    runtime.restart(pane, spawn_request()).unwrap();
    transport.set_backpressured(true);
    let _ = runtime.drain(DrainBudget::default());
    std::thread::sleep(Duration::from_millis(260));

    let batch = runtime.drain(DrainBudget::default());
    assert!(batch.changed_panes().contains(&pane));
    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Backpressured)
    ));

    transport.set_backpressured(false);
    let batch = runtime.drain(DrainBudget::default());
    assert!(batch.changed_panes().contains(&pane));
    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Running)
    ));
}

#[test]
fn stale_spawn_completion_cannot_replace_a_newer_start_generation() {
    let transport = Arc::new(FakeTransport::default());
    let mut runtime = TerminalRuntime::new(transport, 100);
    let pane = TerminalPaneId::new();
    let old = runtime.begin_restart(pane, spawn_request()).unwrap();
    let old_generation = old.generation();
    let current = runtime.begin_restart(pane, spawn_request()).unwrap();
    let current_generation = current.generation();

    runtime
        .finish_restart(pane, old_generation, old.run())
        .unwrap();
    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Starting)
    ));
    assert_eq!(runtime.running_processes(), 0);

    runtime
        .finish_restart(pane, current_generation, current.run())
        .unwrap();
    assert!(matches!(
        runtime.state(pane),
        Some(RuntimePaneState::Running)
    ));
    assert_eq!(runtime.running_processes(), 1);
}

fn spawn_request() -> SpawnRequest {
    SpawnRequest {
        executable: PathBuf::from("fixture"),
        arguments: Vec::new(),
        working_directory: std::env::current_dir().unwrap(),
        environment: Vec::new(),
        size: TerminalSize::new(2, 20).unwrap(),
    }
}

#[derive(Default)]
struct FakeTransport {
    inner: Arc<Mutex<FakeTransportState>>,
}

#[derive(Default)]
struct FakeTransportState {
    fail_next_spawn: bool,
    processes: Vec<Arc<Mutex<FakeProcessState>>>,
}

impl FakeTransport {
    fn process_count(&self) -> usize {
        self.inner.lock().unwrap().processes.len()
    }

    fn fail_next_spawn(&self) {
        self.inner.lock().unwrap().fail_next_spawn = true;
    }

    fn process(&self, pane_index: usize) -> Arc<Mutex<FakeProcessState>> {
        self.inner.lock().unwrap().processes[pane_index].clone()
    }

    fn push_output(&self, _pane: TerminalPaneId, chunk: OutputChunk) {
        self.process(0).lock().unwrap().output.push_back(chunk);
    }

    fn fail_resize(&self, _pane: TerminalPaneId) {
        self.process(0).lock().unwrap().fail_resize = true;
    }

    fn set_backpressured(&self, backpressured: bool) {
        self.process(0).lock().unwrap().backpressured = backpressured;
    }

    fn exit(&self, _pane: TerminalPaneId, code: i32) {
        self.process(0).lock().unwrap().exit = Some(ExitStatus::new(Some(code), None, false));
    }
}

impl TerminalTransport for FakeTransport {
    fn spawn(&self, _request: SpawnRequest) -> Result<Box<dyn TerminalProcess>, TransportError> {
        let mut state = self.inner.lock().unwrap();
        if std::mem::take(&mut state.fail_next_spawn) {
            return Err(TransportError::Adapter("fixture spawn failure".into()));
        }
        let process = Arc::new(Mutex::new(FakeProcessState::default()));
        state.processes.push(process.clone());
        Ok(Box::new(FakeProcess { state: process }))
    }
}

#[derive(Default)]
struct FakeProcessState {
    output: VecDeque<OutputChunk>,
    exit: Option<ExitStatus>,
    fail_resize: bool,
    backpressured: bool,
}

struct FakeProcess {
    state: Arc<Mutex<FakeProcessState>>,
}

impl TerminalProcess for FakeProcess {
    fn write(&mut self, _bytes: &[u8]) -> Result<(), TransportError> {
        Ok(())
    }

    fn resize(&mut self, _size: TerminalSize) -> Result<(), TransportError> {
        if self.state.lock().unwrap().fail_resize {
            Err(TransportError::Adapter("fixture resize failure".into()))
        } else {
            Ok(())
        }
    }

    fn try_read(&mut self) -> Result<Option<OutputChunk>, TransportError> {
        Ok(self.state.lock().unwrap().output.pop_front())
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, TransportError> {
        Ok(self.state.lock().unwrap().exit.clone())
    }

    fn output_backpressured(&self) -> bool {
        self.state.lock().unwrap().backpressured
    }

    fn wait(&mut self, _timeout: Duration) -> Result<ExitStatus, TransportError> {
        self.try_wait()?.ok_or(TransportError::WaitTimeout)
    }

    fn terminate(&mut self, _grace: Duration) -> Result<(), TransportError> {
        self.state.lock().unwrap().exit = Some(ExitStatus::new(None, None, true));
        Ok(())
    }
}
