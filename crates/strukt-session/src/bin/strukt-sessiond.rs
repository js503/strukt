use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use strukt_session::{
    AuthenticatedListener, CatalogError, EndpointIdentity, FixtureMode, LocalStream,
    ProviderCapabilities, ProviderCatalogSnapshot, ProviderError, ProviderKind, RendezvousRecord,
    RendezvousStore, RequestBody, RequestEnvelope, ResponseBody, ResponseEnvelope, ServiceError,
    ServiceInstanceId, ServiceLock, ServiceSecret, SessionCatalog, SessionService, SessionStore,
    decode_cbor, encode_cbor,
};
use strukt_terminal::{PortableTransport, SpawnRequest, TerminalSize, default_shell_request};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const SCROLLBACK_ROWS: usize = 10_000;
const IDLE_TIMEOUT: Duration = Duration::from_mins(30);
const TICK_INTERVAL: Duration = Duration::from_millis(10);
const PERSIST_INTERVAL: Duration = Duration::from_millis(100);
const TERMINATION_GRACE: Duration = Duration::from_millis(500);

fn main() {
    if let Err(error) = run() {
        eprintln!("strukt-sessiond: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse()?;
    let application_data = arguments
        .application_data
        .canonicalize()
        .unwrap_or(arguments.application_data);
    let _service_lock = ServiceLock::acquire(&application_data)?;
    let service_instance = ServiceInstanceId::new()?;
    let secret_reference = "service.secret";
    let secret = Arc::new(ServiceSecret::generate_and_store(
        &application_data.join(secret_reference),
    )?);
    let identity = EndpointIdentity::for_service(&application_data, service_instance)?;
    let listener = AuthenticatedListener::bind(identity.clone(), secret)?;
    let record = RendezvousRecord::new(&identity, service_instance, secret_reference)?;
    let rendezvous = RendezvousStore::at(&application_data);
    rendezvous.publish(&record)?;

    let store = SessionStore::at(application_data.join("catalog"));
    let persisted = store.load()?;
    let transport = Arc::new(PortableTransport::new());
    let service = if let Some(persisted) = &persisted {
        SessionService::restore(
            service_instance,
            persisted,
            transport,
            SCROLLBACK_ROWS,
            IDLE_TIMEOUT,
        )?
    } else {
        SessionService::new(
            service_instance,
            &SessionCatalog::new(),
            transport,
            SCROLLBACK_ROWS,
            IDLE_TIMEOUT,
        )?
    };
    let result = run_service_loop(listener, service, &store, arguments.fixture.as_deref());
    let _ = rendezvous.clear_if_owner(service_instance);
    result
}

fn run_service_loop(
    listener: AuthenticatedListener,
    mut service: SessionService,
    store: &SessionStore,
    fixture: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (connection_sender, connection_receiver) = mpsc::sync_channel(8);
    spawn_accept_loop(listener, connection_sender)?;
    let (event_sender, event_receiver) = mpsc::sync_channel(1024);
    let client_ids = AtomicU64::new(1);
    let mut controlling_client = None;
    let mut shutdown = false;
    let mut last_persist = Instant::now();

    while !shutdown {
        accept_ready_clients(&connection_receiver, &event_sender, &client_ids)?;
        match event_receiver.recv_timeout(TICK_INTERVAL) {
            Ok(ClientEvent::Request {
                client,
                request,
                response,
            }) => {
                let request_id = request.request_id();
                let result = handle_request(
                    &mut service,
                    fixture,
                    client,
                    &mut controlling_client,
                    &request,
                );
                let (response_body, should_shutdown) = match result {
                    Ok(result) => result,
                    Err(error) => {
                        persist_if_dirty(&mut service, store)?;
                        let _ = response
                            .send(ResponseEnvelope::error(request_id, provider_error(&error)));
                        continue;
                    }
                };
                persist_if_dirty(&mut service, store)?;
                let _ = response.send(ResponseEnvelope::ok(request_id, response_body));
                shutdown = should_shutdown;
            }
            Ok(ClientEvent::Disconnected { client }) => {
                if controlling_client == Some(client) {
                    let _ = service.detach(service.service_instance());
                    controlling_client = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let _ = service.tick();
        if last_persist.elapsed() >= PERSIST_INTERVAL {
            persist_if_dirty(&mut service, store)?;
            last_persist = Instant::now();
        }
        if service.should_exit() {
            break;
        }
    }
    persist_if_dirty(&mut service, store)?;
    Ok(())
}

fn spawn_accept_loop(
    listener: AuthenticatedListener,
    sender: SyncSender<LocalStream>,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("strukt-session-accept".to_owned())
        .spawn(move || {
            loop {
                if let Ok(stream) = listener.accept()
                    && sender.send(stream).is_err()
                {
                    break;
                }
            }
        })?;
    Ok(())
}

fn accept_ready_clients(
    receiver: &Receiver<LocalStream>,
    events: &SyncSender<ClientEvent>,
    ids: &AtomicU64,
) -> std::io::Result<()> {
    while let Ok(stream) = receiver.try_recv() {
        let client = ids.fetch_add(1, Ordering::Relaxed);
        spawn_client(stream, client, events.clone())?;
    }
    Ok(())
}

fn spawn_client(
    mut stream: LocalStream,
    client: u64,
    events: SyncSender<ClientEvent>,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name(format!("strukt-session-client-{client}"))
        .spawn(move || {
            while let Ok(request) = read_frame::<RequestEnvelope>(&mut stream) {
                let (response_sender, response_receiver) = mpsc::sync_channel(1);
                if events
                    .send(ClientEvent::Request {
                        client,
                        request,
                        response: response_sender,
                    })
                    .is_err()
                {
                    break;
                }
                let Ok(response) = response_receiver.recv() else {
                    break;
                };
                if write_frame(&mut stream, &response).is_err() {
                    break;
                }
            }
            let _ = events.send(ClientEvent::Disconnected { client });
        })?;
    Ok(())
}

enum ClientEvent {
    Request {
        client: u64,
        request: RequestEnvelope,
        response: SyncSender<ResponseEnvelope>,
    },
    Disconnected {
        client: u64,
    },
}

fn handle_request(
    service: &mut SessionService,
    fixture: Option<&Path>,
    client: u64,
    controlling_client: &mut Option<u64>,
    request: &RequestEnvelope,
) -> Result<(ResponseBody, bool), ServiceError> {
    request
        .validate()
        .map_err(|_| ServiceError::InvalidWireRequest)?;
    let instance = service.service_instance();
    let expected_revision = request.expected_catalog_revision();
    let body = request.body().clone();
    let response = match body {
        RequestBody::Catalog => ResponseBody::Catalog(catalog_snapshot(service)),
        RequestBody::Attach => {
            if controlling_client.is_some_and(|owner| owner != client) {
                return Err(ServiceError::WriterAlreadyAttached);
            }
            if controlling_client.is_none() {
                service.attach(instance)?;
                *controlling_client = Some(client);
            }
            ResponseBody::Attached(catalog_snapshot(service))
        }
        RequestBody::Detach => {
            require_controller(*controlling_client, client)?;
            service.detach(instance)?;
            *controlling_client = None;
            ResponseBody::Detached
        }
        controlled => {
            require_controller(*controlling_client, client)?;
            return handle_controlled_request(service, fixture, expected_revision, controlled);
        }
    };
    Ok((response, false))
}

#[expect(
    clippy::too_many_lines,
    reason = "the protocol reducer remains one exhaustive auditable request match"
)]
fn handle_controlled_request(
    service: &mut SessionService,
    fixture: Option<&Path>,
    expected_revision: u64,
    request: RequestBody,
) -> Result<(ResponseBody, bool), ServiceError> {
    let instance = service.service_instance();
    let response = match request {
        RequestBody::CreateSession {
            name,
            working_directory,
        } => ResponseBody::SessionCreated(service.create_session(
            instance,
            expected_revision,
            name,
            working_directory,
        )?),
        RequestBody::ImportStoppedCatalog { catalog } => {
            service.import_stopped_catalog(instance, expected_revision, &catalog)?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::RenameSession { session, name } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.rename_session(expected_revision, session, name)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::ActivateSession { session } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.activate_session(expected_revision, session)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::DuplicateSession { session } => ResponseBody::SessionDuplicated(
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.duplicate_session(expected_revision, session)
            })?,
        ),
        RequestBody::RemoveSession { session } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.remove_session(expected_revision, session)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::CreateWindow {
            session,
            name,
            working_directory,
        } => ResponseBody::WindowCreated(service.apply_catalog_mutation(instance, |catalog| {
            catalog.create_window(expected_revision, session, name, working_directory)
        })?),
        RequestBody::RenameWindow {
            session,
            window,
            name,
        } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.rename_window(expected_revision, session, window, name)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::ActivateWindow { session, window } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.activate_window(expected_revision, session, window)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::DuplicateWindow { session, window } => {
            ResponseBody::WindowDuplicated(service.apply_catalog_mutation(instance, |catalog| {
                catalog.duplicate_window(expected_revision, session, window)
            })?)
        }
        RequestBody::CloseWindow { session, window } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.close_window(expected_revision, session, window)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::SplitPane { session, axis } => {
            ResponseBody::PaneSplit(service.apply_catalog_mutation(instance, |catalog| {
                catalog.split_focused(expected_revision, session, axis)
            })?)
        }
        RequestBody::FocusPane { session, pane } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.focus_pane(expected_revision, session, pane)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::SetSplitRatio {
            session,
            ratio_basis_points,
        } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.set_focused_split_ratio(expected_revision, session, ratio_basis_points)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::ClosePane { session, pane } => {
            service.apply_catalog_mutation(instance, |catalog| {
                catalog.close_pane(expected_revision, session, pane)
            })?;
            ResponseBody::CatalogChanged(catalog_snapshot(service))
        }
        RequestBody::StartPane {
            session,
            pane,
            rows,
            columns,
        } => start_default_pane(service, expected_revision, session, pane, rows, columns)?,
        RequestBody::StartFixturePane {
            session,
            pane,
            mode,
            rows,
            columns,
        } => start_fixture_pane(
            service,
            fixture,
            expected_revision,
            session,
            pane,
            mode,
            TerminalSize::new(rows, columns)?,
        )?,
        RequestBody::WritePane {
            pane,
            generation,
            bytes,
        } => {
            service.write(instance, pane, generation, &bytes)?;
            ResponseBody::PaneWritten
        }
        RequestBody::ResizePane {
            pane,
            generation,
            rows,
            columns,
        } => {
            service.resize(
                instance,
                pane,
                generation,
                TerminalSize::new(rows, columns)?,
            )?;
            ResponseBody::PaneResized
        }
        RequestBody::Snapshot { pane } => ResponseBody::PaneSnapshot(service.snapshot(pane)?),
        RequestBody::TerminatePane {
            session,
            pane,
            generation,
        } => {
            let job =
                service.begin_terminate(instance, session, pane, generation, TERMINATION_GRACE)?;
            service.finish_terminate(job.run())?;
            ResponseBody::PaneTerminated { pane, generation }
        }
        RequestBody::Shutdown => {
            if service.running_panes() > 0 {
                return Err(ServiceError::RunningPanesPreventShutdown);
            }
            return Ok((ResponseBody::ShuttingDown, true));
        }
        RequestBody::Catalog | RequestBody::Attach | RequestBody::Detach => {
            return Err(ServiceError::InvalidWireRequest);
        }
    };
    Ok((response, false))
}

fn start_default_pane(
    service: &mut SessionService,
    expected_revision: u64,
    session: strukt_session::SessionId,
    pane: strukt_session::PaneId,
    rows: u16,
    columns: u16,
) -> Result<ResponseBody, ServiceError> {
    let spawn = default_shell_request(
        pane_working_directory(service, pane)?,
        TerminalSize::new(rows, columns)?,
    )?;
    finish_pane_start(service, expected_revision, session, pane, spawn)
}

fn start_fixture_pane(
    service: &mut SessionService,
    fixture: Option<&Path>,
    expected_revision: u64,
    session: strukt_session::SessionId,
    pane: strukt_session::PaneId,
    mode: FixtureMode,
    size: TerminalSize,
) -> Result<ResponseBody, ServiceError> {
    let arguments = match mode {
        FixtureMode::Hold => vec![OsString::from("hold")],
    };
    let spawn = SpawnRequest {
        executable: fixture
            .ok_or(ServiceError::FixtureUnavailable)?
            .to_path_buf(),
        arguments,
        working_directory: pane_working_directory(service, pane)?,
        environment: Vec::new(),
        size,
    };
    finish_pane_start(service, expected_revision, session, pane, spawn)
}

fn finish_pane_start(
    service: &mut SessionService,
    expected_revision: u64,
    session: strukt_session::SessionId,
    pane: strukt_session::PaneId,
    spawn: SpawnRequest,
) -> Result<ResponseBody, ServiceError> {
    let job = service.begin_start(
        service.service_instance(),
        expected_revision,
        session,
        pane,
        spawn,
    )?;
    let generation = job.generation();
    service.finish_start(job.run())?;
    Ok(ResponseBody::PaneStarted { pane, generation })
}

fn pane_working_directory(
    service: &SessionService,
    pane: strukt_session::PaneId,
) -> Result<PathBuf, ServiceError> {
    Ok(service
        .catalog()
        .pane(pane)
        .ok_or(CatalogError::PaneNotFound)?
        .2
        .working_directory()
        .to_path_buf())
}

fn require_controller(owner: Option<u64>, client: u64) -> Result<(), ServiceError> {
    if owner == Some(client) {
        Ok(())
    } else {
        Err(ServiceError::NoAttachedClient)
    }
}

fn catalog_snapshot(service: &SessionService) -> ProviderCatalogSnapshot {
    ProviderCatalogSnapshot::new(
        service.service_instance(),
        ProviderKind::NativeLocal,
        ProviderCapabilities::native_local(),
        service.catalog().clone(),
    )
}

fn persist_if_dirty(
    service: &mut SessionService,
    store: &SessionStore,
) -> Result<(), ServiceError> {
    if let Some(record) = service.take_persistence()? {
        store.save(&record)?;
    }
    Ok(())
}

fn provider_error(error: &ServiceError) -> ProviderError {
    match error {
        ServiceError::StaleServiceInstance | ServiceError::StaleGeneration => {
            ProviderError::Unavailable
        }
        ServiceError::Catalog(CatalogError::StaleRevision { .. }) => ProviderError::StaleRevision,
        ServiceError::Catalog(CatalogError::CapacityReached) => ProviderError::CapacityReached,
        ServiceError::Catalog(
            CatalogError::SessionNotFound
            | CatalogError::WindowNotFound
            | CatalogError::PaneNotFound,
        ) => ProviderError::NotFound,
        ServiceError::Runtime(error) => ProviderError::process_failed(error.to_string()),
        ServiceError::TransportRequest(error) => ProviderError::process_failed(error.to_string()),
        ServiceError::NoAttachedClient
        | ServiceError::WriterAlreadyAttached
        | ServiceError::FixtureUnavailable
        | ServiceError::RunningPanesPreventShutdown
        | ServiceError::ImportTargetNotEmpty
        | ServiceError::InvalidWireRequest
        | ServiceError::InvalidStartRequest => ProviderError::InvalidAction,
        other => ProviderError::internal(other.to_string()),
    }
}

fn read_frame<T: serde::de::DeserializeOwned>(stream: &mut impl Read) -> Result<T, std::io::Error> {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "session frame exceeds limit",
        ));
    }
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload)?;
    decode_cbor(&payload).map_err(std::io::Error::other)
}

fn write_frame<T: serde::Serialize>(
    stream: &mut impl Write,
    value: &T,
) -> Result<(), std::io::Error> {
    let frame = encode_cbor(value, MAX_FRAME_BYTES).map_err(std::io::Error::other)?;
    stream.write_all(&frame)?;
    stream.flush()
}

struct Arguments {
    application_data: PathBuf,
    fixture: Option<PathBuf>,
}

impl Arguments {
    fn parse() -> Result<Self, std::io::Error> {
        let mut arguments = std::env::args_os().skip(1);
        let mut application_data = None;
        let mut fixture = None;
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--app-data") => application_data = arguments.next().map(PathBuf::from),
                Some("--fixture") => fixture = arguments.next().map(PathBuf::from),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "usage: strukt-sessiond --app-data PATH [--fixture PATH]",
                    ));
                }
            }
        }
        let application_data = application_data.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "--app-data is required")
        })?;
        if !application_data.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--app-data must be absolute",
            ));
        }
        if fixture.as_ref().is_some_and(|path| !path.is_file()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--fixture must name a file",
            ));
        }
        Ok(Self {
            application_data,
            fixture,
        })
    }
}
