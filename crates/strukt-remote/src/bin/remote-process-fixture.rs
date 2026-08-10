#![forbid(unsafe_code)]

use std::io::{BufRead as _, Write as _};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--oneshot") {
        println!("fixture:oneshot");
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("fixture input");
        writeln!(stdout, "fixture:{line}").expect("fixture output");
        stdout.flush().expect("fixture flush");
        if line == "exit" {
            break;
        }
    }
}
