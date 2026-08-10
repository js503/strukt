use std::fs;
use std::path::PathBuf;

use strukt_remote::{OpenSsh, OpenSshClient, RequestBody, ResponseBody, SshAlias, SshExecutable};
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
    client.disconnect();
}
