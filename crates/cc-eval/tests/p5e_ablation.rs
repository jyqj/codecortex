//! P5-019 mechanism tests; no fake passed quality/release certification.
use cc_eval::benchmark::ablation::{run_mixed_load, ChunkFacet, MixedPlan, MixedQuery};
use std::collections::BTreeMap;

fn plan(concurrency: usize, repetitions: usize) -> MixedPlan {
    let mut files = BTreeMap::new();
    files.insert(
        "src/needle.rs".into(),
        "pub fn unique_needle() -> i32 { 777 }\n".into(),
    );
    for i in 0..32 {
        files.insert(
            format!("src/filler_{i}.rs"),
            format!("pub fn filler_{i}() -> i32 {{ {i} }}\n"),
        );
    }
    MixedPlan {
        schema_version: 1,
        files,
        queries: vec![MixedQuery {
            id: "needle".into(),
            query: "unique_needle".into(),
            required_facets: vec![ChunkFacet {
                path: "src/needle.rs".into(),
                marker: "unique_needle".into(),
            }],
        }],
        concurrency,
        repetitions,
        offered_interval_us: 1000,
        build_every: 3,
        top_k: 5,
        seed: 1905,
    }
}

#[test]
fn mixed_public_dispatch_preserves_all_samples_and_facets() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("run");
    let p = plan(4, 6);
    let s = run_mixed_load(&p, &out).unwrap();
    assert_eq!(
        s["exit_code"], 0,
        "positive mixed fixture must retain valid facets: {s}"
    );
    if std::env::var("CODECORTEX_BENCH_PROCESS_PROBE").as_deref() == Ok("0") {
        assert!(s["process_threads_before"].is_null());
        assert!(s["process_threads_after"].is_null());
    }
    assert_eq!(s["measurement_clock_starts_after_preload_probe"], true);
    assert!(s["preload_probe_elapsed_us"].is_u64());
    assert_eq!(s["completed_jobs"], 8);
    assert_eq!(s["read_latency"]["samples"], 6);
    assert_eq!(s["build_latency"]["samples"], 2);
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(out.join("observations.json")).unwrap()).unwrap();
    for (n, r) in rows.iter().enumerate() {
        assert_eq!(r["sequence"], n);
        assert!(r["finished_us"].as_u64().unwrap() >= r["started_us"].as_u64().unwrap());
        assert_eq!(
            r["end_to_end_us"].as_u64().unwrap(),
            r["client_queue_us"].as_u64().unwrap()
                + r["service_and_transport_us"].as_u64().unwrap(),
            "offered-load time decomposition must use one post-probe clock"
        );
        if r["kind"] == "read" {
            // Failures remain visible, rather than being filtered out for latency.
            assert!(r["raw"].is_object() || r["error"].is_string());
        }
    }
    assert!(
        run_mixed_load(&p, &out).is_err(),
        "never overwrite raw observations"
    );
}

#[test]
fn impossible_facet_is_rejected_before_starting_product() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("invalid");
    let mut p = plan(1, 1);
    p.queries[0].required_facets[0].marker = "not_in_source".into();
    assert!(run_mixed_load(&p, &out).is_err());
    assert!(!out.exists());
}

#[test]
fn unrelated_query_cannot_buy_latency_by_losing_a_required_facet() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = plan(1, 1);
    p.queries[0].query = "nonexistent_symbol_zzzz".into();
    let s = run_mixed_load(&p, &dir.path().join("failed")).unwrap();
    assert_eq!(s["exit_code"], 1);
    assert_eq!(s["status"], "failed");
    assert_eq!(s["read_latency"]["samples"], 1);
}

/// Run explicitly in release, after accepted-source freeze. Reports stay in a
/// unique caller-selected directory; no best-of or altered existing gold.
#[test]
#[ignore = "explicit release diagnostic; set P5E_MIXED_OUT to unique artifact directory"]
fn p5e_release_mixed_c1_c4_c8_c16() {
    let output = std::env::var_os("P5E_MIXED_OUT").expect("P5E_MIXED_OUT required");
    let root = std::path::PathBuf::from(output);
    assert!(!root.exists(), "immutable run directory");
    std::fs::create_dir_all(&root).unwrap();
    let mut summaries = Vec::new();
    for c in [1, 4, 8, 16] {
        let s = run_mixed_load(&plan(c, 30), &root.join(format!("c{c}"))).unwrap();
        summaries.push(s);
    }
    std::fs::write(
        root.join("all-summaries.json"),
        serde_json::to_vec_pretty(&summaries).unwrap(),
    )
    .unwrap();
    // Raw observations are written before this assertion, including any red gate.
    assert!(
        summaries.iter().all(|s| s["exit_code"] == 0),
        "mixed quality/availability gate failed; raw retained"
    );
}
