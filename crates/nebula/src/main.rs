fn main() {
    if let Err(err) = nebula::run_cli() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
