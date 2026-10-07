//! Independent stage diagnostic; fixture is byte-identical to PR135 fixed 591c246.
//! Test-only assembly mirror of cc-server engine.rs assemble_context_once; no
//! product instrumentation or modified budgets. Real public MCP is recorded too.
use cc_eval::{benchmark::normalizer, runner::CodeIndexBackend};
use cc_model::{
    config::{load_project_config, RepoSizeTier},
    context::{ContextEnvelope, ContextNode, ContextSpan, NodeType, Role},
    search::SearchRequest,
    CcError, CcResult, Intent,
};
use cc_search::SearchEngine;
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};
fn save(label: &str, value: &Value) -> CcResult<()> {
    let Ok(dir) = std::env::var("P5E_STAGE_EVIDENCE") else {
        return Ok(());
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{label}.json"));
    assert!(!path.exists(), "immutable diagnostic receipt");
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn verify(value: &Value, root: &Path) {
    let (mut rows, _) = normalizer::mcp(value).unwrap();
    for row in &mut rows {
        normalizer::verify_source(row, root).unwrap();
        assert_eq!(row.evidence_valid, Some(true));
        assert!(row.path.starts_with("scope/"));
    }
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
#[allow(clippy::too_many_arguments)] // Test-only mirror keeps the producer inputs explicit.
fn trace_assembly(
    engine: &SearchEngine,
    db: &cc_db::index_db::IndexDb,
    root: &Path,
    config: &cc_model::query::QueryConfig,
    tier: RepoSizeTier,
    query: &str,
    top_k: usize,
    intent: Intent,
    overrides: SearchRequest,
    generation: cc_model::generation::ReadGeneration,
    label: &str,
) -> CcResult<ContextEnvelope> {
    engine.check_live()?;
    let token_budget = tier.default_token_budget();
    let max_output_chars = tier.max_output_chars();
    let top_k = if top_k == 0 {
        tier.search_top_k()
    } else {
        top_k
    };
    let detected_intent = intent;
    let mut request = SearchRequest {
        query: query.into(),
        top_k,
        include_grep: true,
        ..overrides
    };
    request.intent = Some(detected_intent);
    config.validate()?;
    let control = match &request.control {
        Some(c) => c.limit_total(std::time::Duration::from_millis(config.deadline_ms)),
        None => cc_model::query::QueryControl::new(std::time::Duration::from_millis(
            config.deadline_ms,
        ))?,
    };
    request.control = Some(control.clone());
    let policy = cc_search::query_policy::QueryPolicy::resolve(
        config,
        &request,
        request.semantic.is_some(),
    )?;
    control.check()?;
    // Graph-aware rerank happens entirely inside cc-search: it searches a
    // rerank_window-sized candidate list, folds graph connectivity into
    // rerank_score, and returns the FINAL bounded ranking. The evidence
    // selector chooses a subset without changing that order or its scores.
    let graph_limits = tier.graph_enrich_limits();
    // The returned Arc is a shared, possibly cached pair — read-only by
    // contract (mutating it would corrupt cc-search's graph-aware cache).
    let search_outcome = engine.search_context_candidates(&request, &graph_limits, token_budget)?;
    save(
        &format!("{label}-retrieval"),
        &json!({"hits":search_outcome.0,"lanes":search_outcome.1.lane_outcomes,"cost":search_outcome.1.retrieval_cost,"grep":search_outcome.1.grep_diagnostics,"scope":search_outcome.1.scope_explain,"graph_nodes":search_outcome.1.nodes}),
    )?;
    let enrichment = &search_outcome.1;
    let mut verifier = cc_search::evidence_hydrator::EvidenceHydrator::new(
        db,
        root,
        cc_search::query_policy::hard_scope(&request)?,
        generation,
        control.clone(),
    )?;
    let hits = verifier.hydrate(&search_outcome.0)?;

    save(
        &format!("{label}-hydrated"),
        &json!({"hits":hits,"diagnostics":verifier.diagnostics()}),
    )?;
    let (hits, selection) =
        cc_search::selection::coverage::select_with_query(&hits, detected_intent, top_k, query)?;

    save(
        &format!("{label}-selected"),
        &json!({"hits":hits,"selection":selection}),
    )?;
    let mut nodes = Vec::with_capacity(hits.len());
    let mut spans = Vec::with_capacity(hits.len());
    let mut files = HashSet::new();
    let mut rendered_sections = Vec::new();

    for (idx, hit) in hits.iter().enumerate() {
        files.insert(hit.file_path.clone());
        let node_id = format!("search:{}", hit.chunk_id);
        let title = format!(
            "{} {}:{}-{}",
            hit.file_path, hit.breadcrumb, hit.start_line, hit.end_line
        );
        let role = if hit.reasons.iter().any(|r| r == "doc-file") {
            Role::DocContext
        } else {
            Role::Primary
        };
        let mut node = ContextNode::new(
            node_id.clone(),
            NodeType::SearchHit,
            role,
            title.clone(),
            hit.text.clone(),
        );
        node.file_path = Some(hit.file_path.clone());
        node.start_line = Some(hit.start_line);
        node.end_line = Some(hit.end_line);
        node.score = hit.rerank_score;
        node.confidence = 0.8;
        node.source = hit.source.clone();
        node.reasons = hit.reasons.clone();
        node.invalidation_keys = vec![hit.file_path.clone()];
        node.metadata = hit.metadata.clone();
        node.span_kind = Some("indexed_chunk".to_string());
        node.backing_file_path = Some(hit.file_path.clone());
        node.source_start_line = Some(hit.start_line);
        node.source_end_line = Some(hit.end_line);

        spans.push(ContextSpan {
            node_id,
            file_path: Some(hit.file_path.clone()),
            start_line: Some(hit.start_line),
            end_line: Some(hit.end_line),
            label: hit.breadcrumb.clone(),
        });

        rendered_sections.push(format!(
            "## {}. {}:{}-{}\n{}",
            idx + 1,
            hit.file_path,
            hit.start_line,
            hit.end_line,
            hit.text
        ));
        nodes.push(node);
    }

    // Append graph context nodes within budget.
    let primary_tokens: u32 = nodes.iter().map(|n| n.token_estimate).sum();
    let graph_budget = (token_budget * graph_limits.graph_budget_pct) / 100;
    let mut graph_tokens_used = 0u32;
    let mut graph_rendered: Vec<String> = Vec::new();
    for gnode in &enrichment.nodes {
        if !verifier.graph_current(gnode)? {
            continue;
        }
        if graph_tokens_used + gnode.token_estimate > graph_budget {
            break;
        }
        graph_tokens_used += gnode.token_estimate;
        graph_rendered.push(format!("- {}", gnode.title));
        nodes.push(gnode.clone());
    }

    let token_estimate: u32 = primary_tokens + graph_tokens_used;
    let source_freshness = verifier.diagnostics();
    let incomplete = source_freshness["partial"] == true
        || enrichment
            .grep_diagnostics
            .as_ref()
            .is_some_and(|d| d.is_partial())
        || enrichment.lane_outcomes.iter().any(|lane| {
            matches!(
                lane.status,
                cc_model::retrieval::LaneStatus::Partial
                    | cc_model::retrieval::LaneStatus::Timeout
                    | cc_model::retrieval::LaneStatus::Unavailable
                    | cc_model::retrieval::LaneStatus::Error
                    | cc_model::retrieval::LaneStatus::Cancelled
            )
        });
    let summary = if hits.is_empty() && incomplete {
        format!("No results found within the scan budget for `{query}`; retrieval is incomplete.")
    } else if hits.is_empty() {
        format!("No indexed code results found for `{}`.", query)
    } else {
        format!(
            "Found {} indexed code result(s) across {} file(s) for `{}`.",
            hits.len(),
            files.len(),
            query
        )
    };
    let mut rendered_prompt = format!(
        "Task: code-index search\nIntent: {}\nQuery: {}\n\n{}",
        detected_intent,
        query,
        rendered_sections.join("\n\n")
    );
    if !graph_rendered.is_empty() {
        rendered_prompt.push_str("\n\n## Graph Context\n");
        rendered_prompt.push_str(&graph_rendered.join("\n"));
    }

    // Unified explainability envelope (additive, same pattern as the
    // graph handlers): the enrichment's GraphExplain is attached only
    // when non-empty, so a clean run keeps the response shape unchanged.
    let mut graph_enrichment_summary = serde_json::json!({
        "symbols_resolved": enrichment.symbols_resolved,
        "callers_added": enrichment.callers_added,
        "callees_added": enrichment.callees_added,
        "tests_found": enrichment.tests_found,
    });
    if !enrichment.graph_explain.is_empty() {
        graph_enrichment_summary["graph_explain"] = serde_json::to_value(&enrichment.graph_explain)
            .map_err(|e| CcError::Search(e.to_string()))?;
    }

    control.check()?;
    engine.check_live()?;
    let envelope = ContextEnvelope {
        task: query.to_string(),
        intent: detected_intent,
        query: query.to_string(),
        token_budget,
        token_estimate,
        summary,
        rendered_prompt,
        revision: 0,
        nodes,
        spans,
        reasons: {
            let mut r = vec!["hybrid search over code index".to_string()];
            if enrichment.symbols_resolved > 0 {
                r.push("graph-enriched".to_string());
            }
            r
        },
        invalidations: Vec::new(),
        machine_pack: serde_json::json!({
            "kind": "code_index_context",
            "query": query,
            "top_k": top_k,
            "repo_size_tier": format!("{:?}", tier),
            "token_budget": token_budget,
            "hits": hits,
        }),
        evidence_summary: serde_json::json!({
            "source_freshness": source_freshness,
            "selection": selection,
            "search_hits": hits.len(),
            "files": files.into_iter().collect::<Vec<_>>(),
            "graph_enrichment": graph_enrichment_summary,
            "retrieval": {"lanes": enrichment.lane_outcomes, "grep": enrichment.grep_diagnostics, "scope": enrichment.scope_explain, "cost": enrichment.retrieval_cost, "policy":policy},
        }),
    };
    save(
        &format!("{label}-prepack"),
        &serde_json::to_value(&envelope)?,
    )?;
    let envelope = cc_search::selection::budget::pack(envelope, max_output_chars)?;
    save(
        &format!("{label}-core-packed"),
        &serde_json::to_value(&envelope)?,
    )?;
    verifier.finish()?;
    engine.check_live()?;
    Ok(envelope)
}

#[test]
fn diag_p5e_stage_receipts_same_microfixture() {
    let root = fixture();
    let backend = CodeIndexBackend::new_unindexed(root.path()).unwrap();
    save("index-report", &backend.build_index_report(false).unwrap()).unwrap();
    let config = load_project_config(root.path());
    let index = CodeIndex::new(Some(root.path())).unwrap();
    let db = index.index_db().unwrap();
    let tier = index.repo_size_tier();
    let mut files = Vec::new();
    for entry in index.graph().list_indexed_files().unwrap() {
        files.push(json!({"file":format!("{entry:?}")}));
    }
    save("indexed-files", &json!(files)).unwrap();
    save(
        "impl-indexed-symbols",
        &serde_json::to_value(index.graph().file_symbols("scope/impl.py").unwrap()).unwrap(),
    )
    .unwrap();
    let engine = SearchEngine::new(db.clone(), &config, Some(tier));
    save("default-budgets", &json!({"tier":format!("{tier:?}"), "max_bytes":tier.max_output_chars(),
        "token_budget":tier.default_token_budget(), "effective_cap":tier.max_output_chars().min(tier.default_token_budget() as usize * 4),
        "fixture_noise_documents":2})).unwrap();
    for (label, intent) in [
        ("fix", Intent::Fix),
        ("refactor", Intent::Refactor),
        ("trace", Intent::Trace),
    ] {
        let task = "repair_widget distinctive_widget_support WidgetContract";
        let query = format!("{label} {task}");
        let public_context = backend.call_tool("context", &json!({"task":task,"intent":label,"retrieval_strategy":"local","include_source":true})).unwrap();
        save(&format!("{label}-public-context"), &public_context).unwrap();
        let public = backend
            .call_tool(
                "search",
                &json!({"query":query,"top_k":10,"mode":"hybrid","path_prefix":"scope"}),
            )
            .unwrap();
        verify(&public, root.path());
        save(&format!("{label}-public-search"), &public).unwrap();
        let overrides = SearchRequest {
            path_prefix: Some("scope".into()),
            ..Default::default()
        };
        let env = trace_assembly(
            &engine,
            db,
            root.path(),
            &config.query,
            tier,
            &query,
            10,
            intent,
            overrides.clone(),
            db.reads().read_generation().unwrap(),
            label,
        )
        .unwrap();
        let traced = serde_json::to_value(env).unwrap();
        verify(&traced, root.path());
        let real_core = serde_json::to_value(
            index
                .search()
                .search_in_context_with(&query, 10, Some(intent), overrides)
                .unwrap(),
        )
        .unwrap();
        verify(&real_core, root.path());
        save(&format!("{label}-real-core"), &real_core).unwrap();
        let mut attached = real_core.clone();
        // Stable fixture: the actual finalizer attaches exactly this live DB
        // observation; no generation changes occur between the observations.
        attached["resolution_freshness"] =
            serde_json::to_value(db.reads().resolution_freshness().unwrap()).unwrap();
        save(&format!("{label}-dispatch-before-pack"), &attached).unwrap();
        let final_pack =
            cc_search::selection::budget::pack_value(attached, tier.max_output_chars()).unwrap();
        verify(&final_pack, root.path());
        save(&format!("{label}-dispatch-after-pack"), &final_pack).unwrap();
        assert_eq!(
            final_pack["machine_pack"]["hits"],
            public["machine_pack"]["hits"]
        );
        assert_eq!(
            final_pack["evidence_summary"]["packing"]["omitted_hits"],
            public["evidence_summary"]["packing"]["omitted_hits"]
        );

        // Timing/generation observations are real independent executions. Match
        // exact source, score/order and omission counts rather than fabricate them.
        assert_eq!(
            traced["machine_pack"]["hits"],
            real_core["machine_pack"]["hits"]
        );
        assert_eq!(
            traced["evidence_summary"]["selection"],
            real_core["evidence_summary"]["selection"]
        );
        assert_eq!(
            traced["evidence_summary"]["packing"]["omitted_hits"],
            real_core["evidence_summary"]["packing"]["omitted_hits"]
        );
        for cap in [16000, 14000, 12000, 10000, 8000, 5000] {
            let result = match cc_search::selection::budget::pack_value(public.clone(), cap) {
                Ok(value) => {
                    verify(&value, root.path());
                    json!({"ok":true,"value":value})
                }
                Err(error) => json!({"ok":false,"error":error.to_string()}),
            };
            save(&format!("{label}-public-cap-{cap}"), &result).unwrap();
        }
    }
}

#[test]
#[ignore = "read-only measurement of saved stage receipts; requires evidence dir"]
fn diag_p5e_measure_saved_receipts() {
    let dir = std::path::PathBuf::from(std::env::var("P5E_STAGE_EVIDENCE").unwrap());
    let mut sizes = serde_json::Map::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|x| x == "json") {
            let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let mut fields = serde_json::Map::new();
            if let Some(object) = value.as_object() {
                for (k, v) in object {
                    fields.insert(k.clone(), json!(serde_json::to_vec(v).unwrap().len()));
                }
            }
            sizes.insert(path.file_name().unwrap().to_string_lossy().into(), json!({
                "serialized_utf8_bytes":serde_json::to_vec(&value).unwrap().len(), "fields":fields}));
        }
    }
    save("serialized-sizes", &json!(sizes)).unwrap();
}
