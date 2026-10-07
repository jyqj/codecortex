//! Independent review of fixed scope-label projection. Frozen two-noise fixture;
//! real default MCP responses, exact-pointer adversaries, and real producers.
use cc_eval::{
    benchmark::{normalizer, schema::ResultStatus},
    runner::CodeIndexBackend,
};
use cc_model::{config::load_project_config, query::QueryControl, search::SearchHit};
use cc_search::{
    evidence::{SourceVerifier, SOURCE_FRESHNESS_SCOPE as FILE_SCOPE},
    evidence_hydrator::{EvidenceHydrator, SOURCE_FRESHNESS_SCOPE as GEN_SCOPE},
    selection::{budget::pack_value, coverage::SOURCE_SUPPORT_SCOPE},
};
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
fn save(label: &str, value: &Value) {
    if let Ok(dir) = std::env::var("P5E_FIX_REVIEW_EVIDENCE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{label}.json"));
        assert!(!path.exists(), "independent review receipts immutable");
        std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }
}
fn hits(v: &Value) -> &[Value] {
    v["machine_pack"]["hits"].as_array().unwrap()
}
fn source_checks(v: &Value, root: &Path, scoped: bool) {
    let (mut rows, _) = normalizer::mcp(v).unwrap();
    for row in &mut rows {
        normalizer::verify_source(row, root).unwrap();
        assert_eq!(row.evidence_valid, Some(true));
        if scoped {
            assert!(row.path.starts_with("scope/"));
        }
    }
}
fn accounted(v: &Value, cap: usize) -> usize {
    let bytes = serde_json::to_vec(v).unwrap().len();
    assert!(bytes <= cap);
    assert_eq!(v["evidence_summary"]["packing"]["used_bytes"], bytes);
    assert_eq!(v["token_budget"], 4000);
    assert_eq!(
        v["evidence_summary"]["packing"]["limit_bytes"],
        cap.min(16000)
    );
    bytes
}
fn retained_unchanged(before: &Value, after: &Value) {
    let mut last_rank = None;
    for h in hits(after) {
        let index = hits(before)
            .iter()
            .position(|x| x["chunk_id"] == h["chunk_id"])
            .unwrap();
        assert!(last_rank.is_none_or(|last| index > last));
        last_rank = Some(index);
        let old = &hits(before)[index];
        for field in [
            "chunk_id",
            "file_path",
            "text",
            "rerank_score",
            "score_trace",
        ] {
            assert_eq!(h[field], old[field]);
        }
        for field in [
            "document",
            "source_evidence",
            "qname",
            "evidence_priority",
            "source_freshness",
        ] {
            assert_eq!(h["metadata"][field], old["metadata"][field]);
        }
    }
    for ptr in [
        "/evidence_summary/retrieval/cost",
        "/evidence_summary/source_freshness/generation",
        "/resolution_freshness",
        "/evidence_summary/retrieval/lane_receipts",
    ] {
        assert_eq!(after.pointer(ptr), before.pointer(ptr));
    }
    assert_eq!(normalizer::mcp(after).unwrap().1, ResultStatus::Partial);
    assert_eq!(after["evidence_summary"]["packing"]["partial"], true);
}
fn search(backend: &CodeIndexBackend, intent: &str) -> Value {
    backend.call_tool("search", &json!({"query":format!("{intent} repair_widget distinctive_widget_support WidgetContract"),
        "top_k":10,"mode":"hybrid","path_prefix":"scope"})).unwrap()
}
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

#[test]
fn independent_default_scoped_gate_repack_and_bounded_width() {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    for intent in ["fix", "refactor", "trace"] {
        let raw = search(&backend, intent);
        source_checks(&raw, root.path(), true);
        assert_eq!(
            raw["evidence_summary"]["packing"]["configured_max_bytes"],
            18000
        );
        assert_eq!(raw["evidence_summary"]["packing"]["omitted_hits"], 2);
        let bytes = accounted(&raw, 16000);
        for (path, body) in [
            ("scope/impl.py", "return value + 731"),
            (
                "scope/api.ts",
                "export interface WidgetContract { repair_widget(value: number): number; }",
            ),
        ] {
            let h = hits(&raw).iter().find(|h| h["file_path"] == path).unwrap();
            assert!(h["text"].as_str().unwrap().contains(body));
            if path == "scope/impl.py" {
                assert_eq!(h["metadata"]["qname"], "repair_widget");
            }
        }
        if intent != "trace" {
            assert!(hits(&raw)
                .iter()
                .any(|h| h["file_path"] == "scope/test_widget.py"
                    && h["text"]
                        .as_str()
                        .unwrap()
                        .contains("assert repair_widget(1) == 732")));
        }
        let first = pack_value(raw.clone(), 16000).unwrap();
        let second = pack_value(first.clone(), 16000).unwrap();
        assert_eq!(first, second);
        retained_unchanged(&raw, &first);
        save(
            &format!("{intent}-default-gate"),
            &json!({"public":raw,"first_repack":first,"second_repack":second,"serialized_bytes":bytes}),
        );
        let context = backend
            .call_tool(
                "context",
                &json!({"task":"repair_widget distinctive_widget_support WidgetContract",
            "intent":intent,"retrieval_strategy":"local","include_source":true}),
            )
            .unwrap();
        source_checks(&context, root.path(), false);
        save(&format!("{intent}-unscoped-context-observation"), &context);
        let mut previous = raw.clone();
        let mut steps = Vec::new();
        for cap in [14000, 8000, 16000, 18000, 14000, 16000] {
            let next = pack_value(previous.clone(), cap).unwrap();
            retained_unchanged(&previous, &next);
            source_checks(&next, root.path(), true);
            accounted(&next, cap);
            assert!(
                next["evidence_summary"]["packing"]["omitted_hits"]
                    .as_u64()
                    .unwrap()
                    >= previous["evidence_summary"]["packing"]["omitted_hits"]
                        .as_u64()
                        .unwrap()
            );
            steps.push(json!({"requested_max_bytes":cap,"output":next}));
            previous = next;
        }
        save(&format!("{intent}-monotone-omission-repack"), &json!(steps));
        if intent == "refactor" {
            let mut wide = raw.clone();
            wide["evidence_summary"]["retrieval"]["policy"]["path_source_domain"] =
                json!(cc_search::query_policy::PATH_SOURCE_DOMAIN);
            wide["evidence_summary"]["retrieval"]["policy"]["graph_source_mapping"] =
                json!(cc_search::query_policy::GRAPH_SOURCE_MAPPING);
            for lane in wide["evidence_summary"]["retrieval"]["lane_receipts"]
                .as_array_mut()
                .unwrap()
            {
                if lane["status"] == "complete" {
                    lane["elapsed_us"] = json!(u64::MAX);
                }
            }
            for key in ["index_epoch", "evidence_epoch"] {
                wide["evidence_summary"]["source_freshness"]["generation"][key] = json!(u64::MAX);
            }
            wide["evidence_summary"]["source_freshness"]["generation"]["incarnation"] =
                json!(vec![255_u8; 16]);
            for (key, v) in wide["resolution_freshness"].as_object_mut().unwrap() {
                if key.ends_with("epoch") && v.is_u64() {
                    *v = json!(u64::MAX);
                }
            }
            let packed = pack_value(wide.clone(), 16000).unwrap();
            retained_unchanged(&wide, &packed);
            source_checks(&packed, root.path(), true);
            assert!(hits(&packed)
                .iter()
                .any(|h| h["file_path"] == "scope/impl.py"));
            let bytes = accounted(&packed, 16000);
            assert_eq!(pack_value(packed.clone(), 16000).unwrap(), packed);
            save(
                "bounded-width-independent",
                &json!({"synthetic_timing_epoch_incarnation_only":true,
                "input":wide,"output":packed,"serialized_bytes":bytes,"margin_bytes":16000-bytes,
                "cost_counts_unmodified":true,"not_all_numeric_fields_stressed":true}),
            );
        }
    }
}
#[test]
fn independent_exact_pointer_and_near_known_negatives() {
    assert_eq!(SOURCE_SUPPORT_SCOPE,"literal_program_cue_in_validated_twice_topk_window_only; not_global_uniqueness_or_exact_identity");
    assert_eq!(GEN_SCOPE,"bounded per-file disk verification and optimistic full read generation; not an atomic filesystem snapshot");
    assert_eq!(
        FILE_SCOPE,
        "bounded per-file disk verification; not an atomic filesystem or whole-query snapshot"
    );
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    let raw = search(&backend, "refactor");
    let specs = [
        (
            "support",
            "/evidence_summary/selection/source_support_scope",
            SOURCE_SUPPORT_SCOPE,
            "cue_window:v1",
        ),
        (
            "generation",
            "/evidence_summary/source_freshness/scope",
            GEN_SCOPE,
            "disk_generation:v1",
        ),
        (
            "files",
            "/evidence_summary/source_freshness/scope",
            FILE_SCOPE,
            "disk_files:v1",
        ),
    ];
    let mut records = Vec::new();
    for (domain, pointer, known, label) in specs {
        let cases = vec![
            ("exact", json!(known)),
            ("prefix", json!(format!("caller:{known}"))),
            ("suffix", json!(format!("{known};caller"))),
            ("leading_space", json!(format!(" {known}"))),
            ("trailing_space", json!(format!("{known} "))),
            ("newline", json!(format!("{known}\n"))),
            ("case", json!(known.to_uppercase())),
            ("nbsp", json!(format!("{known}\u{a0}"))),
            ("already_compact", json!(label)),
            ("null", Value::Null),
            ("array", json!([known])),
            ("object", json!({"description":known})),
            (
                "other_known_in_wrong_domain",
                json!(if domain == "support" {
                    GEN_SCOPE
                } else {
                    SOURCE_SUPPORT_SCOPE
                }),
            ),
        ];
        for (name, description) in cases {
            let mut input = raw.clone();
            *input.pointer_mut(pointer).unwrap() = description.clone();
            input["evidence_summary"]["review_unknown_pointer"] = json!({"description":known});
            let packed = pack_value(input.clone(), 14000).unwrap();
            assert!(packed["evidence_summary"]["packing"]["compacted"]
                .as_bool()
                .unwrap());
            let expected = if name == "exact" {
                json!(label)
            } else {
                description
            };
            assert_eq!(packed.pointer(pointer), Some(&expected), "{domain}/{name}");
            assert_eq!(
                packed["evidence_summary"]["review_unknown_pointer"],
                input["evidence_summary"]["review_unknown_pointer"]
            );
            retained_unchanged(&input, &packed);
            source_checks(&packed, root.path(), true);
            accounted(&packed, 14000);
            assert_eq!(pack_value(packed.clone(), 14000).unwrap(), packed);
            records.push(json!({"domain":domain,"case":name,"input":input,"output":packed}));
        }
    }
    save("exact-pointer-and-near-known-negatives", &json!(records));
}
#[test]
fn independent_real_legacy_and_generation_producers_remain_distinct() {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    let raw = search(&backend, "refactor");
    let index = CodeIndex::new(Some(root.path())).unwrap();
    let db = index.index_db().unwrap();
    let actual_hits: Vec<SearchHit> =
        serde_json::from_value(raw["machine_pack"]["hits"].clone()).unwrap();
    let mut files = SourceVerifier::new(db, root.path());
    for h in &actual_hits {
        assert!(files.hit_current(h).unwrap());
    }
    let legacy = files.diagnostics();
    assert!(legacy.get("generation").is_none());
    assert!(legacy.get("hydrator").is_none());
    assert_eq!(legacy["scope"], FILE_SCOPE);
    let config = load_project_config(root.path());
    let control = QueryControl::new(Duration::from_millis(config.query.deadline_ms)).unwrap();
    let mut verifier = EvidenceHydrator::new(
        db,
        root.path(),
        cc_search::query_policy::hard_scope(&cc_model::search::SearchRequest {
            path_prefix: Some("scope".into()),
            ..Default::default()
        })
        .unwrap(),
        db.reads().read_generation().unwrap(),
        control,
    )
    .unwrap();
    verifier.hydrate(&actual_hits).unwrap();
    verifier.finish().unwrap();
    let generation = verifier.diagnostics();
    assert!(generation.get("generation").is_some());
    assert_eq!(generation["scope"], GEN_SCOPE);
    assert!(!files
        .path_current("scope/not_indexed_by_review.py")
        .unwrap());
    let incomplete_legacy = files.diagnostics();
    assert_eq!(incomplete_legacy["partial"], true);
    let mut records = Vec::new();
    for (name, diagnostics, label) in [
        ("legacy", legacy, "disk_files:v1"),
        ("generation", generation, "disk_generation:v1"),
        ("legacy_partial", incomplete_legacy, "disk_files:v1"),
    ] {
        let mut input = raw.clone();
        input["evidence_summary"]["source_freshness"] = diagnostics.clone();
        let packed = pack_value(input.clone(), 14000).unwrap();
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["scope"],
            label
        );
        for (key, v) in diagnostics.as_object().unwrap() {
            if key != "scope" {
                assert_eq!(&packed["evidence_summary"]["source_freshness"][key], v);
            }
        }
        assert_eq!(
            packed["evidence_summary"]["source_freshness"].get("generation"),
            diagnostics.get("generation")
        );
        assert_eq!(
            packed["evidence_summary"]["source_freshness"].get("hydrator"),
            diagnostics.get("hydrator")
        );
        retained_unchanged(&input, &packed);
        source_checks(&packed, root.path(), true);
        accounted(&packed, 14000);
        assert_eq!(pack_value(packed.clone(), 14000).unwrap(), packed);
        records.push(json!({"case":name,"real_producer_diagnostics":diagnostics,"input":input,"output":packed}));
    }
    save("real-producer-domains-and-partial", &json!(records));
}
