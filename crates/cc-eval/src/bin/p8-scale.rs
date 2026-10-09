//! Development-only local P8 measurements. Never certifies a release.
use cc_eval::benchmark::{
    invalid,
    p8_scale::{self, CapacityProfile, ColdStudy, Profile, ScalePlan, ScaleShard, StageScope},
    Result,
};
use clap::{Parser, ValueEnum};
use std::io::Read;
use std::path::PathBuf;

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Smoke,
    Release,
}

#[derive(Clone, Copy, ValueEnum)]
enum Capacity {
    #[value(name = "scale_capacity_v1")]
    ScaleCapacityV1,
}

#[derive(Clone, Copy, ValueEnum)]
enum Stages {
    #[value(name = "cold_only_v1")]
    ColdOnlyV1,
}

#[derive(Parser)]
#[command(
    name = "p8-scale",
    about = "Bounded real-index P8 scale and fanout measurements; default is 60-file local smoke"
)]
struct Cli {
    #[arg(long, value_enum, default_value = "smoke")]
    profile: Mode,
    /// Opt into the registered 200/1024 scale capacity; separate fanout keeps 8/128.
    #[arg(long, value_enum)]
    capacity_profile: Option<Capacity>,
    /// Select a separate cold-only study; omission executes the original stages.
    #[arg(long, value_enum)]
    stage_scope: Option<Stages>,
    #[arg(long, requires_all = ["study_attempt", "stage_scope"])]
    study_run_id: Option<String>,
    #[arg(long, requires_all = ["study_run_id", "stage_scope"])]
    study_attempt: Option<u32>,
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
    /// Zero-based slice of the registered repetition population; never a full certification.
    #[arg(long, requires = "shard_count")]
    shard_index: Option<usize>,
    #[arg(long, requires = "shard_index")]
    shard_count: Option<usize>,
    /// Omit the separate fanout fixture only in an explicitly partial shard.
    #[arg(long, requires = "shard_index")]
    skip_fanout: bool,
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
    #[arg(
        long,
        required_unless_present = "hash_file",
        conflicts_with = "hash_file"
    )]
    output: Option<PathBuf>,
    /// Hash a preserved evidence file with the same BLAKE3 implementation as the runner.
    #[arg(long, hide = true, conflicts_with = "worker_plan")]
    hash_file: Option<PathBuf>,
    #[arg(long, hide = true)]
    worker_plan: Option<PathBuf>,
}

fn run(cli: Cli) -> Result<i32> {
    if let Some(path) = cli.hash_file {
        let mut input = std::fs::File::open(path)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let bytes = input.read(&mut buffer)?;
            if bytes == 0 {
                break;
            }
            hasher.update(&buffer[..bytes]);
        }
        println!("{}", hasher.finalize().to_hex());
        return Ok(0);
    }
    let output = cli.output.ok_or_else(|| invalid("output required"))?;
    if let Some(path) = cli.worker_plan {
        if std::fs::metadata(&path)?.len() > 128 * 1024 {
            return Err(invalid("worker plan exceeds bound"));
        }
        let plan: ScalePlan = serde_json::from_slice(&std::fs::read(path)?)?;
        return p8_scale::run_worker(&plan, &output);
    }
    let plan = ScalePlan {
        schema_version: 1,
        profile: match cli.profile {
            Mode::Smoke => Profile::Smoke,
            Mode::Release => Profile::Release,
        },
        capacity_profile: cli.capacity_profile.map(|profile| match profile {
            Capacity::ScaleCapacityV1 => CapacityProfile::ScaleCapacityV1,
        }),
        stage_scope: cli.stage_scope.map(|scope| match scope {
            Stages::ColdOnlyV1 => StageScope::ColdOnlyV1,
        }),
        cold_study: cli
            .study_run_id
            .zip(cli.study_attempt)
            .map(|(run_id, run_attempt)| ColdStudy {
                run_id,
                run_attempt,
            }),
        files: if cli.matrix {
            p8_scale::RELEASE_SCALES.to_vec()
        } else if cli.files.is_empty() {
            vec![60]
        } else {
            cli.files
        },
        seed: cli.seed,
        repetitions: cli.repetitions,
        shard: cli
            .shard_index
            .zip(cli.shard_count)
            .map(|(index, count)| ScaleShard { index, count }),
        skip_fanout: cli.skip_fanout,
        dirty_budget: cli.dirty_budget,
        max_resume_builds: cli.max_resume_builds,
        batch_sizes: cli.batch_sizes,
        fanouts: cli.fanouts,
        deadline_ms: cli.deadline_ms,
        max_output_bytes: cli.max_output_bytes,
    };
    let result = p8_scale::run_supervised(&plan, &output, &std::env::current_exe()?)?;
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
