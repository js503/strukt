use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use strukt_language::{
    ApprovalStatus, CommandApproval, DescriptorSource, DiscoveryOutcome, ExecutableCandidate,
    LanguageServerDescriptor, discover, load_workspace_registry, select_descriptor,
};
use strukt_workspace::WorkspaceRoot;

#[test]
fn discovery_resolves_path_order_without_executing_candidates() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let marker = workspace_dir.path().join("executed");
    let first_executable = executable(first.path(), "fake-lsp", &marker);
    let _second_executable = executable(second.path(), "fake-lsp", &marker);
    let path_env = std::env::join_paths([first.path(), second.path()]).unwrap();

    let outcome = discover(
        &descriptor(DescriptorSource::BuiltIn, "fake-lsp", true),
        Some(&path_env),
        &workspace,
        ApprovalStatus::Unreviewed,
    )
    .unwrap();

    let DiscoveryOutcome::Available(server) = outcome else {
        panic!("expected available server");
    };
    assert_eq!(
        server.command().executable(),
        first_executable.canonicalize().unwrap()
    );
    assert!(!marker.exists());
}

#[test]
fn workspace_commands_require_exact_approval_and_disabled_stays_disabled() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();
    let bin = tempfile::tempdir().unwrap();
    executable(
        bin.path(),
        "workspace-lsp",
        &workspace_dir.path().join("executed"),
    );
    let path_env = std::env::join_paths([bin.path()]).unwrap();
    let server_descriptor = descriptor(DescriptorSource::Workspace, "workspace-lsp", true);

    let pending = discover(
        &server_descriptor,
        Some(&path_env),
        &workspace,
        ApprovalStatus::Unreviewed,
    )
    .unwrap();
    let DiscoveryOutcome::ApprovalRequired(server) = pending else {
        panic!("expected approval request");
    };
    let approval = CommandApproval::grant(workspace.id().clone(), server.command());
    assert!(matches!(
        discover(
            &server_descriptor,
            Some(&path_env),
            &workspace,
            ApprovalStatus::Approved(&approval),
        )
        .unwrap(),
        DiscoveryOutcome::Available(_)
    ));
    assert!(matches!(
        discover(
            &server_descriptor,
            Some(&path_env),
            &workspace,
            ApprovalStatus::Denied,
        )
        .unwrap(),
        DiscoveryOutcome::Disabled
    ));
    assert!(matches!(
        discover(
            &descriptor(DescriptorSource::BuiltIn, "workspace-lsp", false),
            Some(&path_env),
            &workspace,
            ApprovalStatus::Unreviewed,
        )
        .unwrap(),
        DiscoveryOutcome::Disabled
    ));
}

#[test]
fn unavailable_discovery_keeps_bounded_installation_guidance() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();
    let empty = tempfile::tempdir().unwrap();
    let path_env = std::env::join_paths([empty.path()]).unwrap();
    let descriptor = descriptor(DescriptorSource::BuiltIn, "missing-lsp", true)
        .with_installation_guidance(Some("Install missing-lsp".to_owned()))
        .unwrap();

    assert_eq!(
        discover(
            &descriptor,
            Some(&path_env),
            &workspace,
            ApprovalStatus::Unreviewed,
        )
        .unwrap(),
        DiscoveryOutcome::Unavailable {
            guidance: Some("Install missing-lsp".to_owned())
        }
    );
}

#[test]
fn absolute_candidates_are_canonicalized_before_approval() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();
    let bin = tempfile::tempdir().unwrap();
    let executable = executable(
        bin.path(),
        "absolute-lsp",
        &workspace_dir.path().join("executed"),
    );
    let descriptor = LanguageServerDescriptor::new(
        "absolute",
        "Absolute",
        ["rust"],
        [ExecutableCandidate::absolute(executable.clone()).unwrap()],
        Vec::<OsString>::new(),
        DescriptorSource::BuiltIn,
    )
    .unwrap();

    let DiscoveryOutcome::Available(server) =
        discover(&descriptor, None, &workspace, ApprovalStatus::Unreviewed).unwrap()
    else {
        panic!("expected available server");
    };
    assert_eq!(
        server.command().executable(),
        executable.canonicalize().unwrap()
    );
}

#[test]
fn workspace_registry_is_root_confined_bounded_and_rejects_symlinks() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();
    assert!(load_workspace_registry(&workspace).unwrap().is_none());

    fs::write(
        workspace_dir.path().join(".strukt-language.json"),
        br#"{"schema_version":1,"descriptors":[{"id":"custom","display_name":"Custom","language_ids":["rust"],"executable":"custom-lsp"}]}"#,
    )
    .unwrap();
    assert_eq!(
        load_workspace_registry(&workspace)
            .unwrap()
            .unwrap()
            .for_language("rust")
            .unwrap()
            .source(),
        DescriptorSource::Workspace
    );

    fs::write(
        workspace_dir.path().join(".strukt-language.json"),
        vec![b' '; 256 * 1024 + 1],
    )
    .unwrap();
    assert!(load_workspace_registry(&workspace).is_err());

    fs::write(
        workspace_dir.path().join(".strukt-language.json"),
        b"not-json",
    )
    .unwrap();
    assert!(load_workspace_registry(&workspace).is_err());
}

#[cfg(unix)]
#[test]
fn workspace_registry_rejects_a_symlinked_configuration() {
    use std::os::unix::fs::symlink;

    let workspace_dir = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    symlink(
        outside.path(),
        workspace_dir.path().join(".strukt-language.json"),
    )
    .unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();

    assert!(load_workspace_registry(&workspace).is_err());
}

#[test]
fn descriptor_selection_prefers_explicit_choice_then_workspace_markers() {
    let workspace_dir = tempfile::tempdir().unwrap();
    fs::write(workspace_dir.path().join("Cargo.toml"), "[workspace]").unwrap();
    let workspace = WorkspaceRoot::open(workspace_dir.path()).unwrap();
    let generic = descriptor_with_id("generic", DescriptorSource::BuiltIn, "generic-lsp", true);
    let rust = descriptor_with_id("rust", DescriptorSource::BuiltIn, "rust-analyzer", true)
        .with_workspace_markers([PathBuf::from("Cargo.toml")])
        .unwrap();
    let registry = strukt_language::DescriptorRegistry::new(vec![generic, rust]).unwrap();

    assert_eq!(
        select_descriptor(&registry, "rust", &workspace, None)
            .unwrap()
            .unwrap()
            .id(),
        "rust"
    );
    assert_eq!(
        select_descriptor(&registry, "rust", &workspace, Some("generic"))
            .unwrap()
            .unwrap()
            .id(),
        "generic"
    );
}

fn descriptor(
    source: DescriptorSource,
    executable: &str,
    enabled: bool,
) -> LanguageServerDescriptor {
    descriptor_with_id("fixture", source, executable, enabled)
}

fn descriptor_with_id(
    id: &str,
    source: DescriptorSource,
    executable: &str,
    enabled: bool,
) -> LanguageServerDescriptor {
    LanguageServerDescriptor::new(
        id,
        "Fixture",
        ["rust"],
        [ExecutableCandidate::path_name(executable).unwrap()],
        [OsString::from("--stdio")],
        source,
    )
    .unwrap()
    .with_enabled(enabled)
}

fn executable(directory: &Path, name: &str, marker: &Path) -> PathBuf {
    let path = executable_path(directory, name);
    fs::write(&path, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    make_executable(&path);
    path
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(windows)]
fn make_executable(_path: &Path) {}

#[cfg(windows)]
fn executable_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(format!("{name}.exe"))
}

#[cfg(not(windows))]
fn executable_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(name)
}
