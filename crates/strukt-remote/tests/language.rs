use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use strukt_remote::{RemoteLanguageManager, RemotePath};
use tempfile::tempdir;

#[test]
fn language_stream_preserves_bytes_and_requires_explicit_spawn() {
    let root = tempdir().unwrap();
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_remote-language-fixture"));
    let mut manager = RemoteLanguageManager::new(root.path()).unwrap();
    assert_eq!(manager.running(), 0);
    let id = manager
        .spawn(fixture, Vec::new(), &RemotePath::root())
        .unwrap();
    let frame = b"Content-Length: 2\r\n\r\n{}";
    manager.write(id, frame).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        if let Some(bytes) = manager.try_read(id).unwrap() {
            output.extend(bytes);
            if output.len() >= frame.len() {
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(output, frame);
    manager.terminate(id, Duration::from_millis(100)).unwrap();
    assert_eq!(manager.running(), 0);
}

#[test]
fn language_spawn_rejects_relative_executable_and_missing_cwd() {
    let root = tempdir().unwrap();
    let mut manager = RemoteLanguageManager::new(root.path()).unwrap();
    assert!(
        manager
            .spawn(PathBuf::from("relative"), Vec::new(), &RemotePath::root())
            .is_err()
    );
    assert!(
        manager
            .spawn(
                PathBuf::from(env!("CARGO_BIN_EXE_remote-language-fixture")),
                Vec::new(),
                &RemotePath::new("missing").unwrap(),
            )
            .is_err()
    );
}
