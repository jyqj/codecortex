//! Graph domain handlers: graph queries, trace paths, symbol refs, caller/callee graphs,
//! dependents, dead code, references, and route handlers.

use super::SharedCodeIndex;

/// Execute a graph query (read-only Cypher subset).
pub fn graph_query(runtime: SharedCodeIndex, query: &str) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let budget = rt.output_budget("graph_query");
    let results = rt.graph_query(query).map_err(|e| e.to_string())?;
    // Enforce adaptive item limit when the query itself has no explicit LIMIT.
    let results = if !query.to_uppercase().contains("LIMIT") && results.len() > budget.max_items {
        results.into_iter().take(budget.max_items).collect()
    } else {
        results
    };
    serde_json::to_value(results).map_err(|e| e.to_string())
}

/// Trace call path between two symbols (rich version with optional snippets).
///
/// `source_mode`: `"none"` | `"snippet"` | `"body"` | `"outline"` | None.
/// When None, falls back to `include_snippets`: true→snippet, false→none.
#[allow(clippy::too_many_arguments)]
pub fn trace_path(
    runtime: SharedCodeIndex,
    from: &str,
    to: &str,
    max_depth: usize,
    include_snippets: bool,
    max_snippet_lines: Option<usize>,
    source_mode: Option<&str>,
    from_uid: Option<&str>,
    to_uid: Option<&str>,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let budget = rt.output_budget("trace_path");
    let db = rt.index_db().ok_or("no index database")?;
    let project_root = rt.project_path.as_deref();

    let effective_mode = source_mode.unwrap_or(if include_snippets { "snippet" } else { "none" });
    let (do_snippets, snippet_lines, snippet_budget, include_outgoing) = match effective_mode {
        "body" => (true, usize::MAX, 128 * 1024, true),
        "outline" => (false, 0, 0, false),
        "snippet" => (
            true,
            max_snippet_lines.unwrap_or(3),
            budget.max_snippet_chars,
            false,
        ),
        _ => (false, 0, 0, false),
    };

    let result = crate::graph_trace::trace_path_rich(
        db,
        project_root,
        from,
        to,
        max_depth,
        do_snippets,
        snippet_lines,
        Some(snippet_budget),
        include_outgoing,
        from_uid,
        to_uid,
    )
    .map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Find references to a symbol.
pub fn symbol_refs(
    runtime: SharedCodeIndex,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let refs = rt.symbol_refs(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(refs).map_err(|e| e.to_string())
}

pub fn list_unresolved_refs(
    runtime: SharedCodeIndex,
    limit: usize,
    file_path: Option<&str>,
    kind: Option<&str>,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.list_unresolved_refs(limit, file_path, kind)
        .map_err(|e| e.to_string())
}

/// Find tests impacted by the given set of files.
pub fn find_impacted_tests(
    runtime: SharedCodeIndex,
    files: &[String],
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let tests = rt.find_impacted_tests(files).map_err(|e| e.to_string())?;
    serde_json::to_value(tests).map_err(|e| e.to_string())
}

// ── New handlers ────────────────────────────────────────────────────

/// Get files that depend on (import) the given file, including transitive dependents.
///
/// Extracts: `file_path` (required).
/// Uses a reverse-imports Cypher query to find direct importers, then a second
/// pass for 2-hop transitive dependents.
pub fn get_dependents(
    runtime: SharedCodeIndex,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required parameter: file_path".to_string())?;

    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    // Query direct dependents via parameterized SQL on imports table
    let rows = db
        .query_json(
            "SELECT DISTINCT file_path FROM imports WHERE resolved_path = ?1 LIMIT 200",
            &[file_path.to_string()],
        )
        .map_err(|e| e.to_string())?;

    // Collect unique direct dependents
    let mut dependents: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in &rows {
        if let Some(fp) = row.get("file_path").and_then(|v| v.as_str()) {
            if fp != file_path && seen.insert(fp.to_string()) {
                dependents.push(fp.to_string());
            }
        }
    }

    // 2-hop: find importers of the direct dependents in a single batched query
    // (WHERE resolved_path IN (...)) instead of one SQL round-trip per dependent.
    let mut transitive: Vec<String> = Vec::new();
    if !dependents.is_empty() {
        let placeholders: String = (0..dependents.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT file_path FROM imports WHERE resolved_path IN ({}) LIMIT 10000",
            placeholders
        );
        let rows2 = db.query_json(&sql, &dependents).unwrap_or_default();
        for row in &rows2 {
            if let Some(fp) = row.get("file_path").and_then(|v| v.as_str()) {
                if fp != file_path && seen.insert(fp.to_string()) {
                    transitive.push(fp.to_string());
                }
            }
        }
    }

    dependents.extend(transitive);
    dependents.sort();

    Ok(serde_json::json!({
        "file_path": file_path,
        "dependents": dependents,
        "count": dependents.len(),
    }))
}

/// Find symbols that appear to be dead code (no incoming callers or references).
///
/// Extracts: `scope` (optional file_path prefix filter).
/// Queries all symbols, then checks for incoming call edges and references.
pub fn find_dead_code(
    runtime: SharedCodeIndex,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let scope = params.get("scope").and_then(|v| v.as_str());

    let user_limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let rt = super::lock_index(&runtime)?;
    let budget = rt.output_budget("dead_code");
    let effective_limit = user_limit.unwrap_or(budget.max_items).min(budget.max_items);
    let db = rt.index_db().ok_or("no index database")?;

    // Use an adaptive scan limit: 40x the desired dead_code item cap,
    // capped at 5000 to bound query cost.
    let scan_limit = (effective_limit * 40).min(5000);

    // Fetch all symbols via parameterized SQL
    let all_symbols = if let Some(prefix) = scope {
        let pattern = format!("%{}%", prefix);
        db.query_json(
            &format!(
                "SELECT name, symbol_uid, file_path, kind FROM symbols \
                 WHERE file_path LIKE ?1 LIMIT {}",
                scan_limit
            ),
            &[pattern],
        )
        .map_err(|e| e.to_string())?
    } else {
        db.query_json(
            &format!(
                "SELECT name, symbol_uid, file_path, kind FROM symbols LIMIT {}",
                scan_limit
            ),
            &[],
        )
        .map_err(|e| e.to_string())?
    };

    // Fetch all call edges to build a set of UIDs that have callers.
    // Exclude self-edges (caller == callee) since they don't indicate real usage.
    let edge_rows = db
        .query_json(
            "SELECT DISTINCT callee_symbol_uid FROM call_edges \
             WHERE callee_symbol_uid IS NOT NULL \
               AND (caller_symbol_uid IS NULL OR caller_symbol_uid != callee_symbol_uid) \
             LIMIT 10000",
            &[],
        )
        .unwrap_or_default();
    let mut has_callers = std::collections::HashSet::new();
    for row in &edge_rows {
        if let Some(uid) = row.get("callee_symbol_uid").and_then(|v| v.as_str()) {
            has_callers.insert(uid.to_string());
        }
    }

    let excluded_names = ["main", "__init__", "__main__", "setup", "configure"];
    let excluded_prefixes = ["test_", "Test"];

    // Phase 1: collect candidates that pass the scope / exclusion / no-caller
    // filters. Their reference status is resolved in a batched second pass to
    // avoid one symbol_refs query per candidate.
    struct DeadCandidate<'a> {
        name: &'a str,
        uid: &'a str,
        file_path: &'a str,
        kind: &'a str,
    }
    let mut candidates: Vec<DeadCandidate> = Vec::new();
    for row in &all_symbols {
        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
        let file_path = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("");

        if uid.is_empty() || name.is_empty() {
            continue;
        }

        // Scope filter
        if let Some(prefix) = scope {
            if !file_path.starts_with(prefix) {
                continue;
            }
        }

        // Exclusions
        if excluded_names.contains(&name) {
            continue;
        }
        if excluded_prefixes.iter().any(|p| name.starts_with(p)) {
            continue;
        }

        // Only symbols without callers can be dead code.
        if !has_callers.contains(uid) {
            candidates.push(DeadCandidate {
                name,
                uid,
                file_path,
                kind,
            });
        }
    }

    // Phase 2: batch-load references for all candidate UIDs at once, then
    // determine which have an *external* reference (container differs from the
    // symbol's own name, i.e. not a self-reference inside its own body).
    let mut uids_with_external_refs: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let candidate_uids: Vec<String> = candidates.iter().map(|c| c.uid.to_string()).collect();
    // Map each candidate uid -> its own name for the self-reference check.
    let name_by_uid: std::collections::HashMap<&str, &str> =
        candidates.iter().map(|c| (c.uid, c.name)).collect();
    let batch_size = 200;
    for batch in candidate_uids.chunks(batch_size) {
        let placeholders: String = (0..batch.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT target_symbol_uid, container FROM symbol_refs \
             WHERE target_symbol_uid IN ({})",
            placeholders
        );
        let ref_rows = db.query_json(&sql, batch).unwrap_or_default();
        for row in &ref_rows {
            let target_uid = row
                .get("target_symbol_uid")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if target_uid.is_empty() {
                continue;
            }
            let container = row.get("container").and_then(|v| v.as_str());
            let own_name = name_by_uid.get(target_uid).copied();
            // External reference iff container is NULL or differs from own name.
            let is_external = match (container, own_name) {
                (None, _) => true,
                (Some(c), Some(n)) => c != n,
                (Some(_), None) => true,
            };
            if is_external {
                uids_with_external_refs.insert(target_uid.to_string());
            }
        }
    }

    let mut dead_items: Vec<serde_json::Value> = Vec::new();
    for cand in &candidates {
        if !uids_with_external_refs.contains(cand.uid) {
            dead_items.push(serde_json::json!({
                "symbol_name": cand.name,
                "symbol_uid": cand.uid,
                "file_path": cand.file_path,
                "kind": cand.kind,
                "reason": "no-callers",
            }));
        }
    }

    let total_found = dead_items.len();
    let truncated = total_found > effective_limit;
    if truncated {
        dead_items.truncate(effective_limit);
    }

    Ok(serde_json::json!({
        "dead_code": dead_items,
        "count": dead_items.len(),
        "total_found": total_found,
        "truncated": truncated,
        "scan_limit": scan_limit,
    }))
}

/// Get structured architecture overview as JSON.
///
/// Supports optional `aspects` (comma-separated: languages, packages, entry_points, routes,
/// hotspots, boundaries, communities) and `limit` (default 10) parameters.
/// When aspects is empty/absent, all aspects are returned.
pub fn get_architecture(
    runtime: SharedCodeIndex,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let aspects_str = params
        .get("aspects")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    // Parse aspects list; empty means all
    let aspect_vec: Vec<&str> = if aspects_str.is_empty() {
        vec![]
    } else {
        aspects_str.split(',').map(|s| s.trim()).collect()
    };

    let info = db
        .get_architecture_info(&aspect_vec, limit)
        .map_err(|e| e.to_string())?;

    serde_json::to_value(info).map_err(|e| e.to_string())
}

/// Find HTTP route handlers matching a pattern.
///
/// Extracts: `route_path` (optional pattern to match).
/// Queries the route_edges table via Cypher ROUTES relationship.
pub fn find_route_handlers(
    runtime: SharedCodeIndex,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let route_path = params.get("route_path").and_then(|v| v.as_str());
    let method_filter = params.get("method").and_then(|v| v.as_str());
    let framework_filter = params.get("framework").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    // Build parameterized SQL query for route_edges
    let rows = if let Some(pattern) = route_path {
        let like_pattern = format!("%{}%", pattern);
        db.query_json(
            &format!(
                "SELECT route_path, method, handler_name, file_path, framework, line \
                 FROM route_edges WHERE route_path LIKE ?1 LIMIT {}",
                limit
            ),
            &[like_pattern],
        )
        .map_err(|e| e.to_string())?
    } else {
        db.query_json(
            &format!(
                "SELECT route_path, method, handler_name, file_path, framework, line \
                 FROM route_edges LIMIT {}",
                limit
            ),
            &[],
        )
        .map_err(|e| e.to_string())?
    };

    // Post-filter by method and framework if specified
    let filtered: Vec<serde_json::Value> = rows
        .into_iter()
        .filter(|row| {
            if let Some(method) = method_filter {
                let row_method = row.get("method").and_then(|v| v.as_str()).unwrap_or("");
                if !row_method.eq_ignore_ascii_case(method) {
                    return false;
                }
            }
            if let Some(fw) = framework_filter {
                let row_fw = row.get("framework").and_then(|v| v.as_str()).unwrap_or("");
                if !row_fw.eq_ignore_ascii_case(fw) {
                    return false;
                }
            }
            true
        })
        .map(|row| {
            serde_json::json!({
                "route_path": row.get("route_path").cloned().unwrap_or(serde_json::Value::Null),
                "method": row.get("method").cloned().unwrap_or(serde_json::Value::Null),
                "handler": row.get("handler_name").cloned().unwrap_or(serde_json::Value::Null),
                "file_path": row.get("file_path").cloned().unwrap_or(serde_json::Value::Null),
                "framework": row.get("framework").cloned().unwrap_or(serde_json::Value::Null),
                "line": row.get("line").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "route_handlers": filtered,
        "count": filtered.len(),
    }))
}

/// Find consumers of a topic or queue.
///
/// Queries infra_edges with kind IN ('binds_topic', 'consumes_queue') and joins
/// infra_nodes to resolve source/target names. Filters by topic_or_queue name.
pub fn find_async_consumers(
    runtime: SharedCodeIndex,
    topic_or_queue: &str,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    let pattern = format!("%{}%", topic_or_queue);
    let rows = db
        .query_json(
            "SELECT ie.edge_id, ie.source_node_id, ie.target_node_id, ie.kind, \
                    ie.confidence, ie.properties, \
                    src.name AS source_name, src.kind AS source_kind, \
                    src.file_path AS source_file, \
                    src.bound_symbol_uid AS source_bound_uid, \
                    CASE \
                        WHEN tgt_route.route_id IS NOT NULL THEN 'route' \
                        WHEN tgt_infra.node_id IS NOT NULL THEN 'infra_node' \
                        ELSE 'unknown' \
                    END AS target_type, \
                    COALESCE(tgt_infra.name, tgt_route.handler_name) AS target_name, \
                    tgt_infra.kind AS target_kind, \
                    COALESCE(tgt_infra.file_path, tgt_route.file_path) AS target_file, \
                    COALESCE(tgt_infra.bound_symbol_uid, tgt_route.handler_symbol_uid) AS target_bound_uid, \
                    tgt_route.route_path AS target_route_path, \
                    tgt_route.method AS target_method, \
                    tgt_route.handler_symbol_uid AS target_handler_symbol_uid \
             FROM infra_edges ie \
             LEFT JOIN infra_nodes src ON ie.source_node_id = src.node_id \
             LEFT JOIN infra_nodes tgt_infra ON ie.target_node_id = tgt_infra.node_id \
             LEFT JOIN route_nodes tgt_route ON ie.target_node_id = tgt_route.route_id \
             WHERE ie.kind IN ('binds_topic', 'consumes_queue') \
               AND (src.name LIKE ?1 OR ie.properties LIKE ?1)",
            &[pattern],
        )
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "topic_or_queue": topic_or_queue,
        "consumers": rows,
        "count": rows.len(),
    }))
}

/// Find infrastructure bindings for a service or route.
///
/// Two query dimensions:
/// 1. infra_nodes — matches by name or bound_symbol_uid
/// 2. route_nodes — matches by route_path, normalized_path, handler_name, or handler_symbol_uid
///
/// Then fetches associated infra_edges connected to any matched infra node_ids
/// or route_ids.
pub fn find_service_bindings(
    runtime: SharedCodeIndex,
    service_or_route: &str,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    let pattern = format!("%{}%", service_or_route);

    // ── Dimension 1: infra_nodes ──────────────────────────────────────
    let matched_infra_nodes = db
        .query_json(
            "SELECT node_id, file_path, kind, name, namespace, line, end_line, \
                    properties, bound_symbol_uid, binding_confidence \
             FROM infra_nodes \
             WHERE name LIKE ?1 OR bound_symbol_uid LIKE ?1",
            std::slice::from_ref(&pattern),
        )
        .map_err(|e| e.to_string())?;

    let infra_node_ids: Vec<String> = matched_infra_nodes
        .iter()
        .filter_map(|n| {
            n.get("node_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    // ── Dimension 2: route_nodes ──────────────────────────────────────
    let matched_routes = db
        .query_json(
            "SELECT route_id, file_path, route_path, method, handler_symbol_uid, \
                    handler_name, framework, line, end_line, normalized_path, confidence \
             FROM route_nodes \
             WHERE route_path LIKE ?1 \
                OR normalized_path LIKE ?1 \
                OR handler_name LIKE ?1 \
                OR handler_symbol_uid LIKE ?1",
            &[pattern],
        )
        .map_err(|e| e.to_string())?;

    let route_ids: Vec<String> = matched_routes
        .iter()
        .filter_map(|r| {
            r.get("route_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    // ── Dimension 3: related edges ────────────────────────────────────
    // Collect all IDs (infra node_ids + route_ids) for a single edge query
    let mut all_ids: Vec<String> = Vec::with_capacity(infra_node_ids.len() + route_ids.len());
    all_ids.extend(infra_node_ids);
    all_ids.extend(route_ids);

    let related_edges = if all_ids.is_empty() {
        Vec::new()
    } else {
        let placeholders: Vec<String> = (1..=all_ids.len()).map(|i| format!("?{}", i)).collect();
        let ph_str = placeholders.join(",");
        let sql = format!(
            "SELECT ie.edge_id, ie.source_node_id, ie.target_node_id, ie.kind, \
                    ie.confidence, ie.properties, \
                    src.name AS source_name, src.kind AS source_kind, \
                    CASE \
                        WHEN tgt_route.route_id IS NOT NULL THEN 'route' \
                        WHEN tgt_infra.node_id IS NOT NULL THEN 'infra_node' \
                        ELSE 'unknown' \
                    END AS target_type, \
                    COALESCE(tgt_infra.name, tgt_route.handler_name) AS target_name, \
                    COALESCE(tgt_infra.kind, 'route') AS target_kind, \
                    tgt_route.route_path AS target_route_path, \
                    tgt_route.method AS target_method, \
                    tgt_route.handler_symbol_uid AS target_handler_symbol_uid \
             FROM infra_edges ie \
             LEFT JOIN infra_nodes src ON ie.source_node_id = src.node_id \
             LEFT JOIN infra_nodes tgt_infra ON ie.target_node_id = tgt_infra.node_id \
             LEFT JOIN route_nodes tgt_route ON ie.target_node_id = tgt_route.route_id \
             WHERE ie.source_node_id IN ({ph}) OR ie.target_node_id IN ({ph})",
            ph = ph_str,
        );
        db.query_json(&sql, &all_ids).map_err(|e| e.to_string())?
    };

    Ok(serde_json::json!({
        "service_or_route": service_or_route,
        "matched_infra_nodes": matched_infra_nodes,
        "matched_routes": matched_routes,
        "related_edges": related_edges,
    }))
}

/// List cross-package call boundaries (architecture violations / coupling).
///
/// Delegates to GraphOps::compute_package_boundaries, truncated to the
/// requested limit.
pub fn list_package_boundaries(
    runtime: SharedCodeIndex,
    limit: u32,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    // Fetch all call edges (same as get_architecture does)
    let all_edges = db.call_uid_edges().map_err(|e| e.to_string())?;

    let mut boundaries =
        crate::engine::compute_package_boundaries(db, &all_edges).map_err(|e| e.to_string())?;
    boundaries.truncate(limit as usize);

    Ok(serde_json::json!({
        "package_boundaries": boundaries,
        "count": boundaries.len(),
    }))
}

/// Discover call flow paths connecting multiple symbols.
#[allow(clippy::too_many_arguments)]
pub fn explore_flow(
    runtime: SharedCodeIndex,
    symbols: &[String],
    max_depth: usize,
    include_source: bool,
    max_paths: Option<usize>,
    exact: Option<bool>,
    file_path: Option<&str>,
    max_candidates: Option<usize>,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let budget = rt.output_budget("explore_flow");
    let db = rt.index_db().ok_or("no index database")?;
    let project_root = rt.project_path.as_deref();
    crate::graph_flow::explore_flow(
        db,
        project_root,
        symbols,
        max_depth,
        include_source,
        max_paths.unwrap_or(3),
        exact.unwrap_or(true),
        file_path,
        max_candidates.unwrap_or(5),
        Some(budget.max_output_chars),
    )
    .map_err(|e| e.to_string())
}

/// Find circular dependencies via Tarjan SCC.
pub fn find_circular_deps(
    runtime: SharedCodeIndex,
    granularity: &str,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let budget = rt.output_budget("circular_deps");
    let db = rt.index_db().ok_or("no index database")?;
    let effective_limit = limit.unwrap_or(budget.max_items);
    let result = crate::graph_cycles::find_circular_deps(db, granularity, effective_limit)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// List all environment variables referenced in the codebase with usage counts.
pub fn list_env_vars(runtime: SharedCodeIndex, limit: usize) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;
    let summary = db.env_var_summary(limit).map_err(|e| e.to_string())?;
    let items: Vec<serde_json::Value> = summary
        .iter()
        .map(|(key, count, files)| {
            serde_json::json!({
                "env_key": key,
                "usage_count": count,
                "files": files.split(',').collect::<Vec<_>>(),
            })
        })
        .collect();
    let total = items.len();
    Ok(serde_json::json!({
        "env_vars": items,
        "total": total,
    }))
}

/// Search for environment variable usages by key pattern.
pub fn search_env_vars(
    runtime: SharedCodeIndex,
    pattern: &str,
    file_path: Option<&str>,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;

    let like_pattern = if pattern.contains('%') || pattern.contains('_') {
        pattern.to_string()
    } else {
        format!("%{}%", pattern)
    };

    let (sql, params): (String, Vec<String>) = if let Some(fp) = file_path {
        (
            format!(
                "SELECT env_key, file_path, line, source_symbol_uid \
                 FROM data_flow_edges \
                 WHERE flow_kind = 'env_access' AND env_key LIKE ?1 AND file_path LIKE ?2 \
                 ORDER BY env_key, file_path \
                 LIMIT {}",
                limit
            ),
            vec![like_pattern, format!("%{}%", fp)],
        )
    } else {
        (
            format!(
                "SELECT env_key, file_path, line, source_symbol_uid \
                 FROM data_flow_edges \
                 WHERE flow_kind = 'env_access' AND env_key LIKE ?1 \
                 ORDER BY env_key, file_path \
                 LIMIT {}",
                limit
            ),
            vec![like_pattern],
        )
    };

    let rows = db.query_json(&sql, &params).map_err(|e| e.to_string())?;
    let total = rows.len();
    Ok(serde_json::json!({
        "pattern": pattern,
        "results": rows,
        "total": total,
    }))
}

/// Show type hierarchy: ancestors, descendants, implementors, overrides.
pub fn type_hierarchy(
    runtime: SharedCodeIndex,
    type_name: &str,
    file_path: Option<&str>,
    symbol_uid: Option<&str>,
    direction: &str,
    max_depth: usize,
    include_methods: bool,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or("no index database")?;
    crate::graph_type_hierarchy::type_hierarchy(
        db,
        type_name,
        file_path,
        symbol_uid,
        direction,
        max_depth,
        include_methods,
    )
    .map_err(|e| e.to_string())
}
