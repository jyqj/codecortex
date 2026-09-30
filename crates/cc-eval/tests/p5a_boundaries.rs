//! Independent P5-A boundaries: actual parser, database and retrieval pipeline.

#[test]
fn grep_execution_receipt_matches_zero_weight_and_empty_scope() {
    let (_d, _index, engine) = fixture(
        &[("a.py".into(), "def needle():\n    return 7\n".into())],
        json!({"grep_weight":0.0,"graph_weight":0.0,"path_weight":0.0,"exact_symbol_weight":0.0}),
    );
    let mut req = request("needle");
    req.include_grep = true;
    let ran = engine.search_with_diagnostics(&req).unwrap();
    assert_eq!(lane(&ran, "grep").status, LaneStatus::Complete);
    assert!(
        ran.scope.as_ref().unwrap().budget.grep_enabled,
        "zero fusion weight is not disabled execution"
    );
    req.file_paths = Some(Vec::new());
    let empty = engine.search_with_diagnostics(&req).unwrap();
    assert!(!empty.scope.as_ref().unwrap().budget.grep_enabled);
    assert_eq!(lane(&empty, "grep").status, LaneStatus::Disabled);
    assert_eq!(lane(&empty, "grep").coverage.examined, None);
    assert!(empty
        .lanes
        .iter()
        .all(|lane| lane.status == LaneStatus::Disabled));
    assert_eq!(empty.cost.lexical_sql.statements, 0);
    req.file_paths = None;
    req.include_grep = false;
    let off = engine.search_with_diagnostics(&req).unwrap();
    assert!(!off.scope.as_ref().unwrap().budget.grep_enabled);
    assert_eq!(lane(&off, "grep").status, LaneStatus::Disabled);
}

#[test]
fn identity_materialization_rejects_corruption_before_reranking() {
    for mutation in [
        "DELETE FROM document_manifest",
        "UPDATE document_manifest SET doc_version='invalid-mirror'",
        "UPDATE chunks SET source_json=json_set(source_json,'$.span.end',9999999)",
    ] {
        let (dir, _index, engine) = fixture(
            &[("a.py".into(), "def needle():\n    return 7\n".into())],
            json!({"path_weight":0.0,"graph_weight":0.0,"exact_symbol_weight":0.0}),
        );
        let req = request("needle");
        assert!(!engine
            .search_with_diagnostics(&req)
            .unwrap()
            .hits
            .is_empty());
        let db = rusqlite::Connection::open(dir.path().join(".codecortex/index.sqlite3")).unwrap();
        // Deliberate corruption without an epoch bump: the uncached diagnostics
        // path must validate every candidate, not bless an unversioned fallback.
        db.execute_batch(mutation).unwrap();
        assert!(
            engine.search_with_diagnostics(&req).is_err(),
            "accepted {mutation}"
        );
    }
}

#[test]
fn absent_parser_method_signatures_are_not_fabricated_by_exact_lane() {
    let (_d, index, engine) = fixture(
        &[(
            "method.ts".into(),
            "export class Worker { run(value: string) { return value; } }\n".into(),
        )],
        json!({"graph_weight":0.0,"path_weight":0.0}),
    );
    let symbols = index.graph().file_symbols("method.ts").unwrap();
    let method = symbols.iter().find(|s| s.name == "run").unwrap();
    assert!(
        method.signature.is_none(),
        "baseline parser does not emit a method signature"
    );
    let result = engine
        .search_with_diagnostics(&request("run(value: string)"))
        .unwrap();
    assert_eq!(lane(&result, "exact_symbol").candidate_count, 0);
}

#[test]
fn literal_function_signature_uses_byte_columns_after_unicode_crlf_prefix() {
    let (_d, index, engine) = fixture(&[("overloads.ts".into(),
        "// 中文前缀\r\nfunction run(value: number) { return 1; } function run(value: string) { return 'two'; }\r\n".into())],
        json!({"graph_weight":0.0,"path_weight":0.0}));
    let symbols = index.graph().file_symbols("overloads.ts").unwrap();
    let target = symbols
        .iter()
        .find(|s| s.name == "run" && s.signature.as_deref().is_some_and(|q| q.contains("string")))
        .unwrap();
    let result = engine
        .search_with_diagnostics(&request(target.signature.as_deref().unwrap()))
        .unwrap();
    let exact = lane(&result, "exact_symbol");
    assert!(!exact.candidates.is_empty());
    for candidate in &exact.candidates {
        let hit = result
            .hits
            .iter()
            .find(|h| h.chunk_id == candidate.legacy_chunk_id)
            .unwrap();
        assert!(
            hit.text.contains("return 'two'"),
            "wrong overload: {}",
            hit.text
        );
        assert!(!hit.text.contains("return 1"));
    }
}

#[test]
fn qualified_same_line_methods_do_not_promote_another_owner() {
    let (_d, index, engine) = fixture(&[("owners.ts".into(),
        "export class First { run() { return 1; } } export class Second { run() { return 2; } }\n".into())],
        json!({"graph_weight":0.0,"path_weight":0.0}));
    let symbols = index.graph().file_symbols("owners.ts").unwrap();
    let target = symbols
        .iter()
        .find(|s| s.name == "run" && s.qname.as_deref().is_some_and(|q| q.contains("Second")))
        .unwrap();
    let result = engine
        .search_with_diagnostics(&request(target.qname.as_deref().unwrap()))
        .unwrap();
    let exact = lane(&result, "exact_symbol");
    assert!(
        !exact.candidates.is_empty(),
        "qualified method must be retrievable"
    );
    for candidate in &exact.candidates {
        let hit = result
            .hits
            .iter()
            .find(|h| h.chunk_id == candidate.legacy_chunk_id)
            .unwrap();
        assert!(
            hit.text.contains("return 2"),
            "qualified name matched the wrong same-line owner: {}",
            hit.text
        );
    }
}

#[test]
fn prose_short_words_cannot_cast_path_votes_but_short_path_queries_work() {
    let (_d, _i, engine) = fixture(
        &[
            ("sample.py".into(), "def alpha():\n    return 7\n".into()),
            ("display.py".into(), "def bravo():\n    return 8\n".into()),
            ("ui.py".into(), "def render():\n    return 9\n".into()),
        ],
        json!({"graph_weight":0.0,"exact_symbol_weight":0.0}),
    );
    let prose = engine
        .search_with_diagnostics(&request("Where is a widget handled?"))
        .unwrap();
    assert_eq!(
        lane(&prose, "path").candidate_count,
        0,
        "incidental prose words a/is must not add path votes to sample/display"
    );
    assert_eq!(
        lane(&prose, "path").status,
        LaneStatus::Complete,
        "policy-ineligible tokens are not truncated work"
    );
    let short = engine.search_with_diagnostics(&request("ui")).unwrap();
    assert_eq!(short.hits[0].file_path, "ui.py");
    assert!(lane(&short, "path").candidate_count > 0);
    let exact = engine.search_with_diagnostics(&request("ui.py")).unwrap();
    assert!(lane(&exact, "path").candidates[0].exact_identity);
}

#[test]
fn graph_internal_seed_and_neighbor_limits_never_claim_complete() {
    let many_seeds: Vec<_> = (0..25)
        .map(|n| {
            (
                format!("seed{n}.py"),
                format!("def same_marker_{n}():\n    return {n}\n"),
            )
        })
        .collect();
    let (_d, _i, engine) = fixture(
        &many_seeds,
        json!({"path_weight":0.0,"exact_symbol_weight":0.0,"graph_top_k":100}),
    );
    let r = engine
        .search_with_diagnostics(&request("same_marker"))
        .unwrap();
    let graph = lane(&r, "graph");
    assert_eq!(
        graph.status,
        LaneStatus::Partial,
        "internal seed cap is not a complete query"
    );
    assert!(!graph.coverage.complete);
    assert_eq!(
        graph.truncation_reason.as_deref(),
        Some("graph_expansion_limit")
    );

    let mut source = String::new();
    for n in 0..14 {
        source.push_str(&format!("def leaf_{n}():\n    return {n}\n\n"));
    }
    source.push_str("def hub_unique():\n");
    for n in 0..14 {
        source.push_str(&format!("    leaf_{n}()\n"));
    }
    let (_d, _i, engine) = fixture(
        &[("graph.py".into(), source)],
        json!({"path_weight":0.0,"exact_symbol_weight":0.0,"graph_top_k":100}),
    );
    let r = engine
        .search_with_diagnostics(&request("hub_unique"))
        .unwrap();
    assert_eq!(
        lane(&r, "graph").status,
        LaneStatus::Partial,
        "per-seed neighbor cap is not complete"
    );
    assert_eq!(
        lane(&r, "graph").truncation_reason.as_deref(),
        Some("graph_expansion_limit")
    );
}

use cc_model::{config::ProjectConfig, retrieval::LaneStatus, search::SearchRequest};
use cc_search::{engine::SearchResult, SearchEngine};
use cc_server::engine::CodeIndex;
use serde_json::json;

fn fixture(
    files: &[(String, String)],
    search: serde_json::Value,
) -> (tempfile::TempDir, CodeIndex, SearchEngine) {
    let dir = tempfile::tempdir().unwrap();
    let config = json!({"auto_index":{"enabled":false},"search":search});
    std::fs::write(dir.path().join(".codecortex.json"), config.to_string()).unwrap();
    for (path, text) in files {
        let dest = dir.path().join(path);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, text).unwrap();
    }
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.parse_errors.is_empty(), "{:?}", report.parse_errors);
    let config: ProjectConfig = serde_json::from_value(config).unwrap();
    let search = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
    (dir, index, search)
}
fn request(query: &str) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    }
}
fn lane<'a>(r: &'a SearchResult, id: &str) -> &'a cc_model::retrieval::LaneOutcome {
    r.lanes.iter().find(|l| l.lane_id == id).unwrap()
}

#[test]
fn exact_lane_finds_a_split_definition_and_its_literal_signature() {
    let code = format!(
        "pub fn long_definition(value: i32) -> i32 {{\n{}    value\n}}\n",
        "    consume(value);\n".repeat(130)
    );
    let (_d, index, engine) = fixture(
        &[("src/long.rs".into(), code)],
        json!({"graph_weight":0.0,"path_weight":0.0}),
    );
    let result = engine
        .search_with_diagnostics(&request("long_definition"))
        .unwrap();
    let exact = lane(&result, "exact_symbol");
    assert!(
        !exact.candidates.is_empty(),
        "a split function still has a declaration candidate"
    );
    assert_eq!(
        result.hits[0].symbol_name.as_deref(),
        Some("long_definition")
    );
    assert!(result.hits[0].reasons.iter().any(|r| r == "exact-target"));
    let symbols = index.graph().file_symbols("src/long.rs").unwrap();
    let signature = symbols
        .iter()
        .find(|s| s.name == "long_definition")
        .unwrap()
        .signature
        .as_ref()
        .unwrap();
    assert!(
        signature.contains(' '),
        "fixture must exercise whitespace in signatures"
    );
    let by_signature = engine.search_with_diagnostics(&request(signature)).unwrap();
    assert!(
        !lane(&by_signature, "exact_symbol").candidates.is_empty(),
        "literal stored signature must be reachable: {signature}"
    );
}

#[test]
fn same_line_sibling_definitions_do_not_share_exact_identity() {
    let (_d, _index, engine) = fixture(
        &[(
            "a.ts".into(),
            "export function first(){ return 1; } export function second(){ return 2; }\n".into(),
        )],
        json!({"path_weight":0.0,"graph_weight":0.0}),
    );
    let r = engine.search_with_diagnostics(&request("second")).unwrap();
    assert_eq!(r.hits[0].symbol_name.as_deref(), Some("second"));
    let exact = lane(&r, "exact_symbol");
    for c in &exact.candidates {
        let hit = r
            .hits
            .iter()
            .find(|h| h.chunk_id == c.legacy_chunk_id)
            .unwrap();
        assert!(
            hit.text.contains("function second"),
            "same-line sibling was promoted as the exact definition"
        );
    }
}

#[test]
fn path_token_truncation_is_explicit_and_bounded() {
    let files: Vec<_> = (0..50)
        .map(|n| {
            (
                format!("src/needle{n:02}.py"),
                "def work():\n    return 7\n".into(),
            )
        })
        .collect();
    let (_d, _i, engine) = fixture(
        &files,
        json!({"path_top_k":1,"exact_symbol_weight":0.0,"graph_weight":0.0}),
    );
    let mut req = request("needle");
    req.top_k = 1;
    let r = engine.search_with_diagnostics(&req).unwrap();
    let path = lane(&r, "path");
    assert_eq!(path.status, LaneStatus::Partial);
    assert_eq!(path.truncation_reason.as_deref(), Some("path_token_limit"));
    assert_eq!(path.candidate_count, 1);
    assert!(!path.coverage.complete);
}

#[test]
fn exact_ambiguity_and_limit_are_scoped_before_truncation() {
    let files: Vec<_> = (0..6)
        .map(|n| {
            (
                format!("src/p{n}.py"),
                "def duplicate():\n    return 7\n".into(),
            )
        })
        .collect();
    let (_d, _index, engine) = fixture(
        &files,
        json!({"exact_symbol_top_k":2,"path_weight":0.0,"graph_weight":0.0}),
    );
    let mut req = request("duplicate");
    req.top_k = 2;
    let all = engine.search_with_diagnostics(&req).unwrap();
    let exact = lane(&all, "exact_symbol");
    assert_eq!(exact.status, LaneStatus::Partial);
    assert_eq!(exact.candidate_count, 2);
    assert_eq!(exact.coverage.total_lower_bound, 3);
    assert_eq!(exact.truncation_reason.as_deref(), Some("candidate_limit"));
    req.file_paths = Some(vec!["src/p5.py".into()]);
    let scoped = engine.search_with_diagnostics(&req).unwrap();
    assert_eq!(scoped.hits[0].file_path, "src/p5.py");
    assert_eq!(lane(&scoped, "exact_symbol").status, LaneStatus::Complete);
    req.languages = Some(vec![cc_model::Language::Rust]);
    let empty = engine.search_with_diagnostics(&req).unwrap();
    assert!(empty.hits.is_empty());
    assert!(empty.lanes.iter().all(|l| l.candidates.is_empty()));
}

#[test]
fn path_token_budget_is_not_consumed_by_files_without_documents() {
    let mut files: Vec<_> = (0..40)
        .map(|n| (format!("a/needle{n:02}.py"), String::new()))
        .collect();
    files.push(("z/needle.py".into(), "def actual():\n    return 1\n".into()));
    let (_d, _index, engine) = fixture(
        &files,
        json!({"path_top_k":1,"exact_symbol_weight":0.0,"graph_weight":0.0}),
    );
    let mut req = request("needle");
    req.top_k = 1;
    let result = engine.search_with_diagnostics(&req).unwrap();
    let path = lane(&result, "path");
    assert_eq!(
        path.candidate_count, 1,
        "empty files must not hide an eligible document"
    );
    assert_eq!(result.hits[0].file_path, "z/needle.py");
}

#[test]
fn finite_but_overflowing_ranking_config_cannot_emit_nonfinite_scores() {
    let (_d, index, _engine) = fixture(
        &[("a.py".into(), "def needle():\n    return 7\n".into())],
        json!({}),
    );
    let mut config = ProjectConfig::default();
    config.ranking.overlap_weight = f64::MAX;
    config.ranking.symbol_exact_bonus = f64::MAX;
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
    assert!(engine.search_with_diagnostics(&request("needle")).is_err());
}

#[test]
fn lexical_diagnostics_retain_native_bm25_not_reciprocal_rank() {
    let (_d, _index, engine) = fixture(
        &[("a.py".into(), "def needle():\n    return 7\n".into())],
        json!({"path_weight":0.0,"exact_symbol_weight":0.0,"graph_weight":0.0}),
    );
    let r = engine.search_with_diagnostics(&request("needle")).unwrap();
    let lexical = lane(&r, "lexical");
    assert!(!lexical.candidates.is_empty());
    assert!(
        lexical.candidates[0].raw_score < 0.0,
        "native SQLite bm25 is negative-better; rank is a separate field"
    );
    assert_eq!(lexical.candidates[0].lane_rank, 1);
    let hit = &r.hits[0];
    assert!((hit.fused_score - 1.1 / 51.0).abs() < 1e-12);
    assert_eq!(
        hit.lexical_score, 1.0,
        "legacy wire slot stays reciprocal-rank"
    );
}

#[test]
fn graph_lane_read_failure_is_visible_and_not_result_cached() {
    let (d, _index, engine) = fixture(
        &[("a.py".into(), "def needle():\n    return 7\n".into())],
        json!({"path_weight":0.0,"exact_symbol_weight":0.0}),
    );
    let db = rusqlite::Connection::open(d.path().join(".codecortex/index.sqlite3")).unwrap();
    db.execute_batch("ALTER TABLE call_edges RENAME TO saved_call_edges")
        .unwrap();
    let req = request("needle");
    let broken = engine.search_with_diagnostics(&req).unwrap();
    assert_eq!(lane(&broken, "graph").status, LaneStatus::Error);
    let first = engine.search(&req).unwrap();
    assert!(!first.is_empty());
    db.execute_batch("ALTER TABLE saved_call_edges RENAME TO call_edges")
        .unwrap();
    // Fault injection deliberately does not bump an epoch: a degraded result
    // must not poison either cache for the rest of this generation.
    let second = engine.search(&req).unwrap();
    assert!(
        !std::sync::Arc::ptr_eq(&first, &second),
        "degraded query must retry, not hit the ordinary result cache"
    );
    let repaired = engine.search_with_diagnostics(&req).unwrap();
    assert_eq!(lane(&repaired, "graph").status, LaneStatus::Complete);
    db.execute_batch("ALTER TABLE call_edges RENAME TO saved_call_edges")
        .unwrap();
    let first_graph = engine
        .search_with_graph_context(
            &req,
            &cc_model::config::RepoSizeTier::Small.graph_enrich_limits(),
            4096,
        )
        .unwrap();
    assert!(first_graph
        .1
        .lane_outcomes
        .iter()
        .any(|l| l.lane_id == "graph" && l.status == LaneStatus::Error));
    db.execute_batch("ALTER TABLE saved_call_edges RENAME TO call_edges")
        .unwrap();
    let second_graph = engine
        .search_with_graph_context(
            &req,
            &cc_model::config::RepoSizeTier::Small.graph_enrich_limits(),
            4096,
        )
        .unwrap();
    assert!(!std::sync::Arc::ptr_eq(&first_graph, &second_graph));
}

#[test]
fn exact_paths_preserve_document_type_and_case_sensitive_scope() {
    let files = vec![
        (
            "docs/Guide.md".into(),
            "# Guide\nConfiguration reference.\n".into(),
        ),
        ("src/guide.py".into(), "def guide():\n    return 7\n".into()),
    ];
    let (_d, _i, engine) = fixture(&files, json!({"graph_weight":0.0}));
    let mut req = request("./docs//Guide.md");
    req.top_k = 1;
    req.pinned_file_paths = Some(vec!["src/guide.py".into()]);
    req.file_preselect_limit = Some(1);
    let result = engine.search_with_diagnostics(&req).unwrap();
    assert_eq!(result.hits[0].file_path, "docs/Guide.md");
    assert!(lane(&result, "path").candidates[0].exact_identity);
    req.path_prefix = Some("Docs".into());
    assert!(engine
        .search_with_diagnostics(&req)
        .unwrap()
        .hits
        .is_empty());
}
