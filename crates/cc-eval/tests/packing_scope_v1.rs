//! Real parser/index/retrieval/MCP response -> the exact final packing stage.
//! Substage caps are test inputs, NOT newly advertised public MCP arguments.
use cc_eval::{
    benchmark::{normalizer, schema::ResultStatus},
    runner::CodeIndexBackend,
};
use cc_search::selection::budget::pack_value;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".codecortex.json"),r#"{"auto_index":{"enabled":false},"search":{"graph_weight":0.0,"path_weight":0.0,"exact_symbol_weight":0.0,"grep_weight":0.0,"lexical_top_k":50,"rerank_window":50}}"#).unwrap();
    let files = [
        (
            "scope/impl.py",
            "def repair_widget(value):\n    return value + 731\n",
        ),
        (
            "scope/test_widget.py",
            "def test_repair_widget():\n    assert repair_widget(1) == 732\n",
        ),
        (
            "scope/api.ts",
            "export interface WidgetContract { repair_widget(value: number): number; }\n",
        ),
        (
            "scope/support.py",
            "def distinctive_widget_support(value):\n    return repair_widget(value) + 997\n\ndef supporting_widget_context(value):\n    return repair_widget(value) + 998\n",
        ),
        (
            "outside/forbidden.py",
            "def repair_widget(value):\n    return 999999\n",
        ),
    ];
    for (path, text) in files {
        let p = root.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    // Keep two independent incidental documents. Eight noise documents made
    // the default public response drop impl.py before the substage caps were
    // applied as manifest/freshness receipts grew. The complete fixture must
    // fit first; the unchanged caps below supply the actual output pressure.
    for n in 0..2 {
        let text=format!("# repair_widget WidgetContract widget repair value repeated incidental noise\ndef noisy_widget_{n}(value):\n    return value + {n}\n");
        std::fs::write(root.path().join(format!("scope/noise_{n}.py")), text).unwrap();
    }
    root
}
fn hits(v: &Value) -> &Vec<Value> {
    v["machine_pack"]["hits"].as_array().unwrap()
}
fn proof(v: &Value, root: &Path, scoped: bool) {
    let (mut rows, _) = normalizer::mcp(v).unwrap();
    for row in &mut rows {
        normalizer::verify_source(row, root).unwrap();
        assert_eq!(row.evidence_valid, Some(true));
        if scoped {
            assert!(row.path.starts_with("scope/"));
        }
        assert!(row.span.as_ref().is_some_and(|s| s.end > s.start));
    }
    for h in hits(v) {
        if scoped {
            assert!(!h["text"].as_str().unwrap().contains("999999"));
        }
    }
}
fn collect(v: &Value) -> BTreeMap<String, (Value, Value, Value)> {
    hits(v)
        .iter()
        .map(|h| {
            (
                h["chunk_id"].as_str().unwrap().into(),
                (
                    h["score_trace"].clone(),
                    h["rerank_score"].clone(),
                    h["text"].clone(),
                ),
            )
        })
        .collect()
}
fn evidence(v: &Value, case: &str) {
    if let Ok(dir) = std::env::var("P5E_PRIORITY_EVIDENCE") {
        let p = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&p).unwrap();
        let file = p.join(format!("{case}.json"));
        assert!(!file.exists(), "raw pressure evidence immutable");
        std::fs::write(file, serde_json::to_vec_pretty(v).unwrap()).unwrap();
    }
}

fn public() -> (tempfile::TempDir, Value) {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    let raw = backend.call_tool("search", &json!({"query":"refactor repair_widget distinctive_widget_support WidgetContract", "top_k":10,"mode":"hybrid","path_prefix":"scope"})).unwrap();
    proof(&raw, root.path(), true);
    assert!(hits(&raw).iter().any(|h| h["file_path"] == "scope/impl.py"));
    (root, raw)
}
fn accounted(value: &Value, cap: usize) -> usize {
    let bytes = serde_json::to_vec(value).unwrap().len();
    assert!(bytes <= cap);
    assert_eq!(value["evidence_summary"]["packing"]["used_bytes"], bytes);
    bytes
}
#[test]
fn maximum_numeric_width_preserves_proofs_counts_and_second_pack() {
    let (root, raw) = public();
    let mut value = raw.clone();
    // Synthetic width stress only: no claim these are observed costs/epochs.
    value["evidence_summary"]["retrieval"]["policy"]["path_source_domain"] =
        json!(cc_search::query_policy::PATH_SOURCE_DOMAIN);
    value["evidence_summary"]["retrieval"]["policy"]["graph_source_mapping"] =
        json!(cc_search::query_policy::GRAPH_SOURCE_MAPPING);
    for lane in value["evidence_summary"]["retrieval"]["lane_receipts"]
        .as_array_mut()
        .unwrap()
    {
        if lane["status"] == "complete" {
            lane["elapsed_us"] = json!(u64::MAX);
        }
    }
    for key in ["index_epoch", "evidence_epoch"] {
        value["evidence_summary"]["source_freshness"]["generation"][key] = json!(u64::MAX);
    }
    value["evidence_summary"]["source_freshness"]["generation"]["incarnation"] =
        json!(vec![255_u8; 16]);
    for (key, v) in value["resolution_freshness"].as_object_mut().unwrap() {
        if key.ends_with("epoch") && v.is_u64() {
            *v = json!(u64::MAX);
        }
    }
    let before = value.clone();
    let packed = pack_value(value, 16000).unwrap();
    proof(&packed, root.path(), true);
    let bytes = accounted(&packed, 16000);
    assert!(hits(&packed)
        .iter()
        .any(|h| h["file_path"] == "scope/impl.py"));
    for pointer in [
        "/evidence_summary/retrieval/cost",
        "/evidence_summary/source_freshness/generation",
        "/resolution_freshness",
        "/evidence_summary/retrieval/lane_receipts",
    ] {
        assert_eq!(packed.pointer(pointer), before.pointer(pointer));
    }
    let original = collect(&raw);
    for h in hits(&packed) {
        assert_eq!(
            original[h["chunk_id"].as_str().unwrap()],
            (
                h["score_trace"].clone(),
                h["rerank_score"].clone(),
                h["text"].clone()
            )
        );
        let original_hit = hits(&raw)
            .iter()
            .find(|o| o["chunk_id"] == h["chunk_id"])
            .unwrap();
        for field in ["document", "source_evidence", "qname", "evidence_priority"] {
            assert_eq!(h["metadata"][field], original_hit["metadata"][field]);
        }
    }
    assert_eq!(pack_value(packed.clone(), 16000).unwrap(), packed);
    eprintln!(
        "maximum numeric width serialized bytes={bytes}; margin={}",
        16000 - bytes
    );
    evidence(
        &json!({"synthetic_metadata_width_only":true,"serialized_bytes":bytes,"margin_bytes":16000-bytes,"packed":packed}),
        "maximum-width",
    );
}
#[test]
fn near_known_labels_are_unchanged_and_repacking_never_resurrects_bodies() {
    let (root, raw) = public();
    for suffix in [";caller_unknown", " "] {
        let mut value = raw.clone();
        let support = format!(
            "{}{suffix}",
            cc_search::selection::coverage::SOURCE_SUPPORT_SCOPE
        );
        let freshness = format!(
            "{}{suffix}",
            cc_search::evidence_hydrator::SOURCE_FRESHNESS_SCOPE
        );
        value["evidence_summary"]["selection"]["source_support_scope"] = json!(support);
        value["evidence_summary"]["source_freshness"]["scope"] = json!(freshness);
        let packed = pack_value(value, 14000).unwrap();
        assert_eq!(
            packed["evidence_summary"]["selection"]["source_support_scope"],
            support
        );
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["scope"],
            freshness
        );
        proof(&packed, root.path(), true);
        accounted(&packed, 14000);
        assert_eq!(pack_value(packed.clone(), 14000).unwrap(), packed);
    }
    let small = pack_value(raw.clone(), 8000).unwrap();
    let expanded = pack_value(small.clone(), 16000).unwrap();
    assert_eq!(
        expanded["machine_pack"]["hits"],
        small["machine_pack"]["hits"]
    );
    assert_eq!(
        expanded["evidence_summary"]["packing"]["omitted_hits"],
        small["evidence_summary"]["packing"]["omitted_hits"]
    );
    assert_eq!(normalizer::mcp(&expanded).unwrap().1, ResultStatus::Partial);
    let mut zero = raw;
    zero["machine_pack"]["references"] = json!([]);
    zero["nodes"] = json!([]);
    zero["spans"] = json!([]);
    let packed = pack_value(zero, 16000).unwrap();
    proof(&packed, root.path(), true);
    assert!(hits(&packed)
        .iter()
        .any(|h| h["file_path"] == "scope/impl.py"));
    accounted(&packed, 16000);
    assert_eq!(pack_value(packed.clone(), 16000).unwrap(), packed);
}
#[test]
fn known_scope_mapping_requires_pressure_and_exact_bytes() {
    let (_, raw) = public();
    for (known, compact) in [
        (
            cc_search::evidence_hydrator::SOURCE_FRESHNESS_SCOPE,
            "disk_generation:v1",
        ),
        (cc_search::evidence::SOURCE_FRESHNESS_SCOPE, "disk_files:v1"),
    ] {
        let mut value = raw.clone();
        value["evidence_summary"]["selection"]["source_support_scope"] =
            json!(cc_search::selection::coverage::SOURCE_SUPPORT_SCOPE);
        value["evidence_summary"]["source_freshness"]["scope"] = json!(known);
        value["token_budget"] = json!(100000);
        let roomy = pack_value(value.clone(), 400000).unwrap();
        assert_eq!(
            roomy["evidence_summary"]["source_freshness"]["scope"],
            known
        );
        let pressured = pack_value(value, 14000).unwrap();
        assert_eq!(
            pressured["evidence_summary"]["source_freshness"]["scope"],
            compact
        );
        assert_eq!(
            pressured["evidence_summary"]["selection"]["source_support_scope"],
            "cue_window:v1"
        );
        assert_eq!(pack_value(pressured.clone(), 14000).unwrap(), pressured);
    }
}
