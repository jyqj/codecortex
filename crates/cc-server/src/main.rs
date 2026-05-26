use clap::Parser;

mod cli;
mod engine;
mod handlers;
mod impact;
mod installer;
mod mcp;
pub mod path_guard;
pub mod symbol_extract;
mod tools;
mod watcher;

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
