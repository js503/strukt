use std::io::{BufRead, Write};
use std::time::Duration;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("echo") => echo_one_line(),
        Some("echo-resize") => echo_then_wait_for_completion(),
        Some("wait") => wait_until_terminated(),
        Some("burst") => write_bounded_burst(),
        Some("stress") => write_stress_stream(),
        _ => std::process::exit(64),
    }
}

fn echo_one_line() {
    echo_line();
}

fn echo_then_wait_for_completion() {
    echo_line();
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut completion = String::new();
        if stdin.read_line(&mut completion).unwrap() == 0 || !completion.trim().is_empty() {
            break;
        }
    }
}

fn echo_line() {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let line = line.trim_end_matches(['\r', '\n']);
    println!("fixture-echo:{line}\x1b[31m!\x1b[0m");
}

fn wait_until_terminated() {
    println!("fixture-ready");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn write_bounded_burst() {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&vec![b'x'; 200_000]).unwrap();
    stdout.write_all(b"\nfixture-burst-complete\n").unwrap();
    stdout.flush().unwrap();
}

fn write_stress_stream() {
    const TOTAL_BYTES: usize = 64 * 1024 * 1024;
    const CHUNK_BYTES: usize = 64 * 1024;

    let mut stdout = std::io::stdout().lock();
    // The smoke consumes this process directly through the bounded transport,
    // so visible bytes exercise real ConPTY throughput without parser or
    // scrollback cost. Repeated cursor controls are coalesced by ConPTY and
    // therefore cannot serve as a portable byte-counting payload.
    let chunk = vec![b'x'; CHUNK_BYTES];
    for _ in 0..(TOTAL_BYTES / CHUNK_BYTES) {
        stdout.write_all(&chunk).unwrap();
    }
    stdout.write_all(b"\nfixture-stress-complete\n").unwrap();
    stdout.flush().unwrap();
}
