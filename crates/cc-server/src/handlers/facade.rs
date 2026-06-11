//! Facade handlers: composite dispatch for the 14 MCP tools.
//!
//! Byte/item output caps are applied once at the dispatch exit
//! (`handlers::output_budget::finalize`), not inside these handlers.

use super::{context, core, graph, SharedCodeIndex};
use cc_db::index_db::IndexDb;
use serde_json::{json, Value};

// ── 1. handle_status ────────────────────────────────────────────────

pub fn handle_status(runtime: SharedCodeIndex, aspect: &str) -> Result<Value, String> {
    match aspect {
        "index" => {
            let mut result = core::index_status(runtime.clone())?;
            attach_status_extras(&runtime, &mut result);
            Ok(result)
        }
        "capabilities" => core::index_capabilities(runtime),
        "schema" => core::graph_schema(runtime),
        _ => {
            let index = core::index_status(runtime.clone())?;
            let capabilities = core::index_capabilities(runtime.clone())?;
            let schema = core::graph_schema(runtime.clone())?;
            let diagnostics = {
                let rt = super::lock_index(&runtime)?;
                rt.diagnostics_info()
            };
            let mut result = json!({
                "index": index,
                "capabilities": capabilities,
                "schema": schema,
                "diagnostics": diagnostics,
            });
            if let Some(evidence) = runtime_evidence_summary(&runtime) {
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("runtime_evidence".to_string(), evidence);
                }
            }
            Ok(result)
        }
    }
}

fn attach_status_extras(runtime: &SharedCodeIndex, result: &mut Value) {
    if let Some(evidence) = runtime_evidence_summary(runtime) {
        if let Some(obj) = result.as_object_mut() {
            obj.insert("runtime_evidence".to_string(), evidence);
        }
    }
    if let Ok(rt) = super::lock_index(runtime) {
        let diag = rt.diagnostics_info();
        if let Some(obj) = result.as_object_mut() {
            obj.insert("diagnostics".to_string(), diag);
        }
    }
}

/// Query runtime_evidence stats from the database, returning None if unavailable.
fn runtime_evidence_summary(runtime: &SharedCodeIndex) -> Option<Value> {
    let rt = super::lock_index(runtime).ok()?;
    let db = rt.index_db()?;
    db.runtime_evidence_stats().ok()
}

// ── 2. handle_context ───────────────────────────────────────────────

pub fn handle_context(
    runtime: SharedCodeIndex,
    task: &str,
    max_symbols: Option<usize>,
    include_source: bool,
    intent: Option<&str>,
) -> Result<Value, String> {
    let mut result = context::task_symbols(runtime.clone(), task, max_symbols, Some(1), intent)?;

    if include_source {
        if let Some(matched) = result.get("matched_symbols").and_then(|v| v.as_array()) {
            let names: Vec<String> = matched
                .iter()
                .take(5)
                .filter_map(|v| {
                    v.get("name")
                        .or_else(|| v.get("symbol_name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect();

            if !names.is_empty() {
                let details = context::explore_symbols(
                    runtime,
                    &names,
                    Some(3),
                    Some(3),
                    true,
                    false,
                    false,
                    false,
                    None,
                )?;
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("symbol_details".to_string(), details);
                }
            }
        }
    }

    Ok(result)
}

// ── 3. handle_node ──────────────────────────────────────────────────

pub fn handle_node(runtime: SharedCodeIndex, symbol: &str, include: &str) -> Result<Value, String> {
    let relation_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.repo_size_tier().explore_max_symbols()
    };
    match include {
        "source" => context::get_symbol_source(runtime, symbol, false, true, None),
        "outline" => context::explore_symbols(
            runtime,
            &[symbol.to_string()],
            None,
            None,
            true,
            false,
            false,
            true,
            None,
        ),
        "summary" => core::summarize_file(runtime, symbol),
        _ => {
            let source = context::get_symbol_source(runtime.clone(), symbol, false, true, None)?;
            let callers_val = core::callers(runtime.clone(), symbol, relation_limit)?;
            let callees_val = core::callees(runtime, symbol, relation_limit)?;
            Ok(json!({
                "source": source,
                "callers": callers_val,
                "callees": callees_val,
            }))
        }
    }
}

// ── 4. handle_relations ─────────────────────────────────────────────

pub fn handle_relations(
    runtime: SharedCodeIndex,
    symbol: &str,
    kind: &str,
    limit: usize,
    direction: &str,
) -> Result<Value, String> {
    let max_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.output_budget("relations").max_items
    };
    let limit = limit.min(max_limit);
    // Item-count clamps alone don't bound output size: a handful of records
    // with long signatures/snippets can still blow past the agent context —
    // the dispatch-exit byte cap (output_budget::finalize) handles that.
    match kind {
        "callers" => core::callers(runtime, symbol, limit),
        "callees" => core::callees(runtime, symbol, limit),
        "refs" => graph::symbol_refs(runtime, symbol, limit),
        "hierarchy" => graph::type_hierarchy(runtime, symbol, None, None, direction, 5, true),
        _ => {
            let callers_val = core::callers(runtime.clone(), symbol, limit)?;
            let callees_val = core::callees(runtime, symbol, limit)?;
            Ok(json!({
                "callers": callers_val,
                "callees": callees_val,
            }))
        }
    }
}

// ── 5. handle_impact ────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn handle_impact(
    runtime: SharedCodeIndex,
    scope: &str,
    files: &[String],
    base_branch: Option<&str>,
    granularity: &str,
    file_path: Option<&str>,
    limit: usize,
    confidence_threshold: Option<f32>,
    max_nodes: Option<usize>,
    max_per_layer: Option<usize>,
) -> Result<Value, String> {
    let max_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.output_budget("impact").max_items
    };
    let limit = limit.min(max_limit);
    match scope {
        "tests" => {
            if files.is_empty() {
                let auto_files = core::git_changed_files(runtime.clone(), base_branch)?;
                graph::find_impacted_tests(runtime, &auto_files)
            } else {
                graph::find_impacted_tests(runtime, files)
            }
        }
        "dead_code" => {
            let params = match file_path {
                Some(fp) => json!({"scope": fp, "limit": limit}),
                None => json!({"limit": limit}),
            };
            graph::find_dead_code(runtime, params)
        }
        "circular" => graph::find_circular_deps(runtime, granularity, Some(limit)),
        "dependents" => {
            let fp = file_path
                .ok_or_else(|| "file_path is required for 'dependents' scope".to_string())?;
            graph::get_dependents(runtime, json!({"file_path": fp}))
        }
        _ => {
            // BFS safety caps for the blast-radius path. `limit` (already
            // clamped to the output budget) is the returned-symbol cap; the
            // BFS node/layer caps default via `ImpactOptions::default_for`
            // (node cap = limit×10 ≤ 5000, layer cap = 500) so a hub callee
            // cannot fan out into an unbounded report — and so direct engine
            // callers share the exact same defaults.
            let opts = crate::impact::ImpactOptions::default_for(
                limit,
                confidence_threshold.map(f64::from),
                max_nodes,
                max_per_layer,
            );
            core::analyze_impact(
                runtime,
                files,
                base_branch,
                confidence_threshold,
                opts.result_limit,
                opts.max_nodes,
                opts.max_per_layer,
            )
        }
    }
}

// ── 6. handle_architecture ──────────────────────────────────────────

pub fn handle_architecture(
    runtime: SharedCodeIndex,
    aspect: &str,
    filter: Option<&str>,
    limit: usize,
) -> Result<Value, String> {
    let max_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.output_budget("architecture").max_items
    };
    let limit = limit.min(max_limit);
    match aspect {
        "communities" => core::list_communities(runtime),
        "frameworks" => core::list_frameworks(runtime),
        "routes" => {
            let mut params = json!({"limit": limit});
            if let Some(route_path) = filter {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("route_path".to_string(), json!(route_path));
            }
            graph::find_route_handlers(runtime, params)
        }
        "services" => graph::find_service_bindings(runtime, filter.unwrap_or("")),
        "async" => graph::find_async_consumers(runtime, filter.unwrap_or("")),
        "boundaries" => graph::list_package_boundaries(runtime, limit as u32),
        "env" => {
            if let Some(pattern) = filter {
                graph::search_env_vars(runtime, pattern, None, limit)
            } else {
                graph::list_env_vars(runtime, limit)
            }
        }
        "unresolved" => graph::list_unresolved_refs(runtime, limit, None, None),
        _ => graph::get_architecture(runtime, json!({"limit": limit})),
    }
}

// ── 7. handle_files ─────────────────────────────────────────────────

pub fn handle_files(
    runtime: SharedCodeIndex,
    action: &str,
    path: Option<&str>,
    start_line: Option<u32>,
    end_line: Option<u32>,
    context_lines: u32,
) -> Result<Value, String> {
    match action {
        "region" => {
            let p = path.ok_or_else(|| "path is required for 'region' action".to_string())?;
            let sl = start_line
                .ok_or_else(|| "start_line is required for 'region' action".to_string())?;
            let el =
                end_line.ok_or_else(|| "end_line is required for 'region' action".to_string())?;
            context::prepare_edit_region(runtime, p, sl, el)
        }
        "expand" => {
            let p = path.ok_or_else(|| "path is required for 'expand' action".to_string())?;
            let sl = start_line
                .ok_or_else(|| "start_line is required for 'expand' action".to_string())?;
            let el =
                end_line.ok_or_else(|| "end_line is required for 'expand' action".to_string())?;
            context::expand_code_region(runtime, p, sl, el, context_lines)
        }
        // The "files" item cap (output_budget("files").max_items) is applied
        // at the dispatch exit by output_budget::finalize.
        "list" => core::list_files(runtime),
        other => Err(format!(
            "unknown files action {:?}; expected \"list\", \"region\", or \"expand\"",
            other
        )),
    }
}

// ── ingest_traces ──────────────────────────────────────────────────

#[derive(Default)]
struct IngestTraceStats {
    accepted: u32,
    matched: u32,
    ambiguous: u32,
    unmatched: u32,
    routes_matched: u32,
    spans_processed: u32,
}

struct TraceObservation<'a> {
    service: &'a str,
    method: Option<&'a str>,
    path: &'a str,
    status: Option<&'a str>,
    duration_ms: Option<f64>,
    source: Option<&'a str>,
}

fn ingest_observation(
    db: &IndexDb,
    obs: TraceObservation<'_>,
    now: &str,
    stats: &mut IngestTraceStats,
) -> Result<(), String> {
    let eid = match obs.source {
        Some(source) => format!(
            "{}:{}:{}:{}",
            source,
            obs.service,
            obs.method.unwrap_or("*"),
            obs.path
        ),
        None => format!("{}:{}:{}", obs.service, obs.method.unwrap_or("*"), obs.path),
    };

    if db
        .upsert_runtime_evidence(&eid, obs.service, obs.method, obs.path, obs.status, now)
        .is_ok()
    {
        stats.accepted += 1;
    }

    if let Some(dur) = obs.duration_ms {
        let _ = db.update_evidence_p95(&eid, dur);
    }

    let norm = cc_model::route_normalize::normalize_route_path(obs.path);

    let (edge_id, candidate_count) = db
        .http_edge_match_for_path(&norm, obs.method)
        .map_err(|e| e.to_string())?;

    if let Some(ref eid_db) = edge_id {
        let _ = db.link_evidence_to_edge(&eid, eid_db);
        let _ = db.boost_http_edge_confidence(eid_db, 0.15);
        stats.matched += 1;
        if candidate_count > 1 {
            stats.ambiguous += 1;
        }
    } else {
        stats.unmatched += 1;
    }

    let route_id = db
        .route_id_for_normalized_path(&norm)
        .map_err(|e| e.to_string())?;

    if let Some(ref rid) = route_id {
        let _ = db.update_evidence_route_id(&eid, rid);
        stats.routes_matched += 1;
    }

    stats.spans_processed += 1;
    Ok(())
}

pub fn handle_ingest_traces(
    runtime: SharedCodeIndex,
    traces: &[serde_json::Value],
) -> Result<Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().cloned().ok_or("no index database")?;
    drop(rt);
    let now = chrono::Utc::now().to_rfc3339();
    let mut stats = IngestTraceStats::default();

    for trace in traces {
        if let Some(spans) = trace.get("spans").and_then(|v| v.as_array()) {
            let service = trace
                .get("resource")
                .and_then(|r| r.get("service_name"))
                .and_then(|v| v.as_str())
                .or_else(|| trace.get("service_name").and_then(|v| v.as_str()))
                .unwrap_or("unknown");

            for span in spans {
                let Some(path) = span.get("path").and_then(|v| v.as_str()) else {
                    continue;
                };
                let trace_source;
                let source = match span.get("kind").and_then(|v| v.as_str()) {
                    Some(kind) => {
                        trace_source = format!("otlp:{}", kind);
                        Some(trace_source.as_str())
                    }
                    None => Some("otlp"),
                };
                ingest_observation(
                    &db,
                    TraceObservation {
                        service,
                        method: span.get("method").and_then(|v| v.as_str()),
                        path,
                        status: span.get("status_code").and_then(|v| v.as_str()),
                        duration_ms: span.get("duration_ms").and_then(|v| v.as_f64()),
                        source,
                    },
                    &now,
                    &mut stats,
                )?;
            }
            continue;
        }

        let Some(path) = trace.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        ingest_observation(
            &db,
            TraceObservation {
                service: trace
                    .get("service_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
                method: trace.get("method").and_then(|v| v.as_str()),
                path,
                status: trace.get("status_code").and_then(|v| v.as_str()),
                duration_ms: trace.get("duration_ms").and_then(|v| v.as_f64()),
                source: None,
            },
            &now,
            &mut stats,
        )?;
    }

    // No manual cache invalidation: every evidence write above bumps the
    // persisted evidence_epoch inside cc-db, which keys the bridge/adjacency
    // caches in graph_read_model.
    Ok(json!({
        "accepted": stats.accepted,
        "matched_to_edges": stats.matched,
        "routes_matched": stats.routes_matched,
        "ambiguous": stats.ambiguous,
        "unmatched": stats.unmatched,
        "spans_processed": stats.spans_processed,
        "total_submitted": traces.len(),
    }))
}

// ── ADR ────────────────────────────────────────────────────────────

pub fn handle_adr(
    runtime: SharedCodeIndex,
    action: &str,
    adr_id: Option<&str>,
    title: Option<&str>,
    status: Option<&str>,
    context: Option<&str>,
    decision: Option<&str>,
) -> Result<Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    match action {
        "list" => {
            let records = db.adr_list().map_err(|e| e.to_string())?;
            Ok(json!({ "adrs": records }))
        }
        "get" => {
            let id = adr_id.ok_or("adr_id is required for 'get'")?;
            let record = db.adr_get(id).map_err(|e| e.to_string())?;
            match record {
                Some(v) => Ok(v),
                None => Ok(json!({ "error": format!("ADR '{}' not found", id) })),
            }
        }
        "store" => {
            let id = adr_id.ok_or("adr_id is required for 'store'")?;
            let t = title.unwrap_or("Untitled");
            let s = status.unwrap_or("accepted");
            let c = context.unwrap_or("");
            let d = decision.unwrap_or("");
            let now = chrono::Utc::now().to_rfc3339();
            db.adr_upsert(id, t, s, c, d, &now)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "stored": id }))
        }
        "delete" => {
            let id = adr_id.ok_or("adr_id is required for 'delete'")?;
            let deleted = db.adr_delete(id).map_err(|e| e.to_string())?;
            Ok(json!({ "deleted": deleted, "adr_id": id }))
        }
        _ => Err(format!("unknown adr action: {}", action)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Handler dispatch integration tests ─────────────────────────

    fn build_test_index() -> (tempfile::TempDir, SharedCodeIndex) {
        let fixture_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../cc-eval/fixtures/sample-project");
        let tmp = tempfile::TempDir::new().unwrap();
        for entry in std::fs::read_dir(&fixture_src).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                std::fs::copy(entry.path(), tmp.path().join(entry.file_name())).unwrap();
            }
        }
        let mut idx = crate::engine::CodeIndex::new(Some(tmp.path())).unwrap();
        idx.build_index(true).unwrap();
        (tmp, std::sync::Arc::new(std::sync::RwLock::new(idx)))
    }

    #[test]
    fn handle_status_returns_index_info() {
        let (_tmp, rt) = build_test_index();
        let result = handle_status(rt, "index").unwrap();
        assert!(result.get("indexed_files").is_some());
    }

    #[test]
    fn handle_status_all_includes_capabilities_and_schema() {
        let (_tmp, rt) = build_test_index();
        let result = handle_status(rt, "all").unwrap();
        assert!(result.get("capabilities").is_some());
        assert!(result.get("schema").is_some());
    }

    #[test]
    fn handle_files_list_returns_array() {
        let (_tmp, rt) = build_test_index();
        let result = handle_files(rt, "list", None, None, None, 20).unwrap();
        assert!(result.is_array());
        assert!(!result.as_array().unwrap().is_empty());
    }

    #[test]
    fn handle_files_invalid_action_returns_error() {
        let (_tmp, rt) = build_test_index();
        let result = handle_files(rt, "bogus_action", None, None, None, 20);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown files action"));
    }

    #[test]
    fn handle_files_region_without_path_returns_error() {
        let (_tmp, rt) = build_test_index();
        let result = handle_files(rt, "region", None, Some(1), Some(5), 20);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path is required"));
    }

    #[test]
    fn handle_architecture_overview_returns_result() {
        let (_tmp, rt) = build_test_index();
        let result = handle_architecture(rt, "overview", None, 20).unwrap();
        assert!(result.is_object() || result.is_array());
    }

    #[test]
    fn handle_impact_dead_code_returns_result() {
        let (_tmp, rt) = build_test_index();
        let result = handle_impact(
            rt,
            "dead_code",
            &[],
            None,
            "file",
            None,
            20,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(result.is_object() || result.is_array());
    }

    #[test]
    fn handle_adr_list_empty() {
        let (_tmp, rt) = build_test_index();
        let result = handle_adr(rt, "list", None, None, None, None, None).unwrap();
        assert!(result.get("adrs").is_some());
    }

    #[test]
    fn handle_adr_unknown_action_errors() {
        let (_tmp, rt) = build_test_index();
        let result = handle_adr(rt, "drop_all", None, None, None, None, None);
        assert!(result.is_err());
    }
}
