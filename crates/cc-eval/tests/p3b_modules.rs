//! Independently authored module targets, exercised through real parser and SQLite.
use cc_eval::benchmark::oracle;
use cc_server::engine::CodeIndex;
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn target(root: &Path, file: &str, spec: &str) -> Option<String> {
    oracle::canonical(root).unwrap()["imports"]
        .iter()
        .find(|v| v["file_path"] == file && v["import_string"] == spec)
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
fn assert_parity(root: &Path, index: &mut CodeIndex, label: &str) {
    let before = oracle::canonical(root).unwrap();
    index.build_index(true).unwrap();
    let after = oracle::canonical(root).unwrap();
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join(format!("p3b-{label}.json")),
            serde_json::to_vec_pretty(
                &serde_json::json!({"before":before,"after":after,"equal":before==after}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    let different = before
        .iter()
        .filter(|(k, v)| after.get(*k) != Some(*v))
        .map(|(k, _)| k)
        .collect::<Vec<_>>();
    assert!(different.is_empty(), "{label}: {different:?}");
}
#[test]
fn missing_imported_member_cannot_bind_an_unrelated_global_name() {
    let d = setup();
    let r = d.path();
    put(
        r,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    put(
        r,
        "api.ts",
        "export function different(x:number):number{return x;}\n",
    );
    put(
        r,
        "other.ts",
        "export function ping(x:number):number{return x;}\n",
    );
    put(
        r,
        "use.ts",
        "import {ping} from './api'; export function entry(x:number):number{return ping(x);}\n",
    );
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    let facts = oracle::canonical(r).unwrap();
    let call = facts["call_edges"]
        .iter()
        .find(|v| v["file_path"] == "use.ts")
        .unwrap();
    assert!(call["target_symbol_id"].is_null(), "{call}");
}
#[test]
fn import_require_dynamic_and_type_only_survive_restart() {
    let d = setup();
    let r = d.path();
    put(r, "package.json", r#"{"workspaces":["pkg"]}"#);
    put(
        r,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    put(
        r,
        "pkg/package.json",
        r#"{"name":"demo","exports":{".":{"import":"./esm.ts","require":"./cjs.ts"}}}"#,
    );
    for f in ["esm.ts", "cjs.ts"] {
        put(
            r,
            &format!("pkg/{f}"),
            "export function ping(x:number):number{return x;}\n",
        );
    }
    put(r,"use.ts","import {ping as esm} from 'demo'; const {ping:cjs}=require('demo'); import lib = require('demo'); export async function run(){ await import('demo'); return esm(1)+cjs(2)+lib.ping(3); }\n");
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    let facts = oracle::canonical(r).unwrap();
    let rows = facts["imports"]
        .iter()
        .filter(|v| v["file_path"] == "use.ts")
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let ctx: serde_json::Value =
            serde_json::from_str(row["context_json"].as_str().unwrap()).unwrap();
        let path = if ctx["syntax"] == "require" {
            "pkg/cjs.ts"
        } else {
            "pkg/esm.ts"
        };
        assert_eq!(row["resolved_path"], path);
    }
    drop(i);
    let mut i = CodeIndex::new(Some(r)).unwrap();
    put(
        r,
        "pkg/package.json",
        r#"{"name":"demo","exports":{".":{"import":"./cjs.ts","require":"./esm.ts"}}}"#,
    );
    for _ in 0..8 {
        if i.build_index(false).unwrap().resolution_freshness.complete {
            break;
        }
    }
    assert_parity(r, &mut i, "syntax-reopen");
    put(
        r,
        "only.ts",
        "import type {ping} from 'demo'; export function bad(){return ping(1);}\n",
    );
    i.build_index(false).unwrap();
    let facts = oracle::canonical(r).unwrap();
    assert!(facts["call_edges"]
        .iter()
        .filter(|v| v["file_path"] == "only.ts")
        .all(|v| v["target_symbol_id"].is_null()));
}
#[test]
fn rust_inline_scope_path_attribute_and_default_features_reconcile() {
    let d = setup();
    let r = d.path();
    let cargo =
        "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n[features]\ndefault=[\"x\"]\nx=[]\n";
    put(r, "Cargo.toml", cargo);
    put(r, "src/lib.rs", "pub mod outer;\n");
    put(r,"src/outer.rs","#[path=\"custom.rs\"]pub mod api;\npub mod inner {use super::api::ping; pub fn entry(x:i32)->i32{ping(x)}}\n");
    put(r, "src/custom.rs", "pub fn ping(x:i32)->i32{x}\n");
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(r, "src/outer.rs", "super::api::ping").as_deref(),
        Some("src/custom.rs")
    );
    let old = i.build_index(false).unwrap();
    assert_eq!(old.project_model.rust_source_reads, 0);
    assert_eq!(old.project_model.rust_fact_cache_hits, 3);
    put(r,"src/outer.rs","#[cfg(feature=\"x\")]#[path=\"custom.rs\"]pub mod api;\npub mod inner {use super::api::ping; pub fn entry(x:i32)->i32{ping(x)}}\n");
    i.build_index(false).unwrap();
    put(
        r,
        "Cargo.toml",
        &cargo.replace("default=[\"x\"]", "default=[]"),
    );
    for _ in 0..8 {
        if i.build_index(false).unwrap().resolution_freshness.complete {
            break;
        }
    }
    assert!(target(r, "src/outer.rs", "super::api::ping").is_none());
    assert_parity(r, &mut i, "rust-cfg-inline");
}
#[test]
fn python_namespace_and_src_config_change_retarget_without_running_code() {
    let d = setup();
    let r = d.path();
    put(
        r,
        "pyproject.toml",
        "[tool.setuptools.packages.find]\nwhere=[\"one\"]\n",
    );
    for dir in ["one", "two"] {
        put(
            r,
            &format!("{dir}/ns/api.py"),
            "raise RuntimeError('indexing must not execute me')\ndef ping(x):\n    return x\n",
        );
    }
    put(
        r,
        "use.py",
        "from ns.api import ping\ndef entry(x):\n    return ping(x)\n",
    );
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(r, "use.py", "ns.api").as_deref(),
        Some("one/ns/api.py")
    );
    put(
        r,
        "pyproject.toml",
        "[tool.setuptools.packages.find]\nwhere=[\"two\"]\n",
    );
    for _ in 0..8 {
        if i.build_index(false).unwrap().resolution_freshness.complete {
            break;
        }
    }
    assert_eq!(
        target(r, "use.py", "ns.api").as_deref(),
        Some("two/ns/api.py")
    );
    assert_parity(r, &mut i, "python-config");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; actual stdio subprocess"]
async fn real_mcp_package_configuration_retarget_resumes_after_restart() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let d = setup();
    let root = d.path();
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"ignore":["**/*.json"],"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
    );
    put(root, "package.json", r#"{"workspaces":["pkg"]}"#);
    put(
        root,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    let manifest =
        |file: &str| json!({"name":"demo","exports":{".":format!("./{file}")}}).to_string();
    put(root, "pkg/package.json", &manifest("a.ts"));
    for p in ["pkg/a.ts", "pkg/b.ts"] {
        put(
            root,
            p,
            "export function ping(x:number):number{return x;}\n",
        );
    }
    for p in ["u.ts", "v.ts", "w.ts"] {
        put(
            root,
            p,
            "import {ping} from 'demo'; export function run(x:number):number{return ping(x);}\n",
        );
    }
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
        .await
        .unwrap();
    let first = m
        .call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    assert_eq!(first["resolution_coverage"]["modules_resolved"], 3);
    put(root, "pkg/package.json", &manifest("b.ts"));
    let mut steps = vec![];
    for n in 0..3 {
        if n == 1 {
            m.close().await.unwrap();
            m = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
                .await
                .unwrap();
        }
        let index = m
            .call(
                "index",
                json!({"path":root,"full":false,"changed_paths":["pkg/package.json"]}),
            )
            .await
            .unwrap();
        assert_eq!(index["files_parsed"], 0);
        assert_eq!(index["resolution_freshness"]["complete"], n == 2);
        let graph=m.call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE b.file_path = 'pkg/b.ts' RETURN a.file_path AS caller LIMIT 20"})).await.unwrap();
        assert_eq!(graph["results"].as_array().unwrap().len(), n + 1);
        steps.push(json!({"index":index,"graph":graph}));
    }
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join("p3b-mcp.json"),
            serde_json::to_vec_pretty(&json!({"first":first,"steps":steps})).unwrap(),
        )
        .unwrap();
    }
    m.close().await.unwrap();
}
#[test]
fn workspace_package_export_retargets_after_manifest_only_change() {
    let d = setup();
    let r = d.path();
    put(
        r,
        "package.json",
        r#"{"private":true,"workspaces":["packages/*"]}"#,
    );
    put(
        r,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    put(
        r,
        "packages/api/package.json",
        r#"{"name":"@demo/api","exports":{".":"./one.ts"}}"#,
    );
    for p in ["packages/api/one.ts", "packages/api/two.ts"] {
        put(r, p, "export function ping(x:number):number{return x;}\n");
    }
    put(
        r,
        "use.ts",
        "import {ping} from '@demo/api'; export function entry(){return ping(1);}\n",
    );
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(r, "use.ts", "@demo/api").as_deref(),
        Some("packages/api/one.ts")
    );
    put(
        r,
        "packages/api/package.json",
        r#"{"name":"@demo/api","exports":{".":"./two.ts"}}"#,
    );
    for _ in 0..8 {
        if i.build_index(false).unwrap().resolution_freshness.complete {
            break;
        }
    }
    assert_eq!(
        target(r, "use.ts", "@demo/api").as_deref(),
        Some("packages/api/two.ts")
    );
    let facts = oracle::canonical(r).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(facts, oracle::canonical(r).unwrap());
}
#[test]
fn python_relative_import_never_uses_javascript_probes() {
    let d = setup();
    let r = d.path();
    put(r, "src/pkg/__init__.py", "");
    put(r, "src/pkg/api.py", "def ping(x):\n    return x\n");
    put(
        r,
        "src/pkg/api.ts",
        "export function ping(x:number){return x;}\n",
    );
    put(
        r,
        "src/pkg/use.py",
        "from .api import ping\ndef entry(x):\n    return ping(x)\n",
    );
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(r, "src/pkg/use.py", ".api").as_deref(),
        Some("src/pkg/api.py")
    );
}
#[test]
fn rust_crate_modules_resolve_to_the_declared_file() {
    let d = setup();
    let r = d.path();
    put(
        r,
        "Cargo.toml",
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    put(
        r,
        "src/lib.rs",
        "pub mod api;\nuse crate::api::ping;\npub fn entry(x:i32)->i32{ping(x)}\n",
    );
    put(r, "src/api.rs", "pub fn ping(x:i32)->i32{x}\n");
    let mut i = CodeIndex::new(Some(r)).unwrap();
    i.build_index(true).unwrap();
    assert_eq!(
        target(r, "src/lib.rs", "crate::api::ping").as_deref(),
        Some("src/api.rs")
    );
}
