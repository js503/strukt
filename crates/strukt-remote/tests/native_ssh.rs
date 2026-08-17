use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use strukt_remote::{
    OpenSsh, OpenSshClient, PersistentProvider, RemoteSessionBackend, RequestBody, ResponseBody,
    SshAlias, SshExecutable,
};
use strukt_session::{
    ClientConnectIntent, PaneId, RequestBody as SessionRequest, ResponseBody as SessionResponse,
    SessionClient, SessionId,
};
use tempfile::tempdir;

#[test]
fn fake_ssh_runs_the_real_helper_end_to_end() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("README.md"), "remote through ssh").unwrap();
    let executable =
        SshExecutable::from_path(PathBuf::from(env!("CARGO_BIN_EXE_fake-ssh"))).unwrap();
    let openssh = OpenSsh::new(executable);
    let alias = SshAlias::new("fixture").unwrap();

    let mut client = OpenSshClient::connect(
        &openssh,
        &alias,
        env!("CARGO_PKG_VERSION"),
        &root.path().to_string_lossy(),
        1,
    )
    .unwrap();

    let response = client
        .request(RequestBody::ReadFile {
            path: "README.md".into(),
            offset: 0,
            length: 1_024,
        })
        .unwrap();
    match response {
        ResponseBody::Stream(chunk) => assert_eq!(chunk.bytes, b"remote through ssh"),
        other => panic!("unexpected helper response: {other:?}"),
    }
    client.disconnect();
    assert!(client.diagnostics().is_empty());
}

#[test]
#[ignore = "requires an explicitly configured disposable OpenSSH target"]
fn disposable_real_openssh_runs_the_helper_protocol() {
    let executable = std::env::var_os("STRUKT_REAL_SSH_EXECUTABLE")
        .map(PathBuf::from)
        .expect("set STRUKT_REAL_SSH_EXECUTABLE");
    let alias = std::env::var("STRUKT_REAL_SSH_ALIAS").expect("set STRUKT_REAL_SSH_ALIAS");
    let root = std::env::var("STRUKT_REAL_SSH_ROOT").expect("set STRUKT_REAL_SSH_ROOT");
    let openssh = OpenSsh::new(SshExecutable::from_path(executable).unwrap());
    let alias = SshAlias::new(alias).unwrap();
    let mut client =
        OpenSshClient::connect(&openssh, &alias, env!("CARGO_PKG_VERSION"), &root, 1).unwrap();

    let response = client
        .request(RequestBody::EnumerateFiles {
            include_ignored: false,
        })
        .unwrap();
    assert!(matches!(response, ResponseBody::DirectoryPage { .. }));
    let transport = Arc::new(Mutex::new(client));
    let mut sessions = remote_session_client(Arc::clone(&transport));
    connect_sessions(&mut sessions, ClientConnectIntent::ExplicitAttach);
    let first_instance = sessions.catalog().unwrap().service_instance();
    let session = create_remote_session(&mut sessions, &root);
    let pane = first_pane(&sessions, session);
    start_and_mark(&mut sessions, session, pane);
    assert_remote_marker(&mut sessions, pane);

    transport.lock().unwrap().disconnect();
    sessions.mark_transport_lost("real SSH helper disconnected");

    let reconnected =
        OpenSshClient::connect(&openssh, &alias, env!("CARGO_PKG_VERSION"), &root, 2).unwrap();
    let reconnected = Arc::new(Mutex::new(reconnected));
    let mut sessions = remote_session_client(Arc::clone(&reconnected));
    connect_sessions(&mut sessions, ClientConnectIntent::Reconnect);
    assert_eq!(
        sessions.catalog().unwrap().service_instance(),
        first_instance
    );
    assert_remote_marker(&mut sessions, pane);
    request_session(&mut sessions, SessionRequest::TerminateSession { session });
    request_session(&mut sessions, SessionRequest::Catalog);
    request_session(&mut sessions, SessionRequest::RemoveSession { session });
    assert!(matches!(
        request_session(&mut sessions, SessionRequest::Shutdown),
        SessionResponse::ShuttingDown
    ));
    reconnected.lock().unwrap().disconnect();
}

fn remote_session_client(client: Arc<Mutex<OpenSshClient>>) -> SessionClient {
    let root = std::env::current_dir().unwrap();
    SessionClient::with_backend(
        root.join("m5-real-ssh-session-data"),
        root.join("m5-real-ssh-session-service"),
        Arc::new(RemoteSessionBackend::new(
            client,
            PersistentProvider::Native,
        )),
    )
    .unwrap()
}

fn connect_sessions(client: &mut SessionClient, intent: ClientConnectIntent) {
    let completion = client.begin_connect(intent).unwrap().run();
    client.finish_connect(completion).unwrap();
}

fn request_session(client: &mut SessionClient, body: SessionRequest) -> SessionResponse {
    let completion = client.begin_request(body).unwrap().run();
    client.finish_request(completion).unwrap()
}

fn create_remote_session(client: &mut SessionClient, root: &str) -> SessionId {
    let SessionResponse::SessionCreated(session) = request_session(
        client,
        SessionRequest::CreateSession {
            name: "real-ssh-m5".into(),
            working_directory: PathBuf::from(root),
        },
    ) else {
        panic!("expected remote session creation")
    };
    request_session(client, SessionRequest::Catalog);
    session
}

fn first_pane(client: &SessionClient, session: SessionId) -> PaneId {
    client
        .catalog()
        .unwrap()
        .catalog()
        .session(session)
        .unwrap()
        .active_window()
        .unwrap()
        .focused_pane()
        .id()
}

fn start_and_mark(client: &mut SessionClient, session: SessionId, pane: PaneId) {
    let SessionResponse::PaneStarted { generation, .. } = request_session(
        client,
        SessionRequest::StartPane {
            session,
            pane,
            rows: 24,
            columns: 80,
        },
    ) else {
        panic!("expected remote pane start")
    };
    request_session(client, SessionRequest::Catalog);
    assert!(matches!(
        request_session(
            client,
            SessionRequest::WritePane {
                pane,
                generation,
                bytes: b"echo STRUKT_REAL_SSH_M5\r".to_vec(),
            },
        ),
        SessionResponse::PaneWritten
    ));
}

fn assert_remote_marker(client: &mut SessionClient, pane: PaneId) {
    for _ in 0..80 {
        thread::sleep(Duration::from_millis(25));
        let SessionResponse::PaneSnapshot(snapshot) =
            request_session(client, SessionRequest::Snapshot { pane })
        else {
            panic!("expected remote pane snapshot")
        };
        let text = snapshot
            .rows()
            .iter()
            .flat_map(|row| row.iter().map(strukt_terminal::Cell::text))
            .collect::<String>();
        if text.contains("STRUKT_REAL_SSH_M5") {
            return;
        }
    }
    panic!("real SSH remote session marker was not observed")
}
