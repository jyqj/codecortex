//! Replay the real lifecycle driver's retained observations with the existing
//! normalizer, source verifier, statistics and accounting owners.
use cc_eval::benchmark::{
    invalid, normalizer,
    sampler::{self, CostReceipt, DiskPartition, Resources},
    schema::ResultStatus,
    statistics::{self, LatencySample, LatencyStratum},
    Result,
};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

#[derive(Parser)]
#[command(
    about = "Replay observed P8 lifecycle/cache/resource measurements; never certifies a release"
)]
struct Cli {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawQuery {
    sample_index: usize,
    source_root: PathBuf,
    expected_path: String,
    expected_symbol: String,
    response: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema_version: u32,
    profile: String,
    expected_samples: usize,
    samples: Vec<LatencySample>,
    raw_queries: Vec<RawQuery>,
    resources: Vec<Resources>,
    disk_partitions: Vec<DiskPartition>,
    disk_layout_complete: bool,
    costs: Vec<CostReceipt>,
}

fn replay(input: &mut Input) -> Result<Value> {
    if input.schema_version != 1 || !["smoke", "release"].contains(&input.profile.as_str()) {
        return Err(invalid("unsupported lifecycle measurement input"));
    }
    if input.expected_samples == 0 || input.expected_samples > 3200 || input.samples.len() > 3200 {
        return Err(invalid(
            "lifecycle measurement must have a bounded nonzero plan",
        ));
    }
    if input.profile == "release" && cfg!(debug_assertions) {
        return Err(invalid(
            "release measurement replay requires an optimized driver build",
        ));
    }
    let mut validated = std::collections::BTreeSet::new();
    let mut source_verified_queries = 0;
    for query in &input.raw_queries {
        if !validated.insert(query.sample_index) {
            return Err(invalid("duplicate lifecycle sample source witness"));
        }
        let sample = input
            .samples
            .get_mut(query.sample_index)
            .ok_or_else(|| invalid("raw query sample index is out of range"))?;
        if sample.evidence.classify() == LatencyStratum::ColdBuild {
            return Err(invalid("query witness attached to a build sample"));
        }
        let (mut hits, status) = normalizer::mcp(&query.response)?;
        for hit in &mut hits {
            normalizer::verify_source(hit, &query.source_root)?;
        }
        let declaration = format!("def {}(", query.expected_symbol);
        let expected = hits.iter().any(|hit| {
            hit.path == query.expected_path
                && hit.evidence_valid == Some(true)
                && (hit.symbol_name.as_deref() == Some(query.expected_symbol.as_str())
                    || hit.text.as_deref().is_some_and(|text| {
                        text.lines()
                            .any(|line| line.trim_start().starts_with(&declaration))
                    }))
        });
        if expected {
            source_verified_queries += 1;
        }
        // Source verification cannot erase a failed lifecycle/cache/control
        // observation already recorded for this attempt.
        if !expected {
            sample.status = ResultStatus::ToolError;
        } else if matches!(sample.status, ResultStatus::Success | ResultStatus::NoMatch) {
            sample.status = status;
        }
    }
    // A successful query must have been normalized and source-verified from
    // its retained actual payload. A supplied success label is not evidence.
    for (index, sample) in input.samples.iter().enumerate() {
        if sample.evidence.classify() != LatencyStratum::ColdBuild
            && matches!(sample.status, ResultStatus::Success | ResultStatus::NoMatch)
            && !validated.contains(&index)
        {
            return Err(invalid(
                "successful query is missing its raw/source witness",
            ));
        }
    }
    let layers = statistics::latency_layers(&input.samples, input.expected_samples);
    if input.profile == "release" {
        for layer in &layers.layers {
            let minimum = match layer.stratum {
                LatencyStratum::ColdBuild => 30,
                LatencyStratum::ProcessReopen
                | LatencyStratum::WarmUncached
                | LatencyStratum::CacheHit => 200,
                LatencyStratum::Unknown => 0,
            };
            if layer.samples < minimum {
                return Err(invalid(
                    "release lifecycle strata do not meet the retained sampling floor",
                ));
            }
        }
    }
    let complete = layers.missing_samples == 0
        && layers.unexpected_samples == 0
        && input.samples.iter().all(|sample| {
            sample.status == ResultStatus::Success
                && sample.elapsed_us.is_some()
                && sample.evidence.classify() != LatencyStratum::Unknown
        });
    Ok(json!({"schema_version":1,
        "measurement_status":if complete {"complete_observation"} else {"incomplete_or_failed_observation"},
        "exit_code":if complete {0} else {1},
        "release_certified":false,"performance_improvement_claim":false,
        "profile":input.profile,"latency_layers":layers,
        "memory_ledger":sampler::memory_ledger(&input.resources),
        "disk_ledger":sampler::disk_ledger(&input.disk_partitions,input.disk_layout_complete)?,
        "cost_ledger":sampler::cost_ledger(&input.costs)?,
        "retained_query_witnesses":validated.len(),
        "source_verified_queries":source_verified_queries,
        "statistical_scope":"empirical per-lifecycle distributions; existing IID interval assumption; no baseline ratio or stable p99 claim from sample count alone"}))
}

fn run(cli: Cli) -> Result<i32> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&cli.output)?;
    let result = (|| {
        if std::fs::metadata(&cli.input)?.len() > 128 * 1024 * 1024 {
            return Err(invalid("lifecycle input exceeds 128 MiB"));
        }
        let bytes = std::fs::read(&cli.input)?;
        let mut input: Input = serde_json::from_slice(&bytes)?;
        let mut result = replay(&mut input)?;
        result["input_sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
        result["replay_binary_sha256"] = json!(format!(
            "{:x}",
            Sha256::digest(std::fs::read(std::env::current_exe()?)?)
        ));
        Ok(result)
    })();
    let value = match result {
        Ok(value) => value,
        Err(error) => json!({"schema_version":1,"measurement_status":"invalid_measurement",
                            "exit_code":2,"release_certified":false,"error":error.to_string()}),
    };
    serde_json::to_writer_pretty(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    println!(
        "{}",
        serde_json::to_string(&json!({"exit_code":value["exit_code"],
        "measurement_status":value["measurement_status"],"output":cli.output}))?
    );
    Ok(value["exit_code"].as_i64().unwrap_or(2) as i32)
}

fn main() -> std::process::ExitCode {
    let code = match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::ExitCode::from(code as u8)
}
