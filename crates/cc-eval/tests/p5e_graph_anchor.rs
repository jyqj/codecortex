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
            graph
                .candidates
                .iter()
                .any(|candidate| candidate.legacy_chunk_id == hit.chunk_id)
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

#[test]
fn same_line_unicode_siblings_do_not_share_the_graph_declaration_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let code = "// 汉字\r\nconst 标题 = '中'; export function first(){ return 1; } export function second(){ return 2; }\r\nexport function caller(){ return second(); }\r\n";
    std::fs::write(dir.path().join("siblings.ts"), code).unwrap();
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
            query: "second".into(),
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
    assert_eq!(graph.truncation_reason, None);
    let position = code.find("function second").unwrap();
    assert!(graph
        .candidates
        .iter()
        .any(|candidate| candidate.source_span.start <= position
            && candidate.source_span.end > position));
    for candidate in &graph.candidates {
        let hit = result
            .hits
            .iter()
            .find(|hit| hit.chunk_id == candidate.legacy_chunk_id)
            .unwrap();
        assert!(
            hit.text.contains("function second") || hit.text.contains("function caller"),
            "unrelated sibling was substituted: {}",
            hit.text
        );
        assert!(
            !candidate.exact_identity,
            "graph evidence is not an exact-identity tier"
        );
    }
}

#[test]
fn nested_and_duplicate_file_uids_respect_hard_scope_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("nested.py"),
        "def outer():\n    def target():\n        return 1\n    return target()\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("other.py"), "def target():\n    return 2\n").unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let symbols = index.graph().file_symbols("nested.py").unwrap();
    let target = symbols
        .iter()
        .find(|symbol| symbol.name == "target")
        .unwrap();
    let uid = target.symbol_uid.as_deref().unwrap();
    let control = cc_model::query::QueryControl::new(std::time::Duration::from_secs(5)).unwrap();
    let allowed = cc_db::ChunkScope {
        file_paths: Some(vec!["nested.py".into()]),
        ..Default::default()
    };
    let db = index.index_db().unwrap();
    assert!(db
        .retrieval()
        .symbol_uid_anchor_chunks(&[uid], &allowed, &control)
        .unwrap()
        .contains_key(uid));
    let denied = cc_db::ChunkScope {
        file_paths: Some(vec!["other.py".into()]),
        ..Default::default()
    };
    assert!(db
        .retrieval()
        .symbol_uid_anchor_chunks(&[uid], &denied, &control)
        .unwrap()
        .is_empty());
    control.cancel();
    assert!(db
        .retrieval()
        .symbol_uid_anchor_chunks(&[uid], &allowed, &control)
        .is_err());
}

#[test]
fn same_line_same_name_methods_keep_their_container_uids() {
    let dir = tempfile::tempdir().unwrap();
    let code="const 标题='中'; export class A { target(){ return 1; } } export class B { target(){ return 2; } }\n";
    std::fs::write(dir.path().join("methods.ts"), code).unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let symbols = index.graph().file_symbols("methods.ts").unwrap();
    let methods: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name == "target")
        .collect();
    assert_eq!(methods.len(), 2);
    let uids: Vec<_> = methods
        .iter()
        .map(|symbol| symbol.symbol_uid.as_deref().unwrap())
        .collect();
    let control = cc_model::query::QueryControl::new(std::time::Duration::from_secs(5)).unwrap();
    let anchors = index
        .index_db()
        .unwrap()
        .retrieval()
        .symbol_uid_anchor_chunks(&uids, &cc_db::ChunkScope::default(), &control)
        .unwrap();
    assert_eq!(anchors.len(), 2);
    assert_ne!(
        anchors[uids[0]], anchors[uids[1]],
        "different class bodies cannot be substituted"
    );
}

#[test]
fn edited_then_deleted_graph_anchors_do_not_keep_old_document_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("change.rs");
    std::fs::write(&path, "pub fn target() -> i32 { 1 }\n").unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let first = index
        .search()
        .search_in_context("target", 10, None)
        .unwrap();
    let old_version = first.machine_pack["hits"][0]["metadata"]["document"]["doc_version"].clone();
    let symbols = index.graph().file_symbols("change.rs").unwrap();
    let old_uid = symbols[0].symbol_uid.clone().unwrap();
    std::fs::write(&path, "pub fn target() -> i32 { 200 }\n").unwrap();
    index.build_index(false).unwrap();
    let changed = index
        .search()
        .search_in_context("target", 10, None)
        .unwrap();
    assert_ne!(
        changed.machine_pack["hits"][0]["metadata"]["document"]["doc_version"],
        old_version
    );
    assert!(changed.machine_pack["hits"][0]["text"]
        .as_str()
        .unwrap()
        .contains("200"));
    std::fs::remove_file(path).unwrap();
    index.build_index(false).unwrap();
    let control = cc_model::query::QueryControl::new(std::time::Duration::from_secs(5)).unwrap();
    assert!(index
        .index_db()
        .unwrap()
        .retrieval()
        .symbol_uid_anchor_chunks(&[old_uid.as_str()], &cc_db::ChunkScope::default(), &control)
        .unwrap()
        .is_empty());
}

#[test]
fn graph_candidates_reject_a_corrupted_document_manifest() {
    for mutation in [
        "DELETE FROM document_manifest",
        "UPDATE document_manifest SET doc_version='corrupt-graph-mirror'",
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("corrupt.rs"),
            "pub fn target() -> i32 { 1 }\n",
        )
        .unwrap();
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
        let request = SearchRequest {
            query: "target".into(),
            top_k: 20,
            include_grep: false,
            ..Default::default()
        };
        assert!(!engine
            .search_with_diagnostics(&request)
            .unwrap()
            .hits
            .is_empty());
        let connection =
            rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
        connection.execute_batch(mutation).unwrap();
        assert!(
            engine.search_with_diagnostics(&request).is_err(),
            "graph source was blessed through {mutation}"
        );
    }
}

#[test]
fn four_hundred_twenty_real_uids_use_batched_projection_and_bounded_prefix_reads() {
    let dir = tempfile::tempdir().unwrap();
    for group in 0..21 {
        let mut code = String::new();
        for n in group * 20..(group + 1) * 20 {
            code.push_str(&format!(
                "pub fn anchor_{n:03}(mut n:i32)->i32 {{\n{}    n\n}}\n",
                "    n += 1;\n".repeat(140)
            ));
        }
        std::fs::write(dir.path().join(format!("fanout_{group:02}.rs")), code).unwrap();
    }
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.parse_errors.is_empty());
    let symbols: Vec<_> = (0..21)
        .flat_map(|group| {
            index
                .graph()
                .file_symbols(&format!("fanout_{group:02}.rs"))
                .unwrap()
        })
        .collect();
    let uids: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name.starts_with("anchor_"))
        .map(|symbol| symbol.symbol_uid.as_deref().unwrap())
        .collect();
    assert_eq!(uids.len(), 420);
    let control = cc_model::query::QueryControl::new(std::time::Duration::from_secs(30)).unwrap();
    let started = std::time::Instant::now();
    let result = index
        .index_db()
        .unwrap()
        .retrieval()
        .symbol_uid_anchor_chunks_with_work(&uids, &cc_db::ChunkScope::default(), &control)
        .unwrap();
    assert_eq!(result.anchors.len(), 420);
    assert_eq!(
        result.work.sql.statements, 9,
        "three batches, no per-UID SQL"
    );
    assert_eq!(
        result.work.text.storage_reads, 420,
        "one prefix per distinct declaration line"
    );
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("graph-420-anchor-work.json"),serde_json::to_vec_pretty(&json!({"scope":"420 real indexed long declarations; UID mapping projection/prefix/admission only, not seed-edge or whole-query work","elapsed_us":started.elapsed().as_micros(),"work":result.work,"anchors":result.anchors.len()})).unwrap()).unwrap();
    }
}

#[test]
fn real_bidirectional_fanout_maps_all_twenty_seeds_and_four_hundred_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    for seed in 0..20 {
        let name = if seed < 10 {
            format!("alpha{seed}")
        } else {
            format!("beta{}", seed - 10)
        };
        let calls = (0..10)
            .map(|j| format!("leaf{:03}(n)", seed * 10 + j))
            .collect::<Vec<_>>()
            .join(" + ");
        let mut code = format!("pub fn {name}(n:i32)->i32 {{ {calls} }}\n");
        for j in 0..10 {
            code.push_str(&format!(
                "pub fn leaf{:03}(mut n:i32)->i32 {{\n{} n\n}}\n",
                seed * 10 + j,
                "    n += 1;\n".repeat(140)
            ));
            code.push_str(&format!(
                "pub fn wrap{:03}(n:i32)->i32 {{ {name}(n) }}\n",
                seed * 10 + j
            ));
        }
        std::fs::write(dir.path().join(format!("group{seed:02}.rs")), code).unwrap();
    }
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.parse_errors.is_empty());
    let config:ProjectConfig=serde_json::from_value(json!({"search":{"path_weight":0.0,"exact_symbol_weight":0.0,"graph_top_k":500,"rerank_window":500}})).unwrap();
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
    let started = std::time::Instant::now();
    let result = engine
        .search_with_diagnostics(&SearchRequest {
            query: "alpha beta".into(),
            top_k: 500,
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
        graph.candidate_count, 420,
        "both caller and callee directions must be resolved from actual source"
    );
    assert_eq!(graph.truncation_reason, None);
    assert_eq!(
        graph
            .candidates
            .iter()
            .filter(|candidate| candidate.raw_score == config.ranking.graph_seed_fuzzy_score)
            .count(),
        20
    );
    assert_eq!(
        graph
            .candidates
            .iter()
            .filter(|candidate| candidate.raw_score
                == config.ranking.graph_seed_fuzzy_score * config.ranking.graph_neighbor_decay)
            .count(),
        400
    );
    assert!(graph
        .candidates
        .iter()
        .all(|candidate| !candidate.exact_identity));
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("graph-420-real-fanout.json"),serde_json::to_vec_pretty(&json!({"scope":"actual 20 seeds, 10 callers+10 callees per seed; indexed long-body source; diagnostics not whole budgeted response certification","elapsed_us":started.elapsed().as_micros(),"lane":graph,"hits":result.hits.len()})).unwrap()).unwrap();
    }
}
