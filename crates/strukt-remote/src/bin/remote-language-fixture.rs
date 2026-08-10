#![forbid(unsafe_code)]

use std::io::{Read as _, Write as _};

fn main() {
    let mut buffer = [0_u8; 4096];
    loop {
        let read = std::io::stdin().read(&mut buffer).expect("fixture input");
        if read == 0 {
            break;
        }
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&buffer[..read]).expect("fixture output");
        stdout.flush().expect("fixture flush");
    }
}
