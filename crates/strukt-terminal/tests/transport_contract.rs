use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use strukt_terminal::{
    PortableTransport, SpawnRequest, TerminalSize, TerminalTransport, TransportError,
};

// ConPTY cursor inheritance is coordinated through the hosting console. Keep the
// independent contract cases serial while each case still proves multiple live
// processes; the end-to-end smoke separately exercises concurrent panes.
static NATIVE_CONTRACT: Mutex<()> = Mutex::new(());

#[test]
fn native_transport_spawns_writes_resizes_exits_and_isolates() {
    let _contract = NATIVE_CONTRACT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_terminal-fixture"));
    let transport = PortableTransport::new();
    let mut first = transport.spawn(request(&fixture, "echo", 24, 80)).unwrap();
    let mut second = transport.spawn(request(&fixture, "echo", 10, 40)).unwrap();

    first.write("héllo\r\n".as_bytes()).unwrap();
    second.write(b"other\r\n").unwrap();
    first.resize(TerminalSize::new(30, 100).unwrap()).unwrap();

    assert!(read_until(
        &mut *first,
        "fixture-echo:héllo",
        Duration::from_secs(5)
    ));
    assert!(read_until(
        &mut *second,
        "fixture-echo:other",
        Duration::from_secs(5)
    ));
    assert_eq!(first.wait(Duration::from_secs(5)).unwrap().code(), Some(0));
    assert_eq!(second.wait(Duration::from_secs(5)).unwrap().code(), Some(0));
}

#[test]
fn native_transport_terminates_a_long_running_fixture() {
    let _contract = NATIVE_CONTRACT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_terminal-fixture"));
    let mut child = PortableTransport::new()
        .spawn(request(&fixture, "wait", 24, 80))
        .unwrap();

    assert!(read_until(
        &mut *child,
        "fixture-ready",
        Duration::from_secs(5)
    ));
    child.terminate(Duration::from_millis(500)).unwrap();

    assert!(child.wait(Duration::from_secs(2)).unwrap().was_terminated());
}

#[test]
fn transport_validates_requests_before_spawn() {
    assert!(TerminalSize::new(0, 80).is_err());
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_terminal-fixture"));
    let mut invalid = request(&fixture, "echo", 24, 80);
    invalid.working_directory = PathBuf::from("relative");

    assert!(matches!(
        PortableTransport::new().spawn(invalid),
        Err(TransportError::InvalidWorkingDirectory)
    ));
}

#[test]
fn output_chunks_are_bounded_and_strictly_sequenced() {
    let _contract = NATIVE_CONTRACT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_terminal-fixture"));
    let mut child = PortableTransport::new()
        .spawn(request(&fixture, "burst", 24, 80))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut previous = None;
    let mut output = Vec::new();

    while Instant::now() < deadline {
        if let Some(chunk) = child.try_read().unwrap() {
            assert!(chunk.bytes().len() <= 64 * 1024);
            if let Some(previous) = previous {
                assert_eq!(chunk.sequence(), previous + 1);
            }
            previous = Some(chunk.sequence());
            output.extend_from_slice(chunk.bytes());
        }
        if String::from_utf8_lossy(&output).contains("fixture-burst-complete") {
            break;
        }
        std::thread::yield_now();
    }

    assert!(String::from_utf8_lossy(&output).contains("fixture-burst-complete"));
    assert_eq!(child.wait(Duration::from_secs(5)).unwrap().code(), Some(0));
}

fn request(fixture: &Path, mode: &str, rows: u16, columns: u16) -> SpawnRequest {
    SpawnRequest {
        executable: fixture.to_path_buf(),
        arguments: vec![mode.into()],
        working_directory: std::env::current_dir().unwrap(),
        environment: Vec::new(),
        size: TerminalSize::new(rows, columns).unwrap(),
    }
}

fn read_until(
    process: &mut dyn strukt_terminal::TerminalProcess,
    needle: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    while Instant::now() < deadline {
        match process.try_read() {
            Ok(Some(chunk)) => {
                output.extend_from_slice(chunk.bytes());
                if String::from_utf8_lossy(&output).contains(needle) {
                    return true;
                }
            }
            Ok(None) => std::thread::yield_now(),
            Err(error) => panic!("transport read failed: {error}"),
        }
    }
    false
}
