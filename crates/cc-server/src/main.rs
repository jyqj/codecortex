use clap::Parser;

mod cli;
mod installer;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let args = cli::Cli::parse();
    if let Err(e) = cli::run(args) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
