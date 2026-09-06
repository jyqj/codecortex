//! Independently authored, closed-world call-site gold. MCP builds the index;
//! SQL observes persisted facts without using production scores as relevance.
use cc_db::index_db::IndexDb;
use cc_eval::runner::CodeIndexBackend;
use serde_json::{json, Value};
use std::io::Write;
use tempfile::TempDir;

#[test]
fn closed_world_call_facts_match_independent_gold() {
    let cases: Value =
        serde_json::from_str(include_str!("../benchmarks/facts_smoke.json")).unwrap();
    let mut raw = std::env::var_os("CODECORTEX_FACT_OUTPUT").map(|path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
    });
    if let Some(raw) = &mut raw {
        let commit = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        writeln!(raw,"{}",json!({"kind":"header","contract":"closed-world-call-sites-v1","implementation_commit":String::from_utf8_lossy(&commit.stdout).trim(),"manifest":cases})).unwrap();
    }
    let mut failures = Vec::new();
    for case in cases.as_array().unwrap() {
        let temp = TempDir::new().unwrap();
        for (path, text) in case["files"].as_object().unwrap() {
            let target = temp.path().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, text.as_str().unwrap()).unwrap();
        }
        let backend = CodeIndexBackend::new_unindexed(temp.path()).unwrap();
        let build = backend.build_index_report(true).unwrap();
        let db = IndexDb::open(&temp.path().join(".codecortex/index.sqlite3"))
            .unwrap()
            .0;
        let rows=db.reads().query_json("SELECT file_path, caller_symbol, callee_symbol, line, target_file_path, resolution_strategy FROM call_edges WHERE synthesized_by IS NULL ORDER BY file_path,line,start_col",&[]).unwrap();
        let mut actual:Vec<Value>=rows.iter().map(|r|json!({"file":r["file_path"],"caller":r["caller_symbol"],"callee":r["callee_symbol"],"line":r["line"],"target":r["target_file_path"]})).collect();
        let mut expected = case["calls"].as_array().unwrap().clone();
        actual.sort_by_key(Value::to_string);
        expected.sort_by_key(Value::to_string);
        let facts_match = actual == expected;
        let response = backend
            .call_tool(
                "search",
                &json!({"query":case["query"],"mode":"hybrid","top_k":5}),
            )
            .unwrap();
        let graph = &response["evidence_summary"]["graph_enrichment"];
        let no_phantom_enrichment =
            !expected.is_empty() || (graph["callers_added"] == 0 && graph["callees_added"] == 0);
        let success = facts_match && no_phantom_enrichment;
        if let Some(raw) = &mut raw {
            writeln!(raw,"{}",json!({"kind":"case","id":case["id"],"index_report":build,"actual":rows,"expected":case["calls"],"passed":success,"mcp_search":response})).unwrap();
            raw.flush().unwrap();
        }
        if !success {
            failures.push(format!(
                "{}: expected {expected:?}; actual {actual:?}; no_phantom_enrichment={no_phantom_enrichment}",
                case["id"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} fact failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
