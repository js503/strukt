use strukt_workspace::WorkspaceRoot;
use tempfile::tempdir;

#[test]
fn canonical_paths_produce_stable_workspace_identity() {
    let project = tempdir().expect("temporary project");
    let first = WorkspaceRoot::open(project.path()).expect("open workspace");
    let second = WorkspaceRoot::open(project.path().join(".")).expect("open workspace");

    assert_eq!(first.id(), second.id());
    assert_eq!(first.path(), project.path().canonicalize().unwrap());
}

#[test]
fn regular_files_are_not_workspace_roots() {
    let project = tempdir().expect("temporary project");
    let file = project.path().join("README.md");
    std::fs::write(&file, "strukt").unwrap();

    assert!(WorkspaceRoot::open(file).is_err());
}
