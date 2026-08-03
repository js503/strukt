use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strukt_session::{PaneLifecycle, ServiceInstanceId, SessionCatalog, SessionService};
use strukt_terminal::{
    ExitStatus, OutputChunk, SpawnRequest, TerminalProcess, TerminalSize, TerminalTransport,
    TransportError,
};

#[test]
fn restored_catalog_stays_stopped_until_an_explicit_generation_scoped_start() {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "restored", &directory)
        .expect("create session");
    let pane = first_pane(&catalog, session);
    let transport = Arc::new(FakeTransport::default());
    let instance = ServiceInstanceId::new().expect("service instance");
    let mut service = SessionService::new(
        instance,
        &catalog.stopped_clone().expect("stopped catalog"),
        transport.clone(),
        1_000,
        Duration::from_secs(30),
    )
    .expect("service");

    assert_eq!(transport.spawn_count(), 0);
    assert_eq!(service.running_panes(), 0);

    let job = service
        .begin_start(
            instance,
            service.catalog().revision(),
            session,
            pane,
            spawn_request("ok"),
        )
        .expect("start job");
    assert_eq!(transport.spawn_count(), 0, "spawn remains outside reducer");
    let generation = job.generation();
    let completion = job.run();
    assert_eq!(transport.spawn_count(), 1);
    service.finish_start(completion).expect("finish start");
    assert_eq!(service.running_panes(), 1);
    assert!(matches!(
        service.pane_lifecycle(pane),
        Some(PaneLifecycle::Running)
    ));

    let stale_instance = ServiceInstanceId::new().expect("stale instance");
    assert!(
        service
            .write(stale_instance, pane, generation, b"no")
            .is_err()
    );
    assert!(
        service
            .write(instance, pane, generation - 1, b"no")
            .is_err()
    );
    service
        .write(instance, pane, generation, b"yes")
        .expect("current generation input");
    assert_eq!(transport.writes(), vec![b"yes".to_vec()]);
}

#[test]
fn detach_keeps_processes_alive_and_idle_exit_requires_no_running_panes() {
    let (mut service, transport, instance, session, pane) = running_service();
    service.attach(instance).expect("attach");
    service.detach(instance).expect("detach");

    assert_eq!(service.attached_clients(), 0);
    assert_eq!(service.running_panes(), 1);
    assert!(!service.should_exit());
    assert_eq!(transport.terminate_count(), 0);

    let generation = service.pane_generation(pane).expect("pane generation");
    let job = service
        .begin_terminate(
            instance,
            session,
            pane,
            generation,
            Duration::from_millis(10),
        )
        .expect("terminate job");
    assert_eq!(
        transport.terminate_count(),
        0,
        "termination remains outside reducer"
    );
    service
        .finish_terminate(job.run())
        .expect("finish termination");
    assert_eq!(transport.terminate_count(), 1);
    assert!(service.should_exit());
}

#[test]
fn output_drains_fairly_and_coalesces_snapshot_revisions() {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let noisy_session = catalog
        .create_session(0, "noisy", &directory)
        .expect("noisy session");
    let quiet_session = catalog
        .create_session(catalog.revision(), "quiet", &directory)
        .expect("quiet session");
    let noisy = first_pane(&catalog, noisy_session);
    let quiet = first_pane(&catalog, quiet_session);
    let transport = Arc::new(FakeTransport::default());
    let instance = ServiceInstanceId::new().expect("service instance");
    let mut service = SessionService::new(
        instance,
        &catalog,
        transport.clone(),
        1_000,
        Duration::from_secs(30),
    )
    .expect("service");

    for (session, pane, executable) in [
        (noisy_session, noisy, "noisy"),
        (quiet_session, quiet, "quiet"),
    ] {
        let job = service
            .begin_start(
                instance,
                service.catalog().revision(),
                session,
                pane,
                spawn_request(executable),
            )
            .expect("start job");
        service.finish_start(job.run()).expect("finish start");
    }
    transport.push_output("noisy", OutputChunk::new(0, vec![b'x'; 512 * 1024]));
    transport.push_output("quiet", OutputChunk::new(0, b"quiet-progress".to_vec()));

    let batch = service.tick();

    assert!(batch.changed_panes().contains(&noisy));
    assert!(batch.changed_panes().contains(&quiet));
    assert!(
        service
            .snapshot(quiet)
            .expect("quiet snapshot")
            .rows()
            .iter()
            .flatten()
            .any(|cell| cell.text().contains('q'))
    );
    let first_revision = service
        .snapshot(quiet)
        .expect("quiet snapshot")
        .output_revision();
    assert!(first_revision > 0);
    assert!(!service.tick().changed_panes().contains(&quiet));
    assert_eq!(
        service
            .snapshot(quiet)
            .expect("quiet snapshot")
            .output_revision(),
        first_revision
    );
    let persisted = service
        .take_persistence()
        .expect("persistence projection")
        .expect("dirty service");
    let quiet_history = persisted
        .histories()
        .iter()
        .find(|history| history.pane() == quiet)
        .expect("quiet bounded history");
    assert_eq!(quiet_history.screen().generation(), 0);
    assert_eq!(quiet_history.screen().lifecycle(), &PaneLifecycle::Stopped);
    let restored = SessionService::restore(
        ServiceInstanceId::new().expect("restored instance"),
        &persisted,
        Arc::new(FakeTransport::default()),
        1_000,
        Duration::from_secs(30),
    )
    .expect("restore service");
    let restored_text = restored
        .snapshot(quiet)
        .expect("restored quiet history")
        .rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(strukt_terminal::Cell::text)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(restored_text.contains("quiet-progress"));
    assert_eq!(restored.running_panes(), 0);
}

#[test]
fn batch_restart_results_are_independent() {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "batch", &directory)
        .expect("session");
    let first = first_pane(&catalog, session);
    let second_session = catalog
        .create_session(catalog.revision(), "sibling", &directory)
        .expect("sibling");
    let second = first_pane(&catalog, second_session);
    let transport = Arc::new(FakeTransport::default());
    let instance = ServiceInstanceId::new().expect("service instance");
    let mut service = SessionService::new(
        instance,
        &catalog,
        transport,
        1_000,
        Duration::from_secs(30),
    )
    .expect("service");

    let failed = service
        .begin_start(
            instance,
            service.catalog().revision(),
            session,
            first,
            spawn_request("fail"),
        )
        .expect("failed start job");
    service
        .finish_start(failed.run())
        .expect_err("failure remains pane local");
    let healthy = service
        .begin_start(
            instance,
            service.catalog().revision(),
            second_session,
            second,
            spawn_request("ok"),
        )
        .expect("healthy start job");
    service
        .finish_start(healthy.run())
        .expect("healthy sibling");

    assert!(matches!(
        service.pane_lifecycle(first),
        Some(PaneLifecycle::Failed { .. })
    ));
    assert!(matches!(
        service.pane_lifecycle(second),
        Some(PaneLifecycle::Running)
    ));
}

#[test]
fn catalog_only_management_is_instance_scoped_and_persisted() {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "managed", &directory)
        .expect("session");
    let instance = ServiceInstanceId::new().expect("instance");
    let mut service = SessionService::new(
        instance,
        &catalog,
        Arc::new(FakeTransport::default()),
        1_000,
        Duration::from_secs(30),
    )
    .expect("service");

    let revision = service.catalog().revision();
    service
        .apply_catalog_mutation(instance, |catalog| {
            catalog.rename_session(revision, session, "renamed")
        })
        .expect("rename");
    assert!(
        service
            .apply_catalog_mutation(ServiceInstanceId::new().expect("stale"), |_| Ok(()))
            .is_err()
    );
    let persisted = service
        .take_persistence()
        .expect("projection")
        .expect("dirty mutation");
    assert_eq!(
        persisted
            .catalog()
            .session(session)
            .expect("session")
            .name(),
        "renamed"
    );
}

fn running_service() -> (
    SessionService,
    Arc<FakeTransport>,
    ServiceInstanceId,
    strukt_session::SessionId,
    strukt_session::PaneId,
) {
    let directory = std::env::current_dir().expect("current directory");
    let mut catalog = SessionCatalog::new();
    let session = catalog
        .create_session(0, "running", directory)
        .expect("session");
    let pane = first_pane(&catalog, session);
    let transport = Arc::new(FakeTransport::default());
    let instance = ServiceInstanceId::new().expect("service instance");
    let mut service =
        SessionService::new(instance, &catalog, transport.clone(), 1_000, Duration::ZERO)
            .expect("service");
    let job = service
        .begin_start(
            instance,
            service.catalog().revision(),
            session,
            pane,
            spawn_request("ok"),
        )
        .expect("start job");
    service.finish_start(job.run()).expect("finish start");
    (service, transport, instance, session, pane)
}

fn first_pane(
    catalog: &SessionCatalog,
    session: strukt_session::SessionId,
) -> strukt_session::PaneId {
    catalog
        .session(session)
        .expect("session")
        .active_window()
        .expect("active window")
        .focused_pane()
        .id()
}

fn spawn_request(executable: &str) -> SpawnRequest {
    SpawnRequest {
        executable: PathBuf::from(executable),
        arguments: Vec::new(),
        working_directory: std::env::current_dir().expect("current directory"),
        environment: Vec::new(),
        size: TerminalSize::new(4, 80).expect("terminal size"),
    }
}

#[derive(Default)]
struct FakeTransport {
    state: Arc<Mutex<FakeTransportState>>,
}

#[derive(Default)]
struct FakeTransportState {
    processes: BTreeMap<String, Arc<Mutex<FakeProcessState>>>,
    writes: Vec<Vec<u8>>,
    terminate_count: usize,
}

impl FakeTransport {
    fn spawn_count(&self) -> usize {
        self.state.lock().expect("transport").processes.len()
    }

    fn terminate_count(&self) -> usize {
        self.state.lock().expect("transport").terminate_count
    }

    fn writes(&self) -> Vec<Vec<u8>> {
        self.state.lock().expect("transport").writes.clone()
    }

    fn push_output(&self, executable: &str, output: OutputChunk) {
        self.state.lock().expect("transport").processes[executable]
            .lock()
            .expect("process")
            .output
            .push_back(output);
    }
}

impl TerminalTransport for FakeTransport {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn TerminalProcess>, TransportError> {
        let executable = request.executable.to_string_lossy().into_owned();
        if executable == "fail" {
            return Err(TransportError::Adapter("fixture start failed".to_owned()));
        }
        let process = Arc::new(Mutex::new(FakeProcessState::default()));
        self.state
            .lock()
            .expect("transport")
            .processes
            .insert(executable, process.clone());
        Ok(Box::new(FakeProcess {
            process,
            transport: Arc::clone(&self.state),
        }))
    }
}

#[derive(Default)]
struct FakeProcessState {
    output: VecDeque<OutputChunk>,
    exited: bool,
}

struct FakeProcess {
    process: Arc<Mutex<FakeProcessState>>,
    transport: Arc<Mutex<FakeTransportState>>,
}

impl TerminalProcess for FakeProcess {
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.transport
            .lock()
            .expect("transport")
            .writes
            .push(bytes.to_vec());
        Ok(())
    }

    fn resize(&mut self, _size: TerminalSize) -> Result<(), TransportError> {
        Ok(())
    }

    fn try_read(&mut self) -> Result<Option<OutputChunk>, TransportError> {
        Ok(self.process.lock().expect("process").output.pop_front())
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, TransportError> {
        Ok(self
            .process
            .lock()
            .expect("process")
            .exited
            .then(|| ExitStatus::new(None, None, true)))
    }

    fn wait(&mut self, _timeout: Duration) -> Result<ExitStatus, TransportError> {
        Ok(ExitStatus::new(None, None, true))
    }

    fn terminate(&mut self, _grace: Duration) -> Result<(), TransportError> {
        self.process.lock().expect("process").exited = true;
        self.transport.lock().expect("transport").terminate_count += 1;
        Ok(())
    }
}
