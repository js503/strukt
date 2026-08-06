use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use strukt_session::{
    ClientConnectIntent, PaneId, PaneLifecycle, RendezvousStore, RequestBody, ResponseBody,
    ServiceInstanceId, SessionClient, SessionId, SessionWindow,
};

const TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn run(workspace_root: &Path) -> Result<(), String> {
    if !workspace_root.is_dir() {
        return Err("session smoke root must be an existing directory".into());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let binary_root = executable
        .parent()
        .ok_or_else(|| "application binary directory is unavailable".to_owned())?;
    let helper = binary_root.join(binary_name("strukt-sessiond"));
    let fixture = binary_root.join(binary_name("session-fixture"));
    if !helper.is_file() || !fixture.is_file() {
        return Err("session smoke helper binaries must be built beside strukt-app".into());
    }
    let application_data = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut daemon = Daemon::spawn(&helper, application_data.path(), &fixture)?;
    let first_instance = wait_for_rendezvous(application_data.path(), None)?;

    let mut client =
        SessionClient::new(application_data.path(), &helper).map_err(|error| error.to_string())?;
    connect(&mut client, ClientConnectIntent::ExplicitAttach)?;
    let first = create_session(&mut client, "first", workspace_root)?;
    let second = create_session(&mut client, "second", workspace_root)?;
    let first_pane = initial_pane(&client, first)?;
    let second_pane = initial_pane(&client, second)?;
    let first_generation = start_fixture(&mut client, first, first_pane)?;
    let second_generation = start_fixture(&mut client, second, second_pane)?;
    expect_response(&mut client, RequestBody::Detach, |body| {
        matches!(body, ResponseBody::Detached)
    })?;
    drop(client);

    let mut client =
        SessionClient::new(application_data.path(), &helper).map_err(|error| error.to_string())?;
    connect(&mut client, ClientConnectIntent::Reconnect)?;
    write_and_wait(&mut client, first_pane, first_generation, "alpha")?;
    write_and_wait(&mut client, second_pane, second_generation, "still-alive")?;
    expect_response(
        &mut client,
        RequestBody::TerminateSession { session: first },
        |body| {
            matches!(
                body,
                ResponseBody::SessionTerminated {
                    session,
                    terminated: 1,
                    failed: 0,
                } if *session == first
            )
        },
    )?;
    write_and_wait(
        &mut client,
        second_pane,
        second_generation,
        "after-termination",
    )?;
    drop(client);

    daemon.kill_and_wait()?;
    let mut restarted = Daemon::spawn(&helper, application_data.path(), &fixture)?;
    wait_for_rendezvous(application_data.path(), Some(first_instance))?;
    let mut client =
        SessionClient::new(application_data.path(), &helper).map_err(|error| error.to_string())?;
    connect(&mut client, ClientConnectIntent::Reconnect)?;
    let catalog = client
        .catalog()
        .ok_or_else(|| "reattached catalog is missing".to_owned())?
        .catalog();
    if catalog.sessions().count() != 2
        || !catalog.sessions().all(|session| {
            session
                .windows()
                .iter()
                .flat_map(SessionWindow::panes)
                .all(|pane| pane.lifecycle() == &PaneLifecycle::Stopped && pane.generation() == 0)
        })
    {
        return Err("restarted service did not restore two stopped sessions".into());
    }
    wait_for_text(&mut client, second_pane, "fixture:after-termination")?;
    expect_response(&mut client, RequestBody::Shutdown, |body| {
        matches!(body, ResponseBody::ShuttingDown)
    })?;
    restarted.wait()?;
    if workspace_root.join(".strukt").exists() {
        return Err("session smoke created workspace metadata".into());
    }
    Ok(())
}

fn connect(client: &mut SessionClient, intent: ClientConnectIntent) -> Result<(), String> {
    let completion = client
        .begin_connect(intent)
        .map_err(|error| error.to_string())?
        .run();
    client
        .finish_connect(completion)
        .map_err(|error| error.to_string())
}

fn request(client: &mut SessionClient, body: RequestBody) -> Result<ResponseBody, String> {
    let completion = client
        .begin_request(body)
        .map_err(|error| error.to_string())?
        .run();
    client
        .finish_request(completion)
        .map_err(|error| error.to_string())
}

fn expect_response(
    client: &mut SessionClient,
    body: RequestBody,
    matches: impl FnOnce(&ResponseBody) -> bool,
) -> Result<ResponseBody, String> {
    let response = request(client, body)?;
    if matches(&response) {
        Ok(response)
    } else {
        Err("session smoke received an unexpected response".into())
    }
}

fn create_session(
    client: &mut SessionClient,
    name: &str,
    root: &Path,
) -> Result<SessionId, String> {
    let response = request(
        client,
        RequestBody::CreateSession {
            name: name.to_owned(),
            working_directory: root.to_path_buf(),
        },
    )?;
    let ResponseBody::SessionCreated(session) = response else {
        return Err("session smoke create response is invalid".into());
    };
    refresh(client)?;
    Ok(session)
}

fn refresh(client: &mut SessionClient) -> Result<(), String> {
    expect_response(client, RequestBody::Catalog, |body| {
        matches!(body, ResponseBody::Catalog(_))
    })?;
    Ok(())
}

fn initial_pane(client: &SessionClient, session: SessionId) -> Result<PaneId, String> {
    client
        .catalog()
        .and_then(|snapshot| snapshot.catalog().session(session))
        .and_then(strukt_session::Session::active_window)
        .map(|window| window.focused_pane().id())
        .ok_or_else(|| "session smoke pane is missing".to_owned())
}

fn start_fixture(
    client: &mut SessionClient,
    session: SessionId,
    pane: PaneId,
) -> Result<u64, String> {
    let response = request(
        client,
        RequestBody::StartFixturePane {
            session,
            pane,
            mode: strukt_session::FixtureMode::Hold,
            rows: 8,
            columns: 100,
        },
    )?;
    let ResponseBody::PaneStarted { generation, .. } = response else {
        return Err("session smoke start response is invalid".into());
    };
    refresh(client)?;
    Ok(generation)
}

fn write_and_wait(
    client: &mut SessionClient,
    pane: PaneId,
    generation: u64,
    line: &str,
) -> Result<(), String> {
    expect_response(
        client,
        RequestBody::WritePane {
            pane,
            generation,
            bytes: format!("{line}\n").into_bytes(),
        },
        |body| matches!(body, ResponseBody::PaneWritten),
    )?;
    wait_for_text(client, pane, &format!("fixture:{line}"))
}

fn wait_for_text(client: &mut SessionClient, pane: PaneId, expected: &str) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        let response = request(client, RequestBody::Snapshot { pane })?;
        let ResponseBody::PaneSnapshot(snapshot) = response else {
            return Err("session smoke snapshot response is invalid".into());
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
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(format!("session smoke output did not contain {expected}"))
}

fn wait_for_rendezvous(
    application_data: &Path,
    previous: Option<ServiceInstanceId>,
) -> Result<ServiceInstanceId, String> {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Some(record) = RendezvousStore::at(application_data)
            .load()
            .map_err(|error| error.to_string())?
            && previous != Some(record.service_instance())
        {
            return Ok(record.service_instance());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("session smoke daemon rendezvous timed out".into())
}

fn binary_name(stem: &str) -> PathBuf {
    #[cfg(windows)]
    return PathBuf::from(format!("{stem}.exe"));
    #[cfg(not(windows))]
    return PathBuf::from(stem);
}

struct Daemon {
    child: Option<Child>,
}

impl Daemon {
    fn spawn(helper: &Path, application_data: &Path, fixture: &Path) -> Result<Self, String> {
        let child = Command::new(helper)
            .arg("--app-data")
            .arg(application_data)
            .arg("--fixture")
            .arg(fixture)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(Self { child: Some(child) })
    }

    fn kill_and_wait(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        child.kill().map_err(|error| error.to_string())?;
        child.wait().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn wait(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
        Err("session smoke daemon shutdown timed out".into())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
