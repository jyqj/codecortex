//! Actual parser/SQLite integration; expected paths are independently authored.
use cc_eval::benchmark::oracle;
use cc_server::engine::CodeIndex;
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn target(root: &Path, file: &str) -> Option<String> {
    oracle::canonical(root).unwrap()["imports"]
        .iter()
        .find(|v| v["file_path"] == file)
        .and_then(|v| v["resolved_path"].as_str().map(str::to_owned))
}
fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
    );
    d
}
fn drain(index: &mut CodeIndex) {
    for _ in 0..12 {
        if index
            .build_index(false)
            .unwrap()
            .resolution_freshness
            .complete
        {
            return;
        }
    }
    panic!("configuration invalidation failed to converge");
}
#[test]
fn p3a_ignored_json_config_scoped_change_reloads_without_reparsing_source() {
    use cc_eval::runner::CodeIndexBackend;
    let d = setup();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"ignore":["**/*.json"],"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
    );
    for p in ["a.ts", "b.ts"] {
        put(
            d.path(),
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    for p in ["u.ts", "v.ts", "w.ts"] {
        put(
            d.path(),
            p,
            "import {ping} from '@api'; export function run(){return ping(1);}\n",
        );
    }
    put(d.path(), "tsconfig.json", r#"{"extends":"./cfg/base"}"#);
    let config = |p: &str| {
        format!(
            r#"{{"compilerOptions":{{"moduleResolution":"bundler","paths":{{"@api":["../{p}"]}}}}}}"#
        )
    };
    put(d.path(), "cfg/base.json", &config("a.ts"));
    let b = CodeIndexBackend::new(d.path()).unwrap();
    assert_eq!(target(d.path(), "u.ts").as_deref(), Some("a.ts"));
    put(d.path(), "cfg/base.json", &config("b.ts"));
    let report = b
        .build_index_report_scoped(&["cfg/base.json".into()])
        .unwrap();
    assert_eq!(report["files_parsed"], 0);
    assert_eq!(
        report["project_model"]["inventory_source"],
        "scoped_catalog"
    );
    assert_eq!(report["resolution_freshness"]["complete"], false);
    drop(b);
    let b = CodeIndexBackend::open_existing(d.path()).unwrap();
    let mut ready = false;
    let mut reports = vec![report];
    for _ in 0..8 {
        let next = b
            .build_index_report_scoped(&["cfg/base.json".into()])
            .unwrap();
        let complete = next["resolution_freshness"]["complete"] == true;
        reports.push(next);
        if complete {
            ready = true;
            break;
        }
    }
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(Path::new(&path).join("ignored-config.json"),serde_json::to_vec_pretty(&serde_json::json!({"reports":reports,"facts":oracle::canonical(d.path()).unwrap()})).unwrap()).unwrap();
    }
    assert!(ready, "config replay reports: {reports:?}");
    for p in ["u.ts", "v.ts", "w.ts"] {
        assert_eq!(target(d.path(), p).as_deref(), Some("b.ts"));
    }
    let before = oracle::canonical(d.path()).unwrap();
    assert!(!before["files"]
        .iter()
        .any(|f| f["file_path"].as_str().unwrap().ends_with(".json")));
    b.build_index_report(true).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
}
#[test]
fn p3a_unsupported_module_mode_cannot_be_upgraded_by_global_name_fallback() {
    let d = setup();
    put(
        d.path(),
        "api.ts",
        "export function ping(x:number):number{return x;}\n",
    );
    put(
        d.path(),
        "use.ts",
        "import {ping} from './api'; export function run(){return ping(1);}\n",
    );
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"classic"}}"#,
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    let report = i.build_index(true).unwrap();
    assert_eq!(report.resolution_coverage.modules_unsupported, 1);
    let facts = oracle::canonical(d.path()).unwrap();
    let edge = facts["call_edges"]
        .iter()
        .find(|e| e["file_path"] == "use.ts" && e["callee_symbol"] == "ping")
        .unwrap();
    assert!(edge["target_symbol_id"].is_null());
    assert_eq!(
        edge["resolution_strategy"],
        cc_model::project_model::MODULE_BLOCKED_BINDING
    );
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    drain(&mut i);
    assert_eq!(target(d.path(), "use.ts").as_deref(), Some("api.ts"));
    let facts = oracle::canonical(d.path()).unwrap();
    assert!(facts["call_edges"]
        .iter()
        .any(|e| e["file_path"] == "use.ts" && e["target_file_path"] == "api.ts"));
    i.build_index(true).unwrap();
    assert_eq!(facts, oracle::canonical(d.path()).unwrap());
}
#[test]
fn p3a_a_new_higher_priority_alias_target_invalidates_old_imports() {
    let d = setup();
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@api":["./preferred","./fallback"]}}}"#,
    );
    put(
        d.path(),
        "fallback.ts",
        "export function ping(x:number):number{return x;}\n",
    );
    put(
        d.path(),
        "use.ts",
        "import {ping} from '@api'; export function run(){return ping(1);}\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(target(d.path(), "use.ts").as_deref(), Some("fallback.ts"));
    let facts = oracle::canonical(d.path()).unwrap();
    assert!(facts["resolution_dependencies"]
        .iter()
        .any(|d| d["file_path"] == "use.ts"
            && d["kind"] == "missing_path"
            && d["key"] == "preferred.ts"));
    put(
        d.path(),
        "preferred.ts",
        "export function ping(x:number):number{return x+1;}\n",
    );
    drain(&mut i);
    assert_eq!(target(d.path(), "use.ts").as_deref(), Some("preferred.ts"));
    let before = oracle::canonical(d.path()).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
}
#[test]
fn p3a_hidden_configuration_event_retargets_the_index() {
    use cc_eval::runner::CodeIndexBackend;
    let d = setup();
    for p in ["a.ts", "b.ts"] {
        put(d.path(), p, "export function ping(x:number){return x;}\n");
    }
    put(
        d.path(),
        "use.ts",
        "import {ping} from '@api'; export function run(){return ping(1);}\n",
    );
    put(d.path(), "tsconfig.json", r#"{"extends":"./.config/base"}"#);
    let config = |p: &str| {
        serde_json::json!({"compilerOptions":{"paths":{"@api":[format!("../{p}")]}}}).to_string()
    };
    put(d.path(), ".config/base.json", &config("a.ts"));
    let b = CodeIndexBackend::new(d.path()).unwrap();
    assert_eq!(target(d.path(), "use.ts").as_deref(), Some("a.ts"));
    put(d.path(), ".config/base.json", &config("b.ts"));
    let r = b
        .build_index_report_scoped(&[".config/base.json".into()])
        .unwrap();
    assert_eq!(r["resolution_freshness"]["complete"], true);
    assert_eq!(target(d.path(), "use.ts").as_deref(), Some("b.ts"));
    let before = oracle::canonical(d.path()).unwrap();
    b.build_index_report(true).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
}

#[test]
fn p3a_new_and_deleted_nested_roots_are_observed_with_scoped_events() {
    use cc_eval::runner::CodeIndexBackend;
    let d = setup();
    for p in ["a.ts", "pkg/b.ts"] {
        put(
            d.path(),
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@api":["./a"]}}}"#,
    );
    put(
        d.path(),
        "pkg/use.ts",
        "import {ping} from '@api'; export function run(){return ping(1);}\n",
    );
    let b = CodeIndexBackend::new(d.path()).unwrap();
    assert_eq!(target(d.path(), "pkg/use.ts").as_deref(), Some("a.ts"));
    put(
        d.path(),
        "pkg/tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@api":["./b"]}}}"#,
    );
    b.build_index_report_scoped(&["pkg/tsconfig.json".into()])
        .unwrap();
    assert_eq!(target(d.path(), "pkg/use.ts").as_deref(), Some("pkg/b.ts"));
    std::fs::remove_file(d.path().join("pkg/tsconfig.json")).unwrap();
    b.build_index_report_scoped(&["pkg/tsconfig.json".into()])
        .unwrap();
    assert_eq!(target(d.path(), "pkg/use.ts").as_deref(), Some("a.ts"));
    let before = oracle::canonical(d.path()).unwrap();
    b.build_index_report(true).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
}
#[test]
fn p3a_prepare_rejects_changed_configuration_before_publication() {
    use cc_db::index_db::IndexDb;
    use cc_index::Indexer;
    use std::sync::Arc;
    let d = setup();
    let temp = tempfile::tempdir().unwrap();
    let db = Arc::new(IndexDb::open(&temp.path().join("index.sqlite3")).unwrap().0);
    for p in ["a.ts", "b.ts"] {
        put(
            d.path(),
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    put(d.path(), "use.ts", "import {ping} from '@api'; ping(1);\n");
    let config = |p: &str| format!(r#"{{"compilerOptions":{{"paths":{{"@api":["./{p}"]}}}}}}"#);
    put(d.path(), "tsconfig.json", &config("a.ts"));
    let index = Indexer::new(db.clone(), d.path(), &Default::default());
    index.build_index(d.path(), true).unwrap();
    let prepared = index.prepare_build(d.path(), false, None).unwrap();
    let epoch = db.reads().generation().unwrap();
    put(d.path(), "tsconfig.json", &config("b.ts"));
    assert!(index.commit_build(d.path(), false, None, prepared).is_err());
    assert_eq!(epoch, db.reads().generation().unwrap());
    index.build_index(d.path(), false).unwrap();
    let rows = db
        .reads()
        .query_json(
            "SELECT resolved_path FROM imports WHERE file_path='use.ts'",
            &[],
        )
        .unwrap();
    assert_eq!(rows[0]["resolved_path"], "b.ts");
}
#[test]
fn p3a_corrupt_persisted_config_cache_is_not_reused() {
    use cc_db::index_db::IndexDb;
    let d = setup();
    put(d.path(), "use.ts", "export const x=1;\n");
    put(d.path(), "tsconfig.json", "{}");
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    let db = IndexDb::open(&d.path().join(".codecortex/index.sqlite3"))
        .unwrap()
        .0;
    let key = cc_model::project_model::PROJECT_INPUT_KEY;
    let mut v: serde_json::Value =
        serde_json::from_str(&db.reads().get_metadata(key).unwrap().unwrap()).unwrap();
    v["digest"] = serde_json::json!("bad");
    db.writes().set_metadata(key, &v.to_string()).unwrap();
    assert!(cc_index::project_model::read_inputs(&db).is_err());
    assert!(i.build_index(false).is_err());
    drop(db);
    i.build_index(true).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; real MCP subprocess"]
async fn p3a_real_mcp_configuration_only_retarget_and_restart() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let d = setup();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"ignore":["**/*.json"],"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
    );
    for p in ["a.ts", "b.ts"] {
        put(
            d.path(),
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    for p in ["u.ts", "v.ts", "w.ts"] {
        put(
            d.path(),
            p,
            "import {ping} from '@api'; export function run(){return ping(1);}\n",
        );
    }
    let config = |p: &str| {
        format!(
            r#"{{"compilerOptions":{{"moduleResolution":"bundler","paths":{{"@api":["./{p}"]}}}}}}"#
        )
    };
    put(d.path(), "tsconfig.json", &config("a.ts"));
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
        .await
        .unwrap();
    let first = m
        .call("index", json!({"path":d.path(),"full":true}))
        .await
        .unwrap();
    assert_eq!(first["project_model"]["config_roots"], 1);
    put(d.path(), "tsconfig.json", &config("b.ts"));
    let mut receipts = vec![];
    for n in 0..3 {
        if n == 1 {
            m.close().await.unwrap();
            m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
                .await
                .unwrap();
        }
        let r = m
            .call(
                "index",
                json!({"path":d.path(),"full":false,"changed_paths":["tsconfig.json"]}),
            )
            .await
            .unwrap();
        assert_eq!(r["files_parsed"], 0);
        assert_eq!(r["resolution_freshness"]["complete"], n == 2);
        let graph=m.call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE b.file_path = 'b.ts' RETURN a.file_path AS caller LIMIT 20"})).await.unwrap();
        assert_eq!(graph["results"].as_array().unwrap().len(), n + 1);
        receipts.push(json!({"index":r,"graph":graph}));
    }
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            Path::new(&path).join("p3a-mcp.json"),
            serde_json::to_vec_pretty(&json!({"first":first,"steps":receipts})).unwrap(),
        )
        .unwrap();
    }
    m.close().await.unwrap();
}

#[test]
fn p3a_alias_uses_nearest_config_and_ordered_fallback_targets() {
    let d = setup();
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@app/*":["./root/*"]}}}"#,
    );
    put(
        d.path(),
        "pkg/tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@app/*":["./missing/*","./src/*"]}}}"#,
    );
    for p in ["root/api.ts", "pkg/src/api.ts", "other/api.ts"] {
        put(
            d.path(),
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    put(
        d.path(),
        "pkg/use.ts",
        "import {ping} from '@app/api'; export function run(){return ping(1);}\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(d.path(), "pkg/use.ts").as_deref(),
        Some("pkg/src/api.ts")
    );
    let facts = oracle::canonical(d.path()).unwrap();
    assert!(facts["call_edges"]
        .iter()
        .any(|e| e["file_path"] == "pkg/use.ts" && e["target_file_path"] == "pkg/src/api.ts"));
}
#[test]
fn p3a_inherited_paths_use_defining_config_directory_and_jsonc_strings() {
    let d = setup();
    put(
        d.path(),
        "config/base.json",
        r#"{ /* https://not-a-comment-in-string */ "compilerOptions":{"moduleResolution":"bundler","paths":{"@api":["../src/api.ts"],"https://example/*":["../src/*"]},},}"#,
    );
    put(
        d.path(),
        "pkg/tsconfig.json",
        r#"{"extends":"../config/base"}"#,
    );
    put(
        d.path(),
        "src/api.ts",
        "export function ping(x:number):number{return x;}\n",
    );
    put(
        d.path(),
        "pkg/use.ts",
        "import {ping} from '@api'; export function run(){return ping(1);}\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(d.path(), "pkg/use.ts").as_deref(),
        Some("src/api.ts")
    );
}
#[test]
fn p3a_configuration_only_change_retargets_unchanged_consumers_and_survives_reopen() {
    let d = setup();
    for p in ["a.ts", "b.ts"] {
        put(
            d.path(),
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    for p in ["u.ts", "v.ts", "w.ts"] {
        put(
            d.path(),
            p,
            "import {ping} from '@api'; export function run(){return ping(1);}\n",
        );
    }
    let config = |p: &str| {
        format!(
            r#"{{"compilerOptions":{{"moduleResolution":"bundler","paths":{{"@api":["./{p}"]}}}}}}"#
        )
    };
    put(d.path(), "tsconfig.json", &config("a.ts"));
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(target(d.path(), "u.ts").as_deref(), Some("a.ts"));
    put(d.path(), "tsconfig.json", &config("b.ts"));
    i.build_index(false).unwrap();
    drop(i);
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    drain(&mut i);
    for p in ["u.ts", "v.ts", "w.ts"] {
        assert_eq!(target(d.path(), p).as_deref(), Some("b.ts"));
    }
    let before = oracle::canonical(d.path()).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
}
