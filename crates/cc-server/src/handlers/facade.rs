//! Facade handlers: composite dispatch for the 12 MCP tools.

use super::{context, core, graph};
use crate::engine::CodeIndex;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

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

pub fn handle_status(
    runtime: Arc<Mutex<CodeIndex>>,
    aspect: &str,
) -> Result<Value, String> {
    match aspect {
        "index" => core::index_status(runtime),
        "capabilities" => core::index_capabilities(runtime),
        "schema" => core::graph_schema(runtime),
        "all" | _ => {
            let index = core::index_status(runtime.clone())?;
            let capabilities = core::index_capabilities(runtime.clone())?;
            let schema = core::graph_schema(runtime)?;
            Ok(json!({
                "index": index,
                "capabilities": capabilities,
                "schema": schema,
            }))
        }
    }
}

// ── 2. handle_context ───────────────────────────────────────────────

pub fn handle_context(
    runtime: Arc<Mutex<CodeIndex>>,
    task: &str,
    max_symbols: Option<usize>,
    include_source: bool,
    intent: Option<&str>,
) -> Result<Value, String> {
    let max_chars = {
        let rt = runtime.lock().map_err(|e| e.to_string())?;
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
    runtime: Arc<Mutex<CodeIndex>>,
    symbol: &str,
    include: &str,
) -> Result<Value, String> {
    let (relation_limit, max_chars) = {
        let rt = runtime.lock().map_err(|e| e.to_string())?;
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
        "trail" | _ => {
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
    runtime: Arc<Mutex<CodeIndex>>,
    symbol: &str,
    kind: &str,
    limit: usize,
    direction: &str,
) -> Result<Value, String> {
    let max_limit = {
        let rt = runtime.lock().map_err(|e| e.to_string())?;
        rt.output_budget("relations").max_items
    };
    let limit = limit.min(max_limit);
    match kind {
        "callers" => core::callers(runtime, symbol, limit),
        "callees" => core::callees(runtime, symbol, limit),
        "refs" => graph::symbol_refs(runtime, symbol, limit),
        "hierarchy" => graph::type_hierarchy(runtime, symbol, None, None, direction, 5, true),
        "both" | _ => {
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
    runtime: Arc<Mutex<CodeIndex>>,
    scope: &str,
    files: &[String],
    base_branch: Option<&str>,
    granularity: &str,
    file_path: Option<&str>,
    limit: usize,
) -> Result<Value, String> {
    let max_limit = {
        let rt = runtime.lock().map_err(|e| e.to_string())?;
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
            let fp = file_path.ok_or_else(|| {
                "file_path is required for 'dependents' scope".to_string()
            })?;
            graph::get_dependents(runtime, json!({"file_path": fp}))
        }
        "changes" | _ => core::analyze_impact(runtime, files, base_branch),
    }
}

// ── 6. handle_architecture ──────────────────────────────────────────

pub fn handle_architecture(
    runtime: Arc<Mutex<CodeIndex>>,
    aspect: &str,
    filter: Option<&str>,
    limit: usize,
) -> Result<Value, String> {
    let max_limit = {
        let rt = runtime.lock().map_err(|e| e.to_string())?;
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
        "overview" | _ => graph::get_architecture(runtime, json!({"limit": limit})),
    }
}

// ── 7. handle_files ─────────────────────────────────────────────────

pub fn handle_files(
    runtime: Arc<Mutex<CodeIndex>>,
    action: &str,
    path: Option<&str>,
    start_line: Option<u32>,
    end_line: Option<u32>,
    context_lines: u32,
) -> Result<Value, String> {
    match action {
        "region" => {
            let p = path.ok_or_else(|| "path is required for 'region' action".to_string())?;
            let sl = start_line.ok_or_else(|| "start_line is required for 'region' action".to_string())?;
            let el = end_line.ok_or_else(|| "end_line is required for 'region' action".to_string())?;
            context::prepare_edit_region(runtime, p, sl, el)
        }
        "expand" => {
            let p = path.ok_or_else(|| "path is required for 'expand' action".to_string())?;
            let sl = start_line.ok_or_else(|| "start_line is required for 'expand' action".to_string())?;
            let el = end_line.ok_or_else(|| "end_line is required for 'expand' action".to_string())?;
            context::expand_code_region(runtime, p, sl, el, context_lines)
        }
        "list" | _ => {
            let max_files = {
                let rt = runtime.lock().map_err(|e| e.to_string())?;
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
    }
}

// ── ingest_traces ──────────────────────────────────────────────────

pub fn handle_ingest_traces(
    runtime: Arc<Mutex<CodeIndex>>,
    traces: &[serde_json::Value],
) -> Result<Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let db = rt.index_db().ok_or("no index database")?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut accepted = 0u32;
    let mut matched = 0u32;

    for trace in traces {
        let service = trace.get("service_name").and_then(|v| v.as_str()).unwrap_or("unknown");
        let method = trace.get("method").and_then(|v| v.as_str());
        let path = match trace.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };
        let status = trace.get("status_code").and_then(|v| v.as_str());

        let eid = format!("{}:{}:{}", service, method.unwrap_or("*"), path);
        if db.upsert_runtime_evidence(&eid, service, method, path, status, &now).is_ok() {
            accepted += 1;
        }

        // Try matching to existing http_call_edges by normalized_path
        let norm = cc_model::route_normalize::normalize_route_path(path);
        let conn = db.read_conn().map_err(|e| e.to_string())?;
        let edge_id: Option<String> = conn
            .query_row(
                "SELECT edge_id FROM http_call_edges WHERE normalized_path = ?1 LIMIT 1",
                [&norm],
                |r| r.get(0),
            )
            .ok();
        if let Some(ref eid_db) = edge_id {
            let _ = db.link_evidence_to_edge(&eid, eid_db);
            let _ = db.boost_http_edge_confidence(eid_db, 0.15);
            matched += 1;
        }
    }

    Ok(json!({
        "accepted": accepted,
        "matched_to_edges": matched,
        "total_submitted": traces.len(),
    }))
}

// ── ADR ────────────────────────────────────────────────────────────

pub fn handle_adr(
    runtime: Arc<Mutex<CodeIndex>>,
    action: &str,
    adr_id: Option<&str>,
    title: Option<&str>,
    status: Option<&str>,
    context: Option<&str>,
    decision: Option<&str>,
) -> Result<Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
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
            db.adr_upsert(id, t, s, c, d, &now).map_err(|e| e.to_string())?;
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
