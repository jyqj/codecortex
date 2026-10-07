//! Development-only benchmark CLI. The production codecortex CLI is unchanged.
use cc_eval::benchmark::{self as b, invalid, manifest, report, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Parser)]
#[command(
    name = "cc-eval",
    version,
    about = "Locked, public-protocol code retrieval benchmarks"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Clone, Copy, ValueEnum)]
enum BackendName {
    McpStdio,
    Rg,
    #[cfg(feature = "eval-http")]
    OceHttp,
}
#[derive(Subcommand)]
enum Command {
    /// Replay a self-contained parser/SQLite mutation case; automatically reduce failures.
    MutationCase {
        #[arg(long)]
        case: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 32)]
        shrink_attempts: usize,
    },
    /// Run a source-verified, isolated factorial matrix of existing local binaries.
    Ablate {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Export JSON Schema from the same Rust types used by the runner.
    Schema {
        #[arg(long)]
        output: PathBuf,
    },
    /// Authoring only: refresh explicitly selected lock, never done implicitly by run.
    Freeze {
        #[arg(long)]
        suite: PathBuf,
    },
    /// Repeat --suite for several independent locked corpora.
    Validate {
        #[arg(long, required = true)]
        suite: Vec<PathBuf>,
    },
    Run {
        #[arg(long, required = true)]
        suite: Vec<PathBuf>,
        #[arg(long, value_enum, default_value = "mcp-stdio")]
        backend: BackendName,
        #[arg(long)]
        binary: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "smoke")]
        profile: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long, default_value = "OCE_API_KEY")]
        api_key_env: String,
        #[arg(long)]
        allow_external: bool,
    },
    Replay {
        #[arg(long)]
        run: PathBuf,
    },
    Compare {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        gate: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    ImportOce {
        #[arg(long)]
        queries: PathBuf,
        #[arg(long)]
        metadata: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Mutate {
        #[arg(long)]
        suite: PathBuf,
        #[arg(long)]
        mutation: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}
fn new_output(path: &Path) -> Result<()> {
    if path.exists() {
        return Err(invalid("output already exists; raw runs are immutable"));
    }
    if let Some(p) = path.parent() {
        if !p.as_os_str().is_empty() {
            std::fs::create_dir_all(p)?;
        }
    }
    std::fs::create_dir(path)?;
    Ok(())
}
// CLI composition boundary: options remain explicit rather than process globals.
#[allow(clippy::too_many_arguments)]
async fn run_one(
    path: &Path,
    backend: BackendName,
    binary: Option<&Path>,
    out: &Path,
    profile: &str,
    endpoint: Option<&str>,
    key_env: &str,
    allow_external: bool,
) -> Result<i32> {
    let loaded = manifest::load(path)?;
    let isolated = manifest::materialize(&loaded)?;
    if profile == "performance"
        && (cfg!(debug_assertions)
            || loaded
                .suite
                .repetitions
                .saturating_mul(loaded.queries.len())
                < 200
            || loaded.suite.warmup == 0)
    {
        return Err(invalid("performance profile requires release eval binary, warmup, and at least 200 samples; product build receipt must also be checked"));
    }
    let timeout = Duration::from_millis(loaded.suite.timeout_ms);
    new_output(out)?;
    let engine = b::runner::engine_provenance(binary)?;
    let result = match backend {
        BackendName::McpStdio => {
            let bin = binary.ok_or_else(|| invalid("--binary required for mcp-stdio"))?;
            match b::adapters::mcp_stdio::McpStdio::spawn(bin, isolated.path(), timeout).await {
                Ok(client) => {
                    b::runner::run(&loaded, client, isolated.path(), out, engine, profile).await
                }
                Err(e) => Err(e),
            }
        }
        BackendName::Rg => {
            let client = b::adapters::rg::Ripgrep::new(isolated.path().to_path_buf(), timeout);
            b::runner::run(&loaded, client, isolated.path(), out, engine, profile).await
        }
        #[cfg(feature = "eval-http")]
        BackendName::OceHttp => {
            let ep = endpoint.ok_or_else(|| invalid("--endpoint required for oce-http"))?;
            let key = std::env::var(key_env)
                .map_err(|_| invalid("API credential environment variable missing"))?;
            let client = b::adapters::oce_http::OceHttp::new(
                ep,
                key,
                isolated.path().to_path_buf(),
                timeout,
                allow_external,
            )?;
            b::runner::run(&loaded, client, isolated.path(), out, engine, profile).await
        }
    };
    #[cfg(not(feature = "eval-http"))]
    let _ = (endpoint, key_env, allow_external);
    match result {
        Ok(g) => {
            println!("{}: {} (exit {})", loaded.suite.name, g.status, g.exit_code);
            Ok(g.exit_code)
        }
        Err(e) => {
            report::json(
                &out.join("failure.json"),
                &json!({"status":"invalid_measurement","error":e.to_string()}),
            )?;
            report::json(
                &out.join("gate.json"),
                &json!({"status":"invalid_measurement","exit_code":2,"reasons":[e.to_string()]}),
            )?;
            Err(e)
        }
    }
}
async fn execute(command: Command) -> Result<i32> {
    match command {
        Command::MutationCase {
            case,
            output,
            shrink_attempts,
        } => {
            if std::fs::metadata(&case)?.len() > 8 * 1024 * 1024 {
                return Err(invalid("mutation case file too large"));
            }
            let source = std::fs::read(&case)?;
            if source.len() > 8 * 1024 * 1024 {
                return Err(invalid("mutation case file too large"));
            }
            let case: b::mutation_case::MutationCase = serde_json::from_slice(&source)?;
            let result = b::mutation_case::run(&case, &output, shrink_attempts)?;
            Ok(if result["passed"] == true { 0 } else { 1 })
        }
        Command::Ablate { plan, output } => b::ablation::run(&plan, &output).await,
        Command::Schema { output } => {
            new_output(&output)?;
            report::json(
                &output.join("suite.schema.json"),
                &schemars::schema_for!(b::schema::Suite),
            )?;
            report::json(
                &output.join("query.schema.json"),
                &schemars::schema_for!(b::schema::Query),
            )?;
            report::json(
                &output.join("result.schema.json"),
                &schemars::schema_for!(b::schema::Row),
            )?;
            Ok(0)
        }
        Command::Freeze { suite } => {
            let s = manifest::freeze(&suite)?;
            report::json(&suite, &s)?;
            println!("Explicitly refreshed {}", suite.display());
            Ok(0)
        }
        Command::Validate { suite } => {
            let mut exit = 0;
            for path in suite {
                match manifest::load(&path) {
                    Ok(l) => println!(
                        "{}: {} queries, {} admitted files, locks valid",
                        l.suite.name,
                        l.queries.len(),
                        l.input.files.len()
                    ),
                    Err(e) => {
                        eprintln!("{}: {e}", path.display());
                        exit = 2;
                    }
                }
            }
            Ok(exit)
        }
        Command::Run {
            suite,
            backend,
            binary,
            output,
            profile,
            endpoint,
            api_key_env,
            allow_external,
        } => {
            if !["smoke", "pr", "quality", "performance"].contains(&profile.as_str()) {
                return Err(invalid("unknown measurement profile"));
            }
            if suite.len() == 1 {
                return run_one(
                    &suite[0],
                    backend,
                    binary.as_deref(),
                    &output,
                    &profile,
                    endpoint.as_deref(),
                    &api_key_env,
                    allow_external,
                )
                .await;
            }
            new_output(&output)?;
            let mut results = Vec::new();
            let mut exit = 0;
            for (i, path) in suite.iter().enumerate() {
                let dest = output.join(format!("suite-{i:03}"));
                match run_one(
                    path,
                    backend,
                    binary.as_deref(),
                    &dest,
                    &profile,
                    endpoint.as_deref(),
                    &api_key_env,
                    allow_external,
                )
                .await
                {
                    Ok(code) => {
                        exit = exit.max(code);
                        results.push(json!({"suite":path,"output":dest,"exit_code":code}));
                    }
                    Err(e) => {
                        exit = 2;
                        results.push(json!({"suite":path,"exit_code":2,"error":e.to_string()}));
                    }
                }
            }
            report::json(
                &output.join("suite.json"),
                &json!({"results":results,"exit_code":exit}),
            )?;
            Ok(exit)
        }
        Command::Replay { run } => {
            let g = report::replay(&run)?;
            println!("{}", g.status);
            Ok(g.exit_code)
        }
        Command::Compare {
            baseline,
            candidate,
            gate,
            output,
        } => {
            // Reserve the destination before reading/replaying inputs. This
            // also rejects dangling symlinks and concurrent writers without
            // the old exists-then-truncate race. Failed comparisons retain
            // an explicit receipt while original raw input remains untouched.
            let mut destination = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)?;
            let result = manifest::json_file(&gate)
                .and_then(|policy| b::comparison::compare(&baseline, &candidate, &policy));
            let value = match result {
                Ok(value) => value,
                Err(error) => json!({
                    "schema_version": 1,
                    "status": "invalid_measurement",
                    "exit_code": 2,
                    "reasons": [error.to_string()],
                    "baseline": baseline,
                    "candidate": candidate,
                    "policy_path": gate,
                }),
            };
            serde_json::to_writer_pretty(&mut destination, &value)?;
            destination.flush()?;
            println!("{}", value["status"]);
            Ok(value["exit_code"].as_i64().unwrap_or(2) as i32)
        }
        Command::ImportOce {
            queries,
            metadata,
            output,
        } => {
            if output.exists() {
                return Err(invalid("import destination already exists"));
            }
            let (rows, meta) = b::importer_oce::import(&queries, &metadata)?;
            let receipt = output.with_extension("receipt.json");
            if receipt.exists() {
                return Err(invalid("import receipt already exists"));
            }
            report::jsonl(&output, &rows)?;
            report::json(
                &receipt,
                &json!({"reference_commit":b::importer_oce::REFERENCE_COMMIT,"input_digest":manifest::file_digest(&queries)?,"metadata_digest":manifest::file_digest(&metadata)?,"output_digest":manifest::file_digest(&output)?,"actual_questions":rows.len(),"metadata":meta,"permission_provenance":"caller supplied external files; not redistributed by this package; rights review remains caller responsibility","must_contain":"annotation_only"}),
            )?;
            println!(
                "{} rows; target {}; external corpus not bundled. must_contain is annotation-only.",
                rows.len(),
                meta.repository.commit
            );
            Ok(0)
        }
        Command::Mutate {
            suite,
            mutation,
            output,
        } => {
            let loaded = manifest::load(&suite)?;
            let plan: b::mutations::MutationPlan = manifest::json_file(&mutation)?;
            new_output(&output)?;
            let v = tokio::task::spawn_blocking(move || b::oracle::run(&loaded, &plan, &output))
                .await
                .map_err(|e| invalid(e.to_string()))??;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(v["exit_code"].as_i64().unwrap_or(2) as i32)
        }
    }
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> std::process::ExitCode {
    let command = Cli::parse().command;
    let cancel_output = match &command {
        Command::Run { output, .. }
        | Command::Mutate { output, .. }
        | Command::Ablate { output, .. } => Some(output.clone()),
        _ => None,
    };
    let outcome = tokio::select! {
        value=execute(command)=>value,
        signal=tokio::signal::ctrl_c()=>{
            match signal{Ok(())=>{if let Some(p)=cancel_output{let _=b::report::record_cancelled(&p);}Ok(3)},Err(e)=>Err(e.into())}
        },
    };
    let code = match outcome {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    };
    std::process::ExitCode::from(code as u8)
}
