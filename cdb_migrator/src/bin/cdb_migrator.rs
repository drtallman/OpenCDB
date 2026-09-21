fn main() {
    let code = cdb_migrator::cli::run_os(
        std::env::args_os().skip(1),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(code);
}
