#![forbid(unsafe_code)]

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let initialize = read_message(&mut reader);
    let id = initialize
        .get("id")
        .cloned()
        .unwrap_or(serde_json::json!(1));
    write_message(
        &mut writer,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"capabilities": {"textDocumentSync": 1}}
        }),
    );
    let _initialized = read_message(&mut reader);
    let opened = read_message(&mut reader);
    let uri = opened
        .pointer("/params/textDocument/uri")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    write_message(
        &mut writer,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": uri,
                "diagnostics": [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}
                    },
                    "severity": 2,
                    "message": "fixture diagnostic"
                }]
            }
        }),
    );
}

fn read_message(reader: &mut impl BufRead) -> serde_json::Value {
    let mut length = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read LSP header");
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>().expect("content length"));
        }
    }
    let mut body = vec![0; length.expect("Content-Length header")];
    reader.read_exact(&mut body).expect("read LSP body");
    serde_json::from_slice(&body).expect("parse LSP JSON")
}

fn write_message(writer: &mut impl Write, message: &serde_json::Value) {
    let body = serde_json::to_vec(message).expect("serialize LSP JSON");
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).expect("write LSP header");
    writer.write_all(&body).expect("write LSP body");
    writer.flush().expect("flush LSP response");
}
