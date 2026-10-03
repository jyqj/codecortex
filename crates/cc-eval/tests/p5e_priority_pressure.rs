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

#[test]
fn real_final_mcp_context_keeps_trusted_facet_then_support_then_incidental_under_caps() {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    for intent in ["fix", "refactor", "trace"] {
        let task = "repair_widget distinctive_widget_support WidgetContract";
        let public=backend.call_tool("context",&json!({"task":task,"intent":intent,"retrieval_strategy":"local","include_source":true})).unwrap();
        proof(&public, root.path(), false);
        // Context lacks path-prefix args; actual MCP search supplies explicit
        // HardScope and dispatches the same CodeIndex final source/pack path.
        let query = format!("{intent} {task}");
        let scoped = backend
            .call_tool(
                "search",
                &json!({"query":query,"top_k":10,"mode":"hybrid","path_prefix":"scope"}),
            )
            .unwrap();
        proof(&scoped, root.path(), true);
        evidence(&scoped, &format!("{intent}-scoped-before"));
        let before = collect(&scoped);
        let original_ids: Vec<_> = hits(&scoped)
            .iter()
            .map(|h| h["chunk_id"].as_str().unwrap().to_string())
            .collect();
        assert!(!original_ids.is_empty());
        let first = original_ids[0].clone();
        for h in hits(&scoped) {
            assert!(h["score_trace"]
                .as_array()
                .is_some_and(|trace| !trace.is_empty()));
            assert!(h["rerank_score"].is_number());
        }
        let required = [
            ("scope/impl.py", "return value + 731"),
            (
                "scope/api.ts",
                "export interface WidgetContract { repair_widget(value: number): number; }",
            ),
        ];
        for (path, body) in required {
            assert!(hits(&scoped).iter().any(|h|h["file_path"]==path&&h["text"].as_str().unwrap().contains(body)),"real required implementation/interface body must exist before pressure: {intent}/{path}");
        }
        if intent != "trace" {
            assert!(
                hits(&scoped)
                    .iter()
                    .any(|h| h["file_path"] == "scope/test_widget.py"
                        && h["text"]
                            .as_str()
                            .unwrap()
                            .contains("assert repair_widget(1) == 732")),
                "Fix/Refactor require actual test body before pressure"
            );
        }
        let trusted_ids: Vec<_> = hits(&scoped)
            .iter()
            .filter(|h| h["metadata"]["evidence_priority"] == "intent_facet")
            .map(|h| h["chunk_id"].as_str().unwrap().to_string())
            .collect();
        let priority: Vec<_> = hits(&scoped)
            .iter()
            .map(|h| {
                (
                    h["file_path"].as_str().unwrap().to_string(),
                    h["metadata"]["evidence_priority"].clone(),
                )
            })
            .collect();
        assert!(
            priority.iter().any(|(_, p)| p == "intent_facet"),
            "actual selector must mark trusted facets: {priority:?}"
        );
        assert!(
            priority
                .iter()
                .any(|(_, p)| p == "distinctive_source_support"),
            "actual distinctive source support must coexist: {priority:?}"
        );
        assert!(
            priority.iter().any(|(_, p)| p.is_null()),
            "actual incidental hits must coexist: {priority:?}"
        );
        let mut successful_caps = 0;
        let mut facet_pressure_exercised = false;
        let mut support_pressure_exercised = false;
        let mut one_fitting_cap_preserved_all_facets = false;
        for cap in [16000, 14000, 12000, 10000, 8000, 5000] {
            let packed = match pack_value(scoped.clone(), cap) {
                Ok(value) => value,
                Err(error) => {
                    assert!(
                        error
                            .to_string()
                            .contains("required context metadata exceeds output budget"),
                        "unrelated errors cannot be budget success: {error}"
                    );
                    evidence(
                        &json!({"cap":cap,"outcome":"explicit_truthful_error","error":error.to_string()}),
                        &format!("{intent}-cap-{cap}-error"),
                    );
                    continue;
                }
            };
            successful_caps += 1;
            proof(&packed, root.path(), true);
            assert!(serde_json::to_vec(&packed).unwrap().len() <= cap);
            evidence(&packed, &format!("{intent}-cap-{cap}"));
            let ids: Vec<_> = hits(&packed)
                .iter()
                .map(|h| h["chunk_id"].as_str().unwrap().to_string())
                .collect();
            for h in hits(&packed) {
                let id = h["chunk_id"].as_str().unwrap();
                assert_eq!(
                    before[id],
                    (
                        h["score_trace"].clone(),
                        h["rerank_score"].clone(),
                        h["text"].clone()
                    ),
                    "packing never rewrites ranking/source"
                );
            }
            assert!(ids
                .windows(2)
                .all(|pair| original_ids.iter().position(|x| x == &pair[0])
                    < original_ids.iter().position(|x| x == &pair[1])));
            if !ids.is_empty() {
                assert_eq!(ids[0], first, "fitting rank1 anchor must remain first");
            }
            let retained_priorities: Vec<_> = hits(&packed)
                .iter()
                .map(|h| h["metadata"]["evidence_priority"].as_str())
                .collect();
            if trusted_ids.iter().all(|id| ids.contains(id)) {
                one_fitting_cap_preserved_all_facets = true;
            }
            let dropped_facet = hits(&scoped).iter().any(|h| {
                h["metadata"]["evidence_priority"] == "intent_facet"
                    && !ids.iter().any(|id| id == h["chunk_id"].as_str().unwrap())
            });
            facet_pressure_exercised |= dropped_facet;
            if dropped_facet {
                assert!(
                    !hits(&packed)
                        .iter()
                        .skip(1)
                        .any(|h| h["metadata"]["evidence_priority"].is_null()
                            || h["metadata"]["evidence_priority"] == "distinctive_source_support"),
                    "lower priority may not displace a trusted facet"
                );
            }
            if packed["evidence_summary"]["packing"]["omitted_hits"]
                .as_u64()
                .unwrap()
                > 0
            {
                assert_eq!(packed["evidence_summary"]["packing"]["partial"], true);
                assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
            }
            let dropped_support = hits(&scoped).iter().any(|h| {
                h["metadata"]["evidence_priority"] == "distinctive_source_support"
                    && !ids.iter().any(|id| id == h["chunk_id"].as_str().unwrap())
            });
            support_pressure_exercised |= dropped_support;
            if dropped_support {
                assert!(
                    !hits(&packed)
                        .iter()
                        .skip(1)
                        .any(|h| h["metadata"]["evidence_priority"].is_null()),
                    "incidental bodies cannot evict fitting distinctive source support"
                );
            }
            let _ = retained_priorities;
        }
        assert!(
            one_fitting_cap_preserved_all_facets,
            "at least one fit-cap must preserve every real selected trusted facet body"
        );
        assert!(
            facet_pressure_exercised,
            "caps must actually evict a trusted facet"
        );
        assert!(
            support_pressure_exercised,
            "caps must actually evict source support"
        );
        assert!(
            successful_caps >= 2,
            "at least two real output pressure caps must be exercised"
        );
    }
}

#[test]
fn known_domain_prose_compaction_has_numeric_width_margin_and_preserves_unknown_labels() {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    let raw = backend
        .call_tool(
            "search",
            &json!({
                "query":"refactor repair_widget distinctive_widget_support WidgetContract",
                "top_k":10,"mode":"hybrid","path_prefix":"scope"
            }),
        )
        .unwrap();
    proof(&raw, root.path(), true);
    let original = collect(&raw);
    assert!(hits(&raw).iter().any(|h| h["file_path"] == "scope/impl.py"
        && h["text"].as_str().unwrap().contains("return value + 731")));
    // Synthetic presentation-width stress over actual indexed source, not
    // measured generation/latency evidence or a change to production caps.
    for (case, elapsed, epoch, byte) in [
        ("short", 9_u64, 9_u64, 1_u8),
        ("wide", 9_999_999, 1_234_567, 255),
    ] {
        let mut value = raw.clone();
        let policy = &mut value["evidence_summary"]["retrieval"]["policy"];
        policy["path_source_domain"] = json!(cc_search::query_policy::PATH_SOURCE_DOMAIN);
        policy["graph_source_mapping"] = json!(cc_search::query_policy::GRAPH_SOURCE_MAPPING);
        for lane in value["evidence_summary"]["retrieval"]["lane_receipts"]
            .as_array_mut()
            .unwrap()
        {
            if lane["status"] == "complete" {
                lane["elapsed_us"] = json!(elapsed);
            }
        }
        let generation = &mut value["evidence_summary"]["source_freshness"]["generation"];
        generation["index_epoch"] = json!(epoch);
        generation["evidence_epoch"] = json!(epoch);
        generation["incarnation"] = json!(vec![byte; 16]);
        value["resolution_freshness"]["index_epoch"] = json!(epoch);
        let packed = pack_value(value, 16000).unwrap();
        proof(&packed, root.path(), true);
        assert!(serde_json::to_vec(&packed).unwrap().len() <= 16000);
        assert!(
            hits(&packed)
                .iter()
                .any(|h| h["file_path"] == "scope/impl.py"
                    && h["text"].as_str().unwrap().contains("return value + 731")),
            "numeric width cannot displace fitting required body: {case}"
        );
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["generation"]["index_epoch"],
            epoch
        );
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["generation"]["evidence_epoch"],
            epoch
        );
        assert_eq!(
            packed["evidence_summary"]["retrieval"]["cost"],
            raw["evidence_summary"]["retrieval"]["cost"]
        );
        assert_eq!(
            packed["evidence_summary"]["source_freshness"]["generation"]["incarnation"],
            json!(vec![byte; 16])
        );
        assert_eq!(packed["resolution_freshness"]["index_epoch"], epoch);
        for lane in packed["evidence_summary"]["retrieval"]["lane_receipts"]
            .as_array()
            .unwrap()
        {
            if lane["status"] == "complete" {
                assert_eq!(lane["elapsed_us"], elapsed);
            }
        }
        for h in hits(&packed) {
            assert_eq!(
                original[h["chunk_id"].as_str().unwrap()],
                (
                    h["score_trace"].clone(),
                    h["rerank_score"].clone(),
                    h["text"].clone()
                )
            );
        }
        evidence(
            &json!({"synthetic_metadata_width_only":true,"case":case,"packed":packed}),
            &format!("numeric-width-{case}"),
        );
    }
    let mut unknown = raw.clone();
    let path_label = format!(
        "{};caller_unknown",
        cc_search::query_policy::PATH_SOURCE_DOMAIN
    );
    let graph_label = format!(
        "{};caller_unknown",
        cc_search::query_policy::GRAPH_SOURCE_MAPPING
    );
    unknown["evidence_summary"]["retrieval"]["policy"]["path_source_domain"] = json!(path_label);
    unknown["evidence_summary"]["retrieval"]["policy"]["graph_source_mapping"] = json!(graph_label);
    let packed = pack_value(unknown, 14000).unwrap();
    assert_eq!(
        packed["evidence_summary"]["retrieval"]["policy"]["path_source_domain"],
        path_label
    );
    assert_eq!(
        packed["evidence_summary"]["retrieval"]["policy"]["graph_source_mapping"],
        graph_label
    );
    assert_eq!(normalizer::mcp(&packed).unwrap().1, ResultStatus::Partial);
    proof(&packed, root.path(), true);
    evidence(
        &json!({"synthetic_unknown_domain_labels_only":true,"packed":packed}),
        "unknown-domain-labels",
    );
}
