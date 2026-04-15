fn main() {
    if let Err(err) = auric_app::run_cli() {
        eprintln!("auric error: {err:#}");
        std::process::exit(1);
    }
}
