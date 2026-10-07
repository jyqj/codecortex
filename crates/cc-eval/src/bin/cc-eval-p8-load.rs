//! Local-only P8 mixed-load driver; the product CLI remains unchanged.
use cc_eval::benchmark::{
    self as b, manifest,
    p8_load::{self, LoadConfig},
};
use clap::{Parser, Subcommand};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Parser)]
#[command(
    about = "Bounded local mixed-load/soak evidence; no release certification or external model calls"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Run a bounded synthetic workload with an OS-process deadline supervisor.
    Run {
        #[arg(long)]
        output: PathBuf,
        /// Optional strict JSON configuration; print defaults with `config`.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Print the bounded smoke configuration as JSON.
    Config,
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        output: PathBuf,
    },
}
fn execute(command: Command) -> b::Result<i32> {
    match command {
        Command::Config => {
            println!("{}", serde_json::to_string_pretty(&LoadConfig::default())?);
            Ok(0)
        }
        Command::Worker { output } => p8_load::worker(&output),
        Command::Run { output, config } => {
            let config: LoadConfig = config
                .as_deref()
                .map(manifest::json_file)
                .transpose()?
                .unwrap_or_default();
            config.validate()?;
            let executable = std::env::current_exe()?;
            let cancelled = Arc::new(AtomicBool::new(false));
            let signal = cancelled.clone();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;
            let result = runtime.block_on(async move {
                let mut task = tokio::task::spawn_blocking(move || {
                    p8_load::supervise(&config, &output, &executable, cancelled)
                });
                tokio::select! {
                    result = &mut task => result.map_err(|e| b::invalid(e.to_string()))?,
                    result = tokio::signal::ctrl_c() => {
                        result?;
                        signal.store(true, Ordering::Release);
                        task.await.map_err(|e| b::invalid(e.to_string()))?
                    }
                }
            })?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(result["exit_code"].as_i64().unwrap_or(2) as i32)
        }
    }
}
fn main() -> std::process::ExitCode {
    let code = match execute(Cli::parse().command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::ExitCode::from(code as u8)
}
