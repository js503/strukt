#[cfg(all(unix, not(target_os = "macos")))]
use strukt_workspace::WorkspaceError;
use strukt_workspace::{WorkspaceId, WorkspaceRoot};
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

#[test]
fn workspace_root_round_trips_through_json() {
    let project = tempdir().expect("temporary project");
    let root = WorkspaceRoot::open(project.path()).expect("open workspace");

    let serialized = serde_json::to_string(&root).expect("serialize workspace root");
    let restored: WorkspaceRoot =
        serde_json::from_str(&serialized).expect("deserialize workspace root");

    assert_eq!(restored, root);
}

#[test]
fn workspace_root_rejects_a_tampered_id() {
    let project = tempdir().expect("temporary project");
    let root = WorkspaceRoot::open(project.path()).expect("open workspace");
    let mut serialized = serde_json::to_value(&root).expect("serialize workspace root");
    serialized["id"] = serde_json::Value::String("0".repeat(64));

    assert!(serde_json::from_value::<WorkspaceRoot>(serialized).is_err());
}

#[test]
fn workspace_root_rejects_a_tampered_display_name() {
    let project = tempdir().expect("temporary project");
    let root = WorkspaceRoot::open(project.path()).expect("open workspace");
    let mut serialized = serde_json::to_value(&root).expect("serialize workspace root");
    serialized["display_name"] = serde_json::Value::String("tampered".to_owned());

    assert!(serde_json::from_value::<WorkspaceRoot>(serialized).is_err());
}

#[test]
fn workspace_id_rejects_malformed_representations() {
    assert!(serde_json::from_str::<WorkspaceId>(r#""short""#).is_err());
    assert!(serde_json::from_str::<WorkspaceId>(&format!(r#""{}""#, "A".repeat(64))).is_err());
    assert!(serde_json::from_str::<WorkspaceId>(&format!(r#""{}""#, "g".repeat(64))).is_err());
}

#[cfg(unix)]
#[test]
fn retained_capability_rejects_a_replaced_root_path() {
    let parent = tempdir().unwrap();
    let path = parent.path().join("workspace");
    let original = parent.path().join("original");
    std::fs::create_dir(&path).unwrap();
    let root = WorkspaceRoot::open(&path).unwrap();

    std::fs::rename(&path, &original).unwrap();
    std::fs::create_dir(&path).unwrap();

    assert!(root.validate_location().is_err());
    assert!(root.try_clone_capability().is_ok());
}

#[cfg(windows)]
#[test]
fn retained_capability_prevents_a_root_path_replacement() {
    let parent = tempdir().unwrap();
    let path = parent.path().join("workspace");
    let original = parent.path().join("original");
    let replacement = parent.path().join("replacement");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("original.txt"), "original").unwrap();
    std::fs::create_dir(&replacement).unwrap();
    std::fs::write(replacement.join("replacement.txt"), "replacement").unwrap();
    let root = WorkspaceRoot::open(&path).unwrap();

    let error = std::fs::rename(&path, &original).unwrap_err();

    assert_eq!(error.raw_os_error(), Some(32));
    assert!(!original.exists());
    assert_eq!(
        std::fs::read_to_string(path.join("original.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        std::fs::read_to_string(replacement.join("replacement.txt")).unwrap(),
        "replacement"
    );
    assert!(root.validate_location().is_ok());
    assert!(
        root.try_clone_capability()
            .unwrap()
            .open("original.txt")
            .is_ok()
    );
}

#[cfg(windows)]
#[test]
fn retained_capability_prevents_a_directory_symlink_replacement() {
    let parent = tempdir().unwrap();
    let path = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    let outside = parent.path().join("outside");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("inside.txt"), "inside").unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("outside.txt"), "outside").unwrap();
    let root = WorkspaceRoot::open(&path).unwrap();

    let error = std::fs::rename(&path, &moved).unwrap_err();

    assert_eq!(error.raw_os_error(), Some(32));
    assert!(!moved.exists());
    assert_eq!(
        std::fs::read_to_string(path.join("inside.txt")).unwrap(),
        "inside"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("outside.txt")).unwrap(),
        "outside"
    );
    assert!(root.validate_location().is_ok());
    assert!(
        root.try_clone_capability()
            .unwrap()
            .open("inside.txt")
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn retained_capability_rejects_a_root_path_replaced_by_a_symlink() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().unwrap();
    let path = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    std::fs::create_dir(&path).unwrap();
    let root = WorkspaceRoot::open(&path).unwrap();

    std::fs::rename(&path, &moved).unwrap();
    symlink(&moved, &path).unwrap();

    assert!(root.validate_location().is_err());
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_workspace_paths_are_explicitly_rejected() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let parent = tempdir().expect("temporary parent");
    let first = parent.path().join(OsString::from_vec(vec![b'a', 0xff]));
    let second = parent.path().join(OsString::from_vec(vec![b'a', 0xfe]));
    std::fs::create_dir(&first).expect("create first invalid UTF-8 directory");
    std::fs::create_dir(&second).expect("create second invalid UTF-8 directory");

    assert!(matches!(
        WorkspaceRoot::open(first),
        Err(WorkspaceError::NonUtf8Path(_))
    ));
    assert!(matches!(
        WorkspaceRoot::open(second),
        Err(WorkspaceError::NonUtf8Path(_))
    ));
}
