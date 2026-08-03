use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use strukt_session::{
    EndpointIdentity, FixtureMode, LocalEndpoint, PaneLifecycle, RendezvousStore, RequestBody,
    RequestEnvelope, ResponseBody, ResponseEnvelope, ServiceSecret, SessionId, SessionWindow,
    decode_cbor, encode_cbor,
};

const FRAME_LIMIT: usize = 1024 * 1024;

#[test]
fn daemon_preserves_detached_sessions_and_restores_only_stopped_definitions_after_crash() {
    let data = tempfile::tempdir().expect("temporary application data");
    let fixture = Path::new(env!("CARGO_BIN_EXE_session-fixture"));
    let mut daemon = spawn_daemon(data.path(), fixture);
    let first_instance = wait_for_rendezvous(data.path(), None);

    let mut client = TestClient::connect(data.path());
    client.attach();
    let first = client.create_session("first", data.path());
    let second = client.create_session("second", data.path());
    let first_pane = client.first_pane(first);
    let second_pane = client.first_pane(second);
    let first_generation = client.start_fixture(first, first_pane, FixtureMode::Hold);
    let second_generation = client.start_fixture(second, second_pane, FixtureMode::Hold);

    client.detach();
    drop(client);
    thread::sleep(Duration::from_millis(150));

    let mut client = TestClient::connect(data.path());
    client.attach();
    client.write(first_pane, first_generation, b"alpha\n");
    assert!(
        client
            .wait_for_text(first_pane, "fixture:alpha")
            .contains("fixture:alpha")
    );
    client.write(second_pane, second_generation, b"beta\n");
    assert!(
        client
            .wait_for_text(second_pane, "fixture:beta")
            .contains("fixture:beta")
    );

    client.terminate(first, first_pane, first_generation);
    client.write(second_pane, second_generation, b"still-alive\n");
    assert!(
        client
            .wait_for_text(second_pane, "fixture:still-alive")
            .contains("fixture:still-alive")
    );
    drop(client);

    daemon.kill().expect("kill first daemon");
    daemon.wait().expect("wait for first daemon");
    thread::sleep(Duration::from_millis(100));

    let mut restarted = spawn_daemon(data.path(), fixture);
    wait_for_rendezvous(data.path(), Some(first_instance));
    let mut client = TestClient::connect(data.path());
    let catalog = client.attach();
    assert_eq!(catalog.sessions().count(), 2);
    assert!(catalog.sessions().all(|session| {
        session
            .windows()
            .iter()
            .flat_map(SessionWindow::panes)
            .all(|pane| pane.lifecycle() == &PaneLifecycle::Stopped && pane.generation() == 0)
    }));
    assert!(
        client
            .wait_for_text(second_pane, "fixture:still-alive")
            .contains("fixture:still-alive")
    );
    client.shutdown();
    restarted.wait().expect("wait for restarted daemon");

    assert!(!data.path().join(".strukt").exists());
}

fn spawn_daemon(application_data: &Path, fixture: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_strukt-sessiond"))
        .arg("--app-data")
        .arg(application_data)
        .arg("--fixture")
        .arg(fixture)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn session daemon")
}

fn wait_for_rendezvous(
    application_data: &Path,
    previous: Option<strukt_session::ServiceInstanceId>,
) -> strukt_session::ServiceInstanceId {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(record) = RendezvousStore::at(application_data).load().ok().flatten()
            && previous != Some(record.service_instance())
        {
            return record.service_instance();
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("new session daemon rendezvous did not appear");
}

struct TestClient {
    stream: strukt_session::LocalStream,
    next_request: u64,
    catalog: strukt_session::SessionCatalog,
}

impl TestClient {
    fn connect(application_data: &Path) -> Self {
        let record = RendezvousStore::at(application_data)
            .load()
            .expect("load rendezvous")
            .expect("rendezvous record");
        let identity = EndpointIdentity::from_record(
            application_data,
            record.service_instance(),
            record.endpoint_identity(),
        )
        .expect("endpoint identity");
        let secret = ServiceSecret::load_from(application_data.join(record.secret_reference()))
            .expect("load service secret");
        let stream =
            LocalEndpoint::connect_authenticated(&identity, record.service_instance(), &secret)
                .expect("connect session daemon");
        Self {
            stream,
            next_request: 1,
            catalog: strukt_session::SessionCatalog::new(),
        }
    }

    fn attach(&mut self) -> strukt_session::SessionCatalog {
        let response = self.request(RequestBody::Attach);
        let ResponseBody::Attached(snapshot) = response else {
            panic!("unexpected attach response");
        };
        self.catalog = snapshot.catalog().clone();
        self.catalog.clone()
    }

    fn detach(&mut self) {
        assert!(matches!(
            self.request(RequestBody::Detach),
            ResponseBody::Detached
        ));
    }

    fn create_session(&mut self, name: &str, directory: &Path) -> SessionId {
        let response = self.request(RequestBody::CreateSession {
            name: name.to_owned(),
            working_directory: directory.to_path_buf(),
        });
        let ResponseBody::SessionCreated(session) = response else {
            panic!("unexpected create response");
        };
        self.refresh_catalog();
        session
    }

    fn first_pane(&self, session: SessionId) -> strukt_session::PaneId {
        self.catalog
            .session(session)
            .expect("session")
            .active_window()
            .expect("active window")
            .focused_pane()
            .id()
    }

    fn start_fixture(
        &mut self,
        session: SessionId,
        pane: strukt_session::PaneId,
        mode: FixtureMode,
    ) -> u64 {
        let response = self.request(RequestBody::StartFixturePane {
            session,
            pane,
            mode,
            rows: 8,
            columns: 100,
        });
        let ResponseBody::PaneStarted { generation, .. } = response else {
            panic!("unexpected start response");
        };
        self.refresh_catalog();
        generation
    }

    fn write(&mut self, pane: strukt_session::PaneId, generation: u64, bytes: &[u8]) {
        assert!(matches!(
            self.request(RequestBody::WritePane {
                pane,
                generation,
                bytes: bytes.to_vec(),
            }),
            ResponseBody::PaneWritten
        ));
    }

    fn terminate(&mut self, session: SessionId, pane: strukt_session::PaneId, generation: u64) {
        assert!(matches!(
            self.request(RequestBody::TerminatePane {
                session,
                pane,
                generation,
            }),
            ResponseBody::PaneTerminated { .. }
        ));
    }

    fn wait_for_text(&mut self, pane: strukt_session::PaneId, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let response = self.request(RequestBody::Snapshot { pane });
            let ResponseBody::PaneSnapshot(snapshot) = response else {
                panic!("unexpected snapshot response");
            };
            let text = snapshot
                .rows()
                .iter()
                .map(|row| {
                    row.iter()
                        .map(strukt_terminal::Cell::text)
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.contains(expected) {
                return text;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("pane output did not contain {expected}");
    }

    fn refresh_catalog(&mut self) {
        let response = self.request(RequestBody::Catalog);
        let ResponseBody::Catalog(snapshot) = response else {
            panic!("unexpected catalog response");
        };
        self.catalog = snapshot.catalog().clone();
    }

    fn shutdown(&mut self) {
        assert!(matches!(
            self.request(RequestBody::Shutdown),
            ResponseBody::ShuttingDown
        ));
    }

    fn request(&mut self, body: RequestBody) -> ResponseBody {
        let request = RequestEnvelope::new(self.next_request, self.catalog.revision(), body);
        self.next_request += 1;
        let frame = encode_cbor(&request, FRAME_LIMIT).expect("encode request");
        self.stream.write_all(&frame).expect("write request");
        self.stream.flush().expect("flush request");
        let response: ResponseEnvelope = read_frame(&mut self.stream);
        response
            .result()
            .clone()
            .unwrap_or_else(|error| panic!("daemon request failed: {error}"))
    }
}

fn read_frame(stream: &mut impl Read) -> ResponseEnvelope {
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .expect("read response header");
    let length = u32::from_be_bytes(header) as usize;
    assert!(length <= FRAME_LIMIT);
    let mut payload = vec![0_u8; length];
    stream
        .read_exact(&mut payload)
        .expect("read response payload");
    decode_cbor(&payload).expect("decode response")
}
