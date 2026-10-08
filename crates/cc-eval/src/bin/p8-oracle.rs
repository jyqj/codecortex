//! Read-only executable adapter for the existing complete diagnostic oracle.
//! Neither side is built or repaired before comparing the persisted facts.
use cc_eval::benchmark::{manifest, oracle};
use clap::Parser;
use serde_json::json;
use std::{fs::OpenOptions, io::Write, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    left: PathBuf,
    #[arg(long)]
    right: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

fn run(args: Args) -> Result<i32, Box<dyn std::error::Error>> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)?;
    let started = std::time::Instant::now();
    let result =
        oracle::compare_streaming(&args.left, &args.right, oracle::StreamingLimits::default());
    let (comparison, error) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let equal = comparison.as_ref().and_then(|v| v["equal"].as_bool()) == Some(true);
    let exit_code = if error.is_some() {
        2
    } else if equal {
        0
    } else {
        1
    };
    let report = json!({
        "schema_version": 1,
        "scope": "complete_existing_15_table_diagnostic_oracle_no_repair",
        "binary_digest": manifest::file_digest(&std::env::current_exe()?)?,
        "left": args.left.canonicalize()?, "right": args.right.canonicalize()?,
        "tables": oracle::tables(), "comparison": comparison, "error": error,
        "elapsed_ns": started.elapsed().as_nanos(), "exit_code": exit_code,
    });
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    Ok(exit_code)
}

fn main() {
    let code = match run(Args::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::exit(code);
}
