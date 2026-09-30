//! P2-B actual parser/SQLite/build tests. Canonical comparison retains target
//! identities and strategies AND the new durable resolution evidence tables.
use cc_eval::benchmark::oracle;
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::path::Path;
fn put(root: &Path, path: &str, source: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, source).unwrap();
}
fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, CodeIndex) {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    );
    for (p, s) in files {
        put(d.path(), p, s);
    }
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    (d, i)
}
fn observe(name: &str, value: &Value) {
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}
fn sequence(name: &str, files: &[(&str, &str)], changes: &[(&str, Option<&str>)]) -> Value {
    let (a, mut inc) = setup(files);
    let (b, mut full) = setup(files);
    let mut evidence = Vec::new();
    for (p, s) in changes {
        for root in [a.path(), b.path()] {
            if let Some(s) = s {
                put(root, p, s);
            } else {
                std::fs::remove_file(root.join(p)).unwrap();
            }
        }
        let report = inc.build_index(false).unwrap();
        full.build_index(true).unwrap();
        let ca = oracle::canonical(a.path()).unwrap();
        let cb = oracle::canonical(b.path()).unwrap();
        let differences: Vec<_> = ca
            .iter()
            .filter(|(k, v)| Some(*v) != cb.get(*k))
            .map(|(k, _)| k.clone())
            .collect();
        evidence.push(
            json!({"path":p,"report":report,"differences":differences,"incremental":ca,"full":cb}),
        );
        observe(name, &json!(evidence));
        assert!(differences.is_empty(), "{name}:{p}:{differences:?}");
    }
    // Reopen takes a cold catalog; no writes or errors may disappear on reopen.
    drop(inc);
    let mut reopened = CodeIndex::new(Some(a.path())).unwrap();
    let noop = reopened.build_index(false).unwrap();
    assert_eq!(noop.files_parsed, 0);
    serde_json::to_value(oracle::canonical(a.path()).unwrap()).unwrap()
}
fn call<'a>(facts: &'a Value, file: &str, name: &str) -> &'a Value {
    facts["call_edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["file_path"] == file && e["callee_symbol"] == name)
        .unwrap()
}
#[test]
fn missing_name_then_provider_appears_refreshes_unchanged_consumer() {
    let facts = sequence(
        "missing-name",
        &[("use.py", "def entry(x):\n    return newly_available(x)\n")],
        &[("api.py", Some("def newly_available(x):\n    return x\n"))],
    );
    assert_eq!(
        call(&facts, "use.py", "newly_available")["target_file_path"],
        "api.py"
    );
}
#[test]
fn global_unique_becomes_ambiguous_then_unique_again() {
    let facts = sequence(
        "name-collision",
        &[
            ("a.py", "def shared(x):\n    return x\n"),
            ("use.py", "def entry(x):\n    return shared(x)\n"),
        ],
        &[("b.py", Some("def shared(x):\n    return x + 1\n"))],
    );
    let edge = call(&facts, "use.py", "shared");
    assert!(
        edge["callee_symbol_uid"].is_null(),
        "a tie is not a resolved edge"
    );
    assert_eq!(edge["resolution_strategy"], "ambiguous");
    let rows = facts["resolution_manifests"].as_array().unwrap();
    let m: Value = serde_json::from_str(
        rows.iter().find(|r| r["file_path"] == "use.py").unwrap()["payload"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(m["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["query"] == "shared"
            && r["outcome"]["status"] == "ambiguous"
            && r["outcome"]["candidates"].as_array().unwrap().len() == 2));
    let fixed = sequence(
        "collision-delete",
        &[
            ("a.py", "def shared(x):\n    return x\n"),
            ("b.py", "def shared(x):\n    return x\n"),
            ("use.py", "def entry(x):\n    return shared(x)\n"),
        ],
        &[("b.py", None)],
    );
    assert_eq!(call(&fixed, "use.py", "shared")["target_file_path"], "a.py");
}
#[test]
fn negative_path_and_precedence_changes_refresh_import_routes() {
    let facts=sequence("missing-path",&[("use.ts","import { make } from './later'; export function entry(x: number): number { return make(x); }\n")],&[("later.ts",Some("export function make(x: number): number { return x; }\n"))]);
    assert_eq!(
        call(&facts, "use.ts", "make")["target_file_path"],
        "later.ts"
    );
    let facts=sequence("path-precedence",&[("later.js","export function make(x) {return x;}\n"),("use.ts","import { make } from './later'; export function entry(x: number): number {return make(x);}\n")],&[("later.ts",Some("export function make(x: number): number {return x+1;}\n"))]);
    assert_eq!(
        call(&facts, "use.ts", "make")["target_file_path"],
        "later.ts"
    );
}
#[test]
fn go_package_files_and_same_name_other_packages_are_isolated() {
    let facts = sequence(
        "go-membership",
        &[
            (
                "pkg/api.go",
                "package pkg\nfunc Exported(x int) int {return x}\n",
            ),
            (
                "pkg/use.go",
                "package pkg\nfunc Entry(x int) int {return Exported(x)}\n",
            ),
            (
                "other/api.go",
                "package other\nfunc Exported(x int) int {return x}\n",
            ),
        ],
        &[(
            "pkg/api.go",
            Some("package pkg\nfunc Exported(x int, extra int) int {return x + extra}\n"),
        )],
    );
    assert_eq!(
        call(&facts, "pkg/use.go", "Exported")["target_file_path"],
        "pkg/api.go"
    );
    let facts = sequence(
        "go-add-file",
        &[
            (
                "pkg/use.go",
                "package pkg\nfunc Entry(x int) int {return Available(x)}\n",
            ),
            (
                "other/api.go",
                "package other\nfunc Available(x int) int {return x}\n",
            ),
        ],
        &[
            (
                "pkg/new.go",
                Some("package pkg\nfunc Available(x int) int {return x}\n"),
            ),
            ("pkg/new.go", None),
        ],
    );
    assert!(call(&facts, "pkg/use.go", "Available")["callee_symbol_uid"].is_null());
}
#[test]
fn repeated_warm_catalog_mutations_match_cold_rebuild() {
    let facts = sequence(
        "warm-cold",
        &[
            ("a.py", "def shared(x):\n    return x\n"),
            ("use.py", "def entry(x):\n    return shared(x)\n"),
        ],
        &[
            ("b.py", Some("def shared(x):\n    return x\n")),
            (
                "b.py",
                Some("def shared(x, extra=0):\n    return x + extra\n"),
            ),
            ("b.py", None),
        ],
    );
    assert_eq!(call(&facts, "use.py", "shared")["target_file_path"], "a.py");
}
#[test]
fn go_internal_test_sees_production_not_reverse() {
    let facts = sequence(
        "go-test-variant",
        &[
            (
                "pkg/api.go",
                "package pkg\nfunc API(x int) int {return TestOnly(x)}\n",
            ),
            (
                "pkg/api_test.go",
                "package pkg\nfunc TestOnly(x int) int {return API(x)}\n",
            ),
        ],
        &[(
            "pkg/api.go",
            Some("package pkg\nfunc API(x int, n int) int {return TestOnly(x)}\n"),
        )],
    );
    assert!(call(&facts, "pkg/api.go", "TestOnly")["callee_symbol_uid"].is_null());
    assert_eq!(
        call(&facts, "pkg/api_test.go", "API")["target_file_path"],
        "pkg/api.go"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; real MCP process"]
async fn p2b_public_mcp_retracts_ambiguous_edges_and_updates_go_package() {
    use rmcp::{
        model::CallToolRequestParams,
        transport::{ConfigureCommandExt, TokioChildProcess},
        ServiceExt,
    };
    use std::time::Duration;
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    );
    for (p, s) in [
        ("a.py", "def shared(x):\n    return x\n"),
        ("use.py", "def entry(x):\n    return shared(x)\n"),
        (
            "go/api.go",
            "package local\nfunc Exported(x int) int {return x}\n",
        ),
        (
            "go/use.go",
            "package local\nfunc Entry(x int) int {return Exported(x)}\n",
        ),
        (
            "other/api.go",
            "package other\nfunc Exported(x int) int {return x}\n",
        ),
    ] {
        put(d.path(), p, s);
    }
    let binary = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let client = ()
        .serve(
            TokioChildProcess::new(tokio::process::Command::new(binary).configure(|cmd| {
                for (k, _) in std::env::vars_os() {
                    if k.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(k);
                    }
                }
                cmd.args(["mcp", "--project-path"])
                    .arg(d.path())
                    .current_dir(d.path())
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .stderr(std::process::Stdio::null());
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let call = |name: &str, args: Value| {
        client.call_tool(
            CallToolRequestParams::new(name.to_string())
                .with_arguments(args.as_object().unwrap().clone()),
        )
    };
    let mut receipts = Vec::new();
    for step in 0..3 {
        if step == 1 {
            put(d.path(), "b.py", "def shared(x):\n    return x + 1\n");
        }
        if step == 2 {
            std::fs::remove_file(d.path().join("b.py")).unwrap();
            put(
                d.path(),
                "go/api.go",
                "package local\nfunc Exported(x int, extra int) int {return x+extra}\n",
            );
        }
        let r = tokio::time::timeout(
            Duration::from_secs(25),
            call("index", json!({"path":d.path(),"full":step==0})),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(r.is_error, Some(true), "{r:?}");
        let report = r.structured_content.as_ref().unwrap()["result"].clone();
        let r=tokio::time::timeout(Duration::from_secs(15),call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = 'use.py' AND b.name = 'shared' RETURN b.file_path AS path, b.symbol_uid AS uid LIMIT 10"}))).await.unwrap().unwrap();
        assert_ne!(r.is_error, Some(true), "{r:?}");
        let graph = r.structured_content.as_ref().unwrap()["result"].clone();
        let rows = graph["results"].as_array().unwrap();
        observe(
            &format!("p2b-mcp-step{step}"),
            &json!({"report":report,"graph":graph}),
        );
        if step == 1 {
            assert!(
                rows.is_empty(),
                "ambiguous call must not retain a chosen target: {rows:?}"
            );
            assert!(report["resolution_coverage"]["ambiguous"].as_u64().unwrap() > 0);
        } else {
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|r| r["path"] == "a.py"));
        }
        let r=call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = 'go/use.go' AND b.name = 'Exported' RETURN b.file_path AS path, b.symbol_uid AS uid LIMIT 10"})).await.unwrap();
        assert_ne!(r.is_error, Some(true));
        let go = r.structured_content.as_ref().unwrap()["result"].clone();
        let rows = go["results"].as_array().unwrap();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| r["path"] == "go/api.go"));
        receipts.push(json!({"step":step,"report":report,"graph":graph,"go":go}));
    }
    assert_ne!(
        receipts[0]["go"]["results"][0]["uid"],
        receipts[2]["go"]["results"][0]["uid"]
    );
    observe("p2b-mcp", &json!(receipts));
    client.cancel().await.unwrap();
}

#[test]
fn rust_chain_call_identities_survive_incremental_roundtrip() {
    let before = include_str!("../fixtures/sample-project/api_handler.rs");
    let after = before.replace("0.1.0", "0.1.1");
    let facts = sequence(
        "rust-chain-sites",
        &[("api_handler.rs", before)],
        &[("api_handler.rs", Some(after.as_str()))],
    );
    let calls = facts["call_edges"].as_array().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|e| e["callee_symbol"] == "route")
            .count(),
        4,
        "all four route call sites must remain, not one SQL replacement winner"
    );
}
#[test]
fn go_unqualified_calls_do_not_bind_methods_from_peer_files() {
    let (d, _index) = setup(&[
        (
            "pkg/use.go",
            "package pkg\nfunc Entry(x int) int {return Phantom(x)}\n",
        ),
        (
            "pkg/type.go",
            "package pkg\ntype T struct{}\nfunc (t T) Phantom(x int) int {return x}\n",
        ),
    ]);
    let facts = serde_json::to_value(oracle::canonical(d.path()).unwrap()).unwrap();
    assert!(
        call(&facts, "pkg/use.go", "Phantom")["callee_symbol_uid"].is_null(),
        "method declarations need a receiver, not merely package membership"
    );
}
#[test]
fn declared_dependency_rename_retargets_after_p3b_workspace_migration() {
    let before="[package]\nname=\"consumer\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nvendor={package=\"one\",path=\"../one\"}\n";
    let after = before.replace(
        "package=\"one\",path=\"../one\"",
        "package=\"two\",path=\"../two\"",
    );
    let facts = sequence(
        "cargo-alias-config",
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers=[\"one\",\"two\",\"consumer\"]\nresolver=\"2\"\n",
            ),
            (
                "one/Cargo.toml",
                "[package]\nname=\"one\"\nversion=\"0.1.0\"\n",
            ),
            (
                "two/Cargo.toml",
                "[package]\nname=\"two\"\nversion=\"0.1.0\"\n",
            ),
            ("consumer/Cargo.toml", before),
            ("one/src/lib.rs", "pub fn fetch(x:i32)->i32{x}\n"),
            ("two/src/lib.rs", "pub fn fetch(x:i32)->i32{x+1}\n"),
            (
                "consumer/src/lib.rs",
                "use vendor::fetch;\npub fn entry(x:i32)->i32{fetch(x)}\n",
            ),
        ],
        &[("consumer/Cargo.toml", Some(after.as_str()))],
    );
    // P3-B now consumes the declared path dependency rename, not a global-name guess.
    assert_eq!(
        call(&facts, "consumer/src/lib.rs", "fetch")["target_file_path"],
        "two/src/lib.rs"
    );
    assert!(facts["resolution_dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["file_path"] == "consumer/src/lib.rs" && r["kind"] == "module_config"));
}
#[test]
fn supported_package_name_config_changes_rebind_the_unchanged_importer() {
    let vendor = "[package]\nname=\"vendor\"\nversion=\"0.1.0\"\n";
    let old = "[package]\nname=\"old_vendor\"\nversion=\"0.1.0\"\n";
    let other = "[package]\nname=\"other\"\nversion=\"0.1.0\"\n";
    let facts = sequence(
        "cargo-package-name-config",
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers=[\"one\",\"two\",\"consumer\"]\nresolver=\"2\"\n",
            ),
            ("one/Cargo.toml", vendor),
            ("two/Cargo.toml", other),
            (
                "consumer/Cargo.toml",
                "[package]\nname=\"consumer\"\nversion=\"0.1.0\"\n",
            ),
            ("one/src/lib.rs", "pub fn fetch(x:i32)->i32{x}\n"),
            ("two/src/lib.rs", "pub fn fetch(x:i32)->i32{x+1}\n"),
            (
                "consumer/src/lib.rs",
                "use vendor::fetch;\npub fn entry(x:i32)->i32{fetch(x)}\n",
            ),
        ],
        &[
            ("one/Cargo.toml", Some(old)),
            ("two/Cargo.toml", Some(vendor)),
        ],
    );
    assert_eq!(
        call(&facts, "consumer/src/lib.rs", "fetch")["target_file_path"],
        "two/src/lib.rs"
    );
}
#[test]
fn event_scoped_missing_file_and_candidate_changes_match_full_rebuild() {
    let source = "def entry(x):\n    return discovered(x)\n";
    let (a, mut inc) = setup(&[("use.py", source)]);
    let (b, mut full) = setup(&[("use.py", source)]);
    for (path, text) in [
        ("a.py", Some("def discovered(x):\n    return x\n")),
        ("b.py", Some("def discovered(x):\n    return x\n")),
        ("a.py", None),
    ] {
        for root in [a.path(), b.path()] {
            if let Some(s) = text {
                put(root, path, s);
            } else {
                std::fs::remove_file(root.join(path)).unwrap();
            }
        }
        let gate = inc.build_gate();
        let _permit = gate.lock().unwrap();
        let inputs = inc.build_inputs().unwrap();
        let scope = cc_index::BuildScope {
            changed: if text.is_some() {
                vec![path.into()]
            } else {
                vec![]
            },
            removed: if text.is_none() {
                vec![path.into()]
            } else {
                vec![]
            },
        };
        let prepared = CodeIndex::prepare_build_scoped(&inputs, false, None, Some(&scope)).unwrap();
        let report = inc.commit_build(&inputs, false, None, prepared).unwrap();
        full.build_index(true).unwrap();
        let left = oracle::canonical(a.path()).unwrap();
        let right = oracle::canonical(b.path()).unwrap();
        observe(
            &format!("scoped-{path}-{}", text.is_some()),
            &json!({"report":report,"equal":left==right}),
        );
        assert_eq!(left, right, "{path}");
    }
}
#[test]
fn dependency_fanout_over_budget_is_partial_not_normal() {
    let (d, mut index) = setup(&[
        (
            ".codecortex.json",
            r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
        ),
        ("a.py", "def a(x):\n    return appears(x)\n"),
        ("b.py", "def b(x):\n    return appears(x)\n"),
        ("c.py", "def c(x):\n    return appears(x)\n"),
    ]);
    put(d.path(), "provider.py", "def appears(x):\n    return x\n");
    let report = index.build_index(false).unwrap();
    let value = json!(report);
    observe("dependency-partial", &value);
    assert!(value.to_string().contains("budget_exceeded"), "{value}");
    let facts = serde_json::to_value(oracle::canonical(d.path()).unwrap()).unwrap();
    let resolved = facts["call_edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["file_path"] != "provider.py"
                && e["callee_symbol"] == "appears"
                && !e["callee_symbol_uid"].is_null()
        })
        .count();
    assert_eq!(
        resolved, 1,
        "promotion must respect its one-consumer budget"
    );
    // P2-C owns durable remainder recovery. Explicit full rebuild is the current recovery.
    index.build_index(true).unwrap();
    let facts = serde_json::to_value(oracle::canonical(d.path()).unwrap()).unwrap();
    assert_eq!(
        facts["call_edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["file_path"] != "provider.py"
                && e["callee_symbol"] == "appears"
                && !e["callee_symbol_uid"].is_null())
            .count(),
        3
    );
}
#[test]
fn body_only_edit_keeps_dependent_evidence_and_noop_is_empty() {
    let (d, mut index) = setup(&[
        ("api.py", "def make(x):\n    return x\n"),
        (
            "use.py",
            "from api import make\ndef entry(x):\n    return make(x)\n",
        ),
    ]);
    let before = oracle::canonical(d.path()).unwrap();
    put(
        d.path(),
        "api.py",
        "def make(x):\n    local_only = 123\n    return x + local_only\n",
    );
    let r = index.build_index(false).unwrap();
    assert_eq!(r.files_parsed, 1);
    let after = oracle::canonical(d.path()).unwrap();
    for table in ["resolution_manifests", "call_edges"] {
        let rows = |v: &std::collections::BTreeMap<String, Vec<Value>>| {
            v[table]
                .iter()
                .filter(|r| r["file_path"] == "use.py")
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&before), rows(&after));
    }
}
