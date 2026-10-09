//! Local harness contracts exercise the actual executable and native indexes.
//! The measured 60-file fixture is not release or tail-latency certification.
use cc_eval::benchmark::{
    mutation_case,
    p8_scale::{self, CapacityProfile, Profile, ScalePlan, ScaleShard, RELEASE_SCALES},
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

fn binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_p8-scale"))
}

#[test]
fn matrix_and_release_admission_are_explicit_and_bounded() {
    assert_eq!(RELEASE_SCALES, [1_000, 5_000, 10_000, 50_000, 100_000]);
    let mut plan = ScalePlan::default();
    plan.validate().unwrap();
    plan.files = RELEASE_SCALES.to_vec();
    plan.validate().unwrap();
    for files in [vec![59], vec![100_001], vec![1000, 1000]] {
        plan.files = files;
        assert!(plan.validate().is_err());
    }
    plan.files = vec![1000];
    plan.profile = Profile::Release;
    assert!(
        plan.validate().is_err(),
        "one debug sample cannot certify release"
    );
}

#[test]
fn named_scale_capacity_is_explicit_and_cannot_relax_fanout_pressure() {
    let mut plan = ScalePlan::default();
    assert!(plan.capacity_profile.is_none());
    assert_eq!(plan.fanout_budgets(), (2, 64));
    assert!(serde_json::to_value(&plan)
        .unwrap()
        .get("capacity_profile")
        .is_none());
    plan.capacity_profile = Some(CapacityProfile::ScaleCapacityV1);
    plan.dirty_budget = 200;
    plan.max_resume_builds = 1024;
    assert_eq!(plan.fanout_budgets(), (8, 128));
    assert!(plan
        .validate()
        .unwrap_err()
        .to_string()
        .contains("scale_capacity_v1 requires"));
    plan.profile = Profile::Release;
    plan.files = vec![1000];
    plan.repetitions = 30;
    plan.shard = Some(ScaleShard {
        index: 0,
        count: 30,
    });
    if cfg!(debug_assertions) {
        assert!(plan
            .validate()
            .unwrap_err()
            .to_string()
            .contains("release eval build"));
    } else {
        plan.validate().unwrap();
        assert_eq!(plan.repetition_range().unwrap(), 0..1);
    }
    plan.dirty_budget = 201;
    assert!(plan
        .validate()
        .unwrap_err()
        .to_string()
        .contains("scale_capacity_v1 requires"));
    plan.dirty_budget = 200;
    plan.shard = Some(ScaleShard { index: 0, count: 6 });
    assert!(plan
        .validate()
        .unwrap_err()
        .to_string()
        .contains("scale_capacity_v1 requires"));
}

#[test]
fn shards_partition_registered_repetitions_without_lowering_release_admission() {
    let mut plan = ScalePlan {
        repetitions: 31,
        ..ScalePlan::default()
    };
    let mut observed = Vec::new();
    for index in 0..6 {
        plan.shard = Some(ScaleShard { index, count: 6 });
        observed.extend(plan.repetition_range().unwrap());
    }
    assert_eq!(observed, (0..31).collect::<Vec<_>>());
    for shard in [
        ScaleShard { index: 0, count: 0 },
        ScaleShard { index: 6, count: 6 },
        ScaleShard {
            index: 0,
            count: 32,
        },
    ] {
        plan.shard = Some(shard);
        assert!(plan.validate().is_err());
    }
    plan.files = vec![1000];
    plan.profile = Profile::Release;
    plan.repetitions = 29;
    plan.shard = Some(ScaleShard { index: 0, count: 6 });
    assert!(plan.validate().is_err());
}

#[test]
fn native_evidence_hash_uses_all_file_bytes_without_running_a_profile() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("raw.jsonl");
    let bytes = b"{\"event\":\"control\"}\n".repeat(10000);
    std::fs::write(&path, &bytes).unwrap();
    let output = std::process::Command::new(binary())
        .arg("--hash-file")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        blake3::hash(&bytes).to_hex().as_str()
    );
    assert!(!root.path().join("plan.json").exists());
}

#[test]
fn bounded_python_forwarding_resume_keeps_all_targets_and_fifteen_table_parity() {
    use cc_eval::benchmark::{
        mutation_case::{FactAssertion, MutationCase, Stage},
        mutations::Mutation,
        oracle,
    };
    use std::collections::BTreeMap;
    let count = 48;
    let mut initial = BTreeMap::from([(
        "api.py".to_string(),
        "def ping(x: int) -> int:\n    return x\n".to_string(),
    )]);
    let mut assertions = Vec::new();
    for n in 0..count {
        let next = (n + 1) % count;
        let path = format!("file{n:03}.py");
        initial.insert(path.clone(), format!(
            "from file{next:03} import own{next}\nfrom api import ping\ndef own{n}(x: int) -> int:\n    return ping(x)\ndef run{n}(x: int) -> int:\n    return own{next}(x)\n"
        ));
        for (name, target) in [
            ("ping".to_string(), "api.py".to_string()),
            (format!("own{next}"), format!("file{next:03}.py")),
        ] {
            assertions.push(FactAssertion {
                id: format!("caller_{n}_{name}"),
                table: "call_edges".into(),
                matches: BTreeMap::from([
                    ("file_path".into(), json!(path)),
                    ("callee_symbol".into(), json!(name)),
                    ("target_file_path".into(), json!(target)),
                ]),
                count: 1,
            });
        }
    }
    let case = MutationCase {
        schema_version: 1,
        name: "p8-python-direct-target-dirty-resume".into(),
        initial,
        stages: vec![Stage {
            mutation: Mutation::Write {
                path: "api.py".into(),
                content: "def ping(x: int, offset: int = 1) -> int:\n    return x + offset\n"
                    .into(),
            },
            reopen: false,
            settle: true,
            assertions,
        }],
        dirty_budget: 8,
        max_resume_builds: 128,
    };
    let result = mutation_case::evaluate(&case).unwrap();
    assert_eq!(result["passed"], true, "{result}");
    assert_eq!(
        result["tables"].as_array().unwrap().len(),
        oracle::tables().len()
    );
    assert_eq!(oracle::tables().len(), 15);
    let checkpoint = &result["checkpoints"][0];
    assert_eq!(checkpoint["status"], "compared");
    assert_eq!(checkpoint["different_tables"], json!([]));
    assert_eq!(checkpoint["truth"].as_array().unwrap().len(), 2 * count);
    let reports = checkpoint["reports"].as_array().unwrap();
    assert!(
        reports.len() > 1,
        "the actual eight-file dirty budget must overflow"
    );
    assert_eq!(reports[0]["resolution_freshness"]["complete"], false);
    assert_eq!(
        reports.last().unwrap()["resolution_freshness"]["complete"],
        true
    );
    for report in reports {
        assert_complete_build_timing(report, false);
    }
}

fn assert_complete_build_timing(report: &Value, full: bool) {
    let timing = &report["build_timing"];
    assert_eq!(timing["schema_version"], 1);
    let total = timing["total_us"].as_u64().unwrap();
    let partition: u64 = [
        "prepare_us",
        "commit_write_us",
        "postprocess_compute_us",
        "postprocess_apply_us",
        "between_stages_us",
    ]
    .iter()
    .map(|key| timing[*key].as_u64().unwrap())
    .sum();
    assert!(
        total >= partition && total - partition <= 6,
        "complete function stages must conserve measured wall time: {timing}"
    );
    let prepare = timing["prepare_us"].as_u64().unwrap();
    let snapshot = timing["prepare_snapshot_us"].as_u64().unwrap();
    let staging = if full {
        let duration = timing["full_staging_us"].as_u64().unwrap();
        assert!(duration > 0, "actual SQLite full staging was timed");
        duration
    } else {
        assert!(
            timing["full_staging_us"].is_null(),
            "an unexecuted full staging is not zero-cost work"
        );
        0
    };
    assert!(prepare >= snapshot + staging);
    assert!(total >= report["elapsed_ms"].as_u64().unwrap() * 1000);
    let old = report["phase_timing"].as_object().unwrap();
    assert_eq!(
        old.len(),
        6,
        "existing millisecond phase fields are unchanged"
    );
}

#[test]
fn complete_stage_timing_accounts_for_real_split_handoffs_and_full_staging() {
    use cc_server::engine::CodeIndex;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("api.ts"),
        "export function ping(x:number):number{return x;}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("use.ts"),
        "import {ping} from './api';\nexport function run(x:number):number{return ping(x);}\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    let inputs = index.build_inputs().unwrap();
    let prepared = CodeIndex::prepare_build(&inputs, true, None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let written = index
        .commit_build_write(&inputs, true, None, prepared)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let staged = CodeIndex::compute_postprocess(&inputs, true, None, written).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let full = serde_json::to_value(
        index
            .apply_postprocess(&inputs, true, None, staged)
            .unwrap(),
    )
    .unwrap();
    assert_complete_build_timing(&full, true);
    assert!(
        full["build_timing"]["between_stages_us"].as_u64().unwrap() >= 15_000,
        "handoff time must remain observable instead of disappearing between phase clocks"
    );
    let incremental = serde_json::to_value(index.build_index(false).unwrap()).unwrap();
    assert_complete_build_timing(&incremental, false);
    assert_eq!(incremental["files_updated"], 0);
    assert_eq!(incremental["resolution_freshness"]["complete"], true);
}

#[test]
fn actual_shard_keeps_global_ids_and_changes_exact_batch_including_yaml_and_routes() {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("shard");
    let plan = ScalePlan {
        repetitions: 3,
        shard: Some(ScaleShard { index: 2, count: 3 }),
        batch_sizes: vec![60],
        fanouts: vec![8],
        deadline_ms: 300_000,
        ..ScalePlan::default()
    };
    let report = p8_scale::run_supervised(&plan, &out, binary()).unwrap();
    assert_eq!(report["exit_code"], 0, "{report}");
    assert_eq!(report["shard_only"], true);
    assert_eq!(report["registered_repetitions"], 3);
    assert_eq!(
        report["executed_repetition_range"],
        json!({"start":2,"end":3})
    );
    assert_eq!(report["release_certification"], "not_run");
    let raw: Vec<Value> = std::fs::read_to_string(out.join("raw.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let input = raw.iter().find(|row| row["event"] == "input").unwrap();
    assert_eq!(input["sample"], "scale-60/repetition-2");
    let batch = raw
        .iter()
        .find(|row| row["event"] == "mutation" && row["label"] == "scale-60/repetition-2/batch_60")
        .unwrap();
    let paths: BTreeSet<_> = batch["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| op["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 60);
    assert!(paths.iter().any(|p| p.ends_with(".yaml")));
    assert!(paths.iter().any(|p| p.contains("routes_")));
    assert!(!paths.contains("tsconfig.json"));
    let fanout = raw
        .iter()
        .find(|row| row["event"] == "fanout_finished")
        .unwrap();
    assert_eq!(fanout["repetition"], 2);
    assert_eq!(fanout["first_build_incomplete"], true);
    assert_eq!(fanout["passed"], true);
}

#[test]
fn real_scale_smoke_preserves_builds_counts_mutations_and_full_parity() {
    let temp = tempfile::tempdir().unwrap();
    let out = std::env::var("P8_SCALE_TEST_EVIDENCE")
        .map(|path| Path::new(&path).join("smoke"))
        .unwrap_or_else(|_| temp.path().join("smoke"));
    // This bounds a correctness integration test on shared CI hardware. The
    // CLI smoke default remains 120 seconds; neither budget is a speed gate.
    let plan = ScalePlan {
        deadline_ms: 300_000,
        ..ScalePlan::default()
    };
    let report = p8_scale::run_supervised(&plan, &out, binary()).unwrap();
    assert_eq!(report["exit_code"], 0, "{report}; raw {}", out.display());
    assert_eq!(report["release_certification"], "not_run");
    assert_eq!(report["full_100k_certification"], "not_run");
    let raw: Vec<Value> = std::fs::read_to_string(out.join("raw.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let input = raw.iter().find(|r| r["event"] == "input").unwrap();
    assert_eq!(input["synthetic_files_written"], 60);
    assert_eq!(input["files"].as_array().unwrap().len(), 61);
    let cold = raw.iter().find(|r| r["event"] == "cold_parity").unwrap();
    for field in ["files", "symbols", "chunks"] {
        assert!(
            cold["parity"]["incremental_counts"][field]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    assert!(
        cold["parity"]["incremental_counts"]["edges"]["call_edges"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        cold["parity"]["incremental_counts"]["vectors"]["state"],
        "disabled"
    );
    assert!(cold["parity"]["incremental_counts"]["vectors"]["count"].is_null());
    let finished: Vec<_> = raw
        .iter()
        .filter(|r| r["event"] == "stage_finished")
        .collect();
    let stages: BTreeSet<_> = finished
        .iter()
        .map(|r| r["label"].as_str().unwrap().rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        stages,
        BTreeSet::from(["no_op", "body", "api", "config", "batch_1", "batch_10"])
    );
    for stage in &finished {
        assert_eq!(stage["parity"]["equal"], true, "{stage}");
        assert_eq!(stage["complete"], true);
        assert_eq!(stage["full_complete"], true);
    }
    let builds: Vec<_> = raw
        .iter()
        .filter(|r| r["event"] == "build_finished")
        .collect();
    assert!(
        builds.len() >= 14,
        "every inc and independent full build remains raw"
    );
    for build in &builds {
        assert!(build["report"]["files_scanned"].is_u64());
        assert!(build["report"]["dirty_plan"].is_object());
        assert!(build["report"]["phase_timing"].is_object());
        assert!(build["end_us"].as_u64().unwrap() >= build["start_us"].as_u64().unwrap());
    }
    let noop = builds
        .iter()
        .find(|r| {
            r["label"]
                .as_str()
                .unwrap()
                .ends_with("no_op/incremental-0")
        })
        .unwrap();
    assert_eq!(noop["report"]["files_added"], 0);
    assert_eq!(noop["report"]["files_updated"], 0);
    for event in raw.iter().filter(|r| r["event"] == "mutation") {
        for receipt in event["receipts"].as_array().unwrap() {
            assert_ne!(receipt["before_digest"], receipt["after_digest"]);
        }
    }
    let fanouts: Vec<_> = raw
        .iter()
        .filter(|r| r["event"] == "fanout_finished")
        .collect();
    assert_eq!(fanouts.len(), 2);
    assert!(fanouts.iter().all(|r| r["passed"] == true));
    for fanout in &fanouts {
        assert!(fanout["wall_us"].as_u64().unwrap() > 0);
        assert!(fanout["incremental_engine_elapsed_ms"].is_u64());
        assert!(fanout["incremental_builds"].as_u64().unwrap() > 0);
    }
    for group in report["summary"]["groups"].as_array().unwrap() {
        if group["group"].as_str().unwrap().starts_with("fanout-") {
            assert_eq!(group["whole_fanout_fixture_replay_us"]["n"], 1);
            assert_eq!(
                group["incremental_engine_elapsed_ms_sum_across_resumes"]["n"],
                1
            );
        }
    }
    assert!(
        fanouts.iter().any(|r| r["first_build_incomplete"] == true),
        "must actually exercise a bounded frontier"
    );
    assert!(std::fs::metadata(out.join("raw.jsonl")).unwrap().len() < plan.max_output_bytes);
}

#[test]
fn subprocess_deadline_preserves_report_and_rejects_output_reuse() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("deadline");
    let plan = ScalePlan {
        deadline_ms: 1,
        ..ScalePlan::default()
    };
    let report = p8_scale::run_supervised(&plan, &out, binary()).unwrap();
    assert_eq!(report["exit_code"], 3, "{report}");
    assert_eq!(report["status"], "deadline_exceeded");
    let before = std::fs::read(out.join("report.json")).unwrap();
    assert!(p8_scale::run_supervised(&plan, &out, binary()).is_err());
    assert_eq!(std::fs::read(out.join("report.json")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn subprocess_descendant_cannot_hold_stderr_past_worker_deadline() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let helper = temp.path().join("stderr-holder.sh");
    // This helper tests only supervision, not indexing. Its descendant keeps
    // the inherited stderr pipe open after the worker itself exits normally.
    std::fs::write(
        &helper,
        "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n case \"$1\" in --output) shift; out=\"$1\";; esac\n shift\ndone\nsleep 20 &\nprintf '{\"passed\":true}' > \"$out/worker-summary.json\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    // This scenario needs a ready helper that exits normally before its
    // descendant's inherited stderr is drained; it does not certify cold
    // executable startup within 250ms. Establish that precondition once with
    // a separate, bounded fixture setup. The same supervisor cleans up its
    // fresh process group, including the setup's sleeping descendant.
    let readiness_plan = ScalePlan {
        deadline_ms: 2_000,
        ..ScalePlan::default()
    };
    let readiness =
        p8_scale::run_supervised(&readiness_plan, &temp.path().join("readiness"), &helper);
    let ready = readiness.as_ref().is_ok_and(|report| {
        report["exit_code"] == 0
            && report["worker_exit_code"] == 0
            && report["summary"]["passed"] == true
            && report["stderr_complete"] == true
            && report["fixture_cleanup"]["error"].is_null()
            && report["release_certification"] == "not_run"
    });
    eprintln!("single helper readiness (separate from the 250ms assertion): {readiness:?}");
    if !ready {
        let evidence = temp.keep();
        panic!(
            "helper readiness failed without retry: {readiness:?}; evidence retained at {}",
            evidence.display()
        );
    }
    let plan = ScalePlan {
        deadline_ms: 250,
        ..ScalePlan::default()
    };
    let start = std::time::Instant::now();
    let report = p8_scale::run_supervised(&plan, &temp.path().join("run"), &helper).unwrap();
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "a descendant must not turn the 250ms deadline into a 20s join"
    );
    assert_eq!(report["exit_code"], 0, "{report}");
    assert_eq!(report["stderr_complete"], true);
    assert_eq!(report["release_certification"], "not_run");
}

#[test]
fn failed_independent_fanout_truth_remains_a_failure() {
    let mut case = p8_scale::fanout_case(2, 7, 1, 16).unwrap();
    case.stages[0].assertions[0]
        .matches
        .insert("target_file_path".into(), json!("wrong.ts"));
    let report = mutation_case::evaluate(&case).unwrap();
    assert_eq!(report["passed"], false);
    assert!(report["failure_signature"]
        .as_str()
        .unwrap()
        .starts_with("truth:"));
}

#[test]
fn configured_evidence_budget_stops_without_overwriting_partial_raw() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("budget");
    let plan = ScalePlan {
        max_output_bytes: 1024 * 1024,
        repetitions: 20,
        fanouts: vec![2],
        batch_sizes: vec![1],
        ..ScalePlan::default()
    };
    let report = p8_scale::run_supervised(&plan, &out, binary()).unwrap();
    assert_eq!(report["exit_code"], 3, "{report}");
    assert!(report["summary"]["error"]
        .as_str()
        .unwrap()
        .contains("raw_output_budget_exhausted"));
    let bytes = std::fs::read(out.join("raw.jsonl")).unwrap();
    assert!(!bytes.is_empty());
    for line in std::str::from_utf8(&bytes).unwrap().lines() {
        serde_json::from_str::<Value>(line).unwrap();
    }
    let total: u64 = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(
        total <= plan.max_output_bytes,
        "{total} > {}",
        plan.max_output_bytes
    );
}
