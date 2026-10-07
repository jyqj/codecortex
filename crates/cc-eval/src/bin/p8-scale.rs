//! Development-only local P8 measurements. Never certifies a release.
use cc_eval::benchmark::{
    invalid,
    p8_scale::{self, Profile, ScalePlan},
    Result,
};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Smoke,
    Release,
}

#[derive(Parser)]
#[command(
    name = "p8-scale",
    about = "Bounded real-index P8 scale and fanout measurements; default is 60-file local smoke"
)]
struct Cli {
    #[arg(long, value_enum, default_value = "smoke")]
    profile: Mode,
    /// Comma-separated exact corpus sizes; 60 is the safe default.
    #[arg(long, value_delimiter = ',', conflicts_with = "matrix")]
    files: Vec<usize>,
    /// Explicitly select 1k,5k,10k,50k,100k; does not certify them.
    #[arg(long)]
    matrix: bool,
    #[arg(long, default_value_t = 0x00c0_ffee)]
    seed: u64,
    #[arg(long, default_value_t = 1)]
    repetitions: usize,
    #[arg(long, default_value_t = 2)]
    dirty_budget: usize,
    #[arg(long, default_value_t = 64)]
    max_resume_builds: usize,
    #[arg(long, value_delimiter = ',', default_value = "1,10")]
    batch_sizes: Vec<usize>,
    #[arg(long, value_delimiter = ',', default_value = "2,8")]
    fanouts: Vec<usize>,
    #[arg(long, default_value_t = 120_000)]
    deadline_ms: u64,
    #[arg(long,default_value_t=16*1024*1024)]
    max_output_bytes: u64,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, hide = true)]
    worker_plan: Option<PathBuf>,
}

fn run(cli: Cli) -> Result<i32> {
    if let Some(path) = cli.worker_plan {
        if std::fs::metadata(&path)?.len() > 128 * 1024 {
            return Err(invalid("worker plan exceeds bound"));
        }
        let plan: ScalePlan = serde_json::from_slice(&std::fs::read(path)?)?;
        return p8_scale::run_worker(&plan, &cli.output);
    }
    let plan = ScalePlan {
        schema_version: 1,
        profile: match cli.profile {
            Mode::Smoke => Profile::Smoke,
            Mode::Release => Profile::Release,
        },
        files: if cli.matrix {
            p8_scale::RELEASE_SCALES.to_vec()
        } else if cli.files.is_empty() {
            vec![60]
        } else {
            cli.files
        },
        seed: cli.seed,
        repetitions: cli.repetitions,
        dirty_budget: cli.dirty_budget,
        max_resume_builds: cli.max_resume_builds,
        batch_sizes: cli.batch_sizes,
        fanouts: cli.fanouts,
        deadline_ms: cli.deadline_ms,
        max_output_bytes: cli.max_output_bytes,
    };
    let result = p8_scale::run_supervised(&plan, &cli.output, &std::env::current_exe()?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(result["exit_code"].as_i64().unwrap_or(2) as i32)
}
fn main() {
    let exit = match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::exit(exit);
}
