#![forbid(unsafe_code)]

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() != ["--stdio"] {
        eprintln!("usage: strukt-remote --stdio");
        std::process::exit(2);
    }
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    if let Err(error) = strukt_remote::run_helper_stdio(&mut stdin, &mut stdout) {
        eprintln!("strukt-remote failed: {error}");
        std::process::exit(1);
    }
}
