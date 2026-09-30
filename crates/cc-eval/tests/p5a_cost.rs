//! Local release lane overhead observations; not a public latency certification.
use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_search::SearchEngine;
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::{path::Path, time::Instant};

#[test]
#[ignore = "explicit release P5-A lane/identity cost observations"]
#[allow(clippy::assertions_on_constants)]
fn release_versioned_lane_costs_with_fixed_inputs() {
    assert!(!cfg!(debug_assertions), "release required");
    let mut samples = Vec::new();
    for files in [32, 256] {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".codecortex.json"),
            r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
        )
        .unwrap();
        for n in 0..files {
            std::fs::write(
                tmp.path().join(format!("f{n:03}.py")),
                format!("def marker_{n}():\n    return {n}\n"),
            )
            .unwrap();
        }
        let mut index = CodeIndex::new(Some(tmp.path())).unwrap();
        let build = index.build_index(true).unwrap();
        assert_eq!(build.files_parsed, files);
        let mut engines = Vec::new();
        for independent in [false, true] {
            let mut cfg = ProjectConfig::default();
            if !independent {
                cfg.search.exact_symbol_weight = 0.0;
                cfg.search.path_weight = 0.0;
            }
            engines.push(SearchEngine::new(
                index.index_db().unwrap().clone(),
                &cfg,
                None,
            ));
        }
        for repetition in 0..5 {
            for query in ["marker_17", "f017.py", "nonexistent_marker_omega"] {
                // Alternate order, no best-of selection. Every measurement is
                // recomputed, with all three caches cleared before its timer.
                for variant in [repetition % 2, 1 - repetition % 2] {
                    let engine = &engines[variant];
                    engine.invalidate_cache();
                    let start = Instant::now();
                    let result = engine
                        .search_with_diagnostics(&SearchRequest {
                            query: query.into(),
                            top_k: 5,
                            include_grep: true,
                            ..Default::default()
                        })
                        .unwrap();
                    let elapsed_us = start.elapsed().as_micros();
                    for lane in &result.lanes {
                        lane.validate().unwrap();
                    }
                    for hit in &result.hits {
                        let replay: f64 = hit.score_trace.iter().map(|(_, value)| value).sum();
                        assert!(hit.rerank_score.is_finite() && replay.is_finite());
                        assert!((replay - hit.rerank_score).abs() < 1e-9);
                    }
                    if variant == 1 && query != "nonexistent_marker_omega" {
                        assert_eq!(result.hits[0].file_path, "f017.py");
                    }
                    samples.push(json!({"files":files,"repetition":repetition,
                        "query":query,"independent_exact_path":variant==1,
                        "elapsed_us":elapsed_us,"hits":result.hits.len(),
                        "top1_path":result.hits.first().map(|h|&h.file_path),
                        "lane_receipt_bytes":serde_json::to_vec(&result.lanes).unwrap().len(),
                        "lanes":result.lanes,"partial_cost":result.cost,
                        "runner_native_rss_bytes":cc_index::process_rss_bytes_opt()}));
                }
            }
        }
    }
    let result = json!({"schema_version":1,"status":"passed","samples":samples,
        "scope":"release in-process SearchEngine, fixed synthetic inputs, 1 read connection, 5 repetitions, alternating lane toggle; 60 recomputed searches; not MCP end-to-end, warm-cache, holdout, 100k, peak RSS or p95/p99 certification",
        "cost_coverage":"native RSS snapshots plus existing partial SQL counters; exact prefix decode/identity lookups are included in wall time, not legacy hydration counters; no provider or network"});
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            Path::new(&out).join("p5a-cost.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
}
