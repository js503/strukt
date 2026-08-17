use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use strukt_remote::{SshAlias, SshExecutable};
use strukt_session::{
    ClientConnectIntent, PaneLifecycle, RequestBody, ResponseBody, SessionClient, SessionId,
    WindowId,
};
use strukt_terminal::SplitAxis;

use crate::remote::RemoteRuntime;

pub fn run(root: &Path) -> Result<(), String> {
    progress("connect first SSH helper");
    let fake_ssh = sibling_binary("fake-ssh")?;
    let alias = SshAlias::new("fixture").map_err(display)?;
    let executable = SshExecutable::from_path(fake_ssh).map_err(display)?;
    let root_label = root.to_string_lossy();
    let connection_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    let runtime = RemoteRuntime::connect(
        executable.clone(),
        &alias,
        &root_label,
        connection_id.clone(),
        21,
    )?;
    progress("attach native session provider");
    let mut client = runtime.native_session_client()?;
    connect(&mut client, ClientConnectIntent::ExplicitAttach)?;

    progress("create remote session hierarchies");
    let first = create_session(&mut client, "api", root)?;
    let second = create_session(&mut client, "worker", root)?;
    let (logs_window, logs_split) = create_remote_layout(&mut client, first, root)?;
    let first_pane = session_first_pane(&client, first)?;
    let second_pane = session_first_pane(&client, second)?;
    start_and_mark(&mut client, first, first_pane, b"echo M5_API_MARKER\r")?;
    start_and_mark(&mut client, second, second_pane, b"echo M5_WORKER_MARKER\r")?;
    progress("observe independent pane output");
    expect_marker(&mut client, first_pane, "M5_API_MARKER")?;
    expect_marker(&mut client, second_pane, "M5_WORKER_MARKER")?;

    let first_instance = client
        .catalog()
        .ok_or_else(|| "native remote catalog is missing".to_owned())?
        .service_instance();
    progress("disconnect first SSH helper");
    runtime.disconnect();
    client.mark_transport_lost("smoke SSH disconnect");
    if client.health() != strukt_session::ClientHealth::Stale {
        return Err("SSH disconnect did not freeze the session projection as stale".into());
    }

    progress("connect replacement SSH helper");
    let reconnected_runtime =
        RemoteRuntime::connect(executable, &alias, &root_label, connection_id, 22)?;
    progress("reattach existing native service");
    let mut reconnected = reconnected_runtime.native_session_client()?;
    connect(&mut reconnected, ClientConnectIntent::Reconnect)?;
    let catalog = reconnected
        .catalog()
        .ok_or_else(|| "reconnected native catalog is missing".to_owned())?;
    if catalog.service_instance() != first_instance {
        return Err("reconnect attached to a different native service instance".into());
    }
    if catalog.catalog().sessions().count() != 2 {
        return Err("reconnect did not restore both remote session hierarchies".into());
    }
    let restored_logs = catalog
        .catalog()
        .session(first)
        .and_then(|session| {
            session
                .windows()
                .iter()
                .find(|window| window.id() == logs_window)
        })
        .ok_or_else(|| "reconnect did not restore the remote logs window".to_owned())?;
    if restored_logs.panes().all(|pane| pane.id() != logs_split)
        || restored_logs.panes().count() != 2
    {
        return Err("reconnect did not restore the remote split-pane layout".into());
    }
    for session in [first, second] {
        let pane = session_first_pane(&reconnected, session)?;
        let running = reconnected
            .catalog()
            .and_then(|snapshot| snapshot.catalog().pane(pane))
            .is_some_and(|(_, _, pane)| matches!(pane.lifecycle(), PaneLifecycle::Running));
        if !running {
            return Err("a remote pane stopped across the SSH helper disconnect".into());
        }
    }
    expect_marker(&mut reconnected, first_pane, "M5_API_MARKER")?;
    expect_marker(&mut reconnected, second_pane, "M5_WORKER_MARKER")?;

    progress("terminate remote sessions");
    for session in [first, second] {
        let _ = request(&mut reconnected, RequestBody::TerminateSession { session })?;
        let _ = request(&mut reconnected, RequestBody::Catalog)?;
    }
    progress("shutdown disposable provider service and disconnect replacement helper");
    match request(&mut reconnected, RequestBody::Shutdown)? {
        ResponseBody::ShuttingDown => {}
        other => return Err(format!("unexpected remote service shutdown: {other:?}")),
    }
    reconnected_runtime.disconnect();
    if root.join(".strukt").exists() {
        return Err("M5 smoke wrote workspace metadata".into());
    }
    progress("complete");
    Ok(())
}

fn progress(stage: &str) {
    eprintln!("strukt M5 smoke stage: {stage}");
}

fn create_remote_layout(
    client: &mut SessionClient,
    session: SessionId,
    root: &Path,
) -> Result<(WindowId, strukt_session::PaneId), String> {
    let window = match request(
        client,
        RequestBody::CreateWindow {
            session,
            name: "logs".to_owned(),
            working_directory: root.to_path_buf(),
        },
    )? {
        ResponseBody::WindowCreated(window) => window,
        other => return Err(format!("unexpected remote window creation: {other:?}")),
    };
    let _ = request(client, RequestBody::Catalog)?;
    let split = match request(
        client,
        RequestBody::SplitPane {
            session,
            axis: SplitAxis::Horizontal,
        },
    )? {
        ResponseBody::PaneSplit(pane) => pane,
        other => return Err(format!("unexpected remote pane split: {other:?}")),
    };
    let _ = request(client, RequestBody::Catalog)?;
    Ok((window, split))
}

fn connect(client: &mut SessionClient, intent: ClientConnectIntent) -> Result<(), String> {
    let completion = client.begin_connect(intent).map_err(display)?.run();
    client.finish_connect(completion).map_err(display)
}

fn create_session(
    client: &mut SessionClient,
    name: &str,
    root: &Path,
) -> Result<SessionId, String> {
    match request(
        client,
        RequestBody::CreateSession {
            name: name.to_owned(),
            working_directory: root.to_path_buf(),
        },
    )? {
        ResponseBody::SessionCreated(session) => {
            let _ = request(client, RequestBody::Catalog)?;
            Ok(session)
        }
        other => Err(format!("unexpected remote session creation: {other:?}")),
    }
}

fn session_first_pane(
    client: &SessionClient,
    session: SessionId,
) -> Result<strukt_session::PaneId, String> {
    client
        .catalog()
        .and_then(|snapshot| snapshot.catalog().session(session))
        .and_then(strukt_session::Session::active_window)
        .map(|window| window.focused_pane().id())
        .ok_or_else(|| "remote session has no active pane".to_owned())
}

fn start_and_mark(
    client: &mut SessionClient,
    session: SessionId,
    pane: strukt_session::PaneId,
    marker: &[u8],
) -> Result<(), String> {
    let generation = match request(
        client,
        RequestBody::StartPane {
            session,
            pane,
            rows: 24,
            columns: 80,
        },
    )? {
        ResponseBody::PaneStarted { generation, .. } => generation,
        other => return Err(format!("unexpected remote pane start: {other:?}")),
    };
    let _ = request(client, RequestBody::Catalog)?;
    match request(
        client,
        RequestBody::WritePane {
            pane,
            generation,
            bytes: marker.to_vec(),
        },
    )? {
        ResponseBody::PaneWritten => Ok(()),
        other => Err(format!("unexpected remote pane write: {other:?}")),
    }
}

fn expect_marker(
    client: &mut SessionClient,
    pane: strukt_session::PaneId,
    marker: &str,
) -> Result<(), String> {
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(25));
        let response = request(client, RequestBody::Snapshot { pane })?;
        let ResponseBody::PaneSnapshot(snapshot) = response else {
            return Err("remote service returned an unexpected pane snapshot".into());
        };
        let text = snapshot
            .rows()
            .iter()
            .flat_map(|row| row.iter().map(strukt_terminal::Cell::text))
            .collect::<String>();
        if text.contains(marker) {
            return Ok(());
        }
    }
    Err(format!("remote pane marker was not observed: {marker}"))
}

fn request(client: &mut SessionClient, body: RequestBody) -> Result<ResponseBody, String> {
    let completion = client.begin_request(body).map_err(display)?.run();
    client.finish_request(completion).map_err(display)
}

fn sibling_binary(name: &str) -> Result<PathBuf, String> {
    let mut path = std::env::current_exe().map_err(display)?;
    path.set_file_name(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });
    path.is_file()
        .then_some(path)
        .ok_or_else(|| format!("required M5 smoke binary is missing: {name}"))
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}
