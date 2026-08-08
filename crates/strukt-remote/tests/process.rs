use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use strukt_remote::{RemotePath, RemoteProcessManager, RemoteProcessRequest};
use strukt_terminal::TerminalSize;
use tempfile::tempdir;

#[test]
fn pty_process_supports_input_output_resize_exit_and_isolated_ids() {
    let root = tempdir().unwrap();
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_remote-process-fixture"));
    let mut manager = RemoteProcessManager::new(root.path()).unwrap();
    let request = RemoteProcessRequest::new(
        fixture,
        Vec::new(),
        RemotePath::root(),
        Vec::new(),
        TerminalSize::new(24, 80).unwrap(),
    )
    .unwrap();
    let first = manager.spawn(request.clone()).unwrap();
    let second = manager.spawn(request).unwrap();
    assert_ne!(first, second);

    manager.write(first, b"alpha\r").unwrap();
    manager
        .resize(first, TerminalSize::new(30, 100).unwrap())
        .unwrap();
    let output = drain_until(&mut manager, first, "fixture:alpha");
    assert!(output.contains("fixture:alpha"));
    assert!(
        manager
            .drain(second, 8, 64 * 1024)
            .unwrap()
            .bytes
            .is_empty()
    );

    manager.write(first, b"exit\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while manager.try_wait(first).unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(manager.try_wait(first).unwrap().is_some());
    manager
        .terminate(second, Duration::from_millis(100))
        .unwrap();
}

#[test]
fn process_requests_reject_escape_nul_and_unapproved_shell_mode() {
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_remote-process-fixture"));
    let size = TerminalSize::new(24, 80).unwrap();
    assert!(
        RemoteProcessRequest::new(
            fixture.clone(),
            vec!["bad\0arg".into()],
            RemotePath::root(),
            Vec::new(),
            size,
        )
        .is_err()
    );
    assert!(
        RemoteProcessRequest::new(
            fixture,
            Vec::new(),
            RemotePath::new("missing").unwrap(),
            vec![("BAD=KEY".into(), "value".into())],
            size,
        )
        .is_err()
    );
}

fn drain_until(manager: &mut RemoteProcessManager, id: u64, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    while Instant::now() < deadline {
        let output = manager.drain(id, 32, 256 * 1024).unwrap();
        bytes.extend_from_slice(&output.bytes);
        let text = String::from_utf8_lossy(&bytes);
        if text.contains(needle) {
            return text.into_owned();
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {needle:?}: {}",
        String::from_utf8_lossy(&bytes)
    );
}
