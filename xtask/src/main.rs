fn main() {
    if let Err(err) = xtask::run(std::env::args_os()) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
