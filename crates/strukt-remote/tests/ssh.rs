use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use strukt_remote::{
    OpenSsh, OpenSshError, SshAlias, SshCancellation, SshCommandKind, SshCommandSpec,
    SshExecutable, SshExecutor, SshOutput, parse_effective_config,
};
use tempfile::tempdir;

fn args(spec: &strukt_remote::SshCommandSpec) -> Vec<&str> {
    spec.args
        .iter()
        .map(|argument| argument.to_str().expect("test arguments are UTF-8"))
        .collect()
}

#[test]
fn explicit_executable_wins_and_path_search_is_injected() {
    let root = tempdir().unwrap();
    let explicit = root.path().join("custom-ssh");
    let searched = root
        .path()
        .join(if cfg!(windows) { "ssh.exe" } else { "ssh" });
    fs::write(&explicit, b"fixture").unwrap();
    fs::write(&searched, b"fixture").unwrap();

    assert_eq!(
        SshExecutable::discover(Some(&explicit), &[root.path().to_path_buf()])
            .unwrap()
            .path(),
        explicit
    );
    assert_eq!(
        SshExecutable::discover(None, &[root.path().to_path_buf()])
            .unwrap()
            .path(),
        searched
    );
    assert!(SshExecutable::discover(Some(Path::new("missing")), &[]).is_err());
}

#[test]
fn command_specs_keep_aliases_separate_and_preserve_security_defaults() {
    let ssh = OpenSsh::new(SshExecutable::from_path(PathBuf::from("/tools/ssh")).unwrap());
    let alias = SshAlias::new("ubuntu@ec2-development").unwrap();

    let config = ssh.resolve_config(&alias);
    assert_eq!(config.kind, SshCommandKind::ResolveConfig);
    assert_eq!(args(&config), ["-G", "--", "ubuntu@ec2-development"]);
    assert!(
        !args(&config)
            .iter()
            .any(|arg| arg.contains("StrictHostKeyChecking=no"))
    );

    let probe = ssh.probe(&alias);
    assert_eq!(probe.kind, SshCommandKind::Probe);
    assert_eq!(
        args(&probe),
        [
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "--",
            "ubuntu@ec2-development",
            "true"
        ]
    );
    assert_eq!(probe.deadline, Duration::from_secs(15));
    assert!(!probe.interactive);

    let terminal = ssh.open_terminal(&alias);
    assert_eq!(terminal.kind, SshCommandKind::Terminal);
    assert_eq!(args(&terminal), ["-tt", "--", "ubuntu@ec2-development"]);
    assert!(terminal.interactive);
    assert!(terminal.deadline.is_zero());
}

#[test]
fn helper_command_uses_only_fixed_remote_tokens() {
    let ssh = OpenSsh::new(SshExecutable::from_path(PathBuf::from("ssh")).unwrap());
    let alias = SshAlias::new("ec2-development").unwrap();
    let helper = ssh.open_helper(&alias, "0.1.0").unwrap();
    assert_eq!(helper.kind, SshCommandKind::Helper);
    assert_eq!(
        args(&helper),
        [
            "-T",
            "-o",
            "BatchMode=yes",
            "--",
            "ec2-development",
            "~/.local/share/strukt/bin/0.1.0/strukt-remote",
            "--stdio"
        ]
    );
    for invalid in ["", "../bad", "1.0;bad", "v 1"] {
        assert!(ssh.open_helper(&alias, invalid).is_err());
    }
}

#[test]
fn effective_config_parser_is_case_insensitive_bounded_and_typed() {
    let output = b"hostname 10.0.0.4\nUser ubuntu\nport 22\nproxyjump bastion\nidentityfile ~/.ssh/id_ed25519\n";
    let parsed = parse_effective_config(output).unwrap();
    assert_eq!(parsed.hostname.as_deref(), Some("10.0.0.4"));
    assert_eq!(parsed.user.as_deref(), Some("ubuntu"));
    assert_eq!(parsed.port, Some(22));
    assert_eq!(parsed.proxy_jump.as_deref(), Some("bastion"));
    assert_eq!(
        parsed.identity_files,
        vec![PathBuf::from("~/.ssh/id_ed25519")]
    );

    assert!(parse_effective_config(&vec![b'x'; 256 * 1_024]).is_err());
    assert!(parse_effective_config(b"port not-a-number\n").is_err());
    assert!(parse_effective_config(b"hostname bad\0host\n").is_err());
}

#[test]
fn diagnostics_are_sanitized_and_bounded() {
    let ssh = OpenSsh::new(SshExecutable::from_path(PathBuf::from("ssh")).unwrap());
    let diagnostic = ssh.sanitize_diagnostic(format!("denied\0{}", "x".repeat(8_000)));
    assert!(!diagnostic.contains('\0'));
    assert!(diagnostic.len() <= 2_048);
    assert_eq!(ssh.executable().path().as_os_str(), OsStr::new("ssh"));
}

#[derive(Default)]
struct FakeExecutor {
    calls: Mutex<Vec<SshCommandKind>>,
}

impl SshExecutor for FakeExecutor {
    fn execute(
        &self,
        spec: &SshCommandSpec,
        cancellation: &SshCancellation,
    ) -> Result<SshOutput, OpenSshError> {
        assert!(!cancellation.is_cancelled());
        self.calls.lock().unwrap().push(spec.kind);
        SshOutput::new(
            true,
            b"hostname 10.0.0.4\nuser ubuntu\nport 22\n".to_vec(),
            Vec::new(),
        )
    }
}

#[test]
fn effective_config_execution_is_injectable_and_cancellable() {
    let ssh = OpenSsh::new(SshExecutable::from_path(PathBuf::from("ssh")).unwrap());
    let alias = SshAlias::new("ec2-development").unwrap();
    let executor = FakeExecutor::default();
    let cancellation = SshCancellation::new();

    let config = ssh
        .read_effective_config(&alias, &executor, &cancellation)
        .unwrap();
    assert_eq!(config.hostname.as_deref(), Some("10.0.0.4"));
    assert_eq!(
        *executor.calls.lock().unwrap(),
        [SshCommandKind::ResolveConfig]
    );

    cancellation.cancel();
    assert!(matches!(
        ssh.read_effective_config(&alias, &executor, &cancellation),
        Err(OpenSshError::Cancelled)
    ));
    assert_eq!(executor.calls.lock().unwrap().len(), 1);
}

#[test]
fn captured_outputs_are_bounded_and_keep_stderr_separate() {
    assert!(SshOutput::new(true, vec![b'x'; 256 * 1_024 + 1], Vec::new()).is_err());
    assert!(SshOutput::new(false, Vec::new(), vec![b'x'; 64 * 1_024 + 1]).is_err());
    let output = SshOutput::new(false, b"protocol".to_vec(), b"denied".to_vec()).unwrap();
    assert!(!output.success);
    assert_eq!(output.stdout, b"protocol");
    assert_eq!(output.stderr, b"denied");
}
