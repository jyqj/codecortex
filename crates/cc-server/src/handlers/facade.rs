//! Facade handlers: composite dispatch for the 14 MCP tools.
//!
//! Byte/item output caps are applied once at the dispatch exit
//! (`handlers::output_budget::finalize`), not inside these handlers.

use super::{context, core, graph, SharedCodeIndex};
use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use serde_json::{json, Value};

// ── 1. handle_status ────────────────────────────────────────────────

pub fn handle_status(runtime: SharedCodeIndex, aspect: &str) -> CcResult<Value> {
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
    db.reads().runtime_evidence_stats().ok()
}

// ── 2. handle_context ───────────────────────────────────────────────

pub fn handle_context(
    runtime: SharedCodeIndex,
    task: &str,
    max_symbols: Option<usize>,
    include_source: bool,
    intent: Option<&str>,
) -> CcResult<Value> {
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
                    &crate::engine_query::ExploreOptions {
                        max_callers: Some(3),
                        max_callees: Some(3),
                        include_source: true,
                        ..Default::default()
                    },
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

pub fn handle_node(runtime: SharedCodeIndex, symbol: &str, include: &str) -> CcResult<Value> {
    let relation_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.repo_size_tier().explore_max_symbols()
    };
    match include {
        "source" => context::get_symbol_source(runtime, symbol, false, true, None),
        "outline" => context::explore_symbols(
            runtime,
            &[symbol.to_string()],
            &crate::engine_query::ExploreOptions {
                include_source: true,
                outline: true,
                ..Default::default()
            },
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
) -> CcResult<Value> {
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

/// Wire arguments of the `impact` tool (already sanitized), grouped so the
/// dispatch chain passes one object instead of nine positional values.
#[derive(Debug, Clone, Default)]
pub struct ImpactArgs {
    pub scope: String,
    pub files: Vec<String>,
    pub base_branch: Option<String>,
    pub granularity: String,
    pub file_path: Option<String>,
    pub limit: usize,
    pub confidence_threshold: Option<f32>,
    pub max_nodes: Option<usize>,
    pub max_per_layer: Option<usize>,
}

pub fn handle_impact(runtime: SharedCodeIndex, args: &ImpactArgs) -> CcResult<Value> {
    let max_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.output_budget("impact").max_items
    };
    let limit = args.limit.min(max_limit);
    let base_branch = args.base_branch.as_deref();
    let file_path = args.file_path.as_deref();
    match args.scope.as_str() {
        "tests" => {
            if args.files.is_empty() {
                let auto_files = core::git_changed_files(runtime.clone(), base_branch)?;
                graph::find_impacted_tests(runtime, &auto_files)
            } else {
                graph::find_impacted_tests(runtime, &args.files)
            }
        }
        "dead_code" => graph::find_dead_code(runtime, file_path, Some(limit)),
        "circular" => graph::find_circular_deps(runtime, &args.granularity, Some(limit)),
        "dependents" => {
            let fp = file_path.ok_or_else(|| {
                CcError::InvalidParams("file_path is required for 'dependents' scope".to_string())
            })?;
            graph::get_dependents(runtime, fp)
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
                args.confidence_threshold.map(f64::from),
                args.max_nodes,
                args.max_per_layer,
            );
            core::analyze_impact(runtime, &args.files, base_branch, &opts)
        }
    }
}

// ── 6. handle_architecture ──────────────────────────────────────────

pub fn handle_architecture(
    runtime: SharedCodeIndex,
    aspect: &str,
    filter: Option<&str>,
    limit: usize,
) -> CcResult<Value> {
    let max_limit = {
        let rt = super::lock_index(&runtime)?;
        rt.output_budget("architecture").max_items
    };
    let limit = limit.min(max_limit);
    match aspect {
        "communities" => core::list_communities(runtime),
        "frameworks" => core::list_frameworks(runtime),
        "routes" => graph::find_route_handlers(runtime, filter, None, None, limit),
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
        _ => graph::get_architecture(runtime, "", limit),
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
) -> CcResult<Value> {
    let required = |value: Option<&str>, name: &str, action: &str| {
        value.map(str::to_string).ok_or_else(|| {
            CcError::InvalidParams(format!("{} is required for '{}' action", name, action))
        })
    };
    let required_line = |value: Option<u32>, name: &str, action: &str| {
        value.ok_or_else(|| {
            CcError::InvalidParams(format!("{} is required for '{}' action", name, action))
        })
    };
    match action {
        "region" => {
            let p = required(path, "path", "region")?;
            let sl = required_line(start_line, "start_line", "region")?;
            let el = required_line(end_line, "end_line", "region")?;
            context::prepare_edit_region(runtime, &p, sl, el)
        }
        "expand" => {
            let p = required(path, "path", "expand")?;
            let sl = required_line(start_line, "start_line", "expand")?;
            let el = required_line(end_line, "end_line", "expand")?;
            context::expand_code_region(runtime, &p, sl, el, context_lines)
        }
        // The "files" item cap (output_budget("files").max_items) is applied
        // at the dispatch exit by output_budget::finalize.
        "list" => core::list_files(runtime),
        other => Err(CcError::InvalidParams(format!(
            "unknown files action {:?}; expected \"list\", \"region\", or \"expand\"",
            other
        ))),
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
    /// Evidence writes that failed and were skipped (one count per failed
    /// write). A single observation keeps processing — and the batch keeps
    /// going — but the client sees that persisted state lags the submission.
    write_errors: u32,
}

impl IngestTraceStats {
    /// Count a failed evidence write without aborting the observation.
    fn record_write_error(&mut self, op: &str, err: &dyn std::fmt::Display) {
        tracing::warn!(op, error = %err, "ingest_traces evidence write failed");
        self.write_errors += 1;
    }
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
) -> CcResult<()> {
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

    match db.writes().upsert_runtime_evidence(
        &eid,
        obs.service,
        obs.method,
        obs.path,
        obs.status,
        now,
    ) {
        Ok(()) => stats.accepted += 1,
        Err(e) => stats.record_write_error("upsert_runtime_evidence", &e),
    }

    if let Some(dur) = obs.duration_ms {
        if let Err(e) = db.writes().update_evidence_p95(&eid, dur) {
            stats.record_write_error("update_evidence_p95", &e);
        }
    }

    let norm = cc_model::route_normalize::normalize_route_path(obs.path);

    let (edge_id, candidate_count) = db.reads().http_edge_match_for_path(&norm, obs.method)?;

    if let Some(ref eid_db) = edge_id {
        // `matched` only counts observations whose key writes (link + boost)
        // both landed, so the client-side count matches persisted state.
        let link_ok = match db.writes().link_evidence_to_edge(&eid, eid_db) {
            Ok(()) => true,
            Err(e) => {
                stats.record_write_error("link_evidence_to_edge", &e);
                false
            }
        };
        let boost_ok = match db.writes().boost_http_edge_confidence(eid_db, 0.15) {
            Ok(()) => true,
            Err(e) => {
                stats.record_write_error("boost_http_edge_confidence", &e);
                false
            }
        };
        if link_ok && boost_ok {
            stats.matched += 1;
        }
        if candidate_count > 1 {
            stats.ambiguous += 1;
        }
    } else {
        stats.unmatched += 1;
    }

    let route_id = db.reads().route_id_for_normalized_path(&norm)?;

    if let Some(ref rid) = route_id {
        // `routes_matched` reports the (read-side) route match; a failed
        // write-back is reflected in `write_errors`.
        if let Err(e) = db.writes().update_evidence_route_id(&eid, rid) {
            stats.record_write_error("update_evidence_route_id", &e);
        }
        stats.routes_matched += 1;
    }

    stats.spans_processed += 1;
    Ok(())
}

/// Upper bound on span observations ingested in a single `ingest_traces` call.
/// The parameter layer caps the number of top-level traces, but each trace may
/// carry an arbitrarily large `spans` array, and every span drives DB writes —
/// so bound the total work here to keep a single call from amplifying into an
/// unbounded write storm.
const MAX_INGEST_OBSERVATIONS: u32 = 100_000;

pub fn handle_ingest_traces(
    runtime: SharedCodeIndex,
    traces: &[serde_json::Value],
) -> CcResult<Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().cloned().ok_or(CcError::IndexUnavailable)?;
    drop(rt);
    let now = chrono::Utc::now().to_rfc3339();
    let mut stats = IngestTraceStats::default();
    let mut truncated = false;

    'ingest: for trace in traces {
        if let Some(spans) = trace.get("spans").and_then(|v| v.as_array()) {
            let service = trace
                .get("resource")
                .and_then(|r| r.get("service_name"))
                .and_then(|v| v.as_str())
                .or_else(|| trace.get("service_name").and_then(|v| v.as_str()))
                .unwrap_or("unknown");

            for span in spans {
                if stats.spans_processed >= MAX_INGEST_OBSERVATIONS {
                    truncated = true;
                    break 'ingest;
                }
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

        if stats.spans_processed >= MAX_INGEST_OBSERVATIONS {
            truncated = true;
            break 'ingest;
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
        "write_errors": stats.write_errors,
        "total_submitted": traces.len(),
        // True when the per-call observation budget was hit and later spans
        // were dropped; callers can resubmit the remainder in a fresh call.
        "truncated": truncated,
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
) -> CcResult<Value> {
    let rt = super::lock_index(&runtime)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;

    let required_id = |action: &str| {
        adr_id.ok_or_else(|| CcError::InvalidParams(format!("adr_id is required for '{}'", action)))
    };

    match action {
        "list" => {
            let records = db.reads().adr_list()?;
            Ok(json!({ "adrs": records }))
        }
        "get" => {
            let id = required_id("get")?;
            let record = db.reads().adr_get(id)?;
            match record {
                Some(v) => Ok(v),
                None => Ok(json!({ "error": format!("ADR '{}' not found", id) })),
            }
        }
        "store" => {
            let id = required_id("store")?;
            let t = title.unwrap_or("Untitled");
            let s = status.unwrap_or("accepted");
            let c = context.unwrap_or("");
            let d = decision.unwrap_or("");
            let now = chrono::Utc::now().to_rfc3339();
            db.writes().adr_upsert(id, t, s, c, d, &now)?;
            Ok(json!({ "stored": id }))
        }
        "delete" => {
            let id = required_id("delete")?;
            let deleted = db.writes().adr_delete(id)?;
            Ok(json!({ "deleted": deleted, "adr_id": id }))
        }
        _ => Err(CcError::InvalidParams(format!(
            "unknown adr action: {}",
            action
        ))),
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
        let err = result.unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        assert!(err.to_string().contains("unknown files action"));
    }

    #[test]
    fn handle_files_region_without_path_returns_error() {
        let (_tmp, rt) = build_test_index();
        let result = handle_files(rt, "region", None, Some(1), Some(5), 20);
        let err = result.unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        assert!(err.to_string().contains("path is required"));
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
            &ImpactArgs {
                scope: "dead_code".into(),
                granularity: "file".into(),
                limit: 20,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.is_object() || result.is_array());
    }

    /// `dependents` scope 在 facade 层强制要求 `file_path`（与 `handle_files`
    /// 的 region/expand 必填校验对称），缺省时返回结构化错误。
    #[test]
    fn handle_impact_dependents_missing_file_path_returns_err() {
        let (_tmp, rt) = build_test_index();
        let result = handle_impact(
            rt,
            &ImpactArgs {
                scope: "dependents".into(),
                granularity: "file".into(),
                limit: 20,
                ..Default::default()
            },
        );
        let err = result.unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        assert!(err.to_string().contains("file_path is required"));
    }

    // ── ingest_traces write-failure accounting ──────────────────────

    /// A matched observation whose key evidence writes (link + boost) fail
    /// must NOT count as matched, and every skipped write lands in
    /// `write_errors`, so the client statistics track persisted state.
    #[test]
    fn handle_ingest_traces_gates_matched_on_writes_and_counts_failures() {
        let tmp = tempfile::TempDir::new().unwrap();
        let idx = crate::engine::CodeIndex::new(Some(tmp.path())).unwrap();
        let db = idx.index_db().unwrap().clone();
        {
            let conn = crate::test_seed::seed_conn(&db);
            conn.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
                 VALUES('src/client.ts','TypeScript','h1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,normalized_path,method,call_kind,line,confidence,parser_tier)
                 VALUES('http_get_users','src/client.ts','caller_uid','/api/users','/api/users','GET','http',20,0.88,'tree_sitter')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier,route_id)
                 VALUES('route_get_users','src/client.ts','/api/users','GET','handler_uid','get_users','express',10,12,'/api/users',0.91,'tree_sitter','route_get_users')",
                [],
            )
            .unwrap();
        }
        let rt: SharedCodeIndex = std::sync::Arc::new(std::sync::RwLock::new(idx));

        let traces = vec![json!({
            "service_name": "svc",
            "path": "/api/users",
            "method": "GET",
            "duration_ms": 12.5,
        })];

        // Healthy run: the observation matches and every write lands.
        let healthy = handle_ingest_traces(rt.clone(), &traces).unwrap();
        assert_eq!(healthy["accepted"], 1);
        assert_eq!(healthy["matched_to_edges"], 1);
        assert_eq!(healthy["routes_matched"], 1);
        assert_eq!(healthy["write_errors"], 0);

        // Break the evidence table: the edge/route matches (reads) still
        // succeed, but every evidence write fails.
        crate::test_seed::seed_conn(&db)
            .execute("DROP TABLE runtime_evidence", [])
            .unwrap();
        let degraded = handle_ingest_traces(rt, &traces).unwrap();
        assert_eq!(degraded["accepted"], 0);
        assert_eq!(
            degraded["matched_to_edges"], 0,
            "matched must require link+boost to land, not just an edge match"
        );
        // upsert + p95 + link + route-id write-back all failed and were counted.
        assert_eq!(degraded["write_errors"], 4);
        // A failing observation still completes (batch semantics preserved).
        assert_eq!(degraded["spans_processed"], 1);
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
