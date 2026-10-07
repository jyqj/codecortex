//! Real local MCP workload tests verify harness evidence/control behavior.
//! A product oracle mismatch remains a reported failure, never a waived pass.
use cc_eval::benchmark::{
    manifest,
    p8_load::{self, LoadConfig},
    report,
};
use serde_json::Value;
use std::{
    path::Path,
    process::Command,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

fn run(root: &Path, name: &str, config: &LoadConfig) -> Value {
    p8_load::supervise(
        config,
        &root.join(name),
        Path::new(env!("CARGO_BIN_EXE_cc-eval-p8-load")),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}
fn config() -> LoadConfig {
    LoadConfig {
        files: 4,
        operations: 18,
        offer_interval_ms: 20,
        queue_capacity: 32,
        request_deadline_ms: 20_000,
        run_deadline_ms: 30_000,
        ..LoadConfig::default()
    }
}

#[test]
fn configuration_bounds_reject_unbounded_or_ambiguous_profiles_before_writing() {
    for concurrency in [0, 2, 3, 32] {
        assert!(LoadConfig {
            concurrency,
            ..config()
        }
        .validate()
        .is_err());
    }
    for c in [
        LoadConfig {
            queue_capacity: 0,
            ..config()
        },
        LoadConfig {
            operations: 10_001,
            ..config()
        },
        LoadConfig {
            files: 257,
            ..config()
        },
        LoadConfig {
            request_deadline_ms: 0,
            ..config()
        },
        LoadConfig {
            run_deadline_ms: 3_600_001,
            ..config()
        },
        LoadConfig {
            artifact_budget_bytes: 0,
            ..config()
        },
    ] {
        assert!(c.validate().is_err());
    }
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("invalid");
    assert!(p8_load::supervise(
        &LoadConfig {
            concurrency: 2,
            ..config()
        },
        &out,
        Path::new("missing"),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    assert!(!out.exists());
}

#[test]
fn real_mixed_load_drains_and_retains_exact_oracle_failure_or_success() {
    let d = tempfile::tempdir().unwrap();
    for concurrency in [1, 4, 8, 16] {
        let name = format!("c{concurrency}");
        let result = run(
            d.path(),
            &name,
            &LoadConfig {
                concurrency,
                ..config()
            },
        );
        let out = d.path().join(&name);
        let worker = &result["worker_summary"];
        assert!(
            worker.is_object(),
            "{}; log={}",
            result,
            std::fs::read_to_string(out.join("worker.log")).unwrap()
        );
        assert_eq!(worker["planned"], 18);
        assert_eq!(worker["offered"], 18);
        assert_eq!(worker["terminal_rows"], 18);
        assert_eq!(worker["missing_terminal_rows"], 0);
        assert_eq!(worker["queue_drained"], true);
        assert!(worker["queue_peak"].as_u64().unwrap() <= 32);
        assert!(worker["backend_call_peak"].as_u64().unwrap() <= concurrency as u64);
        if concurrency == 1 {
            assert_eq!(worker["read_build_overlap_observed"], false);
        }
        assert_eq!(worker["completed_mutations"], 6);
        assert!(worker["rss_bytes"].is_null());
        let events: Vec<Value> = report::read_jsonl(&out.join("events.jsonl")).unwrap();
        assert_eq!(
            events.iter().filter(|e| e["event"] == "offered").count(),
            18
        );
        let mutations: Vec<_> = events
            .iter()
            .filter(|e| e["event"] == "mutation_started")
            .collect();
        assert_eq!(mutations.len(), 6);
        assert!(mutations.iter().any(|e| e["mutation"]["kind"] == "delete"));
        assert!(mutations.iter().any(|e| e["mutation"]["kind"] == "rename"));
        for event in events.iter().filter(|e| e["event"] == "terminal") {
            assert!(event["end_to_end_us"].is_u64());
            assert!(event["dispatch_lag_us"].is_u64());
            assert!(
                event["scheduled_us"].as_u64().unwrap() <= event["offered_us"].as_u64().unwrap()
            );
            assert!(
                event["admitted_us"].as_u64().unwrap() >= event["offered_us"].as_u64().unwrap()
            );
        }
        let reconciliation: Value = manifest::json_file(&out.join("reconciliation.json")).unwrap();
        assert_eq!(
            reconciliation["incremental_repaired_before_comparison"],
            false
        );
        assert!(out.join("incremental-final.json").is_file());
        assert!(out.join("full-final.json").is_file());
        if reconciliation["equal"] == true && worker["outcomes"]["success"] == 18 {
            assert_eq!(result["exit_code"], 0);
        } else {
            assert_eq!(
                result["exit_code"], 1,
                "an actual product discrepancy must remain failed"
            );
        }
        assert_eq!(result["performance_certificate"], "not_run");
        assert_eq!(result["binary_binding"]["worker_digest_verified"], true);
        assert_eq!(
            result["binary_binding"]["supervisor_executable_unchanged"],
            true
        );
        assert_eq!(result["stderr"]["limit_bytes"], 65_536);
        assert!(result["stderr"]["retained_bytes"].as_u64().unwrap() <= 65_536);
    }
}

#[test]
fn worker_checks_its_actual_executable_digest_before_running_a_fixture() {
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("different-binary");
    std::fs::create_dir(&out).unwrap();
    report::json(&out.join("config.json"), &config()).unwrap();
    report::json(
        &out.join("manifest.json"),
        &serde_json::json!({"config":config(),
            "compiled_module_digest":manifest::digest(include_bytes!("../src/benchmark/p8_load.rs")),
            "supervisor_binary_digest":"deliberately-different-binary"}),
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_cc-eval-p8-load"))
        .arg("worker")
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(2));
    assert!(String::from_utf8(child.stderr)
        .unwrap()
        .contains("binary binding mismatch"));
    let receipt: Value = manifest::json_file(&out.join("worker-binary.json")).unwrap();
    assert_eq!(receipt["matched"], false);
    assert_eq!(
        receipt["worker_binary_digest"],
        manifest::file_digest(Path::new(env!("CARGO_BIN_EXE_cc-eval-p8-load"))).unwrap()
    );
    assert!(!out.join("live-worktree").exists());
}

#[test]
fn supervisor_rejects_a_binary_removed_after_the_worker_self_check() {
    let d = tempfile::tempdir().unwrap();
    let executable = d.path().join("worker-copy");
    std::fs::copy(env!("CARGO_BIN_EXE_cc-eval-p8-load"), &executable).unwrap();
    let out = d.path().join("removed-binary");
    let worker_executable = executable.clone();
    let worker_out = out.clone();
    let task = std::thread::spawn(move || {
        p8_load::supervise(
            &LoadConfig {
                concurrency: 1,
                operations: 2,
                offer_interval_ms: 1000,
                ..config()
            },
            &worker_out,
            &worker_executable,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    });
    let began = Instant::now();
    while !out.join("worker-binary.json").is_file()
        && !task.is_finished()
        && began.elapsed() < Duration::from_secs(15)
    {
        std::thread::sleep(Duration::from_millis(2));
    }
    let checked_before_removal = out.join("worker-binary.json").is_file();
    std::fs::remove_file(executable).unwrap();
    let result = task.join().unwrap();
    assert!(checked_before_removal, "{result}");
    assert_eq!(result["exit_code"], 2, "{result}");
    assert_eq!(result["status"], "invalid_measurement");
    assert_eq!(result["binary_binding"]["worker_digest_verified"], true);
    assert_eq!(
        result["binary_binding"]["supervisor_executable_unchanged"],
        false
    );
    assert!(result["binary_binding"]["supervisor_after"].is_null());
    assert!(out.join("events.jsonl").is_file());
    assert!(out.join("worker-summary.json").is_file());
    assert!(out.join("supervisor.json").is_file());
}

#[test]
fn cli_keeps_indexes_inside_fixtures_despite_an_inherited_external_cache_override() {
    let d = tempfile::tempdir().unwrap();
    let outside = d.path().join("caller-cache");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"caller-owned sentinel").unwrap();
    let config_path = d.path().join("config.json");
    report::json(
        &config_path,
        &LoadConfig {
            operations: 2,
            concurrency: 1,
            ..config()
        },
    )
    .unwrap();
    let out = d.path().join("isolated");
    let child = Command::new(env!("CARGO_BIN_EXE_cc-eval-p8-load"))
        .arg("run")
        .arg("--output")
        .arg(&out)
        .arg("--config")
        .arg(config_path)
        .env("CODECORTEX_CACHE_DIR", &outside)
        .env("CODECORTEX_MAX_CONCURRENT_PARSE", "128")
        .output()
        .unwrap();
    let receipt: Value = serde_json::from_slice(&child.stdout).unwrap();
    assert!(matches!(child.status.code(), Some(0 | 1)), "{receipt}");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(outside.join("sentinel")).unwrap(),
        b"caller-owned sentinel"
    );
    for fixture in ["live-worktree", "full-worktree"] {
        assert!(out
            .join(fixture)
            .join(".codecortex/index.sqlite3")
            .is_file());
    }
    assert_eq!(receipt["binary_binding"]["worker_digest_verified"], true);
}

#[test]
fn burst_admission_preserves_rejections_and_deadline_failures_in_denominators() {
    let d = tempfile::tempdir().unwrap();
    let config = LoadConfig {
        operations: 120,
        concurrency: 1,
        queue_capacity: 1,
        offer_interval_ms: 0,
        request_deadline_ms: 1,
        ..config()
    };
    let result = run(d.path(), "burst", &config);
    let worker = &result["worker_summary"];
    assert!(worker.is_object(), "{result}");
    assert_eq!(worker["offered"], 120);
    assert_eq!(worker["terminal_rows"], 120);
    assert_eq!(worker["queue_peak"], 1);
    assert!(worker["outcomes"]["queue_rejected"].as_u64().unwrap_or(0) > 0);
    assert!(worker["outcomes"]["timeout"].as_u64().unwrap_or(0) > 0);
    assert_eq!(result["exit_code"], 1);
    let outcomes: u64 = worker["outcomes"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(outcomes, 120);
}

#[test]
fn cancellation_stops_offering_drains_and_keeps_final_reconciliation() {
    let d = tempfile::tempdir().unwrap();
    let result = run(
        d.path(),
        "cancel",
        &LoadConfig {
            operations: 120,
            offer_interval_ms: 20,
            cancel_after_ms: Some(30),
            ..config()
        },
    );
    assert_eq!(result["exit_code"], 3, "{result}");
    assert_eq!(result["cancellation_requested"], true);
    let worker = &result["worker_summary"];
    assert!(worker["offered"].as_u64().unwrap() < 120);
    assert_eq!(worker["queue_drained"], true);
    assert_eq!(worker["offered"], worker["terminal_rows"]);
    assert!(d.path().join("cancel/reconciliation.json").is_file());
    assert!(d.path().join("cancel/incremental-final.json").is_file());
}

#[test]
fn total_deadline_kills_worker_and_keeps_nonzero_partial_receipt() {
    let d = tempfile::tempdir().unwrap();
    let began = Instant::now();
    let result = run(
        d.path(),
        "deadline",
        &LoadConfig {
            run_deadline_ms: 1,
            request_deadline_ms: 1,
            drain_timeout_ms: 1,
            ..config()
        },
    );
    assert_eq!(result["exit_code"], 2);
    assert_eq!(result["forced_stop"], "run_deadline");
    assert!(began.elapsed().as_secs() < 10);
    assert!(d.path().join("deadline/manifest.json").is_file());
    assert!(d.path().join("deadline/supervisor.json").is_file());
    assert_eq!(result["partial_artifacts_retained"], true);
}

#[test]
fn artifact_budget_and_existing_output_fail_without_truncating_evidence() {
    let d = tempfile::tempdir().unwrap();
    let result = run(
        d.path(),
        "budget",
        &LoadConfig {
            artifact_budget_bytes: 1024,
            ..config()
        },
    );
    assert_eq!(result["exit_code"], 2, "{result}");
    let out = d.path().join("budget");
    let before = std::fs::read(out.join("supervisor.json")).unwrap();
    assert!(p8_load::supervise(
        &config(),
        &out,
        Path::new(env!("CARGO_BIN_EXE_cc-eval-p8-load")),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    assert_eq!(std::fs::read(out.join("supervisor.json")).unwrap(), before);
    assert!(std::fs::read_to_string(out.join("worker.log"))
        .unwrap()
        .contains("budget"));
}
