//! Graph domain handlers: graph queries, trace paths, symbol refs, caller/callee graphs,
//! dependents, dead code, references, and route handlers.

use super::SharedCodeIndex;
use cc_model::{CcError, CcResult};

/// Execute a graph query (read-only Cypher subset).
pub fn graph_query(runtime: SharedCodeIndex, query: &str) -> CcResult<serde_json::Value> {
    let (rt, budget) = super::lock_and_budget(&runtime, "graph_query")?;
    let output = rt.graph().graph_query(query)?;

    let default_limit_applied = output.default_limit_applied;
    let limit_applied = output.limit;
    let fast_path = output.fast_path;
    let mut rows = output.rows;
    let has_explicit_limit = query.to_uppercase().contains("LIMIT");

    // Enforce adaptive item limit when the query itself has no explicit LIMIT.
    let budget_truncated = !has_explicit_limit && rows.len() > budget.max_items;
    if budget_truncated {
        rows.truncate(budget.max_items);
    }

    // A default-LIMIT truncation is signalled when no explicit LIMIT was given
    // and the returned rows exactly fill the default limit (likely more exist).
    let default_limit_truncated =
        default_limit_applied && limit_applied.is_some_and(|lim| rows.len() == lim);

    let (truncated, truncated_reason) = if budget_truncated {
        (true, Some("output_budget"))
    } else if default_limit_truncated {
        (true, Some("default_limit"))
    } else {
        (false, None)
    };

    let row_count = rows.len();
    let mut envelope = serde_json::json!({
        "results": rows,
        "row_count": row_count,
        "truncated": truncated,
        "truncated_reason": truncated_reason,
        "limit_applied": limit_applied,
    });
    // Fast-path visibility (ADR-0001): tell the caller which engine served a
    // variable-length traversal and, on fallback, which gate check failed.
    // The value comes straight from the executor's routing decision (carried
    // through GraphQueryOutput), so what's reported is exactly what ran — no
    // second tokenization of the query at the response boundary. Non-traversal
    // queries omit the field entirely (None) — absence means "not applicable",
    // never "fell back".
    if let Some(meta) = fast_path {
        envelope["fast_path"] = meta;
    }
    // Unified explainability envelope (additive): duplicates the legacy
    // truncated/truncated_reason pair in the shared GraphExplain shape so all
    // graph tools expose the same block. Absent when nothing was clipped.
    if let Some(reason) = truncated_reason {
        let mut explain = cc_model::GraphExplainCollector::new();
        explain.mark_truncated(reason);
        if let Some(graph_explain) = explain.finish_non_empty() {
            envelope["graph_explain"] = serde_json::to_value(graph_explain)?;
        }
    }
    Ok(envelope)
}

/// Wire arguments of the `trace` tool (already sanitized), grouped so the
/// dispatch chain passes one object instead of eight positional values.
#[derive(Debug, Clone, Default)]
pub struct TraceArgs {
    pub from: String,
    pub to: String,
    pub max_depth: usize,
    /// Legacy snippet toggle; only consulted when `source_mode` is `None`
    /// (true→"snippet", false→"none").
    pub include_source: bool,
    pub max_snippet_lines: Option<usize>,
    /// `"none"` | `"snippet"` | `"body"` | `"outline"` | None.
    pub source_mode: Option<String>,
    pub from_uid: Option<String>,
    pub to_uid: Option<String>,
}

/// Trace call path between two symbols (rich version with optional snippets).
pub fn trace_path(runtime: SharedCodeIndex, args: &TraceArgs) -> CcResult<serde_json::Value> {
    let (rt, budget) = super::lock_and_budget(&runtime, "trace_path")?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let project_root = rt.project_path.as_deref();

    let effective_mode = args
        .source_mode
        .as_deref()
        .unwrap_or(if args.include_source {
            "snippet"
        } else {
            "none"
        });
    let (do_snippets, snippet_lines, snippet_budget, include_outgoing) = match effective_mode {
        "body" => (true, usize::MAX, 128 * 1024, true),
        "outline" => (false, 0, 0, false),
        "snippet" => (
            true,
            args.max_snippet_lines.unwrap_or(3),
            budget.max_snippet_chars,
            false,
        ),
        _ => (false, 0, 0, false),
    };

    let result = crate::graph_trace::trace_path_rich(
        db,
        project_root,
        &args.from,
        &args.to,
        args.max_depth,
        do_snippets,
        snippet_lines,
        Some(snippet_budget),
        include_outgoing,
        args.from_uid.as_deref(),
        args.to_uid.as_deref(),
    )?;
    Ok(serde_json::to_value(result)?)
}

/// Find references to a symbol.
pub fn symbol_refs(
    runtime: SharedCodeIndex,
    symbol: &str,
    limit: usize,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let refs = rt.graph().symbol_refs(symbol, limit)?;
    Ok(serde_json::to_value(refs)?)
}

pub fn list_unresolved_refs(
    runtime: SharedCodeIndex,
    limit: usize,
    file_path: Option<&str>,
    kind: Option<&str>,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    rt.graph().list_unresolved_refs(limit, file_path, kind)
}

/// Find tests impacted by the given set of files.
pub fn find_impacted_tests(
    runtime: SharedCodeIndex,
    files: &[String],
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let tests = rt.impact().find_impacted_tests(files)?;
    Ok(serde_json::to_value(tests)?)
}

// ── New handlers ────────────────────────────────────────────────────

/// Get files that depend on (import) the given file, including transitive dependents.
///
/// Delegates to `GraphReadModel::dependents_of_file` (cached reverse import
/// adjacency). `file_path` 的校验由上游 params struct 负责，handler 收到已解构的类型化参数。
pub fn get_dependents(runtime: SharedCodeIndex, file_path: &str) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let grm = crate::graph_read_model::GraphReadModel::without_http_bridges(db.clone());
    let dependents = grm.dependents_of_file(file_path)?;

    Ok(serde_json::json!({
        "file_path": file_path,
        "dependents": dependents,
        "count": dependents.len(),
    }))
}

/// Find symbols that appear to be dead code (no incoming callers or references).
///
/// - `scope`: optional file_path 前缀过滤。
/// - `limit`: 用户显式上限，None 时退回 output_budget("dead_code") 的默认项数。
///   查询所有符号后，逐个检查是否有入边或引用。
pub fn find_dead_code(
    runtime: SharedCodeIndex,
    scope: Option<&str>,
    limit: Option<usize>,
) -> CcResult<serde_json::Value> {
    let user_limit = limit;

    let rt = super::lock_index(&runtime)?;
    let budget = rt.output_budget("dead_code");
    let effective_limit = user_limit.unwrap_or(budget.max_items).min(budget.max_items);
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;

    // Adaptive scan limit: 40x the desired dead_code item cap, capped at
    // 5000 — policy owned by `GraphReadModel::dead_code_scan_limit` so direct
    // engine callers share the same default.
    let scan_limit = crate::graph_read_model::GraphReadModel::dead_code_scan_limit(effective_limit);

    let grm = crate::graph_read_model::GraphReadModel::without_http_bridges(db.clone());
    let candidates = grm.dead_code_candidates(scope, scan_limit)?;

    let mut dead_items: Vec<serde_json::Value> = candidates
        .iter()
        .map(|cand| {
            serde_json::json!({
                "symbol_name": cand.name,
                "symbol_uid": cand.uid,
                "file_path": cand.file_path,
                "kind": cand.kind,
                "reason": "no-callers",
            })
        })
        .collect();

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
/// - `aspects`: 逗号分隔（languages, packages, entry_points, routes, hotspots,
///   boundaries, communities），trim 后解析；空串表示返回全部。
/// - `limit`: 各 aspect 返回上限。
pub fn get_architecture(
    runtime: SharedCodeIndex,
    aspects: &str,
    limit: usize,
) -> CcResult<serde_json::Value> {
    let aspects_str = aspects.trim().to_string();

    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;

    // Parse aspects list; empty means all
    let aspect_vec: Vec<&str> = if aspects_str.is_empty() {
        vec![]
    } else {
        aspects_str.split(',').map(|s| s.trim()).collect()
    };

    let info = db.reads().get_architecture_info(&aspect_vec, limit)?;

    Ok(serde_json::to_value(info)?)
}

/// Find HTTP route handlers matching a pattern.
///
/// - `route_path`: 可选 LIKE 子串模式。
/// - `method` / `framework`: 可选大小写无关过滤。
/// - `limit`: 返回上限。
///
/// Delegates to `GraphReadModel::route_handlers`; this handler only shapes the
/// output rows.
pub fn find_route_handlers(
    runtime: SharedCodeIndex,
    route_path: Option<&str>,
    method: Option<&str>,
    framework: Option<&str>,
    limit: usize,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let grm = crate::graph_read_model::GraphReadModel::without_http_bridges(db.clone());
    let rows = grm.route_handlers(route_path, method, framework, limit)?;

    let shaped: Vec<serde_json::Value> = rows
        .into_iter()
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
        "route_handlers": shaped,
        "count": shaped.len(),
    }))
}

/// Find consumers of a topic or queue.
///
/// Delegates to `GraphReadModel::async_consumers` (infra_edges with kind IN
/// ('binds_topic', 'consumes_queue') joined to infra_nodes/routes).
pub fn find_async_consumers(
    runtime: SharedCodeIndex,
    topic_or_queue: &str,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let grm = crate::graph_read_model::GraphReadModel::without_http_bridges(db.clone());
    let rows = grm.async_consumers(topic_or_queue)?;

    Ok(serde_json::json!({
        "topic_or_queue": topic_or_queue,
        "consumers": rows,
        "count": rows.len(),
    }))
}

/// Find infrastructure bindings for a service or route.
///
/// Delegates to `GraphReadModel::service_bindings` (infra node + route match
/// dimensions plus connecting edges).
pub fn find_service_bindings(
    runtime: SharedCodeIndex,
    service_or_route: &str,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let grm = crate::graph_read_model::GraphReadModel::without_http_bridges(db.clone());
    let bindings = grm.service_bindings(service_or_route)?;

    Ok(serde_json::json!({
        "service_or_route": service_or_route,
        "matched_infra_nodes": bindings.matched_infra_nodes,
        "matched_routes": bindings.matched_routes,
        "related_edges": bindings.related_edges,
    }))
}

/// List cross-package call boundaries (architecture violations / coupling).
///
/// Delegates to GraphOps::compute_package_boundaries, truncated to the
/// requested limit.
pub fn list_package_boundaries(
    runtime: SharedCodeIndex,
    limit: u32,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;

    let mut boundaries = crate::engine::compute_package_boundaries(db)?;
    boundaries.truncate(limit as usize);

    Ok(serde_json::json!({
        "package_boundaries": boundaries,
        "count": boundaries.len(),
    }))
}

/// Wire arguments of the `explore` tool's flow mode (already sanitized),
/// beyond the runtime handle and symbol list.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlowArgs<'a> {
    pub max_depth: usize,
    pub include_source: bool,
    pub max_paths: Option<usize>,
    pub exact: Option<bool>,
    pub file_path: Option<&'a str>,
    pub max_candidates: Option<usize>,
}

/// Discover call flow paths connecting multiple symbols.
pub fn explore_flow(
    runtime: SharedCodeIndex,
    symbols: &[String],
    args: &FlowArgs<'_>,
) -> CcResult<serde_json::Value> {
    let (rt, budget) = super::lock_and_budget(&runtime, "explore_flow")?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let project_root = rt.project_path.as_deref();
    crate::graph_flow::explore_flow(
        db,
        project_root,
        symbols,
        &crate::graph_flow::FlowOptions {
            max_depth: args.max_depth,
            include_source: args.include_source,
            max_paths_per_pair: args.max_paths.unwrap_or(3),
            exact: args.exact.unwrap_or(true),
            file_path_filter: args.file_path,
            max_candidates: args.max_candidates.unwrap_or(5),
            max_output_chars: Some(budget.max_output_chars),
        },
    )
}

/// Find circular dependencies via Tarjan SCC.
pub fn find_circular_deps(
    runtime: SharedCodeIndex,
    granularity: &str,
    limit: Option<usize>,
) -> CcResult<serde_json::Value> {
    let (rt, budget) = super::lock_and_budget(&runtime, "circular_deps")?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let effective_limit = limit.unwrap_or(budget.max_items);
    let result = crate::graph_cycles::find_circular_deps(db, granularity, effective_limit)?;
    Ok(serde_json::to_value(result)?)
}

/// List all environment variables referenced in the codebase with usage counts.
pub fn list_env_vars(runtime: SharedCodeIndex, limit: usize) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let summary = db.reads().env_var_summary(limit)?;
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
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;

    let like_pattern = if pattern.contains('%') || pattern.contains('_') {
        pattern.to_string()
    } else {
        format!("%{}%", pattern)
    };

    let file_pattern = file_path.map(|fp| format!("%{}%", fp));
    let rows = db
        .reads()
        .env_access_rows(&like_pattern, file_pattern.as_deref(), limit)?;
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
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let grm = crate::graph_read_model::GraphReadModel::without_http_bridges(db.clone());
    crate::graph_type_hierarchy::type_hierarchy(
        db,
        &grm,
        type_name,
        file_path,
        symbol_uid,
        direction,
        max_depth,
        include_methods,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn graph_query_returns_truncation_envelope() {
        let (_tmp, rt) = build_test_index();
        let result = graph_query(rt, "MATCH (f:Function) RETURN f.name").unwrap();

        // Envelope shape: results array + truncation signals.
        assert!(result.get("results").unwrap().is_array());
        assert!(result.get("row_count").unwrap().is_u64());
        assert!(result.get("truncated").unwrap().is_boolean());
        assert!(result.get("truncated_reason").is_some());
        // No explicit LIMIT → default limit of 50 reported.
        assert_eq!(result.get("limit_applied").unwrap().as_u64(), Some(50));
    }

    #[test]
    fn graph_query_emits_fast_path_metadata_for_varlen_queries() {
        let (_tmp, rt) = build_test_index();

        // Non-traversal query: the field is omitted entirely (not applicable).
        let plain = graph_query(rt.clone(), "MATCH (f:Function) RETURN f.name").unwrap();
        assert!(plain.get("fast_path").is_none());

        // Eligible variable-length traversal: served by the lazy BFS.
        let eligible = graph_query(
            rt.clone(),
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'main' RETURN b.name",
        )
        .unwrap();
        assert_eq!(eligible["fast_path"]["used"].as_bool(), Some(true));
        assert!(eligible["fast_path"].get("reason").is_none());

        // Ineligible variable-length traversal: SQL CTE with a stable reason.
        let fallback = graph_query(rt, "MATCH (a)-[:CALLS*1..2]->(b) RETURN b.name").unwrap();
        assert_eq!(fallback["fast_path"]["used"].as_bool(), Some(false));
        assert_eq!(
            fallback["fast_path"]["reason"].as_str(),
            Some("no_where_clause")
        );
    }

    #[test]
    fn graph_query_explicit_limit_not_default_truncated() {
        let (_tmp, rt) = build_test_index();
        let result = graph_query(rt, "MATCH (f:Function) RETURN f.name LIMIT 1").unwrap();

        assert!(result.get("results").unwrap().is_array());
        assert_eq!(result.get("limit_applied").unwrap().as_u64(), Some(1));
        // An explicit LIMIT must never be reported as a default_limit truncation.
        assert_ne!(
            result.get("truncated_reason").unwrap().as_str(),
            Some("default_limit")
        );
    }

    #[test]
    fn graph_query_attaches_graph_explain_on_truncation() {
        // 60 functions on a Tiny-tier index: the engine's default LIMIT (50)
        // exceeds the output budget (15 items), so the adaptive budget clips
        // the rows and the unified envelope must mirror the legacy fields.
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/many.ts");
        for idx in 0..60 {
            insert_symbol(
                &db,
                &format!("uid_fn{idx}"),
                &format!("fn_{idx}"),
                "function",
                "src/many.ts",
            );
        }

        let result = graph_query(rt, "MATCH (f:Function) RETURN f.name").unwrap();
        assert_eq!(result["truncated"].as_bool(), Some(true));
        assert_eq!(result["truncated_reason"].as_str(), Some("output_budget"));
        assert_eq!(result["graph_explain"]["truncated"].as_bool(), Some(true));
        assert_eq!(
            result["graph_explain"]["truncated_reason"].as_str(),
            Some("output_budget")
        );
    }

    #[test]
    fn graph_query_omits_graph_explain_when_not_truncated() {
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/few.ts");
        insert_symbol(&db, "uid_only", "only_fn", "function", "src/few.ts");

        let result = graph_query(rt, "MATCH (f:Function) RETURN f.name LIMIT 5").unwrap();
        assert_eq!(result["truncated"].as_bool(), Some(false));
        assert!(result.get("graph_explain").is_none());
    }

    #[test]
    fn trace_path_handler_reports_max_depth_truncation() {
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/chain.ts");
        insert_symbol(&db, "uid_alpha", "alpha_fn", "function", "src/chain.ts");
        insert_symbol(&db, "uid_beta", "beta_fn", "function", "src/chain.ts");
        insert_symbol(&db, "uid_gamma", "gamma_fn", "function", "src/chain.ts");
        insert_call_edge(&db, "ce_ab", "src/chain.ts", Some("uid_alpha"), "uid_beta");
        insert_call_edge(&db, "ce_bg", "src/chain.ts", Some("uid_beta"), "uid_gamma");

        // alpha→beta→gamma needs depth 2; max_depth=1 clips the walk, and the
        // response must say so instead of looking like "no path exists".
        let result = trace_path(
            rt,
            &TraceArgs {
                from: "alpha_fn".into(),
                to: "gamma_fn".into(),
                max_depth: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result["path_count"].as_u64(), Some(0));
        assert_eq!(result["graph_explain"]["truncated"].as_bool(), Some(true));
        assert_eq!(
            result["graph_explain"]["truncated_reason"].as_str(),
            Some("max_depth")
        );
        // The declared graph subset rides along whenever the envelope is
        // attached (contract: trace consults CALLS + cross-service bridges).
        assert_eq!(
            result["graph_explain"]["declared_edge_kinds"],
            serde_json::json!(cc_model::graph_catalog::tool_graph_subsets::TRACE.kinds())
        );
    }

    // ── Characterization tests for the 5 raw-SQL graph tools ─────────
    //
    // These pin the current JSON output of get_dependents / find_dead_code /
    // find_route_handlers / find_async_consumers / find_service_bindings on a
    // synthetic index DB so the GraphReadModel collapse cannot change behavior.

    fn synthetic_runtime() -> (
        tempfile::TempDir,
        SharedCodeIndex,
        std::sync::Arc<cc_db::index_db::IndexDb>,
    ) {
        let tmp = tempfile::TempDir::new().unwrap();
        let idx = crate::engine::CodeIndex::new(Some(tmp.path())).unwrap();
        let db = idx.index_db().expect("index db opened").clone();
        // No per-test generation marker needed: the process-unique
        // IndexDb::instance_id keys the process-global graph caches.
        (tmp, std::sync::Arc::new(std::sync::RwLock::new(idx)), db)
    }

    fn insert_file(db: &cc_db::index_db::IndexDb, file_path: &str) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
                 VALUES(?1,'TypeScript',?2,1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                rusqlite::params![file_path, format!("hash:{file_path}")],
            )
            .unwrap();
    }

    fn insert_import(db: &cc_db::index_db::IndexDb, file_path: &str, resolved_path: &str) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO imports(file_path, import_string, resolved_path) VALUES(?1, ?2, ?3)",
                rusqlite::params![file_path, format!("import:{resolved_path}"), resolved_path],
            )
            .unwrap();
    }

    fn insert_symbol(
        db: &cc_db::index_db::IndexDb,
        uid: &str,
        name: &str,
        kind: &str,
        file_path: &str,
    ) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, start_line, end_line)
                 VALUES(?1, ?2, ?3, ?4, ?5, 1, 10)",
                rusqlite::params![format!("sid:{uid}"), uid, name, kind, file_path],
            )
            .unwrap();
    }

    fn insert_call_edge(
        db: &cc_db::index_db::IndexDb,
        edge_id: &str,
        file_path: &str,
        caller_uid: Option<&str>,
        callee_uid: &str,
    ) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid)
                 VALUES(?1, ?2, 'callee', 5, ?3, ?4)",
                rusqlite::params![edge_id, file_path, caller_uid, callee_uid],
            )
            .unwrap();
    }

    fn insert_symbol_ref(
        db: &cc_db::index_db::IndexDb,
        ref_id: &str,
        file_path: &str,
        target_uid: &str,
        container: Option<&str>,
    ) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO symbol_refs(ref_id, file_path, symbol_name, container, ref_kind, line, target_symbol_uid)
                 VALUES(?1, ?2, 'ref', ?3, 'call', 7, ?4)",
                rusqlite::params![ref_id, file_path, container, target_uid],
            )
            .unwrap();
    }

    fn insert_route(
        db: &cc_db::index_db::IndexDb,
        edge_id: &str,
        file_path: &str,
        route_path: &str,
        method: &str,
        handler_name: &str,
        framework: &str,
    ) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO routes(edge_id, file_path, route_path, method, handler_name, framework, line, normalized_path, route_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 10, ?3, ?1)",
                rusqlite::params![edge_id, file_path, route_path, method, handler_name, framework],
            )
            .unwrap();
    }

    fn insert_infra_node(
        db: &cc_db::index_db::IndexDb,
        node_id: &str,
        file_path: &str,
        kind: &str,
        name: &str,
    ) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO infra_nodes(node_id, file_path, kind, name) VALUES(?1, ?2, ?3, ?4)",
                rusqlite::params![node_id, file_path, kind, name],
            )
            .unwrap();
    }

    fn insert_infra_edge(
        db: &cc_db::index_db::IndexDb,
        edge_id: &str,
        source_node_id: &str,
        target_node_id: &str,
        kind: &str,
        properties: &str,
    ) {
        crate::test_seed::seed_conn(db)
            .execute(
                "INSERT INTO infra_edges(edge_id, source_node_id, target_node_id, kind, properties)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![edge_id, source_node_id, target_node_id, kind, properties],
            )
            .unwrap();
    }

    #[test]
    fn dependents_direct_and_two_hop_transitive_sorted() {
        let (_tmp, rt, db) = synthetic_runtime();
        for file in ["src/a.ts", "src/b.ts", "src/c.ts", "src/d.ts"] {
            insert_file(&db, file);
        }
        insert_import(&db, "src/a.ts", "src/a.ts"); // self-import must be excluded
        insert_import(&db, "src/b.ts", "src/a.ts"); // direct dependent
        insert_import(&db, "src/c.ts", "src/b.ts"); // 2-hop transitive dependent
        insert_import(&db, "src/d.ts", "src/c.ts"); // 3 hops away: out of reach

        let result = get_dependents(rt, "src/a.ts").unwrap();

        assert_eq!(
            result.get("dependents").unwrap(),
            &serde_json::json!(["src/b.ts", "src/c.ts"])
        );
        assert_eq!(result.get("count").unwrap().as_u64(), Some(2));
        assert_eq!(result.get("file_path").unwrap().as_str(), Some("src/a.ts"));
    }

    #[test]
    fn dead_code_pins_caller_ref_and_exclusion_filters() {
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/app.ts");
        insert_file(&db, "lib/util.ts");

        // Dead: no callers, no refs.
        insert_symbol(&db, "uid_orphan", "orphan", "function", "src/app.ts");
        // Alive: incoming call edge from another symbol.
        insert_symbol(&db, "uid_used", "used_fn", "function", "src/app.ts");
        insert_call_edge(&db, "ce_used", "src/app.ts", Some("uid_orphan"), "uid_used");
        // Dead: only a self call edge (caller == callee).
        insert_symbol(&db, "uid_selfcall", "self_call", "function", "src/app.ts");
        insert_call_edge(
            &db,
            "ce_self",
            "src/app.ts",
            Some("uid_selfcall"),
            "uid_selfcall",
        );
        // Excluded by name / prefix even with no callers.
        insert_symbol(&db, "uid_main", "main", "function", "src/app.ts");
        insert_symbol(&db, "uid_th", "test_helper", "function", "src/app.ts");
        // Dead: only a self reference (container == own name).
        insert_symbol(&db, "uid_selfref", "self_ref", "function", "src/app.ts");
        insert_symbol_ref(
            &db,
            "sr_self",
            "src/app.ts",
            "uid_selfref",
            Some("self_ref"),
        );
        // Alive: an external reference (container differs from own name).
        insert_symbol(&db, "uid_extref", "ext_ref", "function", "src/app.ts");
        insert_symbol_ref(&db, "sr_ext", "src/app.ts", "uid_extref", Some("caller_fn"));
        // Dead, but outside the "src/" scope filter.
        insert_symbol(&db, "uid_oos", "out_of_scope", "function", "lib/util.ts");

        let scoped = find_dead_code(rt.clone(), Some("src/"), None).unwrap();
        let scoped_names: std::collections::HashSet<String> = scoped["dead_code"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["symbol_name"].as_str().unwrap().to_string())
            .collect();
        let expected: std::collections::HashSet<String> = ["orphan", "self_call", "self_ref"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert_eq!(scoped_names, expected);
        assert_eq!(scoped["count"].as_u64(), Some(3));
        assert_eq!(scoped["truncated"].as_bool(), Some(false));
        for item in scoped["dead_code"].as_array().unwrap() {
            assert_eq!(item["reason"].as_str(), Some("no-callers"));
            assert_eq!(item["kind"].as_str(), Some("function"));
        }

        let unscoped = find_dead_code(rt, None, None).unwrap();
        let unscoped_names: std::collections::HashSet<String> = unscoped["dead_code"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["symbol_name"].as_str().unwrap().to_string())
            .collect();
        assert!(unscoped_names.contains("out_of_scope"));
        assert_eq!(unscoped_names.len(), 4);
    }

    #[test]
    fn find_dead_code_scan_limit_matches_read_model_default() {
        let (_tmp, rt, _db) = synthetic_runtime();

        // Default path: effective limit = output_budget("dead_code").max_items,
        // and the reported scan_limit must equal the GraphReadModel policy for
        // that cap (handler path == direct-engine path).
        let expected_default = {
            let guard = super::super::lock_index(&rt).unwrap();
            let budget = guard.output_budget("dead_code");
            crate::graph_read_model::GraphReadModel::dead_code_scan_limit(budget.max_items)
        };
        let result = find_dead_code(rt.clone(), None, None).unwrap();
        assert_eq!(result["scan_limit"].as_u64(), Some(expected_default as u64));

        // Explicit limit param: 1 × 40 = 40 (well below the 5000 ceiling).
        let result = find_dead_code(rt, None, Some(1)).unwrap();
        assert_eq!(result["scan_limit"].as_u64(), Some(40));
    }

    #[test]
    fn route_handlers_pin_pattern_method_and_framework_filters() {
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/routes.ts");
        insert_route(
            &db,
            "r1",
            "src/routes.ts",
            "/api/users",
            "GET",
            "getUsers",
            "express",
        );
        insert_route(
            &db,
            "r2",
            "src/routes.ts",
            "/api/users",
            "POST",
            "createUser",
            "express",
        );
        insert_route(
            &db,
            "r3",
            "src/routes.ts",
            "/admin",
            "GET",
            "adminHome",
            "fastify",
        );

        let all = find_route_handlers(rt.clone(), None, None, None, 20).unwrap();
        assert_eq!(all["count"].as_u64(), Some(3));

        // Pattern is substring LIKE; method/framework filters are case-insensitive.
        let filtered =
            find_route_handlers(rt, Some("users"), Some("get"), Some("EXPRESS"), 20).unwrap();
        assert_eq!(filtered["count"].as_u64(), Some(1));
        let row = &filtered["route_handlers"][0];
        assert_eq!(row["handler"].as_str(), Some("getUsers"));
        assert_eq!(row["method"].as_str(), Some("GET"));
        assert_eq!(row["route_path"].as_str(), Some("/api/users"));
        assert_eq!(row["framework"].as_str(), Some("express"));
        assert_eq!(row["file_path"].as_str(), Some("src/routes.ts"));
        assert_eq!(row["line"].as_u64(), Some(10));
    }

    #[test]
    fn async_consumers_pin_kind_filter_and_target_resolution() {
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/consumer.ts");
        insert_file(&db, "src/routes.ts");
        insert_infra_node(
            &db,
            "n_consumer",
            "src/consumer.ts",
            "consumer",
            "orders-consumer",
        );
        insert_infra_node(&db, "n_queue", "src/consumer.ts", "queue", "orders-queue");
        insert_infra_node(
            &db,
            "n_other",
            "src/consumer.ts",
            "consumer",
            "payments-svc",
        );
        insert_route(
            &db,
            "route_pay",
            "src/routes.ts",
            "/orders/submit",
            "POST",
            "handleOrder",
            "express",
        );

        // Matched: kind in (binds_topic, consumes_queue) and src.name LIKE %orders%.
        insert_infra_edge(&db, "e1", "n_consumer", "n_queue", "consumes_queue", "{}");
        // Matched via properties LIKE even though target is a route.
        insert_infra_edge(
            &db,
            "e2",
            "n_other",
            "route_pay",
            "binds_topic",
            "{\"topic\":\"orders\"}",
        );
        // Excluded: wrong edge kind.
        insert_infra_edge(&db, "e3", "n_consumer", "n_queue", "deploys", "{}");
        // Excluded: neither source name nor properties match.
        insert_infra_edge(&db, "e4", "n_other", "n_queue", "consumes_queue", "{}");

        let result = find_async_consumers(rt, "orders").unwrap();
        assert_eq!(result["count"].as_u64(), Some(2));
        assert_eq!(result["topic_or_queue"].as_str(), Some("orders"));

        let consumers = result["consumers"].as_array().unwrap();
        let by_edge = |id: &str| {
            consumers
                .iter()
                .find(|row| row["edge_id"].as_str() == Some(id))
                .unwrap_or_else(|| panic!("edge {id} missing"))
        };
        let infra_target = by_edge("e1");
        assert_eq!(infra_target["target_type"].as_str(), Some("infra_node"));
        assert_eq!(infra_target["target_name"].as_str(), Some("orders-queue"));
        assert_eq!(
            infra_target["source_name"].as_str(),
            Some("orders-consumer")
        );
        let route_target = by_edge("e2");
        assert_eq!(route_target["target_type"].as_str(), Some("route"));
        assert_eq!(route_target["target_name"].as_str(), Some("handleOrder"));
        assert_eq!(
            route_target["target_route_path"].as_str(),
            Some("/orders/submit")
        );
    }

    #[test]
    fn service_bindings_pin_two_dimension_match_and_related_edges() {
        let (_tmp, rt, db) = synthetic_runtime();
        insert_file(&db, "src/infra.ts");
        insert_file(&db, "src/routes.ts");
        insert_infra_node(&db, "n_pay", "src/infra.ts", "service", "payment-service");
        insert_infra_node(&db, "n_db", "src/infra.ts", "database", "postgres-main");
        insert_route(
            &db,
            "route_pay",
            "src/routes.ts",
            "/payments",
            "POST",
            "payHandler",
            "express",
        );
        insert_route(
            &db,
            "route_health",
            "src/routes.ts",
            "/health",
            "GET",
            "healthHandler",
            "express",
        );
        // Related: touches a matched infra node and a matched route.
        insert_infra_edge(&db, "e_bind", "n_pay", "route_pay", "exposes_route", "{}");
        // Unrelated: touches neither matched id.
        insert_infra_edge(
            &db,
            "e_other",
            "n_db",
            "route_health",
            "exposes_route",
            "{}",
        );

        let result = find_service_bindings(rt, "payment").unwrap();

        let nodes = result["matched_infra_nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["name"].as_str(), Some("payment-service"));

        let routes = result["matched_routes"].as_array().unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0]["route_path"].as_str(), Some("/payments"));
        assert_eq!(routes[0]["handler_name"].as_str(), Some("payHandler"));

        let edges = result["related_edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["edge_id"].as_str(), Some("e_bind"));
        assert_eq!(edges[0]["source_name"].as_str(), Some("payment-service"));
        assert_eq!(edges[0]["target_type"].as_str(), Some("route"));
        assert_eq!(edges[0]["target_route_path"].as_str(), Some("/payments"));
    }
}
