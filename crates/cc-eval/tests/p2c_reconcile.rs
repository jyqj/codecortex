//! Actual parser / SQLite / incremental-build recovery regressions.
use cc_eval::benchmark::oracle;
use cc_model::freshness::ChangeKind;
use cc_server::engine::CodeIndex;
use serde_json::Value;
use std::path::Path;

fn put(root: &Path, path: &str, source: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, source).unwrap();
}
fn fixture() -> (tempfile::TempDir, CodeIndex) {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
    );
    for name in ["a.py", "b.py", "c.py"] {
        put(d.path(), name, "def entry(x):\n    return later(x)\n");
    }
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    (d, i)
}
fn bound_consumers(root: &Path) -> usize {
    oracle::canonical(root).unwrap()["call_edges"]
        .iter()
        .filter(|e| {
            e["file_path"] != "provider.py"
                && e["callee_symbol"] == "later"
                && e["target_file_path"] == "provider.py"
        })
        .count()
}
fn snapshot(root: &Path) -> Value {
    serde_json::to_value(oracle::canonical(root).unwrap()).unwrap()
}
fn drain(i: &mut CodeIndex, limit: usize) {
    for _ in 0..limit {
        if i.build_index(false).unwrap().resolution_freshness.complete {
            return;
        }
    }
    panic!("durable closure did not converge within test bound");
}

#[test]
fn an_old_provider_root_can_itself_be_invalidated_by_a_later_provider() {
    let (d, mut i) = fixture();
    put(
        d.path(),
        "provider.py",
        "def later(x):\n    return newest(x)\n",
    );
    i.build_index(false).unwrap();
    put(d.path(), "newest.py", "def newest(x):\n    return x\n");
    drain(&mut i, 12);
    let a = snapshot(d.path());
    assert!(a["call_edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["file_path"] == "provider.py" && e["target_file_path"] == "newest.py"));
    i.build_index(true).unwrap();
    assert_eq!(a, snapshot(d.path()));
}

#[test]
fn python_and_rust_forward_cycles_match_full_after_signature_changes() {
    for extension in ["py", "rs"] {
        let d = tempfile::tempdir().unwrap();
        put(
            d.path(),
            ".codecortex.json",
            r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":1}}"#,
        );
        let (api, a, b, use_source, changed) = if extension == "py" {
            (
                "def ping(x):\n    return x\n",
                "from api import ping\nfrom b import helper\n",
                "from a import ping\ndef helper():\n    return 1\n",
                "from b import ping\ndef run(x):\n    return ping(x)\n",
                "def ping(x, y=0):\n    return x+y\n",
            )
        } else {
            (
                "pub fn ping(x:i32)->i32{x}\n",
                "pub use crate::api::ping;\npub use crate::b::helper;\n",
                "pub use crate::a::ping;\npub fn helper()->i32{1}\n",
                "mod api; mod a; mod b; use b::ping; pub fn run()->i32{ping(1)}\n",
                "pub fn ping(x:i32,y:i32)->i32{x+y}\n",
            )
        };
        for (name, source) in [("api", api), ("a", a), ("b", b), ("lib", use_source)] {
            put(d.path(), &format!("{name}.{extension}"), source);
        }
        let mut i = CodeIndex::new(Some(d.path())).unwrap();
        i.build_index(true).unwrap();
        put(d.path(), &format!("api.{extension}"), changed);
        drain(&mut i, 12);
        let before = snapshot(d.path());
        assert!(
            before["call_edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["file_path"] == format!("lib.{extension}")
                    && e["target_file_path"] == format!("api.{extension}")),
            "{extension} lost expected target"
        );
        i.build_index(true).unwrap();
        assert_eq!(before, snapshot(d.path()), "{extension}");
    }
}

#[test]
fn python_graph_rejects_false_calls_and_preserves_unsupported_shadow_on_reload() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false}}"#,
    );
    put(d.path(), "provider.py", "def target(x):\n    return x\n");
    put(d.path(),"fake.py","def target(x):\n    return x\ndef caller(x):\n    # target(x)\n    text='target(x)'\n    return x\n");
    put(
        d.path(),
        "use.py",
        "def run(target):\n    return target(1)\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    for source in [
        "def target(x,y=0):\n    return x+y\n",
        "def target(x,z=0):\n    return x-z\n",
    ] {
        put(d.path(), "provider.py", source);
        i.build_index(false).unwrap();
        let facts = snapshot(d.path());
        assert!(!facts["call_edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["file_path"] == "fake.py"));
        let edge = facts["call_edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["file_path"] == "use.py")
            .unwrap();
        assert!(edge["target_symbol_id"].is_null());
        assert_eq!(
            edge["resolution_strategy"],
            cc_model::resolution::PARSER_UNSUPPORTED_BINDING
        );
        i.build_index(true).unwrap();
        assert_eq!(facts, snapshot(d.path()));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; actual MCP subprocess"]
async fn p2c_public_mcp_reports_debt_resumes_and_survives_restart() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":1,"db_read_pool_size":1}}"#,
    );
    for p in ["a.py", "b.py", "c.py"] {
        put(d.path(), p, "def entry(x):\n    return later(x)\n");
    }
    put(d.path(),"fake.py","def target(x):\n    return x\ndef caller(x):\n    # target(x)\n    text='target(x)'\n    return x\n");
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
        .await
        .unwrap();
    m.call("index", json!({"path":d.path(),"full":true}))
        .await
        .unwrap();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    let mut receipts = Vec::new();
    for step in 0..3 {
        if step == 1 {
            m.close().await.unwrap();
            m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
                .await
                .unwrap();
        }
        let report = m
            .call("index", json!({"path":d.path(),"full":false}))
            .await
            .unwrap();
        let status = m.call("status", json!({"aspect":"index"})).await.unwrap();
        let query=m.call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE b.name = 'later' RETURN a.file_path AS caller LIMIT 20"})).await.unwrap();
        let search = m
            .call("search", json!({"query":"later","top_k":5}))
            .await
            .unwrap();
        assert_eq!(query["results"].as_array().unwrap().len(), step + 1);
        for value in [&report, &status, &query, &search] {
            assert_eq!(
                value["resolution_freshness"]["complete"],
                step == 2,
                "{value}"
            );
        }
        receipts.push(
            json!({"step":step,"index":report,"status":status,"graph":query,"search":search}),
        );
    }
    let fake=m.call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = 'fake.py' RETURN a.name AS caller LIMIT 20"})).await.unwrap();
    assert!(fake["results"].as_array().unwrap().is_empty());
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            std::path::Path::new(&path).join("p2c-mcp.json"),
            serde_json::to_vec_pretty(&json!({"steps":receipts,"no_fake_calls":fake})).unwrap(),
        )
        .unwrap();
    }
    m.close().await.unwrap();
}

#[test]
fn late_provider_change_rebases_without_losing_old_consumers() {
    let (d, mut i) = fixture();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    i.build_index(false).unwrap();
    put(
        d.path(),
        "provider.py",
        "def later(x, other=0):\n    return x+other\n",
    );
    let r = i.build_index(false).unwrap();
    assert!(r.dirty_plan.rebased);
    drain(&mut i, 8);
    assert_eq!(bound_consumers(d.path()), 3);
    let a = snapshot(d.path());
    i.build_index(true).unwrap();
    assert_eq!(a, snapshot(d.path()));
}

#[test]
fn deleting_a_pending_root_revokes_every_consumers_old_target() {
    let (d, mut i) = fixture();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    i.build_index(false).unwrap();
    std::fs::remove_file(d.path().join("provider.py")).unwrap();
    drain(&mut i, 8);
    assert_eq!(bound_consumers(d.path()), 0);
    let a = snapshot(d.path());
    i.build_index(true).unwrap();
    assert_eq!(a, snapshot(d.path()));
}

#[test]
fn fanout_larger_than_a_lookup_window_is_not_truncated_out_of_debt() {
    let (d, mut i) = fixture();
    for n in 0..12 {
        put(
            d.path(),
            &format!("consumer{n}.py"),
            "def entry(x):\n    return later(x)\n",
        );
    }
    i.build_index(true).unwrap();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    let r = i.build_index(false).unwrap();
    assert!(!r.resolution_freshness.complete);
    drain(&mut i, 20);
    assert_eq!(bound_consumers(d.path()), 15);
    let a = snapshot(d.path());
    i.build_index(true).unwrap();
    assert_eq!(a, snapshot(d.path()));
}

#[test]
fn disabled_propagation_retains_debt_until_reenabled_or_full_rebuild() {
    let (d, mut i) = fixture();
    drop(i);
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation":false,"dirty_propagation_max_files":1}}"#,
    );
    i = CodeIndex::new(Some(d.path())).unwrap();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    let r = i.build_index(false).unwrap();
    assert!(!r.resolution_freshness.complete);
    assert_eq!(r.resolution_freshness.reason.as_deref(), Some("disabled"));
    assert_eq!(bound_consumers(d.path()), 0);
    drop(i);
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":1}}"#,
    );
    i = CodeIndex::new(Some(d.path())).unwrap();
    drain(&mut i, 8);
    assert_eq!(bound_consumers(d.path()), 3);
    i.build_index(true).unwrap();
    assert!(i.build_index(false).unwrap().resolution_freshness.complete);
}

#[test]
fn pure_body_change_does_not_re_resolve_other_files() {
    let (d, mut i) = fixture();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    i.build_index(true).unwrap();
    put(d.path(), "provider.py", "def later(x):\n    return x+100\n");
    let r = i.build_index(false).unwrap();
    assert_eq!(r.dirty_plan.selected_dependents, 0);
    assert!(r.dirty_plan.reasons.contains(&ChangeKind::Body));
    assert!(r.resolution_freshness.complete);
}

#[test]
fn reexport_cycle_and_deep_chain_converge_across_bounded_rounds() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":1,"db_read_pool_size":1}}"#,
    );
    put(
        d.path(),
        "api.ts",
        "export function ping(x:number) { return x; }\n",
    );
    for n in 0..20 {
        let prev = if n == 0 {
            "api".to_string()
        } else {
            format!("layer{}", n - 1)
        };
        put(
            d.path(),
            &format!("layer{n}.ts"),
            &format!("export {{ping}} from './{prev}';\n"),
        );
    }
    put(
        d.path(),
        "cycle_a.ts",
        "export {ping} from './layer19';\nexport {b} from './cycle_b';\n",
    );
    put(
        d.path(),
        "cycle_b.ts",
        "export {ping} from './cycle_a';\nexport const b=1;\n",
    );
    put(
        d.path(),
        "use.ts",
        "import {ping} from './cycle_b';\nexport function run(){return ping(1);}\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    put(
        d.path(),
        "api.ts",
        "export function ping(x:string) { return x; }\n",
    );
    drain(&mut i, 40);
    let a = snapshot(d.path());
    i.build_index(true).unwrap();
    assert_eq!(a, snapshot(d.path()));
}

#[test]
fn budget_remainder_is_resumed_by_unchanged_incremental_builds() {
    let (d, mut i) = fixture();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    let first = i.build_index(false).unwrap();
    assert_eq!(
        format!("{:?}", first.dirty_propagation),
        "Some(BudgetExceeded)"
    );
    assert_eq!(bound_consumers(d.path()), 1);
    assert!(!first.resolution_freshness.complete);
    assert!(first.dirty_plan.reasons.contains(&ChangeKind::Inventory));
    i.build_index(false).unwrap();
    assert_eq!(
        bound_consumers(d.path()),
        2,
        "a no-source-change build must drain durable remainder"
    );
    i.build_index(false).unwrap();
    assert_eq!(bound_consumers(d.path()), 3);
    let reconciled = snapshot(d.path());
    i.build_index(true).unwrap();
    assert_eq!(reconciled, snapshot(d.path()));
    let r = i.build_index(false).unwrap();
    assert!(r.resolution_freshness.complete);
    assert!(!r.dirty_plan.resumed);
}
#[test]
fn budget_remainder_survives_reopen_and_an_empty_event_scope() {
    let (d, mut i) = fixture();
    put(d.path(), "provider.py", "def later(x):\n    return x\n");
    i.build_index(false).unwrap();
    drop(i);
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    let inputs = i.build_inputs().unwrap();
    let scope = cc_index::BuildScope {
        changed: vec![],
        removed: vec![],
    };
    let prepared = CodeIndex::prepare_build_scoped(&inputs, false, None, Some(&scope)).unwrap();
    i.commit_build(&inputs, false, None, prepared).unwrap();
    assert_eq!(
        bound_consumers(d.path()),
        2,
        "restart must not erase unprocessed invalidation"
    );
    i.build_index(false).unwrap();
    assert_eq!(bound_consumers(d.path()), 3);
}
