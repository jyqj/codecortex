//! Facade handlers: composite dispatch for the 12 MCP tools.

use super::{context, core, graph};
use crate::engine::CodeIndex;
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

pub(crate) fn enforce_output_limit(value: Value, max_chars: usize) -> Value {
    let serialized = serde_json::to_string(&value).unwrap_or_default();
    if serialized.len() <= max_chars {
        return value;
    }
    let truncated = &serialized[..max_chars.min(serialized.len())];
    let safe_end = truncated
        .rfind('}')
        .or_else(|| truncated.rfind(']'))
        .unwrap_or(truncated.len());
    json!({
        "_truncated": true,
        "_original_chars": serialized.len(),
        "_max_chars": max_chars,
        "partial": serde_json::from_str::<Value>(&serialized[..=safe_end]).unwrap_or(value),
    })
}

// ── 1. handle_status ────────────────────────────────────────────────

pub fn handle_status(runtime: Arc<RwLock<CodeIndex>>, aspect: &str) -> Result<Value, String> {
    match aspect {
        "index" => {
            let mut result = core::index_status(runtime.clone())?;
            // Attach runtime evidence stats if database is available
            if let Some(evidence) = runtime_evidence_summary(&runtime) {
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("runtime_evidence".to_string(), evidence);
                }
            }
            Ok(result)
        }
        "capabilities" => core::index_capabilities(runtime),
        "schema" => core::graph_schema(runtime),
        _ => {
            let index = core::index_status(runtime.clone())?;
            let capabilities = core::index_capabilities(runtime.clone())?;
            let schema = core::graph_schema(runtime.clone())?;
            let mut result = json!({
                "index": index,
                "capabilities": capabilities,
                "schema": schema,
            });
            // Attach runtime evidence stats if database is available
            if let Some(evidence) = runtime_evidence_summary(&runtime) {
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("runtime_evidence".to_string(), evidence);
                }
            }
            Ok(result)
        }
    }
}

/// Query runtime_evidence stats from the database, returning None if unavailable.
fn runtime_evidence_summary(runtime: &Arc<RwLock<CodeIndex>>) -> Option<Value> {
    let rt = super::lock_index(runtime).ok()?;
    let db = rt.index_db()?;
    db.runtime_evidence_stats().ok()
}

// ── 2. handle_context ───────────────────────────────────────────────

pub fn handle_context(
    runtime: Arc<RwLock<CodeIndex>>,
    task: &str,
    max_symbols: Option<usize>,
    include_source: bool,
    intent: Option<&str>,
) -> Result<Value, String> {
    let max_chars = {
        let rt = super::lock_index(&runtime)?;
        rt.repo_size_tier().max_output_chars()
    };
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

    Ok(enforce_output_limit(result, max_chars))
}

// ── 3. handle_node ──────────────────────────────────────────────────

pub fn handle_node(
    runtime: Arc<RwLock<CodeIndex>>,
    symbol: &str,
    include: &str,
) -> Result<Value, String> {
    let (relation_limit, max_chars) = {
        let rt = super::lock_index(&runtime)?;
        let tier = rt.repo_size_tier();
        (tier.explore_max_symbols(), tier.max_output_chars())
    };
    let result = match include {
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
    };
    result.map(|v| enforce_output_limit(v, max_chars))
}

// ── 4. handle_relations ─────────────────────────────────────────────

pub fn handle_relations(
    runtime: Arc<RwLock<CodeIndex>>,
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

pub fn handle_impact(
    runtime: Arc<RwLock<CodeIndex>>,
    scope: &str,
    files: &[String],
    base_branch: Option<&str>,
    granularity: &str,
    file_path: Option<&str>,
    limit: usize,
) -> Result<Value, String> {
    let max_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.output_budget("impact").max_items
    };
    let limit = limit.min(max_limit);
    match scope {
        "tests" => graph::find_impacted_tests(runtime, files),
        "dead_code" => {
            let params = match file_path {
                Some(fp) => json!({"scope": fp}),
                None => json!({}),
            };
            graph::find_dead_code(runtime, params)
        }
        "circular" => graph::find_circular_deps(runtime, granularity, Some(limit)),
        "dependents" => {
            let fp = file_path
                .ok_or_else(|| "file_path is required for 'dependents' scope".to_string())?;
            graph::get_dependents(runtime, json!({"file_path": fp}))
        }
        _ => core::analyze_impact(runtime, files, base_branch),
    }
}

// ── 6. handle_architecture ──────────────────────────────────────────

pub fn handle_architecture(
    runtime: Arc<RwLock<CodeIndex>>,
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
    runtime: Arc<RwLock<CodeIndex>>,
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
        "list" => {
            let max_files = {
                let rt = super::lock_index(&runtime)?;
                rt.output_budget("files").max_items
            };
            let mut result = core::list_files(runtime)?;
            if let Some(arr) = result.as_array_mut() {
                if arr.len() > max_files {
                    let total = arr.len();
                    arr.truncate(max_files);
                    arr.push(json!({"_truncated": true, "_total": total, "_shown": max_files}));
                }
            }
            Ok(result)
        }
        other => Err(format!(
            "unknown files action {:?}; expected \"list\", \"region\", or \"expand\"",
            other
        ))
    }
}

// ── ingest_traces ──────────────────────────────────────────────────

pub fn handle_ingest_traces(
    runtime: Arc<RwLock<CodeIndex>>,
    traces: &[serde_json::Value],
) -> Result<Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut accepted = 0u32;
    let mut matched = 0u32;
    let mut ambiguous = 0u32;
    let mut unmatched = 0u32;
    let mut routes_matched = 0u32;
    let mut spans_processed = 0u32;

    for trace in traces {
        // ── OTLP resource+spans format ──────────────────────────────
        if let Some(spans) = trace.get("spans").and_then(|v| v.as_array()) {
            // Resolve service_name: resource.service_name > top-level service_name > "unknown"
            let service = trace
                .get("resource")
                .and_then(|r| r.get("service_name"))
                .and_then(|v| v.as_str())
                .or_else(|| trace.get("service_name").and_then(|v| v.as_str()))
                .unwrap_or("unknown");

            for span in spans {
                let method = span.get("method").and_then(|v| v.as_str());
                let path = match span.get("path").and_then(|v| v.as_str()) {
                    Some(p) => p,
                    None => continue,
                };
                let status = span.get("status_code").and_then(|v| v.as_str());
                let duration_ms = span.get("duration_ms").and_then(|v| v.as_f64());
                let span_kind = span.get("kind").and_then(|v| v.as_str());

                // Build trace_source tag: "otlp:SERVER", "otlp:CLIENT", or "otlp"
                let trace_source = match span_kind {
                    Some(k) => format!("otlp:{}", k),
                    None => "otlp".to_string(),
                };

                let eid = format!(
                    "{}:{}:{}:{}",
                    trace_source,
                    service,
                    method.unwrap_or("*"),
                    path
                );
                if db
                    .upsert_runtime_evidence(&eid, service, method, path, status, &now)
                    .is_ok()
                {
                    accepted += 1;
                }

                if let Some(dur) = duration_ms {
                    let _ = db.update_evidence_p95(&eid, dur);
                }

                // Joint matching: same logic as flat traces
                let norm = cc_model::route_normalize::normalize_route_path(path);
                let conn = db.read_conn().map_err(|e| e.to_string())?;

                let edge_id: Option<String> = if let Some(m) = method {
                    conn.query_row(
                        "SELECT edge_id FROM http_call_edges WHERE normalized_path = ?1 AND method = ?2 LIMIT 1",
                        rusqlite::params![&norm, m],
                        |r| r.get(0),
                    )
                    .ok()
                } else {
                    None
                };

                let edge_id = edge_id.or_else(|| {
                    conn.query_row(
                        "SELECT edge_id FROM http_call_edges WHERE normalized_path = ?1 LIMIT 1",
                        [&norm],
                        |r| r.get(0),
                    )
                    .ok()
                });

                let candidate_count: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM http_call_edges WHERE normalized_path = ?1",
                        [&norm],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);

                if let Some(ref eid_db) = edge_id {
                    let _ = db.link_evidence_to_edge(&eid, eid_db);
                    let _ = db.boost_http_edge_confidence(eid_db, 0.15);
                    matched += 1;
                    if candidate_count > 1 {
                        ambiguous += 1;
                    }
                } else {
                    unmatched += 1;
                }

                let route_id: Option<String> = conn
                    .query_row(
                        "SELECT route_id FROM route_nodes WHERE normalized_path = ?1 LIMIT 1",
                        [&norm],
                        |r| r.get(0),
                    )
                    .ok();

                if let Some(ref rid) = route_id {
                    let _ = db.update_evidence_route_id(&eid, rid);
                    routes_matched += 1;
                }

                spans_processed += 1;
            }
            continue;
        }

        // ── Flat format (existing logic, unchanged) ─────────────────
        let service = trace
            .get("service_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let method = trace.get("method").and_then(|v| v.as_str());
        let path = match trace.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };
        let status = trace.get("status_code").and_then(|v| v.as_str());
        let duration_ms = trace.get("duration_ms").and_then(|v| v.as_f64());

        let eid = format!("{}:{}:{}", service, method.unwrap_or("*"), path);
        if db
            .upsert_runtime_evidence(&eid, service, method, path, status, &now)
            .is_ok()
        {
            accepted += 1;
        }

        // Update p95_ms with exponential moving average if duration provided
        if let Some(dur) = duration_ms {
            let _ = db.update_evidence_p95(&eid, dur);
        }

        // Joint matching: try method+path first, then fallback to path-only
        let norm = cc_model::route_normalize::normalize_route_path(path);
        let conn = db.read_conn().map_err(|e| e.to_string())?;

        // Try exact match: method + normalized_path
        let edge_id: Option<String> = if let Some(m) = method {
            conn.query_row(
                "SELECT edge_id FROM http_call_edges WHERE normalized_path = ?1 AND method = ?2 LIMIT 1",
                rusqlite::params![&norm, m],
                |r| r.get(0),
            )
            .ok()
        } else {
            None
        };

        // Fallback: path-only match
        let edge_id = edge_id.or_else(|| {
            conn.query_row(
                "SELECT edge_id FROM http_call_edges WHERE normalized_path = ?1 LIMIT 1",
                [&norm],
                |r| r.get(0),
            )
            .ok()
        });

        // Count total candidates for ambiguity detection
        let candidate_count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM http_call_edges WHERE normalized_path = ?1",
                [&norm],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if let Some(ref eid_db) = edge_id {
            let _ = db.link_evidence_to_edge(&eid, eid_db);
            let _ = db.boost_http_edge_confidence(eid_db, 0.15);
            matched += 1;
            if candidate_count > 1 {
                ambiguous += 1;
            }
        } else {
            unmatched += 1;
        }

        // Match to route_nodes by normalized_path
        let route_id: Option<String> = conn
            .query_row(
                "SELECT route_id FROM route_nodes WHERE normalized_path = ?1 LIMIT 1",
                [&norm],
                |r| r.get(0),
            )
            .ok();

        if let Some(ref rid) = route_id {
            let _ = db.update_evidence_route_id(&eid, rid);
            routes_matched += 1;
        }
    }

    Ok(json!({
        "accepted": accepted,
        "matched_to_edges": matched,
        "routes_matched": routes_matched,
        "ambiguous": ambiguous,
        "unmatched": unmatched,
        "spans_processed": spans_processed,
        "total_submitted": traces.len(),
    }))
}

// ── ADR ────────────────────────────────────────────────────────────

pub fn handle_adr(
    runtime: Arc<RwLock<CodeIndex>>,
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
    use serde_json::json;

    // ── enforce_output_limit: passthrough when under limit ──────────

    #[test]
    fn enforce_output_limit_passthrough_when_small() {
        let input = json!({"key": "value"});
        let result = enforce_output_limit(input.clone(), 10000);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: truncation when over limit ────────────

    #[test]
    fn enforce_output_limit_truncates_when_over() {
        let large = json!({"data": "x".repeat(500)});
        let result = enforce_output_limit(large.clone(), 50);

        assert!(result.get("_truncated").is_some());
        assert_eq!(result["_truncated"], true);
        assert!(result["_original_chars"].as_u64().unwrap() > 50);
        assert_eq!(result["_max_chars"], 50);
        assert!(result.get("partial").is_some());
    }

    // ── enforce_output_limit: zero max_chars ────────────────────────

    #[test]
    fn enforce_output_limit_zero_max() {
        let input = json!({"a": 1});
        let result = enforce_output_limit(input.clone(), 0);
        // Should produce a truncated wrapper (since serialized len > 0)
        assert!(result.get("_truncated").is_some());
    }

    // ── enforce_output_limit: exact boundary ────────────────────────

    #[test]
    fn enforce_output_limit_at_exact_boundary() {
        let input = json!({"k": "v"});
        let serialized_len = serde_json::to_string(&input).unwrap().len();
        // At exact length, should pass through
        let result = enforce_output_limit(input.clone(), serialized_len);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: one less than serialized len ──────────

    #[test]
    fn enforce_output_limit_one_less_than_len() {
        let input = json!({"k": "v"});
        let serialized_len = serde_json::to_string(&input).unwrap().len();
        let result = enforce_output_limit(input, serialized_len - 1);
        assert!(result.get("_truncated").is_some());
    }

    // ── enforce_output_limit: nested structure ──────────────────────

    #[test]
    fn enforce_output_limit_nested_json() {
        let input = json!({
            "outer": {
                "inner": [1, 2, 3],
                "deep": {"value": "hello"}
            }
        });
        let serialized_len = serde_json::to_string(&input).unwrap().len();

        // Under limit: passthrough
        let result = enforce_output_limit(input.clone(), serialized_len + 100);
        assert_eq!(result, input);

        // Over limit: truncated
        let result = enforce_output_limit(input, 30);
        assert!(result.get("_truncated").is_some());
    }

    // ── enforce_output_limit: array value ───────────────────────────

    #[test]
    fn enforce_output_limit_with_array() {
        let input = json!([1, 2, 3, 4, 5]);
        let result = enforce_output_limit(input.clone(), 100000);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: string value ──────────────────────────

    #[test]
    fn enforce_output_limit_with_string() {
        let input = json!("hello world");
        let result = enforce_output_limit(input.clone(), 100000);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: null value ────────────────────────────

    #[test]
    fn enforce_output_limit_with_null() {
        let input = json!(null);
        let result = enforce_output_limit(input.clone(), 100);
        assert_eq!(result, input);
    }

    // ── Handler dispatch integration tests ─────────────────────────

    fn build_test_index() -> (tempfile::TempDir, Arc<std::sync::RwLock<crate::engine::CodeIndex>>) {
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
        (tmp, Arc::new(std::sync::RwLock::new(idx)))
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
        let result = handle_impact(rt, "dead_code", &[], None, "file", None, 20).unwrap();
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
