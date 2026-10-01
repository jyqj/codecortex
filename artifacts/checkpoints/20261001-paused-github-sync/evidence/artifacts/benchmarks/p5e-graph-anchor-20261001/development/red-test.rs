//! Real indexed split-symbol graph source mapping, not fabricated DB rows.
use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_search::SearchEngine;
use cc_server::engine::CodeIndex;
use serde_json::json;

#[test]
fn graph_neighbor_of_a_long_function_has_a_real_declaration_document() {
    let dir = tempfile::tempdir().unwrap();
    let code = format!(
        "pub fn graph_seed_entry(v: i32) -> i32 {{ graph_long_target(v) }}\n\npub fn graph_long_target(mut value: i32) -> i32 {{\n{}    value\n}}\n",
        "    value += 1;\n".repeat(400)
    );
    std::fs::write(dir.path().join("long.rs"), &code).unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let config: ProjectConfig =
        serde_json::from_value(json!({"search":{"path_weight":0.0,"exact_symbol_weight":0.0}}))
            .unwrap();
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
    let result = engine
        .search_with_diagnostics(&SearchRequest {
            query: "graph_seed_entry".into(),
            top_k: 20,
            include_grep: false,
            ..Default::default()
        })
        .unwrap();
    let graph = result
        .lanes
        .iter()
        .find(|lane| lane.lane_id == "graph")
        .unwrap();
    assert_eq!(
        graph.truncation_reason, None,
        "a split declaration is mappable: {graph:?}"
    );
    let declaration = code.find("pub fn graph_long_target").unwrap();
    assert!(
        result.hits.iter().any(|hit| {
            let proof = &hit.metadata["source_evidence"];
            hit.reasons
                .iter()
                .any(|reason| reason.starts_with("graph@"))
                && proof["span"]["start"]
                    .as_u64()
                    .is_some_and(|start| start as usize <= declaration)
                && proof["span"]["end"]
                    .as_u64()
                    .is_some_and(|end| end as usize > declaration)
                && hit.text.contains("pub fn graph_long_target")
        }),
        "real graph callee source must survive statement partitioning"
    );
}
