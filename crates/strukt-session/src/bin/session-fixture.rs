use std::io::{BufRead, Write};

fn main() {
    if std::env::args().nth(1).as_deref() != Some("hold") {
        std::process::exit(64);
    }
    println!("fixture-ready");
    std::io::stdout().flush().expect("flush fixture ready");
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        println!("fixture:{}", line.trim_end_matches('\r'));
        std::io::stdout().flush().expect("flush fixture output");
    }
}
