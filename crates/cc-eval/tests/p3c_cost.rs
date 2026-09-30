//! Release mechanism observations, not overall speedup, RSS or tail latency.
use cc_db::index_db::IndexDb;
use cc_index::{BuildScope, Indexer};
use cc_model::config::IndexingConfig;
use serde_json::json;
use std::{path::Path, sync::Arc, time::Instant};
fn put(root: &Path, file: &str, text: &str) {
    let p = root.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
#[test]
#[ignore = "explicit release Go config discovery and frontier cost"]
#[allow(clippy::assertions_on_constants)]
fn p3c_release_go_config_change_scales_with_consumers_not_unrelated_files() {
    assert!(!cfg!(debug_assertions), "run in release mode");
    let mut samples = vec![];
    for n in [100, 1000] {
        let source = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let root = source.path();
        for i in 0..n {
            put(
                root,
                &format!("cold/f{i:04}.go"),
                &format!("package cold\nfunc Cold{i}() int {{return {i}}}\n"),
            );
        }
        for i in 0..25 {
            put(root,&format!("use{i:02}.go"),&format!("package app\nimport \"example.com/api\"\nfunc Run{i}() int {{return api.Ping()}}\n"));
        }
        for name in ["one", "two"] {
            put(
                root,
                &format!("{name}/go.mod"),
                "module example.com/api\ngo 1.22\n",
            );
            put(
                root,
                &format!("{name}/api.go"),
                "package api\nfunc Ping() int {return 1}\n",
            );
        }
        let config = |name: &str| {
            format!("module example.com/app\ngo 1.22\nrequire example.com/api v1.0.0\nreplace example.com/api => ./{name}\n")
        };
        put(root, "go.mod", &config("one"));
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&storage.path().join("index.db"), 1)
                .unwrap()
                .0,
        );
        let cfg = IndexingConfig {
            dirty_propagation_max_files: 5,
            ..Default::default()
        };
        let index = Indexer::new(db.clone(), root, &cfg);
        let t = Instant::now();
        let r = index.build_index(root, true).unwrap();
        samples.push(json!({"n":n,"kind":"full","elapsed_us":t.elapsed().as_micros(),"report":r}));
        for iteration in 0..3 {
            let t = Instant::now();
            let r = index.build_index(root, false).unwrap();
            assert_eq!(r.project_model.go_source_reads, 0);
            assert_eq!(r.project_model.go_fact_cache_hits, n + 27);
            assert_eq!(r.dirty_plan.selected_dependents, 0);
            samples
                .push(json!({"n":n,"kind":"noop","elapsed_us":t.elapsed().as_micros(),"report":r}));
            let target = if iteration % 2 == 0 { "two" } else { "one" };
            put(root, "go.mod", &config(target));
            let scope = BuildScope {
                changed: vec!["go.mod".into()],
                removed: vec![],
            };
            let mut completed = false;
            let mut selected = 0;
            for step in 0..8 {
                let t = Instant::now();
                let prepared = index
                    .prepare_build_scoped(root, false, None, Some(&scope))
                    .unwrap();
                let r = index.commit_build(root, false, None, prepared).unwrap();
                assert_eq!(r.files_parsed, 0);
                assert_eq!(r.project_model.go_source_reads, 0);
                assert!(r.dirty_plan.selected_dependents <= 5);
                selected += r.dirty_plan.selected_dependents;
                completed = r.resolution_freshness.complete;
                samples.push(json!({"n":n,"kind":"config_resume","iteration":iteration,"step":step,"elapsed_us":t.elapsed().as_micros(),"report":r}));
                if completed {
                    break;
                }
            }
            assert!(completed);
            assert_eq!(
                selected, 25,
                "unrelated Go files must not be re-resolved on this configuration-only event"
            );
            let rows=db.reads().query_json("SELECT DISTINCT target_file_path FROM call_edges WHERE file_path LIKE 'use%.go'",&[]).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["target_file_path"], format!("{target}/api.go"));
        }
    }
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(Path::new(&path).join("p3c-go-cost.json"),serde_json::to_vec_pretty(&json!({"samples":samples,"scope":"release in-process isolated SQLite; go_source_reads counts compact fact capture, not scanner or commit IO; no p95/RSS/100k or speedup claim"})).unwrap()).unwrap();
    }
}
