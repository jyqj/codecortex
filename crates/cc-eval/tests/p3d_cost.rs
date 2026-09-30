//! Mixed multi-package mechanism costs. No fitted performance or hidden filesystem counters.
#[path = "common/p3d_fixture.rs"]
mod fixture;
use cc_db::index_db::IndexDb;
use cc_index::{module_resolution, project_model::discover, Indexer, Scanner};
use cc_model::{
    config::IndexingConfig,
    project_model::{ModuleStatus, ProjectInputs},
};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Instant};
#[test]
#[ignore = "explicit release multi-package discovery, cache reuse and pure imports"]
#[allow(clippy::assertions_on_constants)]
fn release_multilanguage_import_cost_and_disk_independence() {
    assert!(!cfg!(debug_assertions), "run in release");
    let mut samples = Vec::new();
    for n in [10, 100] {
        let source = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let root = source.path();
        let lookups = fixture::fixture(root, n);
        let config = IndexingConfig::default();
        let db = Arc::new(IndexDb::open(&storage.path().join("index.db")).unwrap().0);
        let index = Indexer::new(db, root, &config);
        let t = Instant::now();
        let r = index.build_index(root, true).unwrap();
        samples.push(json!({"packages_per_language":n,"kind":"full_build","elapsed_us":t.elapsed().as_micros(),"report":r}));
        for repetition in 0..3 {
            let t = Instant::now();
            let r = index.build_index(root, false).unwrap();
            assert_eq!(r.files_parsed, 0);
            assert_eq!(r.project_model.rust_source_reads, 0);
            assert_eq!(r.project_model.go_source_reads, 0);
            assert_eq!(r.project_model.rust_fact_cache_hits, 2 * n);
            assert_eq!(r.project_model.go_fact_cache_hits, 2 * n);
            assert!(r.project_model.config_parse_cache_hits >= 4 * n);
            samples.push(json!({"packages_per_language":n,"kind":"warm_build","repetition":repetition,"elapsed_us":t.elapsed().as_micros(),"report":r}));
        }
        let scanner = Scanner::new(root, &config);
        let t = Instant::now();
        let (files, walk) = scanner.scan_with_manifest();
        let scan_us = t.elapsed().as_micros();
        let t = Instant::now();
        let c = discover(
            root,
            files.into_iter().map(|f| f.rel_path).collect(),
            Some(&walk),
            None,
            &ProjectInputs::default(),
        )
        .unwrap();
        samples.push(json!({"packages_per_language":n,"kind":"standalone_capture","scan_us":scan_us,"capture_us":t.elapsed().as_micros(),"report":c.report()}));
        // Every query below runs after its entire source/config tree is removed.
        source.close().unwrap();
        for language in ["typescript", "python", "rust", "go"] {
            let queries = lookups
                .iter()
                .filter(|q| q.language == language)
                .collect::<Vec<_>>();
            for repetition in 0..5 {
                let t = Instant::now();
                for iteration in 0..1000 {
                    let q = queries[iteration % queries.len()];
                    let r = module_resolution::resolve(c.model(), &q.importer, &q.spec);
                    assert_eq!(r.status, ModuleStatus::Resolved, "{language}: {r:?}");
                    assert!(
                        r.resolved_path.as_ref() == Some(&q.target)
                            || r.resolved_package
                                .as_ref()
                                .is_some_and(|p| p.files == [q.target.clone()])
                    );
                }
                samples.push(json!({"packages_per_language":n,"kind":"pure_resolve","language":language,"repetition":repetition,"imports":1000,"elapsed_us":t.elapsed().as_micros(),"source_tree_removed":true}));
            }
        }
    }
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(Path::new(&path).join("p3d-import-cost.json"),serde_json::to_vec_pretty(&json!({"samples":samples,"scope":"release isolated fixtures; capture/read counters exclude scan and publication IO; deletion is a disk-independence assertion not syscall instrumentation; no p95/RSS/100k/speedup claim"})).unwrap()).unwrap();
    }
}
