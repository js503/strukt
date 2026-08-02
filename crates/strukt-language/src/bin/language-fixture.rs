#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::io::{self, Read as _, Write as _};
use std::time::Duration;

use serde_json::{Value, json};
use strukt_language::{FrameDecoder, FrameLimits, encode_frame};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureMode {
    Healthy,
    Fragmented,
    Delayed,
    Malformed,
    Oversized,
    StderrFlood,
    CrashAfterInitialize,
    IgnoreShutdown,
}

impl FixtureMode {
    fn parse(value: &OsStr) -> Option<Self> {
        match value.to_str()? {
            "healthy" => Some(Self::Healthy),
            "fragmented" => Some(Self::Fragmented),
            "delayed" => Some(Self::Delayed),
            "malformed" => Some(Self::Malformed),
            "oversized" => Some(Self::Oversized),
            "stderr-flood" => Some(Self::StderrFlood),
            "crash-after-initialize" => Some(Self::CrashAfterInitialize),
            "ignore-shutdown" => Some(Self::IgnoreShutdown),
            _ => None,
        }
    }
}

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments
        .next()
        .as_deref()
        .and_then(FixtureMode::parse)
        .filter(|_| arguments.next().is_none());
    let Some(mode) = mode else {
        eprintln!("expected exactly one fixture mode");
        std::process::exit(2);
    };
    match run(mode) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("fixture failed: {error}");
            std::process::exit(3);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run(mode: FixtureMode) -> io::Result<i32> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    let mut chunk = [0; 4096];
    let mut shutdown_seen = false;
    let mut failure_emitted = false;

    loop {
        let read = input.read(&mut chunk)?;
        if read == 0 {
            return Ok(i32::from(!shutdown_seen));
        }
        let frames = decoder.push(&chunk[..read]).map_err(io::Error::other)?;
        for frame in frames {
            let message: Value = serde_json::from_slice(frame.body()).map_err(io::Error::other)?;
            let method = message.get("method").and_then(Value::as_str);
            let id = message.get("id").cloned();
            if mode == FixtureMode::Delayed && id.is_some() {
                std::thread::sleep(Duration::from_millis(75));
            }
            match method {
                Some("initialize") => {
                    write_json(
                        &mut output,
                        mode,
                        &json!({
                            "jsonrpc":"2.0",
                            "id":id,
                            "result":{"capabilities":{
                                "positionEncoding":"utf-16",
                                "textDocumentSync":1,
                                "completionProvider":{},
                                "hoverProvider":true,
                                "definitionProvider":true
                            }}
                        }),
                    )?;
                    if !failure_emitted {
                        emit_mode_failure(mode, &mut output)?;
                        failure_emitted = true;
                    }
                    if mode == FixtureMode::CrashAfterInitialize {
                        return Ok(42);
                    }
                }
                Some("initialized") => {}
                Some("textDocument/didOpen" | "textDocument/didChange") => {
                    write_json(
                        &mut output,
                        mode,
                        &json!({
                            "jsonrpc":"2.0",
                            "method":"textDocument/publishDiagnostics",
                            "params":{
                                "uri":"file:///workspace/main.rs",
                                "version":1,
                                "diagnostics":[{
                                    "range":{"start":{"line":0,"character":1},"end":{"line":0,"character":3}},
                                    "severity":1,
                                    "source":"strukt-fixture",
                                    "code":"fixture",
                                    "message":"fixture diagnostic"
                                }]
                            }
                        }),
                    )?;
                }
                Some("textDocument/completion") => write_json(
                    &mut output,
                    mode,
                    &json!({"jsonrpc":"2.0","id":id,"result":{"isIncomplete":false,"items":[{"label":"fixtureCompletion","insertText":"fixture"}]}}),
                )?,
                Some("textDocument/hover") => write_json(
                    &mut output,
                    mode,
                    &json!({"jsonrpc":"2.0","id":id,"result":{"contents":{"kind":"markdown","value":"**fixture hover**"}}}),
                )?,
                Some("textDocument/definition") => write_json(
                    &mut output,
                    mode,
                    &json!({"jsonrpc":"2.0","id":id,"result":[{"uri":"file:///workspace/definition.rs","range":{"start":{"line":1,"character":0},"end":{"line":1,"character":4}}}]}),
                )?,
                Some("$/cancelRequest") => write_json(
                    &mut output,
                    mode,
                    &json!({"jsonrpc":"2.0","method":"$/struktFixture/cancelObserved","params":message["params"].clone()}),
                )?,
                Some("shutdown") => {
                    shutdown_seen = true;
                    if mode != FixtureMode::IgnoreShutdown {
                        write_json(
                            &mut output,
                            mode,
                            &json!({"jsonrpc":"2.0","id":id,"result":null}),
                        )?;
                    }
                }
                Some("exit") => return Ok(i32::from(!shutdown_seen)),
                Some(_) if id.is_some() => write_json(
                    &mut output,
                    mode,
                    &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
                )?,
                Some(_) | None => {}
            }
        }
    }
}

fn emit_mode_failure(mode: FixtureMode, output: &mut impl io::Write) -> io::Result<()> {
    match mode {
        FixtureMode::Malformed => {
            output.write_all(b"Content-Length: 1\r\n\r\n{")?;
            output.flush()
        }
        FixtureMode::Oversized => {
            output.write_all(b"Content-Length: 16777217\r\n\r\n")?;
            output.flush()
        }
        FixtureMode::StderrFlood => {
            let mut stderr = io::stderr().lock();
            let chunk = vec![b'e'; 64 * 1024];
            for _ in 0..32 {
                stderr.write_all(&chunk)?;
            }
            stderr.flush()
        }
        FixtureMode::Healthy
        | FixtureMode::Fragmented
        | FixtureMode::Delayed
        | FixtureMode::CrashAfterInitialize
        | FixtureMode::IgnoreShutdown => Ok(()),
    }
}

fn write_json(output: &mut impl io::Write, mode: FixtureMode, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    let frame = encode_frame(&body, FrameLimits::default()).map_err(io::Error::other)?;
    if mode == FixtureMode::Fragmented {
        let split = frame.len() / 2;
        output.write_all(&frame[..split])?;
        output.flush()?;
        output.write_all(&frame[split..])?;
    } else {
        output.write_all(&frame)?;
    }
    output.flush()
}
