use std::fs;

use strukt_remote::{RemoteFilesystem, RemoteFilesystemError, RemotePath};
use tempfile::tempdir;

#[test]
fn remote_paths_are_relative_normalized_linux_paths() {
    assert_eq!(
        RemotePath::new("src/main.rs").unwrap().as_str(),
        "src/main.rs"
    );
    assert_eq!(
        RemotePath::new("nested/path").unwrap().segments().count(),
        2
    );
    assert!(RemotePath::root().is_root());

    for invalid in [
        "",
        "/absolute",
        "./file",
        "a/../file",
        "a//file",
        "a\\file",
        "C:/file",
        "a\0file",
    ] {
        assert!(RemotePath::new(invalid).is_err(), "accepted {invalid:?}");
    }
    assert!(RemotePath::new("x".repeat(4_097)).is_err());
}

#[cfg(unix)]
#[test]
fn followed_symlink_cannot_escape_the_retained_root() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret"), "outside").unwrap();
    symlink(outside.path().join("secret"), root.path().join("escape")).unwrap();
    let filesystem = RemoteFilesystem::open(root.path()).unwrap();

    assert!(matches!(
        filesystem.read(&RemotePath::new("escape").unwrap()),
        Err(RemoteFilesystemError::Confined(_))
    ));
}

#[test]
fn replacing_the_root_is_detected_before_operations() {
    let parent = tempdir().unwrap();
    let root = parent.path().join("root");
    let replacement = parent.path().join("replacement");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&replacement).unwrap();
    fs::write(root.join("file"), "old").unwrap();
    fs::write(replacement.join("file"), "new").unwrap();
    let filesystem = RemoteFilesystem::open(&root).unwrap();
    fs::rename(&root, parent.path().join("old-root")).unwrap();
    fs::rename(&replacement, &root).unwrap();

    assert!(matches!(
        filesystem.read(&RemotePath::new("file").unwrap()),
        Err(RemoteFilesystemError::WorkspaceChanged)
    ));
}
