//! Explicit release mechanism observations; not tail latency or compiler certification.
use cc_db::index_db::IndexDb;
use cc_index::{module_resolution, project_model::discover, BuildScope, Indexer, Scanner};
use cc_model::{config::IndexingConfig, project_model::ModuleStatus};
use serde_json::json;
use std::{hint::black_box, path::Path, sync::Arc, time::Instant};
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn save(name: &str, value: &serde_json::Value) {
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            Path::new(&path).join(name),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}
#[test]
#[ignore = "explicit --release project-model mechanism measurements"]
#[allow(clippy::assertions_on_constants)]
fn p3a_release_config_discovery_and_bounded_retarget() {
    assert!(!cfg!(debug_assertions), "run with --release");
    let mut samples = Vec::new();
    for n in [1000, 5000] {
        let source = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let root = source.path();
        for i in 0..n {
            put(
                root,
                &format!("cold{i}.ts"),
                &format!("export function cold{i}(){{return 0;}}\n"),
            );
        }
        for i in 0..25 {
            put(
                root,
                &format!("use{i:02}.ts"),
                &format!(
                    "import {{ping}} from '@api'; export function run{i}(){{return ping(1);}}\n"
                ),
            );
        }
        for p in ["a.ts", "b.ts"] {
            put(root, p, "export function ping(x:number){return x;}\n");
        }
        put(root, "tsconfig.json", r#"{"extends":"./cfg/base"}"#);
        let configuration = |target: &str| {
            json!({"compilerOptions":{"moduleResolution":"bundler","paths":{"@api":[format!("../{target}")]}}}).to_string()
        };
        put(root, "cfg/base.json", &configuration("a.ts"));
        let config = IndexingConfig {
            ignore: vec!["**/*.json".into()],
            dirty_propagation_max_files: 5,
            ..Default::default()
        };
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&storage.path().join("index.sqlite3"), 1)
                .unwrap()
                .0,
        );
        let index = Indexer::new(db.clone(), root, &config);
        let start = Instant::now();
        let full = index.build_index(root, true).unwrap();
        assert_eq!(full.project_model.config_reads, 2);
        assert_eq!(full.project_model.inventory_source, "shared_walk");
        samples.push(
            json!({"n":n,"kind":"full","elapsed_us":start.elapsed().as_micros(),"report":full}),
        );
        for iteration in 0..12 {
            let start = Instant::now();
            let noop = index.build_index(root, false).unwrap();
            assert_eq!(noop.files_parsed, 0);
            assert_eq!(noop.project_model.config_reads, 2);
            assert_eq!(noop.project_model.config_parse_cache_hits, 2);
            assert_eq!(noop.dirty_plan.selected_dependents, 0);
            samples.push(json!({"n":n,"kind":"noop","iteration":iteration,"elapsed_us":start.elapsed().as_micros(),"report":noop}));
            let target = if iteration & 1 == 0 { "b.ts" } else { "a.ts" };
            put(root, "cfg/base.json", &configuration(target));
            let scope = BuildScope {
                changed: vec!["cfg/base.json".into()],
                removed: vec![],
            };
            let mut ready = false;
            for step in 0..8 {
                let start = Instant::now();
                let prepared = index
                    .prepare_build_scoped(root, false, None, Some(&scope))
                    .unwrap();
                let r = index.commit_build(root, false, None, prepared).unwrap();
                assert_eq!(r.files_parsed, 0);
                assert_eq!(r.project_model.config_reads, 2);
                assert_eq!(r.project_model.config_root_probes, 0);
                assert_eq!(r.project_model.inventory_source, "scoped_catalog");
                assert!(r.dirty_plan.selected_dependents <= 5);
                ready = r.resolution_freshness.complete;
                if step == 0 {
                    assert!(!ready);
                }
                samples.push(json!({"n":n,"kind":"config_resume","iteration":iteration,"step":step,"elapsed_us":start.elapsed().as_micros(),"report":r}));
                if ready {
                    break;
                }
            }
            assert!(ready, "bounded configuration invalidation must converge");
            let rows = db
                .reads()
                .query_json(
                    "SELECT resolved_path FROM imports WHERE file_path LIKE 'use%.ts'",
                    &[],
                )
                .unwrap();
            assert_eq!(rows.len(), 25);
            assert!(rows.iter().all(|r| r["resolved_path"] == target));
            assert_eq!(
                db.reads()
                    .query_json("SELECT COUNT(*) AS n FROM resolution_frontier", &[])
                    .unwrap()[0]["n"],
                0
            );
        }
    }
    save(
        "p3a-cost.json",
        &json!({"scope":"release in-process indexer; no subprocess, RSS, p95 or compiler claim","config_reads_scope":"discovery reads only; publication separately revalidates each captured input","samples":samples}),
    );
}
#[test]
#[ignore = "explicit --release immutable resolution cost observations"]
#[allow(clippy::assertions_on_constants)]
fn p3a_release_many_imports_share_the_same_captured_inputs() {
    assert!(!cfg!(debug_assertions), "run with --release");
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "import {x} from '@api';\n");
    put(d.path(), "api.ts", "export const x=1;\n");
    put(d.path(), "tsconfig.json", r#"{"extends":"./base"}"#);
    put(
        d.path(),
        "base.json",
        r#"{"compilerOptions":{"paths":{"@api":["./api"]}}}"#,
    );
    let (files, manifest) = Scanner::new(d.path(), &Default::default()).scan_with_manifest();
    let captured = discover(
        d.path(),
        files.into_iter().map(|f| f.rel_path).collect(),
        Some(&manifest),
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(captured.report().config_reads, 2);
    for p in ["tsconfig.json", "base.json", "api.ts", "use.ts"] {
        std::fs::remove_file(d.path().join(p)).unwrap();
    }
    let mut samples = Vec::new();
    for count in [100, 1000, 10000] {
        for repeat in 0..10 {
            let start = Instant::now();
            for _ in 0..count {
                let r = module_resolution::resolve(
                    black_box(captured.model()),
                    "use.ts",
                    black_box("@api"),
                );
                assert_eq!(r.status, ModuleStatus::Resolved);
                assert_eq!(r.resolved_path.as_deref(), Some("api.ts"));
                black_box(r);
            }
            samples.push(json!({"imports":count,"repeat":repeat,"elapsed_ns":start.elapsed().as_nanos(),"discovery_reads":captured.report().config_reads,"source_removed_before_resolution":true}));
        }
    }
    save(
        "p3a-pure-cost.json",
        &json!({"scope":"pure immutable-snapshot resolution; no disk files remain","samples":samples}),
    );
}
