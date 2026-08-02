use std::ffi::OsString;
use std::path::PathBuf;

use strukt_language::{
    CommandApproval, DescriptorRegistry, DescriptorSource, ExecutableCandidate,
    LanguageServerDescriptor, ResolvedCommand, built_in_descriptors, registry_from_json,
};
use strukt_workspace::WorkspaceId;

#[test]
fn registry_matches_language_without_language_specific_control_flow() {
    let registry = DescriptorRegistry::new(vec![descriptor(
        "rust-analyzer",
        ["rust"],
        [ExecutableCandidate::path_name("rust-analyzer").unwrap()],
    )])
    .unwrap();

    assert_eq!(registry.for_language("rust").unwrap().id(), "rust-analyzer");
    assert!(registry.for_language("python").is_none());
}

#[test]
fn registry_rejects_duplicate_ids_and_invalid_executable_shapes() {
    let first = descriptor(
        "server",
        ["rust"],
        [ExecutableCandidate::path_name("server").unwrap()],
    );
    let second = descriptor(
        "server",
        ["python"],
        [ExecutableCandidate::absolute(absolute_server_path()).unwrap()],
    );

    assert!(DescriptorRegistry::new(vec![first, second]).is_err());
    assert!(ExecutableCandidate::path_name("tools/server").is_err());
    assert!(ExecutableCandidate::absolute(PathBuf::from("relative/server")).is_err());
}

#[test]
fn workspace_approval_is_exact_and_invalidates_on_argument_change() {
    let command =
        ResolvedCommand::new(absolute_server_path(), vec![OsString::from("--stdio")]).unwrap();
    let workspace = workspace_id();
    let approval = CommandApproval::grant(workspace.clone(), &command);

    assert!(approval.authorizes(&workspace, &command));
    assert!(
        !approval.authorizes(
            &workspace,
            &ResolvedCommand::new(
                absolute_server_path(),
                vec![OsString::from("--stdio"), OsString::from("--unsafe")],
            )
            .unwrap()
        )
    );
    assert!(!approval.authorizes(&other_workspace_id(), &command));
}

#[test]
fn versioned_json_registry_preserves_unknown_descriptor_fields() {
    let json = br#"{
        "schema_version": 1,
        "descriptors": [{
            "id": "custom",
            "display_name": "Custom",
            "language_ids": ["rust"],
            "executable": "custom-lsp",
            "arguments": ["--stdio"],
            "future": {"enabled": true}
        }]
    }"#;

    let registry = registry_from_json(json, DescriptorSource::Workspace).unwrap();
    let descriptor = registry.for_language("rust").unwrap();

    assert_eq!(descriptor.id(), "custom");
    assert_eq!(descriptor.source(), DescriptorSource::Workspace);
    assert_eq!(
        descriptor.unknown_fields().get("future"),
        Some(&serde_json::json!({"enabled": true}))
    );
}

#[test]
fn registry_json_and_builtins_are_bounded_and_cover_editor_languages() {
    assert!(registry_from_json(&vec![b' '; 256 * 1024 + 1], DescriptorSource::User).is_err());

    let builtins = built_in_descriptors().unwrap();
    for language in [
        "rust",
        "javascript",
        "typescript",
        "python",
        "json",
        "toml",
        "markdown",
        "shell",
        "yaml",
        "html",
        "css",
    ] {
        assert!(
            builtins.for_language(language).is_some(),
            "missing {language}"
        );
    }
}

fn descriptor<const L: usize, const C: usize>(
    id: &str,
    languages: [&str; L],
    candidates: [ExecutableCandidate; C],
) -> LanguageServerDescriptor {
    LanguageServerDescriptor::new(
        id,
        id,
        languages,
        candidates,
        Vec::<OsString>::new(),
        DescriptorSource::BuiltIn,
    )
    .unwrap()
}

fn workspace_id() -> WorkspaceId {
    serde_json::from_str(&format!("\"{}\"", "a".repeat(64))).unwrap()
}

fn other_workspace_id() -> WorkspaceId {
    serde_json::from_str(&format!("\"{}\"", "b".repeat(64))).unwrap()
}

#[cfg(not(windows))]
fn absolute_server_path() -> PathBuf {
    PathBuf::from("/workspace/tools/server")
}

#[cfg(windows)]
fn absolute_server_path() -> PathBuf {
    PathBuf::from(r"C:\workspace\tools\server.exe")
}
