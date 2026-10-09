use clap::Parser;

mod cli;
mod installer;

fn main() {
    let _diagnostics =
        if std::env::var_os("CODECORTEX_RUNTIME_DIAGNOSTICS").is_some_and(|value| value == "1") {
            Some(cc_server::runtime_diagnostics::install())
        } else {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
            None
        };

    let args = cli::Cli::parse();
    if let Err(e) = cli::run(args) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
