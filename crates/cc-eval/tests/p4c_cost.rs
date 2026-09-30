//! Measured document overhead and public verification, not a production performance certificate.
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::{path::Path, time::Instant};
#[test]
#[ignore = "release mechanism measurements"]
#[allow(clippy::assertions_on_constants)]
fn document_manifest_and_freshness_mechanism_cost() {
    assert!(!cfg!(debug_assertions));
    let mut samples = Vec::new();
    for files in [20, 200] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join(".codecortex.json"),
            r#"{"auto_index":{"enabled":false}}"#,
        )
        .unwrap();
        for n in 0..files {
            std::fs::write(
                root.join(format!("f{n}.py")),
                format!("def document_marker_{n}():\n    return {n}\n"),
            )
            .unwrap();
        }
        let mut index = CodeIndex::new(Some(root)).unwrap();
        let start = Instant::now();
        let build = index.build_index(true).unwrap();
        let build_us = start.elapsed().as_micros();
        assert_eq!(build.document_changes.files_projected, files);
        let db = rusqlite::Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
        let (docs, bytes): (i64, i64) = db
            .query_row(
                "SELECT COUNT(*),SUM(length(CAST(record_json AS BLOB))) FROM document_manifest",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(docs >= files as i64);
        for repetition in 0..5 {
            let start = Instant::now();
            let noop = index.build_index(false).unwrap();
            let noop_us = start.elapsed().as_micros();
            assert_eq!(noop.files_parsed, 0);
            assert_eq!(noop.document_changes.files_projected, 0);
            let start = Instant::now();
            let result = index
                .search()
                .search_in_context("document_marker_0", 5, None)
                .unwrap();
            let search_us = start.elapsed().as_micros();
            assert!(!result.machine_pack["hits"].as_array().unwrap().is_empty());
            assert_eq!(
                result.evidence_summary["source_freshness"]["partial"],
                false
            );
            samples.push(json!({"files":files,"repetition":repetition,"documents":docs,"manifest_bytes":bytes,"full_build_us_single":build_us,"noop_us":noop_us,"public_search_us":search_us,"freshness":result.evidence_summary["source_freshness"]}));
        }
    }
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("p4c-cost.json"),serde_json::to_vec_pretty(&json!({"samples":samples,"scope":"release actual full/noop indexes and public search with bounded disk verification; full build one observation each; no provider, RSS, 100k or p95 certificate"})).unwrap()).unwrap();
    }
}
