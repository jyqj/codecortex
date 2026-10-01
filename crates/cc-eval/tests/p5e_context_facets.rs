use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_search::SearchEngine;
use cc_server::engine::CodeIndex;

fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".codecortex" {
            continue;
        }
        let path = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &path)
        } else if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), path).unwrap();
        }
    }
}

#[test]
fn mixed_language_framework_context_preserves_the_requested_handler() {
    let dir = tempfile::tempdir().unwrap();
    copy(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sample-project"),
        dir.path(),
    );
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let config = ProjectConfig::default();
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
    let query = "understand the Flask API routes and their handlers";
    let request = SearchRequest {
        query: query.into(),
        top_k: 10,
        ..Default::default()
    };
    let result = engine.search_with_diagnostics(&request).unwrap();
    let graph_result = engine
        .search_with_graph_context(
            &request,
            &cc_model::config::RepoSizeTier::Tiny.graph_enrich_limits(),
            4000,
        )
        .unwrap();
    let context_candidates = engine
        .search_context_candidates(
            &request,
            &cc_model::config::RepoSizeTier::Tiny.graph_enrich_limits(),
            4000,
        )
        .unwrap();
    let envelope = index.search().search_in_context(query, 10, None).unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("framework-ranking.json"),serde_json::to_vec_pretty(&serde_json::json!({"plain":result.hits,"graph":graph_result.0,"window":context_candidates.0,"envelope":envelope})).unwrap()).unwrap();
    }
    assert!(envelope.machine_pack["hits"].as_array().unwrap().iter().any(|hit|hit["file_path"]=="server.py" && hit["text"].as_str().is_some_and(|text|text.contains("api_get_user"))),"requested framework implementation must retain real handler body, not an omitted reference");
    let handler = envelope.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| {
            hit["file_path"] == "server.py"
                && hit["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("api_get_user"))
        })
        .unwrap();
    assert!(handler["text"]
        .as_str()
        .unwrap()
        .contains("@app.route('/users/<int:user_id>'"));
    assert!(handler["text"]
        .as_str()
        .unwrap()
        .contains("return jsonify(get_user(user_id))"));
    let (mut hits, _) =
        cc_eval::benchmark::normalizer::mcp(&serde_json::to_value(&envelope).unwrap()).unwrap();
    for hit in &mut hits {
        cc_eval::benchmark::normalizer::verify_source(hit, dir.path()).unwrap();
        assert_eq!(hit.evidence_valid, Some(true));
    }
}

#[test]
fn distinctive_program_support_is_literal_bounded_owned_and_not_injected() {
    use cc_search::selection::coverage::select_with_query;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("noise.py"),
        "def noise_api():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("focus.py"),"from neostack import NeoStack\n\ndef needle_handler():\n    return 7\n\ndef unrelated_sibling():\n    return 99\n").unwrap();
    std::fs::write(
        dir.path().join("decoy.py"),
        "def performance_format_superuser():\n    return 77\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("title.py"),
        "def needle_title_decoy():\n    return 'Understand'\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let engine = SearchEngine::new(
        index.index_db().unwrap().clone(),
        &ProjectConfig::default(),
        None,
    );
    let mut ranked = engine
        .search_with_diagnostics(&SearchRequest {
            query: "noise_api".into(),
            file_paths: Some(vec!["noise.py".into()]),
            top_k: 1,
            ..Default::default()
        })
        .unwrap()
        .hits;
    let import = engine
        .search_with_diagnostics(&SearchRequest {
            query: "NeoStack".into(),
            file_paths: Some(vec!["focus.py".into()]),
            top_k: 1,
            ..Default::default()
        })
        .unwrap()
        .hits[0]
        .clone();
    let function = engine
        .search_with_diagnostics(&SearchRequest {
            query: "needle_handler".into(),
            file_paths: Some(vec!["focus.py".into()]),
            top_k: 1,
            ..Default::default()
        })
        .unwrap()
        .hits[0]
        .clone();
    let sibling = engine
        .search_with_diagnostics(&SearchRequest {
            query: "unrelated_sibling".into(),
            file_paths: Some(vec!["focus.py".into()]),
            top_k: 1,
            ..Default::default()
        })
        .unwrap()
        .hits[0]
        .clone();
    ranked.extend([import, function, sibling]);
    let (chosen, report) = select_with_query(
        &ranked,
        cc_model::Intent::Locate,
        3,
        "understand NeoStack needle",
    )
    .unwrap();
    assert!(chosen
        .iter()
        .any(|hit| hit.symbol_name.as_deref() == Some("needle_handler")));
    assert!(!report.source_support_anchors.is_empty());
    assert_eq!(chosen[0].chunk_id, ranked[0].chunk_id);
    for hit in &chosen {
        let original = ranked
            .iter()
            .find(|old| old.chunk_id == hit.chunk_id)
            .unwrap();
        assert_eq!(hit.rerank_score, original.rerank_score);
        assert_eq!(hit.score_trace, original.score_trace);
    }
    let valid_decoy = engine
        .search_with_diagnostics(&SearchRequest {
            query: "performance_format_superuser".into(),
            file_paths: Some(vec!["decoy.py".into()]),
            top_k: 1,
            ..Default::default()
        })
        .unwrap()
        .hits[0]
        .clone();
    let decoys = vec![ranked[0].clone(), valid_decoy];
    for query in ["understand ORM needle", "understand User needle"] {
        assert!(
            select_with_query(&decoys, cc_model::Intent::Locate, 3, query)
                .unwrap()
                .1
                .source_support_anchors
                .is_empty()
        );
    }
    for query in ["理解NeoStack needle", "use ns.NeoStack needle"] {
        assert!(
            !select_with_query(&ranked, cc_model::Intent::Locate, 3, query)
                .unwrap()
                .1
                .source_support_anchors
                .is_empty()
        );
    }
    let title_hit = engine
        .search_with_diagnostics(&SearchRequest {
            query: "needle_title_decoy".into(),
            file_paths: Some(vec!["title.py".into()]),
            top_k: 1,
            ..Default::default()
        })
        .unwrap()
        .hits[0]
        .clone();
    let mut title_ranked = ranked.clone();
    title_ranked.insert(1, title_hit);
    let (_, title_report) = select_with_query(
        &title_ranked,
        cc_model::Intent::Locate,
        3,
        "Understand NeoStack needle",
    )
    .unwrap();
    assert!(!title_report.source_support_anchors.is_empty());
    assert!(title_report
        .source_support_anchors
        .iter()
        .all(|id| title_ranked
            .iter()
            .find(|hit| hit.chunk_id == *id)
            .unwrap()
            .file_path
            == "focus.py"));
    let mut ambiguous = ranked.clone();
    let mut duplicate = ambiguous[1].clone();
    duplicate.file_path = "different.py".into();
    ambiguous.insert(2, duplicate);
    assert!(select_with_query(
        &ambiguous,
        cc_model::Intent::Locate,
        3,
        "understand NeoStack needle"
    )
    .unwrap()
    .1
    .source_support_anchors
    .is_empty());
    let mut no_owner = ranked.clone();
    no_owner[2].metadata["source_evidence"]["owner"] = serde_json::Value::Null;
    assert!(select_with_query(
        &no_owner,
        cc_model::Intent::Locate,
        3,
        "understand NeoStack needle"
    )
    .unwrap()
    .1
    .source_support_anchors
    .is_empty());
    let mut injected = ranked;
    injected[0].metadata["coverage_priority"] =
        serde_json::json!("distinctive_source_implementation");
    injected[0].metadata["evidence_priority"] = serde_json::json!("intent_facet");
    let clean = select_with_query(&injected, cc_model::Intent::Locate, 3, "absentProgramToken")
        .unwrap()
        .0;
    assert!(clean
        .iter()
        .all(|hit| hit.metadata.get("coverage_priority").is_none()
            && hit.metadata.get("evidence_priority").is_none()));
}
