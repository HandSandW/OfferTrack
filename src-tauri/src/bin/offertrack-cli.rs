// Keep a console subsystem: JSON stdout must also work in Windows release builds.
fn main() {
    std::process::exit(offertrack_lib::agent_cli::run(
        std::env::args_os().skip(1),
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    ));
}
