use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use strukt_remote::{
    OpenSsh, OpenSshClient, RemoteErrorKind, RequestBody, ResponseBody, SshAlias, SshExecutable,
};

use crate::remote::RemoteRuntime;

pub fn run(root: &Path) -> Result<(), String> {
    prepare_fixture(root)?;
    let fake_ssh = sibling_binary("fake-ssh")?;
    let openssh = OpenSsh::new(SshExecutable::from_path(fake_ssh.clone()).map_err(display)?);
    let alias = SshAlias::new("fixture").map_err(display)?;
    let root_label = root.to_string_lossy();
    let mut client =
        OpenSshClient::connect(&openssh, &alias, env!("CARGO_PKG_VERSION"), &root_label, 1)
            .map_err(display)?;

    expect_directory(client.request(RequestBody::ListDirectory {
        path: String::new(),
        cursor: None,
        limit: 1_000,
    }))?;
    let revision = expect_revision(client.request(RequestBody::Stat {
        path: "README.md".into(),
    }))?;
    expect_bytes(
        client.request(RequestBody::ReadFile {
            path: "README.md".into(),
            offset: 0,
            length: 1_024,
        }),
        b"remote smoke\n",
    )?;
    let updated = expect_revision(client.request(RequestBody::WriteFile {
        path: "README.md".into(),
        expected_revision: revision.clone(),
        bytes: b"remote smoke updated\n".to_vec(),
    }))?;
    if updated == revision {
        return Err("remote save did not advance its revision".into());
    }
    expect_conflict(client.request(RequestBody::WriteFile {
        path: "README.md".into(),
        expected_revision: revision,
        bytes: b"stale write".to_vec(),
    }))?;
    expect_nonempty_directory(client.request(RequestBody::Search {
        query: "updated".into(),
        include_ignored: false,
        limit: 100,
    }))?;
    expect_git(client.request(RequestBody::GitSummary))?;
    smoke_process(&mut client, root)?;
    smoke_language(&mut client, root)?;
    client.disconnect();

    let mut reconnected =
        OpenSshClient::connect(&openssh, &alias, env!("CARGO_PKG_VERSION"), &root_label, 2)
            .map_err(display)?;
    expect_bytes(
        reconnected.request(RequestBody::ReadFile {
            path: "README.md".into(),
            offset: 0,
            length: 1_024,
        }),
        b"remote smoke updated\n",
    )?;
    reconnected.disconnect();
    smoke_app_coordinator(&fake_ssh, root)?;
    if root.join(".strukt").exists() {
        return Err("remote smoke wrote workspace metadata".into());
    }
    Ok(())
}

fn smoke_app_coordinator(fake_ssh: &Path, root: &Path) -> Result<(), String> {
    let runtime = RemoteRuntime::connect(
        SshExecutable::from_path(fake_ssh.to_path_buf()).map_err(display)?,
        &SshAlias::new("fixture").map_err(display)?,
        &root.to_string_lossy(),
        3,
    )?;
    let files = runtime.list_root(3);
    if !files
        .result
        .as_ref()
        .is_ok_and(|paths| paths.iter().any(|path| path == "src/main.rs"))
    {
        return Err("app remote Quick Open did not discover nested files".into());
    }
    let search = runtime.search(3, "updated".into());
    if search.result.as_ref().is_err() || search.result.as_ref().is_ok_and(Vec::is_empty) {
        return Err("app remote search did not publish results".into());
    }
    if runtime.git_summary(3).result.is_err() {
        return Err("app remote Git summary failed".into());
    }
    let fixture = sibling_binary("remote-process-fixture")?;
    let task = runtime.run_task(
        3,
        fixture.to_string_lossy().into_owned(),
        vec!["--oneshot".into()],
    );
    if !task
        .result
        .as_deref()
        .is_ok_and(|output| output.contains("fixture:oneshot"))
    {
        return Err("app remote task did not publish bounded output".into());
    }
    runtime.disconnect();
    Ok(())
}

fn prepare_fixture(root: &Path) -> Result<(), String> {
    fs::write(root.join("README.md"), "remote smoke\n").map_err(display)?;
    fs::create_dir_all(root.join("src")).map_err(display)?;
    fs::write(root.join("src/main.rs"), "fn main() {}\n").map_err(display)?;
    run_git(root, &["init", "-q"])?;
    run_git(root, &["config", "user.email", "smoke@strukt.dev"])?;
    run_git(root, &["config", "user.name", "strukt smoke"])?;
    run_git(root, &["add", "README.md", "src/main.rs"])?;
    run_git(root, &["commit", "-qm", "fixture"])
}

fn run_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .map_err(display)?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("git fixture command failed: {}", args.join(" ")))
}

fn smoke_process(client: &mut OpenSshClient, root: &Path) -> Result<(), String> {
    let executable = sibling_binary("remote-process-fixture")?;
    let process_id = expect_process(client.request(RequestBody::Spawn {
        executable: executable.to_string_lossy().into_owned(),
        args: Vec::new(),
        cwd: String::new(),
        shell: false,
    }))?;
    expect_ack(client.request(RequestBody::ProcessInput {
        process_id,
        bytes: b"hello\n".to_vec(),
    }))?;
    let _ = root;
    let mut output = Vec::new();
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(25));
        match client
            .request(RequestBody::DrainProcess {
                process_id,
                max_bytes: 4_096,
            })
            .map_err(display)?
        {
            ResponseBody::Stream(chunk) => output.extend_from_slice(&chunk.bytes),
            other => return Err(format!("unexpected remote task output: {other:?}")),
        }
        if output.windows(13).any(|value| value == b"fixture:hello") {
            break;
        }
    }
    if !output.windows(13).any(|value| value == b"fixture:hello") {
        return Err(format!(
            "remote task response was not observed in {} bounded bytes",
            output.len()
        ));
    }
    expect_ack(client.request(RequestBody::TerminateProcess { process_id }))
}

fn smoke_language(client: &mut OpenSshClient, _root: &Path) -> Result<(), String> {
    let executable = sibling_binary("remote-language-fixture")?;
    let process_id = expect_process(client.request(RequestBody::SpawnLanguage {
        executable: executable.to_string_lossy().into_owned(),
        args: Vec::new(),
        cwd: String::new(),
    }))?;
    let payload = b"Content-Length: 2\r\n\r\n{}".to_vec();
    expect_ack(client.request(RequestBody::LanguageInput {
        process_id,
        bytes: payload.clone(),
    }))?;
    let mut observed = None;
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(25));
        match client
            .request(RequestBody::ReadLanguage { process_id })
            .map_err(display)?
        {
            ResponseBody::Acknowledged => {}
            ResponseBody::Stream(chunk) => {
                observed = Some(chunk.bytes);
                break;
            }
            other => return Err(format!("unexpected remote language output: {other:?}")),
        }
    }
    if observed.as_deref() != Some(payload.as_slice()) {
        return Err("remote language payload was not observed before the bounded deadline".into());
    }
    expect_ack(client.request(RequestBody::TerminateLanguage { process_id }))
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
        .ok_or_else(|| format!("required smoke binary is missing: {name}"))
}

fn expect_directory(result: Result<ResponseBody, impl std::fmt::Display>) -> Result<(), String> {
    match result.map_err(display)? {
        ResponseBody::DirectoryPage { .. } => Ok(()),
        other => Err(format!("unexpected remote directory response: {other:?}")),
    }
}

fn expect_nonempty_directory(
    result: Result<ResponseBody, impl std::fmt::Display>,
) -> Result<(), String> {
    match result.map_err(display)? {
        ResponseBody::DirectoryPage { entries, .. } if !entries.is_empty() => Ok(()),
        other => Err(format!("unexpected remote search response: {other:?}")),
    }
}

fn expect_revision(result: Result<ResponseBody, impl std::fmt::Display>) -> Result<String, String> {
    match result.map_err(display)? {
        ResponseBody::Metadata { revision, .. } => Ok(revision),
        other => Err(format!("unexpected remote metadata response: {other:?}")),
    }
}

fn expect_bytes(
    result: Result<ResponseBody, impl std::fmt::Display>,
    expected: &[u8],
) -> Result<(), String> {
    match result.map_err(display)? {
        ResponseBody::Stream(chunk) if chunk.bytes == expected => Ok(()),
        other => Err(format!("unexpected remote file response: {other:?}")),
    }
}

fn expect_conflict(result: Result<ResponseBody, impl std::fmt::Display>) -> Result<(), String> {
    match result.map_err(display)? {
        ResponseBody::Error(error) if error.kind == RemoteErrorKind::Conflict => Ok(()),
        other => Err(format!("unexpected remote conflict response: {other:?}")),
    }
}

fn expect_git(result: Result<ResponseBody, impl std::fmt::Display>) -> Result<(), String> {
    match result.map_err(display)? {
        ResponseBody::GitSummary {
            branch: Some(_), ..
        } => Ok(()),
        other => Err(format!("unexpected remote Git response: {other:?}")),
    }
}

fn expect_process(result: Result<ResponseBody, impl std::fmt::Display>) -> Result<u64, String> {
    match result.map_err(display)? {
        ResponseBody::ProcessStarted { process_id } => Ok(process_id),
        other => Err(format!("unexpected remote process response: {other:?}")),
    }
}

fn expect_ack(result: Result<ResponseBody, impl std::fmt::Display>) -> Result<(), String> {
    match result.map_err(display)? {
        ResponseBody::Acknowledged => Ok(()),
        other => Err(format!("unexpected remote acknowledgement: {other:?}")),
    }
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}
