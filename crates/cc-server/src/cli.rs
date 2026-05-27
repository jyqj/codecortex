use cc_model::{CcError, CcResult};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "codecortex",
    version,
    about = "Code indexing and code graph search"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start MCP stdio server
    #[command(name = "mcp", alias = "serve")]
    Mcp {
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// Install CodeCortex MCP configuration for detected AI agents
    #[command(name = "install")]
    Install {
        #[arg(long)]
        force: bool,
    },
    /// Remove CodeCortex MCP configuration from all detected AI agents
    #[command(name = "uninstall")]
    Uninstall,
}

pub fn run(cli: Cli) -> CcResult<()> {
    match cli.command {
        Command::Mcp { project_path } => cmd_serve(project_path),
        Command::Install { force } => cmd_install(force),
        Command::Uninstall => cmd_uninstall(),
    }
}

fn cmd_serve(project_path: Option<PathBuf>) -> CcResult<()> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| CcError::Other(format!("tokio runtime: {}", e)))?;
    rt.block_on(crate::mcp::run_mcp_server(project_path))
}

fn cmd_install(force: bool) -> CcResult<()> {
    let binary_path = std::env::current_exe().unwrap_or_default();
    let report = crate::installer::install_all(&binary_path, force);
    for agent in &report.agents_configured {
        println!("  Configured {}", agent);
    }
    for hook in &report.hooks_installed {
        println!("  Installed hook: {}", hook);
    }
    for err in &report.errors {
        eprintln!("  Error: {}", err);
    }
    println!("\n{} agent(s) configured.", report.agents_configured.len());
    Ok(())
}

fn cmd_uninstall() -> CcResult<()> {
    let report = crate::installer::uninstall_all();
    for agent in &report.agents_configured {
        println!("  Removed from {}", agent);
    }
    for err in &report.errors {
        eprintln!("  Error: {}", err);
    }
    println!(
        "\nRemoved from {} agent(s).",
        report.agents_configured.len()
    );
    Ok(())
}
