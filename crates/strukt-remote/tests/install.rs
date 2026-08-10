use std::fs;

use sha2::{Digest, Sha256};
use strukt_remote::{HelperArtifact, HelperInstallError, RemoteBuildTarget};
use tempfile::tempdir;

#[test]
fn installer_selects_and_verifies_the_exact_linux_artifact() {
    let directory = tempdir().unwrap();
    let x86 = directory.path().join("strukt-remote-linux-x86_64");
    let arm = directory.path().join("strukt-remote-linux-aarch64");
    fs::write(&x86, b"x86 helper").unwrap();
    fs::write(&arm, b"arm helper").unwrap();
    let expected = format!("{:x}", Sha256::digest(b"arm helper"));

    let artifact = HelperArtifact::select(
        RemoteBuildTarget::LinuxAarch64,
        "0.1.0",
        directory.path(),
        &expected,
    )
    .unwrap();

    assert_eq!(artifact.path(), arm);
    assert_eq!(artifact.version(), "0.1.0");
    assert_eq!(artifact.checksum(), expected);
    assert_eq!(
        artifact.install_path(),
        ".local/share/strukt/bin/0.1.0/strukt-remote"
    );
    assert!(artifact.consent_summary().contains("Linux aarch64"));
    assert!(artifact.consent_summary().contains(&expected));
}

#[test]
fn installer_rejects_missing_mismatched_and_hostile_inputs() {
    let directory = tempdir().unwrap();
    let artifact = directory.path().join("strukt-remote-linux-x86_64");
    fs::write(&artifact, b"helper").unwrap();

    assert!(matches!(
        HelperArtifact::select(
            RemoteBuildTarget::LinuxX86_64,
            "0.1.0",
            directory.path(),
            &"0".repeat(64),
        ),
        Err(HelperInstallError::ChecksumMismatch)
    ));
    assert!(matches!(
        HelperArtifact::select(
            RemoteBuildTarget::LinuxAarch64,
            "0.1.0",
            directory.path(),
            &"0".repeat(64),
        ),
        Err(HelperInstallError::ArtifactMissing)
    ));
    assert!(matches!(
        HelperArtifact::select(
            RemoteBuildTarget::LinuxX86_64,
            "../escape",
            directory.path(),
            &format!("{:x}", Sha256::digest(b"helper")),
        ),
        Err(HelperInstallError::InvalidVersion)
    ));
}

#[test]
fn bootstrap_is_fixed_private_atomic_and_versioned() {
    let script = strukt_remote::helper_install_bootstrap();

    assert!(script.contains("umask 077"));
    assert!(script.contains("mktemp"));
    assert!(script.contains("chmod 700"));
    assert!(script.contains("mv"));
    assert!(script.contains("sha256sum") || script.contains("shasum"));
    assert!(!script.contains("sudo"));
}

#[test]
fn install_command_keeps_alias_and_validated_metadata_out_of_bootstrap_text() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("strukt-remote-linux-x86_64");
    fs::write(&path, b"helper bytes").unwrap();
    let checksum = format!("{:x}", Sha256::digest(b"helper bytes"));
    let artifact = HelperArtifact::select(
        RemoteBuildTarget::LinuxX86_64,
        "0.1.0",
        directory.path(),
        &checksum,
    )
    .unwrap();
    let ssh =
        strukt_remote::OpenSsh::new(strukt_remote::SshExecutable::from_path("ssh".into()).unwrap());
    let alias = strukt_remote::SshAlias::new("build-box").unwrap();

    let spec = ssh
        .install_helper(&alias, artifact.version(), artifact.checksum())
        .unwrap();

    assert_eq!(spec.kind, strukt_remote::SshCommandKind::Install);
    assert_eq!(spec.args[4], "build-box");
    assert_eq!(artifact.bytes(), b"helper bytes");
    let remote_command = spec.args[5].to_string_lossy();
    assert!(remote_command.contains("umask 077"));
    assert!(remote_command.contains("0.1.0"));
    assert!(remote_command.contains(&checksum));
    assert!(!remote_command.contains("build-box"));
}

#[test]
fn installer_streams_the_verified_artifact_through_typed_ssh_stdio() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("strukt-remote-linux-x86_64");
    fs::write(&path, b"helper bytes").unwrap();
    let checksum = format!("{:x}", Sha256::digest(b"helper bytes"));
    let artifact = HelperArtifact::select(
        RemoteBuildTarget::LinuxX86_64,
        "0.1.0",
        directory.path(),
        &checksum,
    )
    .unwrap();
    let ssh = strukt_remote::OpenSsh::new(
        strukt_remote::SshExecutable::from_path(env!("CARGO_BIN_EXE_fake-ssh").into()).unwrap(),
    );
    let spec = ssh
        .install_helper(
            &strukt_remote::SshAlias::new("fixture").unwrap(),
            artifact.version(),
            artifact.checksum(),
        )
        .unwrap();

    let output = strukt_remote::execute_helper_install(
        &spec,
        &artifact,
        &strukt_remote::SshCancellation::new(),
    )
    .unwrap();

    assert!(output.success);
    assert_eq!(output.stdout, b"installed 12 bytes\n");
    assert!(output.stderr.is_empty());
}
