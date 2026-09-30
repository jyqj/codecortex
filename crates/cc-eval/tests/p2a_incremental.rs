//! Real parser + SQLite + incremental/full equivalence, with independent binding
//! assertions. No mock resolver, changed gold, or dropped strategy/UID columns.
use cc_eval::benchmark::{manifest, mutations::MutationPlan, oracle};
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn record(name: &str, value: &impl serde::Serialize) {
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            Path::new(&path).join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}
fn original_oracle(language: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let suite =
        manifest::load(&root.join(format!("benchmarks/manifests/p0-{language}-api.json"))).unwrap();
    let mutation: MutationPlan = serde_json::from_slice(
        &std::fs::read(root.join(format!("benchmarks/mutations/p0-{language}-signature.json")))
            .unwrap(),
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    let result = oracle::run(&suite, &mutation, out.path()).unwrap();
    let inc: Value = serde_json::from_slice(
        &std::fs::read(out.path().join("checkpoint-0-incremental.json")).unwrap(),
    )
    .unwrap();
    record(&format!("original-{language}-oracle"), &result);
    record(&format!("original-{language}-facts"), &inc);
    assert_current_binding(
        &inc,
        if language == "rust" {
            "lib.rs"
        } else {
            "consumer.py"
        },
        if language == "rust" {
            "provider.rs"
        } else {
            "provider.py"
        },
        "transform",
    );
    assert_eq!(
        result["equal"], true,
        "unmodified original oracle: {}",
        result["checkpoints"][0]["different_tables"]
    );
}
fn assert_current_binding(facts: &Value, consumer: &str, provider: &str, name: &str) {
    let target = facts["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["file_path"] == provider && s["name"] == name)
        .unwrap();
    let uid = &target["symbol_uid"];
    assert!(uid.is_string());
    let edges: Vec<_> = facts["call_edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["file_path"] == consumer && e["callee_symbol"] == name)
        .collect();
    assert!(
        !edges.is_empty(),
        "missing independently expected {consumer}->{provider}:{name}"
    );
    for edge in edges {
        assert_eq!(&edge["callee_symbol_uid"], uid);
        assert_eq!(edge["target_file_path"], provider);
    }
    for edge in facts["call_edges"].as_array().unwrap() {
        if let Some(uid) = edge["callee_symbol_uid"].as_str() {
            assert!(
                facts["symbols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["symbol_uid"] == uid),
                "dangling {edge}"
            );
        }
    }
}
#[test]
fn original_python_signature_matches_full_and_has_current_uid() {
    original_oracle("python");
}
#[test]
fn original_rust_signature_matches_full_and_has_current_uid() {
    original_oracle("rust");
}

fn fixture(files: &[(&str, &str)], direct: bool) -> (tempfile::TempDir, CodeIndex) {
    let d = tempfile::tempdir().unwrap();
    put(d.path(),".codecortex.json",&json!({"auto_index":{"enabled":false},"indexing":{"use_direct_writer":direct,"db_read_pool_size":1}}).to_string());
    for (p, s) in files {
        put(d.path(), p, s);
    }
    let mut index = CodeIndex::new(Some(d.path())).unwrap();
    index.build_index(true).unwrap();
    (d, index)
}
fn compare_sequence(
    name: &str,
    files: &[(&str, &str)],
    changes: &[(&str, &str)],
    direct: bool,
) -> Value {
    let (a, mut incremental) = fixture(files, direct);
    let (b, mut full) = fixture(files, direct);
    let mut observations = Vec::new();
    for (path, source) in changes {
        put(a.path(), path, source);
        put(b.path(), path, source);
        let report = incremental.build_index(false).unwrap();
        full.build_index(true).unwrap();
        let ca = oracle::canonical(a.path()).unwrap();
        let cb = oracle::canonical(b.path()).unwrap();
        let differences: Vec<_> = ca
            .iter()
            .filter(|(table, rows)| Some(*rows) != cb.get(*table))
            .map(|(t, _)| t.clone())
            .collect();
        observations.push(json!({"mutation":path,"different_tables":differences,"report":report,"incremental":ca,"full":cb}));
        record(name, &observations);
        assert!(differences.is_empty(), "{name} {path}: {differences:?}");
    }
    let noop = incremental.build_index(false).unwrap();
    assert_eq!(noop.files_parsed, 0);
    assert_eq!(noop.public_surface_coverage.parsed_files, 0);
    serde_json::to_value(oracle::canonical(a.path()).unwrap()).unwrap()
}
#[test]
fn typescript_signature_and_local_type_dependencies_converge() {
    compare_sequence("ts-signature",&[
        ("api.ts","export function transform(x: number): number { return x; }\n"),
        ("use.ts","import { transform } from './api';\nexport function entry(x: number): number { return transform(x); }\n")
    ],&[("api.ts","export function transform(x: number, extra = 0): number { return x + extra; }\n")],false);
    compare_sequence("ts-local-type",&[
        ("api.ts","type Shape = { value: string }; export function transform(x: Shape): Shape { return x; }\n"),
        ("use.ts","import { transform } from './api'; export const entry = transform;\n")
    ],&[("api.ts","type Shape = { value: number; optional?: boolean }; export function transform(x: Shape): Shape { return x; }\n")],false);
}
#[test]
fn forwarding_alias_and_type_only_changes_survive_incremental_writes() {
    let facts = compare_sequence("ts-forward",&[
        ("base.ts","export type Shape = { a: string }; export function transform(x: number): number { return x; }\n"),
        ("facade.ts","export type { Shape as Public } from './base'; export { transform } from './base';\n"),
        ("consumer.ts","import { transform } from './facade'; export function entry(x: number): number { return transform(x); }\n")
    ],&[
        ("base.ts","export type Shape = { a: number; b?: string }; export function transform(x: number, n=1): number { return x + n; }\n"),
        ("facade.ts","export type { Shape as Renamed } from './base'; export { transform } from './base';\n")
    ],false);
    assert_current_binding(&facts, "consumer.ts", "base.ts", "transform");
}
#[test]
fn python_package_forwarding_declares_star_incompleteness() {
    let files = [
        ("pkg/api.py", "def transform(x):\n    return x\n"),
        (
            "pkg/__init__.py",
            "from .api import transform\n__all__ = ['transform']\n",
        ),
        (
            "consumer.py",
            "from pkg import transform\ndef entry(x):\n    return transform(x)\n",
        ),
    ];
    let facts = compare_sequence(
        "py-package",
        &files,
        &[(
            "pkg/api.py",
            "def transform(x, extra=0):\n    return x + extra\n",
        )],
        false,
    );
    assert_current_binding(&facts, "consumer.py", "pkg/api.py", "transform");
    let (d, mut i) = fixture(
        &[
            ("api.py", "def f(x):\n    return x\n"),
            ("facade.py", "from api import *\n"),
        ],
        false,
    );
    put(
        d.path(),
        "facade.py",
        "from api import *\n# changed comment to force parse\n",
    );
    let r = i.build_index(false).unwrap();
    assert_eq!(r.public_surface_coverage.unknown, 1);
    assert!(r
        .public_surface_coverage
        .unknown_reasons
        .contains_key("python_star_surface_requires_resolution"));
}
#[test]
fn rust_workspace_pub_use_signature_change_converges() {
    let facts = compare_sequence("rust-forward", &[
        ("Cargo.toml", "[workspace]\nmembers = [\"crate_a\",\"crate_b\",\"crate_c\"]\nresolver = \"2\"\n"),
        ("crate_a/Cargo.toml", "[package]\nname=\"crate_a\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"),
        ("crate_b/Cargo.toml", "[package]\nname=\"crate_b\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\ncrate_a={path=\"../crate_a\"}\n"),
        ("crate_c/Cargo.toml", "[package]\nname=\"crate_c\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\ncrate_b={path=\"../crate_b\"}\n"),
        ("crate_a/src/lib.rs", "pub fn transform() -> i32 { 1 }\n"),
        ("crate_b/src/lib.rs", "pub use crate_a::transform;\n"),
        ("crate_c/src/lib.rs", "use crate_b::transform;\npub fn entry() -> i64 { transform() as i64 }\n"),
    ], &[("crate_a/src/lib.rs", "pub fn transform() -> i64 { 1 }\n")], false);
    assert_current_binding(
        &facts,
        "crate_c/src/lib.rs",
        "crate_a/src/lib.rs",
        "transform",
    );
    assert!(facts["imports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["file_path"] == "crate_b/src/lib.rs"
            && r["resolved_path"] == "crate_a/src/lib.rs"
            && r["is_reexport"] == 1));
}

#[test]
fn body_only_changes_keep_surface_and_do_not_promote_callers() {
    for (provider,consumer,before,after,use_source) in [
        ("a.py","b.py","def transform(x):\n    return x\n","def transform(x):\n    private = 123\n    return x + private\n","from a import transform\ndef entry(x):\n    return transform(x)\n"),
        ("a.rs","b.rs","pub fn transform(x: i32) -> i32 { x }\n","pub fn transform(x: i32) -> i32 { let private = 123; x + private }\n","mod a; use a::transform; pub fn entry(x: i32) -> i32 { transform(x) }\n"),
        ("a.ts","b.ts","export function transform(x: number): number {return x;}\n","export function transform(x: number): number {const privateValue=123;return x+privateValue;}\n","import { transform } from './a'; export function entry(x: number): number {return transform(x);}\n"),
    ] {
        let (d,mut i)=fixture(&[(provider,before),(consumer,use_source)],false);
        let old=oracle::canonical(d.path()).unwrap();put(d.path(),provider,after);
        let report=i.build_index(false).unwrap();let new=oracle::canonical(d.path()).unwrap();
        assert_eq!(old["public_surfaces"],new["public_surfaces"]);
        assert_eq!(report.files_parsed,1);
        let provider_symbols=new["symbols"].iter().filter(|s|s["file_path"]==provider).count();
        assert_eq!(report.symbols_total,provider_symbols,"unchanged consumer unnecessarily promoted: {provider}");
        let old_edges:Vec<_>=old["call_edges"].iter().filter(|e|e["file_path"]==consumer).collect();
        let new_edges:Vec<_>=new["call_edges"].iter().filter(|e|e["file_path"]==consumer).collect();
        assert_eq!(old_edges,new_edges);
        record(&format!("body-only-{provider}"),&report);
    }
}
#[test]
fn p2a_closeout_declaration_relocation_preserves_current_reference_addresses() {
    for (provider, consumer, before, after, caller) in [
        ("api.py", "consumer.py", "def transform(x):\n    return x\n", "# relocation, same interface\n\ndef transform(x):\n    return x\n", "from api import transform\ndef entry(x):\n    return transform(x)\n"),
        ("api.rs", "lib.rs", "pub fn transform(x: i32) -> i32 { x }\n", "// relocation, same interface\n\npub fn transform(x: i32) -> i32 { x }\n", "mod api; use api::transform; pub fn entry(x: i32) -> i32 { transform(x) }\n"),
        ("api.ts", "consumer.ts", "export function transform(x: number): number { return x; }\n", "// relocation, same interface\n\nexport function transform(x: number): number { return x; }\n", "import { transform } from './api'; export function entry(x: number): number { return transform(x); }\n"),
    ] {
        let facts = compare_sequence(&format!("relocation-{provider}"), &[(provider, before), (consumer, caller)], &[(provider, after)], false);
        assert_current_binding(&facts, consumer, provider, "transform");
        let target = facts["symbols"].as_array().unwrap().iter().find(|s| s["file_path"] == provider && s["name"] == "transform").unwrap();
        for edge in facts["call_edges"].as_array().unwrap().iter().filter(|e| e["file_path"] == consumer && e["callee_symbol"] == "transform") {
            assert_eq!(edge["target_symbol_id"], target["symbol_id"], "unchanged UID must not hide a stale location-derived ID");
        }
    }
}
#[test]
fn direct_writer_surface_storage_delete_and_reopen() {
    compare_sequence(
        "direct-store",
        &[("api.py", "def f(x):\n    return x\n")],
        &[("api.py", "def f(x, y=0):\n    return x + y\n")],
        true,
    );
    let (d, mut i) = fixture(&[("api.py", "def f(x):\n    return x\n")], false);
    let before = oracle::canonical(d.path()).unwrap();
    drop(i);
    i = CodeIndex::new(Some(d.path())).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
    std::fs::remove_file(d.path().join("api.py")).unwrap();
    i.build_index(false).unwrap();
    assert!(oracle::canonical(d.path()).unwrap()["public_surfaces"].is_empty());
}
#[test]
fn unsupported_language_remains_visible_in_public_build_coverage() {
    let (d, mut i) = fixture(
        &[(
            "Api.java",
            "public class Api { public int value() { return 1; } }\n",
        )],
        false,
    );
    put(
        d.path(),
        "Api.java",
        "public class Api { public String value() { return \"changed\"; } }\n",
    );
    let r = i.build_index(false).unwrap();
    assert_eq!(r.public_surface_coverage.unknown, 1);
    assert_eq!(r.public_surface_coverage.parsed_files, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; real MCP process"]
async fn p2a_public_mcp_reports_coverage_and_refreshes_provider() {
    use rmcp::{
        model::CallToolRequestParams,
        transport::{ConfigureCommandExt, TokioChildProcess},
        ServiceExt,
    };
    use std::time::Duration;
    let directory = tempfile::tempdir().unwrap();
    put(
        directory.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    );
    put(
        directory.path(),
        "api.py",
        "def transform(value):\n    return value\n",
    );
    put(
        directory.path(),
        "consumer.py",
        "from api import transform\ndef entry(x):\n    return transform(x)\n",
    );
    let binary = PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let c = ()
        .serve(
            TokioChildProcess::new(tokio::process::Command::new(binary).configure(|cmd| {
                for (key, _) in std::env::vars_os() {
                    if key.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(key);
                    }
                }
                cmd.args(["mcp", "--project-path"])
                    .arg(directory.path())
                    .current_dir(directory.path())
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .stderr(std::process::Stdio::null());
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let request = |name: &str, args: Value| {
        CallToolRequestParams::new(name.to_string())
            .with_arguments(args.as_object().unwrap().clone())
    };
    let mut receipts = Vec::new();
    for (full, source) in [
        (true, "def transform(value):\n    return value\n"),
        (
            false,
            "def transform(value, extra=0):\n    return value + extra\n",
        ),
    ] {
        put(directory.path(), "api.py", source);
        let r = tokio::time::timeout(
            Duration::from_secs(20),
            c.call_tool(request(
                "index",
                json!({"path":directory.path(),"full":full}),
            )),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(r.is_error, Some(true));
        let report = &r.structured_content.as_ref().unwrap()["result"];
        assert!(
            report["public_surface_coverage"]["parsed_files"]
                .as_u64()
                .unwrap()
                > 0
        );
        let r = c
            .call_tool(request(
                "search",
                json!({"query":"transform","mode":"symbol","exact":true}),
            ))
            .await
            .unwrap();
        assert_ne!(r.is_error, Some(true));
        let symbols = r.structured_content.as_ref().unwrap()["result"].clone();
        let target = symbols
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["file_path"] == "api.py")
            .unwrap();
        if !full {
            assert!(target["signature"].as_str().unwrap().contains("extra"));
        }
        let edge_result = tokio::time::timeout(Duration::from_secs(20), c.call_tool(request("graph_query", json!({
            "query": "MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = 'consumer.py' AND b.name = 'transform' RETURN b.symbol_uid AS uid, b.file_path AS path LIMIT 10"
        })))).await.unwrap().unwrap();
        assert_ne!(edge_result.is_error, Some(true), "{edge_result:?}");
        let graph = &edge_result.structured_content.as_ref().unwrap()["result"];
        let edges = graph["results"].as_array().unwrap();
        assert!(!edges.is_empty(), "consumer call edge missing: {graph}");
        assert!(
            edges
                .iter()
                .all(|e| e["uid"] == target["symbol_uid"] && e["path"] == "api.py"),
            "{graph}"
        );
        receipts
            .push(json!({"full":full,"report":report,"provider":target,"consumer_targets":edges}));
    }
    assert_ne!(
        receipts[0]["provider"]["symbol_uid"],
        receipts[1]["provider"]["symbol_uid"]
    );
    record("mcp-surface", &receipts);
    c.cancel().await.unwrap();
}
