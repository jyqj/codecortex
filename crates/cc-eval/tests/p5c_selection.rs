//! P5-C selection and persistent read identity using actual parser/SQLite data.
use cc_model::{
    search::{SearchHit, SearchRequest},
    Intent,
};
use cc_search::{selection::coverage::select, SearchEngine};
use cc_server::engine::CodeIndex;

fn fixture() -> (tempfile::TempDir, CodeIndex, Vec<SearchHit>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = ["src/a.rs", "src/b.rs", "src/c.rs", "tests/a.rs"];
    // Same bytes deliberately occur in multiple files; provenance cannot be
    // replaced by content-only deduplication.
    for path in paths {
        let dest = dir.path().join(path);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, "pub fn needle() -> i32 { 7 }\n").unwrap();
    }
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let ranked = paths
        .iter()
        .map(|path| {
            let result = engine
                .search_with_diagnostics(&SearchRequest {
                    query: "needle".into(),
                    file_paths: Some(vec![(*path).into()]),
                    top_k: 1,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(result.hits.len(), 1);
            result.hits[0].clone()
        })
        .collect();
    (dir, index, ranked)
}

#[test]
fn locate_keeps_rank_order_without_forced_file_diversity() {
    let (_dir, _index, ranked) = fixture();
    let (selected, report) = select(&ranked, Intent::Locate, 3).unwrap();
    assert_eq!(report.original_ranks, vec![1, 2, 3]);
    assert_eq!(selected.len(), 3);
    for (before, after) in ranked.iter().zip(&selected) {
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }
    assert_eq!(report.overlap.redundant_bytes, 0);
    assert_eq!(
        report.overlap.unique_bytes,
        ranked.iter().map(|h| h.text.len()).sum::<usize>()
    );
}

#[test]
fn fix_reserves_test_evidence_but_never_displaces_rank_one() {
    let (_dir, _index, ranked) = fixture();
    let (selected, report) = select(&ranked, Intent::Fix, 3).unwrap();
    assert_eq!(report.original_ranks, vec![1, 2, 4]);
    assert_eq!(selected[0].chunk_id, ranked[0].chunk_id);
    assert_eq!(selected[2].file_path, "tests/a.rs");
    assert_eq!(report.facets["test"], 1);
    assert!(report.unmet_facets.contains(&"interface".to_string()));
    for _ in 0..3 {
        let (_, repeat) = select(&ranked, Intent::Fix, 3).unwrap();
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::to_value(&repeat).unwrap()
        );
    }
    assert_eq!(
        select(&ranked, Intent::Fix, 1).unwrap().0[0].chunk_id,
        ranked[0].chunk_id
    );
    assert!(select(&ranked, Intent::Fix, 0).unwrap().0.is_empty());
}

#[test]
fn duplicate_source_is_suppressed_but_cross_file_copy_is_not() {
    let (_dir, _index, ranked) = fixture();
    let duplicated = vec![ranked[0].clone(), ranked[0].clone(), ranked[3].clone()];
    assert_eq!(duplicated[0].text, duplicated[2].text);
    let (chosen, report) = select(&duplicated, Intent::Locate, 3).unwrap();
    assert_eq!(report.original_ranks, vec![1, 3]);
    assert_eq!(chosen.len(), 2);
    assert_eq!(report.overlap.fully_covered_candidates, 1);
    assert_eq!(report.overlap.redundant_bytes, ranked[0].text.len());
    assert_ne!(chosen[0].file_path, chosen[1].file_path);
}

#[test]
fn a_single_file_can_contribute_multiple_disjoint_pieces() {
    let (_dir, _index, ranked) = fixture();
    let mut pieces = Vec::new();
    for (start, end) in [(0, 4), (4, 9), (9, 15)] {
        let mut hit = ranked[0].clone();
        hit.metadata["source_evidence"]["span"] = serde_json::json!({"start":start,"end":end});
        pieces.push(hit);
    }
    // Selection only accounts already-validated spans; independent interval
    // tests intentionally supply a known partition, not parser-generated gold.
    let (selected, report) = select(&pieces, Intent::Locate, 3).unwrap();
    assert_eq!(selected.len(), 3);
    assert_eq!(report.overlap.unique_bytes, 15);
    assert_eq!(report.overlap.redundant_bytes, 0);
}

#[test]
fn public_context_reports_selection_and_cache_windows_do_not_alias() {
    let (_dir, index, _) = fixture();
    let envelope = index
        .search()
        .search_in_context("needle", 2, Some(Intent::Locate))
        .unwrap();
    assert_eq!(envelope.evidence_summary["selection"]["selected"], 2);
    assert_eq!(envelope.machine_pack["hits"].as_array().unwrap().len(), 2);
    assert_eq!(
        envelope.evidence_summary["selection"]["spec"],
        cc_search::selection::SELECTION_SPEC
    );
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let req = SearchRequest {
        query: "needle".into(),
        top_k: 1,
        ..Default::default()
    };
    let limits = cc_model::config::RepoSizeTier::Tiny.graph_enrich_limits();
    assert_eq!(
        engine
            .search_with_graph_context(&req, &limits, 4000)
            .unwrap()
            .0
            .len(),
        1
    );
    assert!(
        engine
            .search_context_candidates(&req, &limits, 4000)
            .unwrap()
            .0
            .len()
            > 1
    );
    assert_eq!(
        engine
            .search_with_graph_context(&req, &limits, 4000)
            .unwrap()
            .0
            .len(),
        1
    );
}

#[test]
fn malformed_incarnation_fails_even_when_result_is_warm() {
    let (_dir, index, _) = fixture();
    let db = index.index_db().unwrap();
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let request = SearchRequest {
        query: "needle".into(),
        ..Default::default()
    };
    assert!(!engine.search(&request).unwrap().is_empty());
    assert!(!engine.search(&request).unwrap().is_empty());
    let connection = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    connection
        .execute(
            "UPDATE metadata SET value='bad' WHERE key='index_incarnation'",
            [],
        )
        .unwrap();
    assert!(engine.search(&request).is_err());
}

#[test]
fn only_selected_evidence_can_suppress_an_overlapping_facet() {
    let (_dir, _index, ranked) = fixture();
    let mut interface = ranked[2].clone();
    interface.symbol_kind = Some(cc_model::SymbolKind::Interface);
    // Same validated source coordinates can support distinct structural views.
    // An earlier, unselected view must not veto the required interface facet.
    let candidates = vec![
        ranked[0].clone(),
        ranked[1].clone(),
        ranked[2].clone(),
        interface,
        ranked[3].clone(),
    ];
    let (_, report) = select(&candidates, Intent::Fix, 4).unwrap();
    assert_eq!(report.original_ranks, vec![1, 2, 4, 5]);
    assert_eq!(report.facets["interface"], 1);
    assert_eq!(report.source_covered_omissions, 1);
}

#[test]
fn auxiliary_metadata_does_not_change_read_generation() {
    let (_dir, index, _) = fixture();
    let db = index.index_db().unwrap();
    let before = db.reads().read_generation().unwrap();
    let connection = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO metadata(key,value) VALUES('worker_heartbeat','test')",
            [],
        )
        .unwrap();
    assert_eq!(before, db.reads().read_generation().unwrap());
}
