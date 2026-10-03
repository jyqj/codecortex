//! Final evidence checks use real parser/SQLite/source bytes, never a provider oracle.
use cc_model::{
    generation::ReadGeneration,
    query::QueryControl,
    retrieval::{HardScope, LaneCoverage, LaneOutcome, LaneStatus, LANE_OUTCOME_SCHEMA_VERSION},
    search::{SearchHit, SearchRequest},
    semantic::{SemanticRecall, SemanticRequest},
    CcError, CcResult, ContextNode, Intent, Language, NodeType, Role,
};
use cc_search::{
    evidence_hydrator::{validate_envelope_generation, EvidenceHydrator},
    SearchEngine,
};
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    time::Duration,
};

fn fixture() -> (
    tempfile::TempDir,
    CodeIndex,
    Arc<cc_db::index_db::IndexDb>,
    Vec<SearchHit>,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "// 中文\r\npub fn needle() -> &'static str { \"α\" }\r\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b.py"),
        "def needle():\n    return 'beta'\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap().clone();
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let hits = engine
        .search(&SearchRequest {
            query: "needle".into(),
            top_k: 10,
            ..Default::default()
        })
        .unwrap()
        .to_vec();
    assert!(hits.iter().any(|h| h.file_path == "a.rs"));
    (dir, index, db, hits)
}
fn control() -> QueryControl {
    QueryControl::new(Duration::from_secs(10)).unwrap()
}

#[test]
fn final_hits_preserve_original_utf8_crlf_proofs_and_metadata() {
    let (dir, _index, db, hits) = fixture();
    let generation = db.reads().read_generation().unwrap();
    let mut hydrator =
        EvidenceHydrator::new(&db, dir.path(), HardScope::default(), generation, control())
            .unwrap();
    let verified = hydrator.hydrate(&hits).unwrap();
    assert_eq!(verified.len(), hits.len());
    for (a, b) in hits.iter().zip(&verified) {
        assert_eq!(a.text, b.text);
        assert_eq!(a.rerank_score, b.rerank_score);
        assert_eq!(a.metadata["document"], b.metadata["document"]);
        assert_eq!(a.metadata["source_evidence"], b.metadata["source_evidence"]);
    }
    assert_eq!(hydrator.diagnostics()["partial"], false);
    hydrator.finish().unwrap();
}

#[test]
fn no_lane_may_substitute_final_identity_coordinates_or_scope() {
    let (dir, _index, db, hits) = fixture();
    let original = hits.iter().find(|h| h.file_path == "a.rs").unwrap().clone();
    for n in 0..8 {
        let mut hit = original.clone();
        match n {
            0 => {
                hit.metadata.as_object_mut().unwrap().remove("document");
            }
            1 => hit.metadata["document"]["doc_version"] = json!("0".repeat(64)),
            2 => hit.metadata["source_evidence"]["span"]["start"] = json!(999999),
            3 => hit.language = Language::Python,
            4 => hit.file_path = "b.py".into(),
            5 => hit.text.push('!'),
            6 => hit.start_line += 1,
            _ => hit.metadata["source_evidence"]["source"]["snapshot_id"] = json!("0".repeat(64)),
        }
        let mut hydrator = EvidenceHydrator::new(
            &db,
            dir.path(),
            HardScope::default(),
            db.reads().read_generation().unwrap(),
            control(),
        )
        .unwrap();
        assert!(
            hydrator.hydrate(&[hit]).is_err(),
            "accepted invalid final evidence case {n}"
        );
    }
    for scope in [
        HardScope {
            file_paths: Some(vec![]),
            ..Default::default()
        },
        HardScope {
            file_paths: Some(vec!["b.py".into()]),
            ..Default::default()
        },
        HardScope {
            languages: Some(vec![Language::Python]),
            ..Default::default()
        },
    ] {
        let mut hydrator = EvidenceHydrator::new(
            &db,
            dir.path(),
            scope,
            db.reads().read_generation().unwrap(),
            control(),
        )
        .unwrap();
        assert!(hydrator.hydrate(std::slice::from_ref(&original)).is_err());
    }
}

#[test]
fn disk_changes_and_deletions_are_explicit_omissions_not_new_source() {
    let (dir, _index, db, hits) = fixture();
    let original = hits.iter().find(|h| h.file_path == "a.rs").unwrap().clone();
    for deleted in [false, true] {
        if deleted {
            std::fs::remove_file(dir.path().join("a.rs")).unwrap();
        } else {
            std::fs::write(dir.path().join("a.rs"), "pub fn changed() {}\n").unwrap();
        }
        let mut hydrator = EvidenceHydrator::new(
            &db,
            dir.path(),
            HardScope::default(),
            db.reads().read_generation().unwrap(),
            control(),
        )
        .unwrap();
        assert!(hydrator
            .hydrate(std::slice::from_ref(&original))
            .unwrap()
            .is_empty());
        assert_eq!(hydrator.diagnostics()["partial"], true);
        assert_eq!(
            hydrator.diagnostics()["omitted_files"]["a.rs"],
            if deleted {
                "deleted"
            } else {
                "stale_with_disk_change"
            }
        );
    }
}

#[test]
fn swap_same_epoch_and_cancel_reject_final_publication() {
    let (dir, _index, db, hits) = fixture();
    let generation = db.reads().read_generation().unwrap();
    let mut hydrator =
        EvidenceHydrator::new(&db, dir.path(), HardScope::default(), generation, control())
            .unwrap();
    hydrator.hydrate(&hits).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "UPDATE metadata SET value=lower(hex(randomblob(16))) WHERE key='index_incarnation'",
        [],
    )
    .unwrap();
    assert!(matches!(
        hydrator.finish(),
        Err(CcError::RetrievalChanged { .. })
    ));
    let ctl = control();
    let mut cancelled = EvidenceHydrator::new(
        &db,
        dir.path(),
        HardScope::default(),
        db.reads().read_generation().unwrap(),
        ctl.clone(),
    )
    .unwrap();
    ctl.cancel();
    assert!(matches!(
        cancelled.hydrate(&hits),
        Err(CcError::QueryCancelled)
    ));
}

#[test]
fn warmed_search_cannot_hide_deleted_manifest_or_forged_snapshot() {
    let (dir, index, db, hits) = fixture();
    index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .unwrap();
    let chosen = hits.iter().find(|h| h.file_path == "a.rs").unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    let mut forged = chosen.clone();
    forged.metadata["source_evidence"]["source"]["snapshot_id"] = json!("f".repeat(64));
    conn.execute(
        "UPDATE chunks SET source_json=? WHERE chunk_id=?",
        rusqlite::params![
            forged.metadata["source_evidence"].to_string(),
            chosen.chunk_id
        ],
    )
    .unwrap();
    let mut hydrator = EvidenceHydrator::new(
        &db,
        dir.path(),
        HardScope::default(),
        db.reads().read_generation().unwrap(),
        control(),
    )
    .unwrap();
    assert!(hydrator.hydrate(&[forged]).is_err());
    conn.execute(
        "DELETE FROM document_manifest WHERE chunk_id=?",
        [&chosen.chunk_id],
    )
    .unwrap();
    assert!(index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .is_err());
}

#[test]
fn warm_context_revalidates_the_full_record_not_only_its_reference_mirror() {
    let (_dir, index, db, _) = fixture();
    let value = index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .unwrap();
    let id = value.machine_pack["hits"][0]["chunk_id"].as_str().unwrap();
    let before = db.reads().read_generation().unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "UPDATE document_manifest SET record_json='{}' WHERE chunk_id=?",
        [id],
    )
    .unwrap();
    assert_eq!(before, db.reads().read_generation().unwrap());
    assert!(index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .is_err());
}

#[test]
fn graph_nodes_and_post_handler_metadata_keep_the_same_scope_and_generation() {
    let (dir, index, db, _) = fixture();
    let scope = HardScope {
        file_paths: Some(vec!["a.rs".into()]),
        ..Default::default()
    };
    let mut hydrator = EvidenceHydrator::new(
        &db,
        dir.path(),
        scope,
        db.reads().read_generation().unwrap(),
        control(),
    )
    .unwrap();
    let mut node = ContextNode::new(
        "edge".into(),
        NodeType::CallEdge,
        Role::Neighbor,
        "Caller".into(),
        "relation description".into(),
    );
    node.file_path = Some("b.py".into());
    assert!(hydrator.graph_current(&node).is_err());
    node.file_path = Some("a.rs".into());
    assert!(hydrator.graph_current(&node).unwrap());
    node.span_kind = Some("indexed_chunk".into());
    assert!(
        hydrator.graph_current(&node).is_err(),
        "unproven source slice is not a graph description"
    );
    let value = serde_json::to_value(
        index
            .search()
            .search_in_context("needle", 1, Some(Intent::Locate))
            .unwrap(),
    )
    .unwrap();
    validate_envelope_generation(&db, &value).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO metadata(key,value) VALUES('evidence_epoch','99')",
        [],
    )
    .unwrap();
    assert!(matches!(
        validate_envelope_generation(&db, &value),
        Err(CcError::RetrievalChanged { .. })
    ));
}

/// Real parser-produced source identity; only recall scoring is fake. The
/// production adapter must independently fence its publication and generation.
struct DenseReceipt {
    db: Arc<cc_db::index_db::IndexDb>,
    generation: ReadGeneration,
    outcome: LaneOutcome,
    switch_during_recall: bool,
}
impl SemanticRecall for DenseReceipt {
    fn recall(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            control.check()?;
            assert_eq!(request.generation, self.generation);
            assert_eq!(self.db.reads().read_generation()?, self.generation);
            if self.switch_during_recall {
                self.db
                    .switch_semantic_active_space("next", "hydration-test")?;
            }
            Ok(self.outcome.clone())
        })
    }
}

fn dense_receipt(db: &Arc<cc_db::index_db::IndexDb>, hit: &SearchHit) -> LaneOutcome {
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let outcome = engine
        .search_with_diagnostics(&SearchRequest {
            query: "needle".into(),
            top_k: 10,
            include_grep: false,
            ..Default::default()
        })
        .unwrap();
    let mut candidate = outcome
        .lanes
        .iter()
        .flat_map(|lane| &lane.candidates)
        .find(|candidate| candidate.legacy_chunk_id == hit.chunk_id)
        .unwrap()
        .clone();
    candidate.lane_id = "semantic".into();
    candidate.lane_rank = 1;
    candidate.exact_identity = false;
    candidate.raw_score = 0.8;
    candidate.scoring_spec = "fake-hydration-fence-test-v1".into();
    let receipt = LaneOutcome {
        schema_version: LANE_OUTCOME_SCHEMA_VERSION,
        lane_id: "semantic".into(),
        weight: 1.0,
        status: LaneStatus::Complete,
        elapsed_us: 0,
        candidate_count: 1,
        coverage: LaneCoverage::complete(Some(1), 1),
        truncation_reason: None,
        candidates: vec![candidate],
    };
    receipt.validate().unwrap();
    receipt
}

fn seed_dense_publication(db: &cc_db::index_db::IndexDb, hit: &SearchHit) {
    let reference: cc_model::identity::DocumentRef =
        serde_json::from_value(hit.metadata["document"].clone()).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute_batch("INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('old','{}','active'),('next','{}','backfilling');").unwrap();
    conn.execute(
        "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,space_id,artifact_ref,published_at,published_incarnation)
         VALUES(?1,?2,?3,?4,'test-input','old','test-artifact','2026-01-01','test-incarnation')",
        rusqlite::params![reference.doc_key, reference.doc_version, hit.file_path, reference.encoding_key],
    ).unwrap();
}

fn switch_dense_space(db: &cc_db::index_db::IndexDb) -> ReadGeneration {
    let before = db.reads().read_generation().unwrap();
    let stats = db
        .switch_semantic_active_space("next", "hydration-test")
        .unwrap();
    assert!(stats.visible_set_switched);
    let after = db.reads().read_generation().unwrap();
    assert_ne!(after.semantic_epoch, before.semantic_epoch);
    assert_eq!(after.index_epoch, before.index_epoch);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    after
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stable_generation_with_skipped_dense_basis_is_partial_in_final_response() {
    let (_dir, index, db, hits) = fixture();
    let hit = hits.iter().find(|hit| hit.file_path == "a.rs").unwrap();
    let outcome = dense_receipt(&db, hit);
    seed_dense_publication(&db, hit);
    // A real typed switch commits BEFORE the query. Its current generation
    // is stable throughout recall/assembly, but the returned publication is
    // foreign. This is the defensive SemanticRecall port boundary, not a
    // claim that the built-in exact backend emits stale-space candidates.
    let generation = switch_dense_space(&db);
    index.set_semantic_recall(Some(Arc::new(DenseReceipt {
        db: db.clone(),
        generation,
        outcome,
        switch_during_recall: false,
    })));
    let envelope = cc_server::handlers::context::search_async(
        Arc::new(RwLock::new(index)),
        "conceptual query without lexical matches".into(),
        5,
        Some(Intent::Locate),
        SearchRequest {
            retrieval_strategy: Some(cc_model::query::RetrievalStrategy::Semantic),
            include_grep: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    validate_envelope_generation(&db, &envelope).unwrap();
    let freshness = &envelope["evidence_summary"]["source_freshness"];
    assert_eq!(freshness["generation"], json!(generation));
    assert_eq!(freshness["dense_manifest_fence"]["skipped"], 1);
    assert!(envelope["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    let lanes = envelope["evidence_summary"]["retrieval"]["lanes"]
        .as_array()
        .unwrap();
    assert_eq!(
        lanes
            .iter()
            .find(|lane| lane["lane_id"] == "semantic")
            .unwrap()["status"],
        "complete"
    );
    assert!(lanes
        .iter()
        .all(|lane| matches!(lane["status"].as_str(), Some("complete" | "disabled"))));
    let (_, status) = cc_eval::benchmark::normalizer::mcp(&envelope).unwrap();
    assert_eq!(
        (
            freshness["partial"].as_bool(),
            envelope["summary"].as_str().unwrap().contains("incomplete"),
            status
        ),
        (
            Some(true),
            true,
            cc_eval::benchmark::schema::ResultStatus::Partial
        ),
        "a publication omission cannot become a complete empty answer",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn current_dense_basis_keeps_final_response_complete() {
    let (_dir, index, db, hits) = fixture();
    let hit = hits.iter().find(|hit| hit.file_path == "a.rs").unwrap();
    let outcome = dense_receipt(&db, hit);
    seed_dense_publication(&db, hit);
    let generation = db.reads().read_generation().unwrap();
    index.set_semantic_recall(Some(Arc::new(DenseReceipt {
        db: db.clone(),
        generation,
        outcome,
        switch_during_recall: false,
    })));
    let envelope = index
        .query_handle()
        .unwrap()
        .search_async(
            "conceptual query without lexical matches".into(),
            5,
            Some(Intent::Locate),
            SearchRequest {
                retrieval_strategy: Some(cc_model::query::RetrievalStrategy::Semantic),
                include_grep: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(envelope.machine_pack["hits"][0]["file_path"], "a.rs");
    assert_eq!(
        envelope.evidence_summary["source_freshness"]["partial"],
        false
    );
    assert_eq!(
        envelope.evidence_summary["source_freshness"]["dense_manifest_fence"]["skipped"],
        0
    );
    let (_, status) =
        cc_eval::benchmark::normalizer::mcp(&serde_json::to_value(envelope).unwrap()).unwrap();
    assert_eq!(status, cc_eval::benchmark::schema::ResultStatus::Success);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_space_switch_during_recall_rejects_the_old_generation() {
    let (_dir, index, db, hits) = fixture();
    let hit = hits.iter().find(|hit| hit.file_path == "a.rs").unwrap();
    let outcome = dense_receipt(&db, hit);
    seed_dense_publication(&db, hit);
    let generation = db.reads().read_generation().unwrap();
    index.set_semantic_recall(Some(Arc::new(DenseReceipt {
        db: db.clone(),
        generation,
        outcome,
        switch_during_recall: true,
    })));
    let result = index
        .query_handle()
        .unwrap()
        .search_async(
            "conceptual query without lexical matches".into(),
            5,
            Some(Intent::Locate),
            SearchRequest {
                retrieval_strategy: Some(cc_model::query::RetrievalStrategy::Semantic),
                include_grep: false,
                ..Default::default()
            },
        )
        .await;
    assert_ne!(
        db.reads().read_generation().unwrap().semantic_epoch,
        generation.semantic_epoch
    );
    assert!(matches!(result, Err(CcError::RetrievalChanged { .. })));
}

#[test]
fn dense_omission_persists_across_hydration_windows_but_generation_changes_fail() {
    let (dir, _index, db, hits) = fixture();
    let mut hit = hits
        .iter()
        .find(|hit| hit.file_path == "a.rs")
        .unwrap()
        .clone();
    seed_dense_publication(&db, &hit);
    hit.reasons.push("semantic@1".into());
    let generation = db.reads().read_generation().unwrap();
    let mut hydrator =
        EvidenceHydrator::new(&db, dir.path(), HardScope::default(), generation, control())
            .unwrap();
    assert_eq!(
        hydrator.hydrate(std::slice::from_ref(&hit)).unwrap().len(),
        1
    );
    switch_dense_space(&db);
    assert!(matches!(
        hydrator.finish(),
        Err(CcError::RetrievalChanged { .. })
    ));
    let mut fresh = EvidenceHydrator::new(
        &db,
        dir.path(),
        HardScope::default(),
        db.reads().read_generation().unwrap(),
        control(),
    )
    .unwrap();
    assert!(fresh
        .hydrate(std::slice::from_ref(&hit))
        .unwrap()
        .is_empty());
    assert!(fresh
        .hydrate(std::slice::from_ref(&hit))
        .unwrap()
        .is_empty());
    assert!(fresh.hydrate(&[]).unwrap().is_empty());
    assert_eq!(
        fresh.diagnostics()["dense_manifest_fence"]["skipped"],
        1,
        "fence memoizes per document"
    );
    assert_eq!(fresh.diagnostics()["omitted_files"], json!({}));
    assert_eq!(fresh.diagnostics()["partial"], true);
}
