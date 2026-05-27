use clap::Parser;

// Binary-only modules (not part of the library)
mod cli;
mod installer;
mod mcp;
mod watcher;

// Re-export library modules so existing `use crate::` paths in binary-only
// modules continue to work.
pub use cc_server::engine;
pub use cc_server::graph_cycles;
pub use cc_server::graph_flow;
pub use cc_server::graph_trace;
pub use cc_server::graph_type_hierarchy;
pub use cc_server::graph_types;
pub use cc_server::handlers;
pub use cc_server::impact;
pub use cc_server::path_guard;
pub use cc_server::symbol_extract;
pub use cc_server::tools;

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
