//! Artifact-only diagnostic driver: public protocol and independent fixed facets.
mod graph_driver;
mod mixed_stdio;
use cc_eval::benchmark::{
    ablation::{run_mixed_load, MixedPlan},
    adapters::{mcp_stdio::McpStdio, Backend},
    normalizer,
    schema::ResultStatus,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn write(p: &std::path::Path, v: &Value) {
    std::fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "graph" {
        std::process::exit(
            graph_driver::run(
                &PathBuf::from(&a[2]),
                &PathBuf::from(&a[3]),
                &PathBuf::from(&a[4]),
            )
            .await,
        );
    }
    if a[1] == "replay-graph" {
        std::process::exit(graph_driver::replay(
            &PathBuf::from(&a[2]),
            &PathBuf::from(&a[3]),
            &PathBuf::from(&a[4]),
        ));
    }
    if a[1] == "replay-mixed" {
        std::process::exit(mixed_stdio::replay(
            &PathBuf::from(&a[2]),
            &PathBuf::from(&a[3]),
            &PathBuf::from(&a[4]),
        ));
    }
    if a[1] == "mixed-stdio" {
        let plan: MixedPlan = serde_json::from_slice(&std::fs::read(&a[3]).unwrap()).unwrap();
        let code = mixed_stdio::run(&plan, &PathBuf::from(&a[2]), &PathBuf::from(&a[4])).await;
        std::process::exit(code);
    }
    if a[1] == "mixed" {
        // Drop the async runtime before invoking the synchronous duplex driver.
        let plan: MixedPlan = serde_json::from_slice(&std::fs::read(&a[2]).unwrap()).unwrap();
        let out = PathBuf::from(&a[3]);
        let s = tokio::task::spawn_blocking(move || run_mixed_load(&plan, &out))
            .await
            .unwrap()
            .unwrap();
        println!("{s}");
        std::process::exit(s["exit_code"].as_i64().unwrap() as i32);
    }
    if a[1] == "replay-facets" {
        let source = PathBuf::from(&a[2]);
        let observed = PathBuf::from(&a[3]);
        let output = PathBuf::from(&a[4]);
        assert!(!output.exists());
        let bytes = std::fs::read(source.join("facet-plan.json")).unwrap();
        let plan: Value = serde_json::from_slice(&bytes).unwrap();
        let lock: Value =
            serde_json::from_slice(&std::fs::read(observed.join("input-lock.json")).unwrap())
                .unwrap();
        assert_eq!(
            lock["plan_blake3"],
            blake3::hash(&bytes).to_hex().to_string()
        );
        let root = temp_dir();
        for (p, s) in plan["files"].as_object().unwrap() {
            cc_eval::benchmark::validation::relative_path(p).unwrap();
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, s.as_str().unwrap()).unwrap();
        }
        let rows: Vec<Value> =
            serde_json::from_slice(&std::fs::read(observed.join("observations.json")).unwrap())
                .unwrap();
        let mut mismatches = Vec::new();
        let mut failed = 0;
        for (i, row) in rows.iter().enumerate() {
            let q = plan["queries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|q| q["id"] == row["query_id"])
                .unwrap();
            let check = if row["raw"].is_null() {
                json!({"status":"error","passed":false,"facets":[],"verified_source_spans":[]})
            } else {
                evaluate_facets(q, &row["raw"], &root)
            };
            for field in ["status", "passed", "facets", "verified_source_spans"] {
                if check[field] != row[field] {
                    mismatches.push(json!({"row":i,"field":field,"recorded":row[field],"recomputed":check[field]}));
                }
            }
            if check["passed"] != true {
                failed += 1;
            }
        }
        let summary: Value =
            serde_json::from_slice(&std::fs::read(observed.join("summary.json")).unwrap()).unwrap();
        if summary["per_query"] != summarize_queries(&rows) {
            mismatches.push(json!({"field":"per_query","recorded":summary["per_query"],"recomputed":summarize_queries(&rows)}));
        }
        if summary["failed_requests"] != failed {
            mismatches.push(json!({"field":"failed_requests","recorded":summary["failed_requests"],"recomputed":failed}));
        }
        write(
            &output,
            &json!({"replayed_rows":rows.len(),"failed_requests":failed,"mismatches":mismatches,"status":if mismatches.is_empty(){"replay_consistent"}else{"invalid_measurement"}}),
        );
        std::fs::remove_dir_all(root).unwrap();
        std::process::exit(if mismatches.is_empty() { 0 } else { 2 });
    }
    assert_eq!(a[1], "facets");
    let binary = PathBuf::from(&a[2]);
    let source = PathBuf::from(&a[3]);
    let out = PathBuf::from(&a[4]);
    assert!(!out.exists());
    std::fs::create_dir_all(&out).unwrap();
    let plan: Value =
        serde_json::from_slice(&std::fs::read(source.join("facet-plan.json")).unwrap()).unwrap();
    let root = temp_dir();
    let mut lock = Vec::new();
    for (p, s) in plan["files"].as_object().unwrap() {
        cc_eval::benchmark::validation::relative_path(p).unwrap();
        let path = root.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, s.as_str().unwrap()).unwrap();
        lock.push(json!({"path":p,"hash":blake3::hash(s.as_str().unwrap().as_bytes()).to_hex().to_string()}));
    }
    std::fs::write(
        root.join(".codecortex.json"),
        "{\"auto_index\":{\"enabled\":false}}",
    )
    .unwrap();
    write(
        &out.join("input-lock.json"),
        &json!({"files":lock,"plan_blake3":blake3::hash(&std::fs::read(source.join("facet-plan.json")).unwrap()).to_hex().to_string(),"binary_blake3":blake3::hash(&std::fs::read(&binary).unwrap()).to_hex().to_string(),"driver_blake3":blake3::hash(&std::fs::read(std::env::current_exe().unwrap()).unwrap()).to_hex().to_string(),"profile":"immutable development mechanism, not holdout"}),
    );
    let mut client = McpStdio::spawn(&binary, &root, Duration::from_secs(30))
        .await
        .unwrap();
    let prepared = client
        .call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    write(&out.join("prepare.json"), &prepared);
    let mut rows = Vec::new();
    let mut failures = 0;
    for rep in 0..30 {
        for q in plan["queries"].as_array().unwrap() {
            let started = Instant::now();
            let raw = client
                .call(
                    "search",
                    json!({"query":q["query"],"top_k":q["top_k"],"mode":"hybrid"}),
                )
                .await;
            let elapsed_us = started.elapsed().as_micros() as u64;
            let check = match &raw {
                Ok(payload) => evaluate_facets(q, payload, &root),
                Err(_) => {
                    json!({"status":"error","passed":false,"facets":[],"verified_source_spans":[]})
                }
            };
            let passed = check["passed"] == true;
            let status = &check["status"];
            let facets = &check["facets"];
            let verified = &check["verified_source_spans"];
            if !passed {
                failures += 1;
            }
            rows.push(json!({"query_id":q["id"],"repetition":rep,"status":status,"passed":passed,"elapsed_us":elapsed_us,"facets":facets,"verified_source_spans":verified,"normalization_error":check.get("normalization_error"),"originating_work":raw.as_ref().ok().and_then(cc_eval::benchmark::sampler::retrieval_work),"error":raw.as_ref().err().map(ToString::to_string),"raw":raw.ok()}));
        }
    }
    let _ = client.close().await;
    let drift: Vec<_> = plan["files"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(p, s)| {
            (std::fs::read(root.join(p)).ok().as_deref() != Some(s.as_str().unwrap().as_bytes()))
                .then_some(p.clone())
        })
        .collect();
    if !drift.is_empty() {
        failures += 1;
    }
    write(
        &out.join("source-recheck.json"),
        &json!({"drifted_admitted_files":drift,"status":if drift.is_empty(){"unchanged"}else{"invalid_measurement"}}),
    );
    write(&out.join("observations.json"), &json!(rows));
    write(
        &out.join("summary.json"),
        &json!({"requests":rows.len(),"failed_requests":failures,"per_query":summarize_queries(&rows),"exit_code":if failures==0{0}else{1},"status":if failures==0{"passed_mechanism_scope"}else{"failed"},"limitations":["fixed authored development mechanism; not holdout","no source mutation or OS atomic snapshot claim","all responses retained; no best-of; Partial and facet loss fail"]}),
    );
    std::fs::remove_dir_all(root).unwrap();
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
fn temp_dir() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "p5e-facets-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    root
}

fn evaluate_facets(q: &Value, payload: &Value, root: &std::path::Path) -> Value {
    let (mut hits, s) = match normalizer::mcp(payload) {
        Ok(v) => v,
        Err(e) => {
            return json!({"status":"invalid_measurement","passed":false,"facets":[],"verified_source_spans":[],"normalization_error":e.to_string()})
        }
    };
    let mut verified = Vec::new();
    for hit in &mut hits {
        let e = normalizer::verify_source(hit, root)
            .err()
            .map(|e| e.to_string());
        verified.push(json!({"path":hit.path,"valid":hit.evidence_valid,"verification_error":e,"span":hit.span}));
    }
    let facets: Vec<_> = q["required_facets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let covered = hits.iter().any(|h| {
                h.evidence_valid == Some(true)
                    && h.path == f["path"].as_str().unwrap()
                    && h.text
                        .as_deref()
                        .is_some_and(|t| t.contains(f["marker"].as_str().unwrap()))
            });
            json!({"path":f["path"],"marker":f["marker"],"covered":covered})
        })
        .collect();
    let passed = s == ResultStatus::Success
        && hits.iter().all(|h| h.evidence_valid == Some(true))
        && facets.iter().all(|f| f["covered"] == true);
    json!({"status":format!("{s:?}"),"passed":passed,"facets":facets,"verified_source_spans":verified})
}

fn summarize_queries(rows: &[Value]) -> Value {
    let mut groups = std::collections::BTreeMap::<String, Vec<&Value>>::new();
    for row in rows {
        groups
            .entry(row["query_id"].as_str().unwrap().to_string())
            .or_default()
            .push(row);
    }
    let mut reports = serde_json::Map::new();
    for (id, group) in groups {
        let times: Vec<_> = group
            .iter()
            .map(|r| r["elapsed_us"].as_u64().unwrap())
            .collect();
        let first: Vec<_> = group
            .iter()
            .filter(|r| r["repetition"] == 0)
            .map(|r| r["elapsed_us"].as_u64().unwrap())
            .collect();
        let repeated: Vec<_> = group
            .iter()
            .filter(|r| r["repetition"] != 0)
            .map(|r| r["elapsed_us"].as_u64().unwrap())
            .collect();
        let mut statuses = std::collections::BTreeMap::<String, usize>::new();
        for row in &group {
            *statuses
                .entry(row["status"].as_str().unwrap().to_string())
                .or_default() += 1;
        }
        reports.insert(id,json!({"requests":group.len(),"failed":group.iter().filter(|r|r["passed"]!=true).count(),"status_counts":statuses,
            "all_latency":cc_eval::benchmark::statistics::distribution(&times),"first_after_build_latency":cc_eval::benchmark::statistics::distribution(&first),
            "repeated_same_process_latency":cc_eval::benchmark::statistics::distribution(&repeated),"originating_work_receipts":group.iter().filter(|r|!r["originating_work"].is_null()).count(),
            "limits":["repeated query cache outcome not assumed","originating work is not actual cache-hit work","repetitions are not independent quality questions"]}));
    }
    Value::Object(reports)
}
