//! Local release observations for final source verification + bounded assembly.
use cc_model::Intent;
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::time::Instant;

#[test]
#[ignore = "explicit release P5-C final evidence/packing cost observations"]
#[allow(clippy::assertions_on_constants)]
fn release_final_evidence_and_packing_costs() {
    assert!(!cfg!(debug_assertions), "release required");
    let mut samples = Vec::new();
    for files in [32, 256] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".codecortex.json"),
            r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
        )
        .unwrap();
        for i in 0..files {
            std::fs::write(
                dir.path().join(format!("f{i:03}.rs")),
                format!("pub fn needle_{i:03}() -> &'static str {{ \"fixed payload {i}\" }}\n"),
            )
            .unwrap();
        }
        let mut index = CodeIndex::new(Some(dir.path())).unwrap();
        index.build_index(true).unwrap();
        let handle = index.query_handle().unwrap();
        for query in ["needle_017", "f017.rs", "nonexistent_identifier_omega"] {
            for repetition in 0..5 {
                let start = Instant::now();
                let result = handle
                    .search_in_context(query, 10, Some(Intent::Locate))
                    .unwrap();
                let bytes = serde_json::to_vec(&result).unwrap();
                let elapsed = start.elapsed().as_micros();
                assert!(bytes.len() <= 16000);
                assert_eq!(
                    result.evidence_summary["packing"]["used_bytes"],
                    bytes.len()
                );
                assert!(result.evidence_summary["source_freshness"]["generation"].is_object());
                if query == "needle_017" {
                    assert_eq!(result.machine_pack["hits"][0]["symbol_name"], "needle_017");
                }
                let value = serde_json::to_value(&result).unwrap();
                let (mut hits, status) = cc_eval::benchmark::normalizer::mcp(&value).unwrap();
                for h in &mut hits {
                    cc_eval::benchmark::normalizer::verify_source(h, dir.path()).unwrap();
                    assert_eq!(h.evidence_valid, Some(true));
                }
                samples.push(json!({"files":files,"query":query,"repetition":repetition,
                    "cache_phase":if repetition==0 {"first_query"} else {"repeated_query"},
                    "elapsed_us":elapsed,"bytes":bytes.len(),"returned_hits":hits.len(),"status":status,
                    "packing":result.evidence_summary["packing"],
                    "source_read_bytes":result.evidence_summary["source_freshness"]["read_budget_charged"],
                    "rss_boundary_bytes":cc_index::process_rss_bytes_opt()}));
            }
        }
    }
    assert_eq!(samples.len(), 30);
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("p5c-final-evidence-cost.json"),
            serde_json::to_vec_pretty(&json!({"status":"passed","samples":samples,
                "scope":"30 in-process release queries; 32/256 files, one read connection, 3 queries x 5 repeats; includes current-source verification and serialization; not MCP latency, open-loop throughput, 100k, peak RSS or tail certification"})).unwrap()).unwrap();
    }
}
