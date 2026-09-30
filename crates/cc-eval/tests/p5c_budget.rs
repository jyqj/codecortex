//! Whole-JSON budget tests. Source validation and score formulas are unchanged.
use cc_eval::benchmark::{normalizer, schema::ResultStatus};
use cc_model::{
    lane_receipt::LaneReceipt,
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus},
    source::SourceSnapshot,
    ContextEnvelope, Intent,
};
use cc_search::selection::budget::pack_value;
use cc_server::{engine::CodeIndex, handlers, query_handle::QueryHandle};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

fn fixture() -> (tempfile::TempDir, CodeIndex, Value) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("a.rs"), "pub fn needle() -> i32 { 7 }\n").unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let value = serde_json::to_value(
        index
            .search()
            .search_in_context("needle", 1, Some(Intent::Locate))
            .unwrap(),
    )
    .unwrap();
    (dir, index, value)
}
fn assert_size(value: &Value, cap: usize) {
    let bytes = serde_json::to_vec(value).unwrap();
    assert!(bytes.len() <= cap, "{} > {cap}", bytes.len());
    assert_eq!(
        value["evidence_summary"]["packing"]["used_bytes"]
            .as_u64()
            .unwrap() as usize,
        bytes.len()
    );
    assert_eq!(
        value["token_estimate"].as_u64().unwrap() as usize,
        bytes.len().div_ceil(4)
    );
    let recovered: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(*value, recovered);
    assert!(value.get("_truncated").is_none());
    assert!(
        value.get("partial").is_none(),
        "no JSON prefix preview is a context response"
    );
}

#[test]
fn small_source_survives_metadata_compaction_and_size_is_self_accounted() {
    let (dir, _index, mut value) = fixture();
    let original_hit = value["machine_pack"]["hits"][0].clone();
    value["machine_pack"]["hits"][0]["metadata"]["verbose"] = json!("meta".repeat(10000));
    value["rendered_prompt"] = json!("duplicated render".repeat(10000));
    let packed = pack_value(value, 8000).unwrap();
    assert_size(&packed, 8000);
    assert_eq!(
        packed["machine_pack"]["hits"][0]["text"],
        original_hit["text"]
    );
    assert_eq!(
        packed["machine_pack"]["hits"][0]["metadata"]["source_evidence"],
        original_hit["metadata"]["source_evidence"]
    );
    assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 0);
    let (mut hits, _) = normalizer::mcp(&packed).unwrap();
    assert_eq!(hits.len(), 1);
    normalizer::verify_source(&mut hits[0], dir.path()).unwrap();
    assert_eq!(hits[0].evidence_valid, Some(true));
    assert_eq!(pack_value(packed.clone(), 8000).unwrap(), packed);
}

#[test]
fn lower_rank_references_never_evict_a_fitting_primary_body() {
    let (_dir, _index, mut value) = fixture();
    let primary = value["machine_pack"]["hits"][0].clone();
    let mut hits = vec![primary.clone()];
    for i in 0..16 {
        let mut hit = primary.clone();
        hit["chunk_id"] = json!(format!("tail:{i}"));
        hit["text"] = json!("large lower-ranked evidence ".repeat(1000));
        hits.push(hit);
    }
    value["machine_pack"]["hits"] = json!(hits);
    value["evidence_summary"]
        .as_object_mut()
        .unwrap()
        .remove("packing");
    let packed = pack_value(value, 8000).unwrap();
    assert_size(&packed, 8000);
    assert_eq!(packed["machine_pack"]["hits"][0]["text"], primary["text"]);
    assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 16);
    assert_eq!(packed["evidence_summary"]["packing"]["partial"], true);
    assert_eq!(pack_value(packed.clone(), 8000).unwrap(), packed);
}

#[test]
fn oversized_graph_and_diagnostic_metadata_yield_before_primary_source() {
    let (_dir, _index, mut value) = fixture();
    let expected = value["machine_pack"]["hits"][0]["text"].clone();
    let mut graph = value["nodes"][0].clone();
    graph["node_type"] = json!("CALL_EDGE");
    graph["node_id"] = json!("graph:oversized");
    graph["text"] = json!("derived graph description ".repeat(10000));
    value["nodes"].as_array_mut().unwrap().push(graph);
    value["evidence_summary"]["source_freshness"]["partial"] = json!(true);
    value["evidence_summary"]["source_freshness"]["omitted_files"] =
        json!({"long/path": "stale diagnostic ".repeat(10000)});
    let packed = pack_value(value, 8000).unwrap();
    assert_size(&packed, 8000);
    assert_eq!(packed["machine_pack"]["hits"][0]["text"], expected);
    assert_eq!(
        packed["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    assert_eq!(packed["evidence_summary"]["packing"]["omitted_nodes"], 1);
    assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
}

#[test]
fn malformed_context_objects_fail_without_panicking_or_erasing_old_status() {
    let (_dir, _index, base) = fixture();
    for target in [
        "/machine_pack/hits/0",
        "/machine_pack/hits/0/metadata",
        "/nodes/0",
    ] {
        let mut value = base.clone();
        *value.pointer_mut(target).unwrap() = json!(42);
        assert!(pack_value(value, 8000).is_err());
    }
    let mut value = base;
    value["evidence_summary"]["packing"]["partial"] = json!("false");
    assert!(pack_value(value, 8000).is_err());
}

#[test]
fn first_huge_body_becomes_reference_not_unbounded_or_fake_slice() {
    let (_dir, _index, mut value) = fixture();
    let body = "字符串\"\\\n".repeat(10000);
    let snapshot = SourceSnapshot::new(body.as_bytes());
    value["machine_pack"]["hits"][0]["text"] = json!(body);
    value["machine_pack"]["hits"][0]["metadata"]["source_evidence"] = json!({
        "source":snapshot.identity(),"span":snapshot.whole(),
        "slice_digest":snapshot.slice_digest(snapshot.whole()).unwrap(),
        "boundary":"budget_test", "owner":null,"signature":null
    });
    let identity = value["machine_pack"]["hits"][0]["metadata"]["document"].clone();
    value["machine_pack"]["hits"][0]["metadata"]["stage_a_file_reasons"] =
        json!(["symbol:neighbor_diagnostic"]);
    let packed = pack_value(value, 8000).unwrap();
    assert_size(&packed, 8000);
    assert!(packed["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        packed["machine_pack"]["references"][0]["document"],
        identity
    );
    assert_eq!(
        packed["machine_pack"]["references"][0]["body_omitted"],
        true
    );
    assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 1);
    assert_eq!(
        packed["machine_pack"]["references"][0]["retrieval_reasons"],
        json!(["symbol:neighbor_diagnostic"])
    );
    assert_eq!(
        packed["machine_pack"]["references"][0]["symbol_name"],
        "needle"
    );
    assert_eq!(
        packed["machine_pack"]["references"][0]["symbol_kind"],
        "function"
    );
    assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
    let repacked = pack_value(packed.clone(), 8000).unwrap();
    assert_eq!(repacked, packed);
}

#[test]
fn projected_lane_receipts_keep_partial_errors_and_reject_bad_headers() {
    let (_dir, _index, base) = fixture();
    for status in [
        LaneStatus::Partial,
        LaneStatus::Timeout,
        LaneStatus::Error,
        LaneStatus::Unavailable,
        LaneStatus::Cancelled,
    ] {
        let lane = LaneOutcome {
            status,
            truncation_reason: Some("injected_limit_or_error".into()),
            coverage: LaneCoverage::not_run(),
            ..LaneOutcome::disabled("semantic", 1.0)
        };
        let receipt = LaneReceipt::from(&lane);
        receipt.validate().unwrap();
        let mut value = base.clone();
        value["evidence_summary"]["retrieval"]
            .as_object_mut()
            .unwrap()
            .remove("lanes");
        value["evidence_summary"]["retrieval"]["lane_receipts"] = json!([receipt]);
        value["machine_pack"]["hits"] = json!([]);
        assert_eq!(normalizer::mcp(&value).unwrap().1, ResultStatus::Partial);
        value["evidence_summary"]["retrieval"]["lane_receipts"][0]["candidate_details_omitted"] =
            json!(false);
        assert!(normalizer::mcp(&value).is_err());
    }
    let mut value = base;
    let receipt =
        serde_json::to_value(LaneReceipt::from(&LaneOutcome::disabled("semantic", 1.0))).unwrap();
    value["evidence_summary"]["retrieval"]["lane_receipts"] = json!([receipt.clone()]);
    assert!(
        normalizer::mcp(&value).is_err(),
        "full and projected ambiguity must fail"
    );
    value["evidence_summary"]["retrieval"]
        .as_object_mut()
        .unwrap()
        .remove("lanes");
    value["evidence_summary"]["retrieval"]["lane_receipts"] = json!([receipt.clone(), receipt]);
    assert!(
        normalizer::mcp(&value).is_err(),
        "duplicate votes/receipts must fail"
    );
}

#[test]
fn post_handler_metadata_and_old_omission_flags_share_final_budget() {
    let (_dir, _index, mut value) = fixture();
    value["resolution_freshness"] =
        json!({"complete":false,"status":"pending","details":"x".repeat(1000)});
    value["evidence_summary"]["packing"]["partial"] = json!(true);
    value["evidence_summary"]["packing"]["metadata_omitted"] = json!(true);
    value["rendered_prompt"] = json!("x".repeat(30000));
    let packed = pack_value(value, 10000).unwrap();
    assert_size(&packed, 10000);
    assert_eq!(packed["resolution_freshness"]["complete"], false);
    assert_eq!(packed["evidence_summary"]["packing"]["partial"], true);
    assert_eq!(
        packed["evidence_summary"]["packing"]["metadata_omitted"],
        true
    );
    assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
    assert!(pack_value(packed, 8).is_err());
}

#[test]
fn malformed_packing_status_cannot_be_interpreted_as_complete() {
    let (_dir, _index, base) = fixture();
    for invalid in [
        json!({"spec":"unknown","partial":false}),
        json!({"spec":cc_model::context::CONTEXT_PACKING_SPEC,"partial":"false"}),
        json!(null),
    ] {
        let mut value = base.clone();
        value["evidence_summary"]["packing"] = invalid;
        assert!(normalizer::mcp(&value).is_err());
    }
    let mut malformed = base;
    malformed["machine_pack"]["references"] = json!("not an array");
    assert!(pack_value(malformed, 16000).is_err());
}

#[tokio::test]
#[ignore = "explicit CODECORTEX_BENCH_BINARY; real MCP structured response budget"]
async fn real_stdio_search_preserves_full_json_budget_and_source_proofs() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let (dir, index, _) = fixture();
    drop(index);
    for i in 0..9 {
        std::fs::write(
            dir.path().join(format!("extra{i}.rs")),
            format!(
                "pub fn needle() -> &'static str {{ \"{}\" }}\n",
                "payload ".repeat(64)
            ),
        )
        .unwrap();
    }
    let binary = std::path::PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product"),
    );
    let mut client = McpStdio::spawn(&binary, dir.path(), std::time::Duration::from_secs(20))
        .await
        .unwrap();
    client
        .call("index", json!({"path":dir.path(),"full":true}))
        .await
        .unwrap();
    let mut observations = Vec::new();
    for _ in 0..2 {
        let value = client
            .call("search", json!({"query":"needle","top_k":8}))
            .await
            .unwrap();
        assert_size(&value, 16000);
        assert!(value["resolution_freshness"].is_object());
        let (mut hits, status) = normalizer::mcp(&value).unwrap();
        assert!(!hits.is_empty(), "primary small source must remain visible");
        for hit in &mut hits {
            normalizer::verify_source(hit, dir.path()).unwrap();
            assert_eq!(hit.evidence_valid, Some(true));
        }
        observations.push(json!({"bytes":serde_json::to_vec(&value).unwrap().len(),"packing":value["evidence_summary"]["packing"],"status":status,"verified_hits":hits.len()}));
    }
    client.close().await.unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("p5c-budget-stdio.json"),
            serde_json::to_vec_pretty(&observations).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn actual_async_response_counts_final_freshness_and_survives_repacking() {
    let (_dir, index, _) = fixture();
    let runtime = Arc::new(RwLock::new(index));
    let value = handlers::context::search_async(
        runtime.clone(),
        "needle".into(),
        1,
        Some(Intent::Locate),
        Default::default(),
    )
    .await
    .unwrap();
    assert_size(&value, 16000);
    assert!(value["resolution_freshness"].is_object());
    let handle = QueryHandle::capture(&runtime).unwrap();
    let envelope: ContextEnvelope = handle
        .search_async("needle".into(), 1, Some(Intent::Locate), Default::default())
        .await
        .unwrap();
    assert!(serde_json::to_vec(&envelope).unwrap().len() <= 16000);
}
