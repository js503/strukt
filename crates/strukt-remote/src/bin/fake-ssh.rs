#![forbid(unsafe_code)]

use std::io::Read as _;

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args == ["-V"] {
        eprintln!("OpenSSH_fake_1.0");
        return;
    }
    if args.first().is_some_and(|argument| argument == "-G") {
        println!("hostname fixture.invalid\nuser fixture\nport 22");
        return;
    }
    if args.last().is_some_and(|argument| argument == "true") {
        return;
    }
    if args
        .last()
        .is_some_and(|argument| argument.to_string_lossy().contains("umask 077"))
    {
        let mut artifact = Vec::new();
        std::io::stdin()
            .read_to_end(&mut artifact)
            .expect("fake installer input");
        if artifact.is_empty() {
            eprintln!("fake-ssh: empty install artifact");
            std::process::exit(1);
        }
        println!("installed {} bytes", artifact.len());
        return;
    }
    let helper_request = args.len() >= 2
        && args.last().is_some_and(|argument| argument == "--stdio")
        && args[args.len() - 2]
            .to_string_lossy()
            .ends_with("/strukt-remote");
    if !helper_request {
        eprintln!("fake-ssh: unsupported command");
        std::process::exit(2);
    }
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    if let Err(error) = strukt_remote::run_helper_stdio(&mut stdin, &mut stdout) {
        eprintln!("fake-ssh: helper failed: {error}");
        std::process::exit(1);
    }
}
