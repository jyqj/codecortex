//! Negative controls execute the actual replay CLI; synthetic rows are not
//! product latency evidence and cannot bypass raw/source or failure handling.
use serde_json::{json, Value};
use std::path::Path;

fn input() -> Value {
    json!({"schema_version":1,"profile":"smoke","expected_samples":1,
        "samples":[{"evidence":{"operation":"query","index_was_empty":false,
            "first_query_in_process":false,"reopened_existing_index":false,
            "warmup_completed":true,"result_cache":"miss"},"status":"success","elapsed_us":10}],
        "raw_queries":[],"resources":[],"disk_partitions":[],"disk_layout_complete":false,"costs":[]})
}

fn execute(root: &Path, value: &Value) -> (std::process::Output, Value) {
    let path = root.join("input.json");
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    let output = root.join("report.json");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_p8-measurements"))
        .args([
            "--input",
            path.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    (
        result,
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap(),
    )
}

#[test]
fn zero_or_unwitnessed_success_is_nonzero_with_immutable_failure_report() {
    for empty in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut value = input();
        if empty {
            value["expected_samples"] = json!(0);
            value["samples"] = json!([]);
        }
        let (result, report) = execute(directory.path(), &value);
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(report["measurement_status"], "invalid_measurement");
        let path = directory.path().join("report.json");
        let before = std::fs::read(&path).unwrap();
        let repeat = std::process::Command::new(env!("CARGO_BIN_EXE_p8-measurements"))
            .args([
                "--input",
                directory.path().join("input.json").to_str().unwrap(),
                "--output",
                path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!repeat.status.success());
        assert_eq!(before, std::fs::read(path).unwrap());
    }
}

#[test]
fn valid_source_cannot_erase_an_observed_cache_control_failure() {
    let directory = tempfile::tempdir().unwrap();
    let source = "def p8_lifecycle_000000(value):\n    return value + 17\n";
    std::fs::write(directory.path().join("a.py"), source).unwrap();
    let mut value = input();
    value["samples"][0]["status"] = json!("protocol_error");
    value["raw_queries"] = json!([{"sample_index":0,"source_root":directory.path(),
        "expected_path":"a.py","expected_symbol":"p8_lifecycle_000000",
        "response":[{"file_path":"a.py","name":"p8_lifecycle_000000",
            "start_line":1,"end_line":2,"text":source.trim_end()}]}]);
    let (result, report) = execute(directory.path(), &value);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(report["source_verified_queries"], 1);
    assert_eq!(report["latency_layers"]["layers"][2]["errors"], 1);
    assert_eq!(report["latency_layers"]["layers"][2]["completed"], 0);
    assert_eq!(
        report["measurement_status"],
        "incomplete_or_failed_observation"
    );
}
