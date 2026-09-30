//! Explicit release mechanism observations: no external services or developer index.
use cc_db::index_db::IndexDb;
use cc_index::{module_resolution, project_model::discover, BuildScope, Indexer, Scanner};
use cc_model::config::IndexingConfig;
use serde_json::json;
use std::{hint::black_box, path::Path, sync::Arc, time::Instant};
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn save(name: &str, value: serde_json::Value) {
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}
#[test]
#[ignore = "explicit --release package configuration mechanism measurements"]
#[allow(clippy::assertions_on_constants)]
fn package_config_resume_does_not_reparse_consumers_or_reload_per_import() {
    assert!(!cfg!(debug_assertions));
    let mut samples = vec![];
    for n in [1000, 5000] {
        let d = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let root = d.path();
        for k in 0..n {
            put(
                root,
                &format!("cold{k}.ts"),
                &format!("export const cold{k}={k};\n"),
            );
        }
        for k in 0..25 {
            put(root,&format!("use{k:02}.ts"),&format!("import {{ping}} from 'demo'; export function run{k}(x:number):number{{return ping(x);}}\n"));
        }
        put(root, "package.json", r#"{"workspaces":["pkg"]}"#);
        put(
            root,
            "tsconfig.json",
            r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
        );
        for p in ["pkg/a.ts", "pkg/b.ts"] {
            put(
                root,
                p,
                "export function ping(x:number):number{return x;}\n",
            );
        }
        let manifest =
            |p: &str| json!({"name":"demo","exports":{".":format!("./{p}")}}).to_string();
        put(root, "pkg/package.json", &manifest("a.ts"));
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
        let i = Indexer::new(db.clone(), root, &config);
        let start = Instant::now();
        let report = i.build_index(root, true).unwrap();
        assert_eq!(report.project_model.config_reads, 3);
        samples.push(
            json!({"n":n,"kind":"full","elapsed_us":start.elapsed().as_micros(),"report":report}),
        );
        for iteration in 0..10 {
            let start = Instant::now();
            let report = i.build_index(root, false).unwrap();
            assert_eq!(report.files_parsed, 0);
            assert_eq!(report.dirty_plan.selected_dependents, 0);
            assert_eq!(report.project_model.config_parse_cache_hits, 3);
            samples.push(json!({"n":n,"kind":"noop","iteration":iteration,"elapsed_us":start.elapsed().as_micros(),"report":report}));
            let file = if iteration % 2 == 0 { "b.ts" } else { "a.ts" };
            put(root, "pkg/package.json", &manifest(file));
            let scope = BuildScope {
                changed: vec!["pkg/package.json".into()],
                removed: vec![],
            };
            let mut ready = false;
            for step in 0..8 {
                let start = Instant::now();
                let p = i
                    .prepare_build_scoped(root, false, None, Some(&scope))
                    .unwrap();
                let report = i.commit_build(root, false, None, p).unwrap();
                assert_eq!(report.files_parsed, 0);
                assert_eq!(report.project_model.config_reads, 3);
                assert_eq!(report.project_model.config_root_probes, 0);
                assert!(report.dirty_plan.selected_dependents <= 5);
                ready = report.resolution_freshness.complete;
                samples.push(json!({"n":n,"kind":"package_resume","iteration":iteration,"step":step,"elapsed_us":start.elapsed().as_micros(),"report":report}));
                if ready {
                    break;
                }
            }
            assert!(ready);
            let rows = db
                .reads()
                .query_json(
                    "SELECT resolved_path FROM imports WHERE file_path LIKE 'use%.ts'",
                    &[],
                )
                .unwrap();
            assert_eq!(rows.len(), 25);
            assert!(rows
                .iter()
                .all(|r| r["resolved_path"] == format!("pkg/{file}")));
        }
    }
    save(
        "p3b-package-cost.json",
        json!({"scope":"release in-process indexed configuration mechanism; not RSS or tail latency certification","samples":samples}),
    );
}
#[test]
#[ignore = "explicit --release Rust catalog/fact-cache observations"]
#[allow(clippy::assertions_on_constants)]
fn rust_module_queries_reuse_captured_facts_and_indexed_lookup() {
    assert!(!cfg!(debug_assertions));
    let mut samples = vec![];
    for n in [100, 1000] {
        let d = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let root = d.path();
        put(
            root,
            "Cargo.toml",
            "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n",
        );
        let mut lib = String::new();
        for k in 0..n {
            lib.push_str(&format!("pub mod m{k};\n"));
            put(
                root,
                &format!("src/m{k}.rs"),
                &format!("pub fn value{k}()->usize{{{k}}}\n"),
            );
        }
        put(root, "src/lib.rs", &lib);
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&storage.path().join("index.sqlite3"), 1)
                .unwrap()
                .0,
        );
        let i = Indexer::new(db.clone(), root, &Default::default());
        let report = i.build_index(root, true).unwrap();
        samples.push(json!({"n":n,"kind":"full","report":report}));
        for round in 0..6 {
            let start = Instant::now();
            let report = i.build_index(root, false).unwrap();
            assert_eq!(report.files_parsed, 0);
            assert_eq!(report.project_model.rust_source_reads, 0);
            assert_eq!(report.project_model.rust_fact_cache_hits, n + 1);
            samples.push(json!({"n":n,"kind":"noop","round":round,"elapsed_us":start.elapsed().as_micros(),"report":report}));
        }
        let (files, walk) = Scanner::new(root, &Default::default()).scan_with_manifest();
        let captured = discover(
            root,
            files.into_iter().map(|f| f.rel_path).collect(),
            Some(&walk),
            None,
            &Default::default(),
        )
        .unwrap();
        std::fs::remove_file(root.join("Cargo.toml")).unwrap();
        for k in 0..n {
            std::fs::remove_file(root.join(format!("src/m{k}.rs"))).unwrap();
        }
        for round in 0..10 {
            let start = Instant::now();
            for k in 0..10000 {
                let j = k % n;
                let out = module_resolution::resolve(
                    captured.model(),
                    "src/lib.rs",
                    &format!("crate::m{j}::value{j}"),
                );
                assert_eq!(out.resolved_path, Some(format!("src/m{j}.rs")));
                black_box(out);
            }
            samples.push(json!({"n":n,"kind":"pure","round":round,"lookups":10000,"elapsed_us":start.elapsed().as_micros(),"disk_sources_removed":true}));
        }
    }
    save(
        "p3b-rust-cost.json",
        json!({"scope":"release catalog and persisted syntax reuse; no interpreter/compiler oracle","samples":samples}),
    );
}
