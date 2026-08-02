use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use strukt_language::{
    FrameDecoder, FrameError, FrameLimits, LanguageProcess, LanguageTransport, ResolvedCommand,
    SpawnRequest, StdioTransport, encode_frame,
};

#[test]
fn healthy_fixture_supports_the_public_alpha_protocol_subset() {
    let workspace = tempfile::tempdir().unwrap();
    let mut process = spawn_fixture("healthy", workspace.path());
    let mut decoder = FrameDecoder::new(FrameLimits::default());

    send(
        process.as_mut(),
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    let initialize = read_until(process.as_mut(), &mut decoder, |message| message["id"] == 1);
    assert_eq!(
        initialize["result"]["capabilities"]["positionEncoding"],
        "utf-16"
    );
    send(
        process.as_mut(),
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    );
    send(
        process.as_mut(),
        &json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params":{"textDocument":{"uri":"file:///workspace/main.rs","languageId":"rust","version":1,"text":"a😀\r\n"}}
        }),
    );
    let diagnostics = read_until(process.as_mut(), &mut decoder, |message| {
        message["method"] == "textDocument/publishDiagnostics"
    });
    assert_eq!(
        diagnostics["params"]["diagnostics"][0]["message"],
        "fixture diagnostic"
    );

    for (id, method) in [
        (2, "textDocument/completion"),
        (3, "textDocument/hover"),
        (4, "textDocument/definition"),
    ] {
        send(
            process.as_mut(),
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"textDocument":{"uri":"file:///workspace/main.rs"},"position":{"line":0,"character":1}}}),
        );
        let response = read_until(process.as_mut(), &mut decoder, |message| {
            message["id"] == id
        });
        assert!(response.get("result").is_some());
    }

    send(
        process.as_mut(),
        &json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":99}}),
    );
    let cancellation = read_until(process.as_mut(), &mut decoder, |message| {
        message["method"] == "$/struktFixture/cancelObserved"
    });
    assert_eq!(cancellation["params"]["id"], 99);

    send(
        process.as_mut(),
        &json!({"jsonrpc":"2.0","id":5,"method":"shutdown","params":null}),
    );
    read_until(process.as_mut(), &mut decoder, |message| message["id"] == 5);
    send(
        process.as_mut(),
        &json!({"jsonrpc":"2.0","method":"exit","params":null}),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(exit) = process.try_wait().unwrap() {
            assert!(exit.success());
            break;
        }
        assert!(Instant::now() < deadline, "fixture did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn fragmented_and_delayed_modes_still_obey_framing() {
    for mode in ["fragmented", "delayed"] {
        let workspace = tempfile::tempdir().unwrap();
        let mut process = spawn_fixture(mode, workspace.path());
        let mut decoder = FrameDecoder::new(FrameLimits::default());
        send(
            process.as_mut(),
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        );
        let response = read_until(process.as_mut(), &mut decoder, |message| message["id"] == 1);
        assert!(response.get("result").is_some());
        process.terminate(Duration::from_millis(10)).unwrap();
    }
}

#[test]
fn malformed_and_oversized_modes_produce_bounded_protocol_failures() {
    let malformed_workspace = tempfile::tempdir().unwrap();
    let mut malformed = spawn_fixture("malformed", malformed_workspace.path());
    send(
        malformed.as_mut(),
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    assert!(wait_for_invalid_json(malformed.as_mut()));
    malformed.terminate(Duration::from_millis(10)).unwrap();

    let oversized_workspace = tempfile::tempdir().unwrap();
    let mut oversized = spawn_fixture("oversized", oversized_workspace.path());
    send(
        oversized.as_mut(),
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    assert_eq!(
        wait_for_frame_error(oversized.as_mut()),
        FrameError::BodyTooLarge {
            declared: 16_777_217
        }
    );
    oversized.terminate(Duration::from_millis(10)).unwrap();
}

#[test]
fn stderr_crash_and_ignored_shutdown_modes_remain_bounded() {
    let stderr_workspace = tempfile::tempdir().unwrap();
    let mut stderr = spawn_fixture("stderr-flood", stderr_workspace.path());
    send(
        stderr.as_mut(),
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    let retained = wait_for_stderr(stderr.as_mut());
    assert!(!retained.is_empty());
    assert!(retained.len() <= 1024 * 1024);
    stderr.terminate(Duration::from_millis(10)).unwrap();

    let crash_workspace = tempfile::tempdir().unwrap();
    let mut crash = spawn_fixture("crash-after-initialize", crash_workspace.path());
    send(
        crash.as_mut(),
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    let exit = wait_for_exit(crash.as_mut());
    assert_eq!(exit.code(), Some(42));
    assert!(crash.write(b"after-exit").is_err());

    let ignore_workspace = tempfile::tempdir().unwrap();
    let mut ignore = spawn_fixture("ignore-shutdown", ignore_workspace.path());
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    send(
        ignore.as_mut(),
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    read_until(ignore.as_mut(), &mut decoder, |message| message["id"] == 1);
    send(
        ignore.as_mut(),
        &json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
    );
    assert!(!has_message_with_id(
        ignore.as_mut(),
        &mut decoder,
        2,
        Duration::from_millis(100)
    ));
    ignore.terminate(Duration::from_millis(10)).unwrap();
}

fn spawn_fixture(mode: &str, workspace: &std::path::Path) -> Box<dyn LanguageProcess> {
    let command = ResolvedCommand::new(
        PathBuf::from(env!("CARGO_BIN_EXE_language-fixture")),
        vec![OsString::from(mode)],
    )
    .unwrap();
    StdioTransport
        .spawn(SpawnRequest::new(command, workspace.to_path_buf()).unwrap())
        .unwrap()
}

fn send(process: &mut dyn LanguageProcess, message: &Value) {
    let body = serde_json::to_vec(message).unwrap();
    process
        .write(&encode_frame(&body, FrameLimits::default()).unwrap())
        .unwrap();
}

fn read_until(
    process: &mut dyn LanguageProcess,
    decoder: &mut FrameDecoder,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(bytes) = process.try_read().unwrap() {
            for frame in decoder.push(&bytes).unwrap() {
                let message: Value = serde_json::from_slice(frame.body()).unwrap();
                if predicate(&message) {
                    return message;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for fixture message"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_invalid_json(process: &mut dyn LanguageProcess) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    loop {
        if let Some(bytes) = process.try_read().unwrap() {
            for frame in decoder.push(&bytes).unwrap() {
                if serde_json::from_slice::<Value>(frame.body()).is_err() {
                    return true;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not emit invalid JSON"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_frame_error(process: &mut dyn LanguageProcess) -> FrameError {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    loop {
        if let Some(bytes) = process.try_read().unwrap()
            && let Err(error) = decoder.push(&bytes)
        {
            return error;
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not emit a frame error"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_exit(process: &mut dyn LanguageProcess) -> strukt_language::ProcessExit {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(exit) = process.try_wait().unwrap() {
            return exit;
        }
        assert!(Instant::now() < deadline, "fixture did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_stderr(process: &mut dyn LanguageProcess) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(bytes) = process.try_read_stderr().unwrap() {
            return bytes;
        }
        assert!(Instant::now() < deadline, "fixture did not emit stderr");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn has_message_with_id(
    process: &mut dyn LanguageProcess,
    decoder: &mut FrameDecoder,
    id: u64,
    duration: Duration,
) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        if let Some(bytes) = process.try_read().unwrap() {
            for frame in decoder.push(&bytes).unwrap() {
                let message: Value = serde_json::from_slice(frame.body()).unwrap();
                if message["id"] == id {
                    return true;
                }
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
