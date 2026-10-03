//! P7-012 local receipt/packing acceptance: real parser/DB/handler output,
//! fake recall only. No provider quality or live timeout certification.
use cc_eval::benchmark::{normalizer, schema::ResultStatus};
use cc_model::{
    query::{QueryControl, RetrievalStrategy},
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus},
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest},
    CcResult, Intent,
};
use cc_search::{evidence_hydrator::validate_envelope_generation, selection::budget::pack_value};
use cc_server::{engine::CodeIndex, handlers};
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

struct ReceiptPort(LaneOutcome);
impl SemanticRecall for ReceiptPort {
    fn recall(
        &self,
        _request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            control.check()?;
            Ok(self.0.clone())
        })
    }
}

async fn response(
    status: LaneStatus,
    dense_candidate: bool,
    lexical_match: bool,
    switch_before_query: bool,
) -> (tempfile::TempDir, Arc<cc_db::index_db::IndexDb>, Value) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("a.rs"), "pub fn needle() -> i32 { 7 }\n").unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap().clone();
    let mut outcome = LaneOutcome {
        status,
        truncation_reason: (status != LaneStatus::Complete)
            .then(|| "injected_backfill_or_failure".into()),
        coverage: if status == LaneStatus::Complete {
            LaneCoverage::complete(Some(0), 0)
        } else if status == LaneStatus::Partial {
            LaneCoverage::partial(Some(0), 0)
        } else {
            LaneCoverage::not_run()
        },
        ..LaneOutcome::disabled("semantic", 1.0)
    };
    if dense_candidate {
        assert!(status.is_fusable());
        let engine = cc_search::SearchEngine::new(db.clone(), &Default::default(), None);
        let detailed = engine
            .search_with_diagnostics(&SearchRequest {
                query: "needle".into(),
                top_k: 5,
                include_grep: false,
                ..Default::default()
            })
            .unwrap();
        let mut candidate = detailed
            .lanes
            .iter()
            .flat_map(|lane| &lane.candidates)
            .next()
            .unwrap()
            .clone();
        candidate.lane_id = "semantic".into();
        candidate.lane_rank = 1;
        candidate.exact_identity = false;
        candidate.raw_score = 0.8;
        candidate.scoring_spec = "fake-partial-budget-v1".into();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.execute_batch("INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('old','{}','active'),('next','{}','backfilling');").unwrap();
        conn.execute("INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,space_id,artifact_ref,published_at,published_incarnation)
            SELECT doc_key,doc_version,file_path,encoding_key,'test-input','old','test-artifact','2026-01-01','test-incarnation'
            FROM document_manifest WHERE doc_key=?1", [&candidate.document.doc_key]).unwrap();
        drop(conn);
        outcome.candidate_count = 1;
        outcome.coverage = if status == LaneStatus::Complete {
            LaneCoverage::complete(Some(1), 1)
        } else {
            LaneCoverage::partial(Some(1), 1)
        };
        outcome.candidates = vec![candidate];
        if switch_before_query {
            assert!(
                db.switch_semantic_active_space("next", "partial-budget-test")
                    .unwrap()
                    .visible_set_switched
            );
        }
    }
    outcome.validate().unwrap();
    index.set_semantic_recall(Some(Arc::new(ReceiptPort(outcome))));
    let value = handlers::context::search_async(
        Arc::new(RwLock::new(index)),
        if lexical_match {
            "needle"
        } else {
            "conceptual query without lexical matches"
        }
        .into(),
        5,
        Some(Intent::Locate),
        SearchRequest {
            retrieval_strategy: Some(RetrievalStrategy::Semantic),
            include_grep: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    validate_envelope_generation(&db, &value).unwrap();
    (dir, db, value)
}

fn semantic_header(value: &Value) -> &Value {
    let retrieval = &value["evidence_summary"]["retrieval"];
    let lanes = retrieval
        .get("lanes")
        .or_else(|| retrieval.get("lane_receipts"))
        .unwrap();
    lanes
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["lane_id"] == "semantic")
        .unwrap()
}

fn assert_accounted(value: &Value, cap: usize) {
    let encoded = serde_json::to_vec(value).unwrap();
    assert!(encoded.len() <= cap);
    assert_eq!(
        value["evidence_summary"]["packing"]["used_bytes"],
        encoded.len()
    );
    assert_eq!(value["token_estimate"], encoded.len().div_ceil(4));
    assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap(), *value);
}

fn assert_projected_header(before: &Value, after: &Value) {
    assert!(after["evidence_summary"]["retrieval"]
        .get("lanes")
        .is_none());
    let receipts: Vec<cc_model::lane_receipt::LaneReceipt> =
        serde_json::from_value(after["evidence_summary"]["retrieval"]["lane_receipts"].clone())
            .unwrap();
    for receipt in receipts {
        receipt.validate().unwrap();
    }
    for key in [
        "status",
        "coverage",
        "candidate_count",
        "truncation_reason",
        "weight",
        "elapsed_us",
    ] {
        assert_eq!(
            semantic_header(before)[key],
            semantic_header(after)[key],
            "changed {key}"
        );
    }
    assert_eq!(semantic_header(after)["candidate_details_omitted"], true);
}

fn compact_optional_diagnostics(mut value: Value) -> Value {
    // Stress only optional explanatory metadata. Candidate/source identities,
    // lane coverage and omission truth remain the actual handler's output.
    value["evidence_summary"]["source_freshness"]["scope"] =
        json!("bounded source verification explanation; ".repeat(4096));
    pack_value(value, 8192).unwrap()
}

#[tokio::test]
async fn executed_lane_states_survive_real_projection_and_empty_result_classification() {
    for (status, lexical_match, expected) in [
        (LaneStatus::Complete, false, ResultStatus::NoMatch),
        (LaneStatus::Partial, false, ResultStatus::Partial),
        (LaneStatus::Partial, true, ResultStatus::Partial),
        (LaneStatus::Timeout, false, ResultStatus::Partial),
        (LaneStatus::Unavailable, false, ResultStatus::Partial),
    ] {
        let (dir, db, original) = response(status, false, lexical_match, false).await;
        assert_eq!(normalizer::mcp(&original).unwrap().1, expected);
        let packed = compact_optional_diagnostics(original.clone());
        assert_accounted(&packed, 8192);
        assert_projected_header(&original, &packed);
        validate_envelope_generation(&db, &packed).unwrap();
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["partial"],
            false
        );
        assert_eq!(
            packed["evidence_summary"]["packing"]["partial"], false,
            "metadata-only compaction cannot manufacture incomplete evidence"
        );
        let (mut hits, classification) = normalizer::mcp(&packed).unwrap();
        assert_eq!(
            classification, expected,
            "{status:?}, lexical={lexical_match}"
        );
        assert_eq!(!hits.is_empty(), lexical_match);
        for hit in &mut hits {
            normalizer::verify_source(hit, dir.path()).unwrap();
            assert_eq!(hit.evidence_valid, Some(true));
        }
        assert_eq!(pack_value(packed.clone(), 8192).unwrap(), packed);
        assert_eq!(
            normalizer::mcp(&pack_value(packed.clone(), 12000).unwrap())
                .unwrap()
                .1,
            expected
        );
        assert!(
            pack_value(packed, 512).is_err(),
            "an impossible receipt budget must fail explicitly"
        );
    }
}

#[tokio::test]
async fn partial_dense_candidates_remain_usable_without_losing_receipt_counts_or_scores() {
    let (dir, db, original) = response(LaneStatus::Partial, true, false, false).await;
    let hit = original["machine_pack"]["hits"][0].clone();
    assert!(hit["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().is_some_and(|s| s.starts_with("semantic@"))));
    let packed = compact_optional_diagnostics(original.clone());
    assert_projected_header(&original, &packed);
    assert_eq!(semantic_header(&packed)["candidate_count"], 1);
    assert_eq!(semantic_header(&packed)["coverage"]["complete"], false);
    assert_eq!(packed["evidence_summary"]["packing"]["partial"], false);
    assert_eq!(
        packed["evidence_summary"]["source_freshness"]["partial"],
        false
    );
    for key in [
        "chunk_id",
        "text",
        "fused_score",
        "rerank_score",
        "score_trace",
    ] {
        assert_eq!(packed["machine_pack"]["hits"][0][key], hit[key]);
    }
    for key in ["document", "source_evidence"] {
        assert_eq!(
            packed["machine_pack"]["hits"][0]["metadata"][key],
            hit["metadata"][key]
        );
    }
    let (mut hits, status) = normalizer::mcp(&packed).unwrap();
    assert_eq!(status, ResultStatus::Partial);
    normalizer::verify_source(&mut hits[0], dir.path()).unwrap();
    assert_eq!(hits[0].evidence_valid, Some(true));
    validate_envelope_generation(&db, &packed).unwrap();
}

#[tokio::test]
async fn source_only_partial_survives_removal_of_dense_fence_details() {
    let (_dir, db, original) = response(LaneStatus::Complete, true, false, true).await;
    assert_eq!(
        original["evidence_summary"]["source_freshness"]["dense_manifest_fence"]["skipped"],
        1
    );
    let packed = compact_optional_diagnostics(original.clone());
    assert_projected_header(&original, &packed);
    assert!(packed["evidence_summary"]["source_freshness"]
        .get("dense_manifest_fence")
        .is_none());
    assert_eq!(
        packed["evidence_summary"]["source_freshness"]["details_omitted"],
        true
    );
    assert_eq!(
        packed["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    assert_eq!(semantic_header(&packed)["status"], "complete");
    assert_eq!(semantic_header(&packed)["candidate_count"], 1);
    assert_eq!(packed["evidence_summary"]["packing"]["partial"], false);
    assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
    validate_envelope_generation(&db, &packed).unwrap();
    let repacked = pack_value(packed.clone(), 12000).unwrap();
    assert_eq!(
        repacked["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    assert_eq!(normalizer::mcp(&repacked).unwrap().1, ResultStatus::Partial);
}

#[tokio::test]
async fn budget_omission_cannot_upgrade_complete_dense_recall_to_empty_no_match() {
    let (_dir, db, original) = response(LaneStatus::Complete, true, false, false).await;
    assert_eq!(normalizer::mcp(&original).unwrap().1, ResultStatus::Success);
    let mut exercised_empty_omission = false;
    for cap in (2048..=8192).step_by(256) {
        let Ok(packed) = pack_value(original.clone(), cap) else {
            continue;
        };
        assert_accounted(&packed, cap);
        if !packed["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            continue;
        }
        exercised_empty_omission = true;
        assert_projected_header(&original, &packed);
        assert_eq!(semantic_header(&packed)["status"], "complete");
        assert_eq!(semantic_header(&packed)["candidate_count"], 1);
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["partial"],
            false
        );
        assert_eq!(packed["evidence_summary"]["packing"]["partial"], true);
        assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 1);
        assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
        let repacked = pack_value(packed, 12000).unwrap();
        assert_eq!(repacked["evidence_summary"]["packing"]["partial"], true);
        assert_eq!(normalizer::mcp(&repacked).unwrap().1, ResultStatus::Partial);
        validate_envelope_generation(&db, &repacked).unwrap();
        break;
    }
    assert!(
        exercised_empty_omission,
        "must exercise a valid reference-only budget, not just budget errors"
    );
}
