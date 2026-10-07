//! Synthetic output pressure over a real parser/index/context result. These
//! tests exercise optional-diagnostic packing, not new SQL-cost observations.
use cc_eval::benchmark::{normalizer, schema::ResultStatus};
use cc_model::Intent;
use cc_search::selection::budget::pack_value;
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};

const CAP: usize = 16000;

fn fixture() -> (tempfile::TempDir, CodeIndex, Value) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("a.rs"), "pub fn needle() -> i32 { 7 }\n").unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let mut value = serde_json::to_value(
        index
            .search()
            .search_in_context("needle", 1, Some(Intent::Locate))
            .unwrap(),
    )
    .unwrap();
    // Complete the existing metadata projection first. The retained hit is
    // genuine; only duplicate render/node views and test-only prose are inputs
    // to this stage control, so later padding cannot be hidden by those views.
    value["nodes"] = json!([]);
    value["spans"] = json!([]);
    value["rendered_prompt"] = json!("duplicate view".repeat(10000));
    value["machine_pack"]["hits"][0]["metadata"]["test_only_prose"] =
        json!("metadata pressure".repeat(10000));
    let packed = pack_value(value, CAP).unwrap();
    assert_eq!(packed["machine_pack"]["hits"].as_array().unwrap().len(), 1);
    assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 0);
    assert!(packed["evidence_summary"]["source_freshness"]["validation_work"].is_object());
    assert!(
        serde_json::to_vec(&packed["evidence_summary"]["source_freshness"])
            .unwrap()
            .len()
            < CAP / 8,
        "this control must not trigger the separate oversized-diagnostic projection"
    );
    (root, index, packed)
}

fn accounted_and_verified(value: &Value, root: &std::path::Path, cap: usize) {
    let encoded = serde_json::to_vec(value).unwrap();
    assert!(encoded.len() <= cap);
    assert_eq!(
        value["evidence_summary"]["packing"]["used_bytes"],
        encoded.len()
    );
    assert_eq!(value["token_estimate"], encoded.len().div_ceil(4));
    let (mut rows, _) = normalizer::mcp(value).unwrap();
    assert_eq!(rows.len(), 1);
    normalizer::verify_source(&mut rows[0], root).unwrap();
    assert_eq!(rows[0].evidence_valid, Some(true));
    assert_eq!(pack_value(value.clone(), cap).unwrap(), *value);
}

#[test]
fn optional_work_yields_only_to_a_fitting_complete_body_and_stays_omitted() {
    let (root, _index, mut input) = fixture();
    let original_hit = input["machine_pack"]["hits"][0].clone();
    let original_freshness = input["evidence_summary"]["source_freshness"].clone();
    // Fixed cap, precise synthetic pressure: the whole wire object is 64 bytes
    // over budget. This is not a larger budget or a changed source body.
    input["test_only_output_padding"] = json!("");
    let size = serde_json::to_vec(&input).unwrap().len();
    assert!(size < CAP);
    input["test_only_output_padding"] = json!("x".repeat(CAP + 64 - size));
    assert_eq!(serde_json::to_vec(&input).unwrap().len(), CAP + 64);
    let packed = pack_value(input.clone(), CAP).unwrap();
    assert_eq!(packed["machine_pack"]["hits"], json!([original_hit]));
    assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 0);
    assert_eq!(
        packed["evidence_summary"]["packing"]["partial"],
        input["evidence_summary"]["packing"]["partial"]
    );
    let freshness = &packed["evidence_summary"]["source_freshness"];
    assert!(freshness.get("validation_work").is_none());
    assert_eq!(freshness["details_omitted"], true);
    for (key, value) in original_freshness.as_object().unwrap() {
        if key != "validation_work" && key != "details_omitted" {
            assert_eq!(&freshness[key], value, "changed {key}");
        }
    }
    accounted_and_verified(&packed, root.path(), CAP);
    // Substage-only budget expansion; public request arguments are unchanged.
    let mut larger = packed.clone();
    larger["token_budget"] = json!(8000);
    let expanded = pack_value(larger, 32000).unwrap();
    assert!(expanded["evidence_summary"]["source_freshness"]
        .get("validation_work")
        .is_none());
    assert_eq!(
        expanded["evidence_summary"]["source_freshness"]["details_omitted"],
        true
    );
    assert_eq!(
        expanded["machine_pack"]["hits"],
        packed["machine_pack"]["hits"]
    );
    assert_eq!(expanded["evidence_summary"]["packing"]["omitted_hits"], 0);
    accounted_and_verified(&expanded, root.path(), 32000);
}

#[test]
fn failed_omission_preserves_whole_receipt_and_marker_value_or_absence() {
    let (root, _index, base) = fixture();
    for marker in [
        None,
        Some(json!(false)),
        Some(json!(true)),
        Some(Value::Null),
    ] {
        for with_work in [true, false] {
            let mut input = base.clone();
            let freshness = input["evidence_summary"]["source_freshness"]
                .as_object_mut()
                .unwrap();
            if let Some(value) = marker.clone() {
                freshness.insert("details_omitted".into(), value);
            } else {
                freshness.remove("details_omitted");
            }
            if !with_work {
                freshness.remove("validation_work");
            }
            let original_freshness = Value::Object(freshness.clone());
            // Omitting the receipt cannot make this derived graph view fit.
            // Its removal then leaves ample space for the original receipt.
            input["nodes"] = json!([{
                "node_id":"test-only-oversized-graph", "node_type":"CALL_EDGE",
                "text":"derived graph pressure".repeat(CAP)
            }]);
            let packed = pack_value(input.clone(), CAP).unwrap();
            assert_eq!(
                packed["evidence_summary"]["source_freshness"],
                original_freshness
            );
            assert_eq!(
                packed["machine_pack"]["hits"],
                input["machine_pack"]["hits"]
            );
            assert_eq!(packed["evidence_summary"]["packing"]["omitted_hits"], 0);
            assert_eq!(packed["evidence_summary"]["packing"]["omitted_nodes"], 1);
            assert_eq!(packed["evidence_summary"]["packing"]["partial"], true);
            assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
            accounted_and_verified(&packed, root.path(), CAP);
        }
    }
}
