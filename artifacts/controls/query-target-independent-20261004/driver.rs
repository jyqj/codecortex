//! Independent review controls, actual CodeIndex + MCP + unchanged normalizer.
use cc_eval::benchmark::normalizer;
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::path::Path;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let controls = Path::new(&args[1]);
    let root = Path::new(&args[2]);
    let out = Path::new(&args[3]);
    assert!(
        !root.exists() && !out.exists(),
        "fresh destinations required"
    );
    std::fs::create_dir_all(root).unwrap();
    std::fs::create_dir_all(out).unwrap();
    for entry in std::fs::read_dir(controls.join("fixtures")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), root.join(entry.file_name())).unwrap();
    }
    std::fs::write(
        root.join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let backend = cc_eval::runner::CodeIndexBackend::open_existing(root).unwrap();
    let rows: Vec<Value> =
        serde_json::from_slice(&std::fs::read(controls.join("matrix.json")).unwrap()).unwrap();
    let mut results = vec![];
    for row in rows {
        let query = row["query"].as_str().unwrap();
        let conversation_queries: Option<Vec<String>> = row
            .get("conversation_queries")
            .map(|v| serde_json::from_value(v.clone()).unwrap());
        let overrides = cc_model::search::SearchRequest {
            conversation_queries: conversation_queries.clone(),
            ..Default::default()
        };
        let engine = serde_json::to_value(
            index
                .search()
                .search_in_context_with(query, 10, None, overrides)
                .unwrap(),
        )
        .unwrap();
        let mut params = json!({"query":query,"top_k":10,"mode":"hybrid"});
        if let Some(context) = conversation_queries {
            params["conversation_queries"] = json!(context);
        }
        let mcp = backend.call_tool("search", &params).unwrap();
        let mut result = row.clone();
        for (api, payload) in [("engine", engine), ("mcp", mcp)] {
            let (mut hits, status) = normalizer::mcp(&payload).unwrap();
            for h in &mut hits {
                normalizer::verify_source(h, root).unwrap();
                assert_eq!(h.evidence_valid, Some(true));
            }
            result[api] = json!({"raw":payload,"normalized":hits,"status":status});
        }
        results.push(result);
    }
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    println!(
        "{} controls through engine/MCP/normalizer; source verification passed",
        results.len()
    );
}
