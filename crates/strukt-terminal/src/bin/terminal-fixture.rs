use std::io::{BufRead, Write};
use std::time::Duration;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("echo") => echo_one_line(),
        Some("wait") => wait_until_terminated(),
        Some("burst") => write_bounded_burst(),
        _ => std::process::exit(64),
    }
}

fn echo_one_line() {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let line = line.trim_end_matches(['\r', '\n']);
    println!("fixture-echo:{line}");
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
