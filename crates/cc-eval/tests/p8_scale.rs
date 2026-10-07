//! Local harness contracts exercise the actual executable and native indexes.
//! The measured 60-file fixture is not release or tail-latency certification.
use cc_eval::benchmark::{
    mutation_case,
    p8_scale::{self, Profile, ScalePlan, RELEASE_SCALES},
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
