//! Lazy-BFS fast path for variable-length `CALLS` traversals.
//!
//! Benchmark evidence (50k symbols / 200k call_edges, release):
//! the `WITH RECURSIVE` CTE in `translate_variable_length` costs 7-71 ms p50,
//! while a lazy per-node BFS over `IndexDb::call_edges_from_uid_lite` costs
//! 0.11-0.28 ms p50 for LIMIT-50 queries. This module implements that BFS for
//! a narrow, conservatively-gated query shape and mirrors the CTE's observable
//! semantics exactly:
//!
//! - The CTE deduplicates `(root_uid, uid, depth)` tuples via `UNION`, so a
//!   node reachable at several depths yields one tuple *per depth* (a cycle
//!   can re-reach the root at depth >= 1). The BFS therefore keys its visited
//!   set on `(root, uid, depth)` — implemented as a per-level node set — and
//!   keeps expanding re-reached nodes at later depths, unlike a plain
//!   visited-node BFS.
//! - The outer query INNER JOINs `symbols` on both endpoints, applies the
//!   destination label kind filter and destination WHERE equalities, projects
//!   with `SELECT DISTINCT`, then applies `depth >= min` and LIMIT (default
//!   50). Source label kind filters are NOT applied by the SQL path (only the
//!   seed WHERE pins the source); the BFS mirrors that quirk.
//! - Traversal continues through callee UIDs that have no `symbols` row (the
//!   CTE recursion runs on the edge table alone; the symbols JOIN only gates
//!   output).
//!
//! Anything outside the gated shape falls back to the SQL CTE untouched.

use super::ast::*;
use super::executor::{label_kind_filter, label_table, DEFAULT_CYPHER_LIMIT};
use cc_db::index_db::IndexDb;
use cc_model::CcResult;
use serde_json::Value as JsonValue;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Environment toggle: `CODECORTEX_CYPHER_FAST_PATH=0` disables the fast path.
/// Anything else (including unset) leaves it enabled.
pub(crate) fn env_enabled() -> bool {
    env_flag(std::env::var("CODECORTEX_CYPHER_FAST_PATH").ok().as_deref())
}

fn env_flag(value: Option<&str>) -> bool {
    value != Some("0")
}

/// Largest LIMIT the fast path will serve. The lazy BFS wins decisively for
/// small-LIMIT traversals (early termination); for huge limits the full
/// enumeration plus per-node lookups merely ties the CTE, so stay out.
const MAX_FAST_PATH_LIMIT: usize = 1000;

/// Chunk size for batched `symbol_uid IN (...)` resolution.
const SYMBOL_BATCH: usize = 400;

/// Columns the SQL path can project from `symbols` (mirrors `table_json_expr`).
const SYMBOL_COLUMNS: &[&str] = &[
    "symbol_id",
    "symbol_uid",
    "name",
    "kind",
    "file_path",
    "container",
    "start_line",
    "end_line",
    "qname",
    "signature",
];

struct Projection {
    var: String,
    prop: String,
    from_src: bool,
    alias: Option<String>,
    output_name: String,
}

struct FastPlan {
    min_hops: usize,
    max_hops: usize,
    /// Source-side equality conditions on `symbols` columns — the CTE seed WHERE.
    seed_conds: Vec<(&'static str, String)>,
    /// Destination-side equality conditions, applied to each output tuple.
    dst_conds: Vec<(&'static str, String)>,
    /// Destination label kind filter (e.g. `:Function` → kind = 'function').
    dst_kind: Option<&'static str>,
    projections: Vec<Projection>,
    /// (projection index, descending) pairs, in ORDER BY order.
    order_keys: Vec<(usize, bool)>,
    limit: usize,
}

/// Attempt the lazy-BFS fast path. Returns `Ok(None)` when the query is not
/// eligible (the caller then runs the SQL CTE path unchanged).
pub(crate) fn try_execute(query: &CypherQuery, db: &IndexDb) -> CcResult<Option<CypherResult>> {
    let plan = match build_plan(query) {
        Ok(plan) => plan,
        Err(reason) => {
            tracing::debug!(reason, "cypher varlen fast path fallback to SQL CTE");
            return Ok(None);
        }
    };
    let result = run_bfs(query, &plan, db)?;
    tracing::debug!(
        rows = result.row_count,
        "cypher varlen fast path taken (lazy BFS over call_edges)"
    );
    Ok(Some(result))
}

// ── Eligibility gate ───────────────────────────────

/// Conservative gate: accept ONLY the narrow hot shape
/// `MATCH (a)-[:CALLS*m..n]->(b) WHERE <AND of name/symbol_uid string
/// equalities, at least one on a> RETURN <simple symbol properties of a/b>
/// [ORDER BY returned properties] [LIMIT k]`.
fn build_plan(query: &CypherQuery) -> Result<FastPlan, &'static str> {
    if query.match_clauses.len() != 1 {
        return Err("multiple MATCH clauses");
    }
    let match_clause = &query.match_clauses[0];
    if match_clause.is_optional {
        return Err("OPTIONAL MATCH");
    }
    if match_clause.patterns.len() != 1 {
        return Err("multiple patterns in MATCH");
    }
    let pattern = &match_clause.patterns[0];
    if pattern.nodes.len() != 2 || pattern.rels.len() != 1 {
        return Err("not a single relationship segment");
    }
    let rel = &pattern.rels[0];
    if rel.min_hops == 1 && rel.max_hops == 1 {
        return Err("single-hop pattern (not variable-length)");
    }
    if rel.rel_type.as_deref().unwrap_or("CALLS") != "CALLS" {
        return Err("edge kind is not CALLS");
    }
    // The SQL path currently ignores direction for variable-length paths
    // (always traverses caller -> callee); rather than bake that quirk in,
    // only take cleanly forward patterns and leave the rest to SQL.
    if rel.direction != RelDirection::Outgoing {
        return Err("non-outgoing direction");
    }

    let src_node = &pattern.nodes[0];
    let dst_node = &pattern.nodes[1];
    // Inline props are ignored by translate_variable_length; stay out of
    // that surprising territory entirely.
    if !src_node.props.is_empty() || !dst_node.props.is_empty() {
        return Err("inline node properties");
    }
    for node in [src_node, dst_node] {
        if let Some(label) = node.label.as_deref() {
            if label_table(label) != "symbols" {
                return Err("node label not backed by the symbols table");
            }
        }
    }
    // Same default aliases as translate_variable_length.
    let src_var = src_node.var.as_deref().unwrap_or("src");
    let dst_var = dst_node.var.as_deref().unwrap_or("dst");
    if src_var == dst_var {
        return Err("same variable on both endpoints");
    }

    let where_clause = query
        .where_clause
        .as_ref()
        .ok_or("no WHERE clause to pin the seed")?;
    let mut seed_conds = Vec::new();
    let mut dst_conds = Vec::new();
    collect_eq_conds(
        &where_clause.expr,
        src_var,
        dst_var,
        &mut seed_conds,
        &mut dst_conds,
    )?;
    if seed_conds.is_empty() {
        return Err("no source-side equality to pin the seed");
    }

    if query.return_clause.items.is_empty() {
        return Err("empty RETURN clause");
    }
    let mut projections = Vec::with_capacity(query.return_clause.items.len());
    for item in &query.return_clause.items {
        let ReturnItem::Prop(prop_ref, alias) = item else {
            return Err("RETURN item is not a simple property");
        };
        let from_src = if prop_ref.var == src_var {
            true
        } else if prop_ref.var == dst_var {
            false
        } else {
            return Err("RETURN references a variable outside the pattern");
        };
        if !SYMBOL_COLUMNS.contains(&prop_ref.prop.as_str()) {
            return Err("RETURN property outside the symbols projection set");
        }
        projections.push(Projection {
            var: prop_ref.var.clone(),
            prop: prop_ref.prop.clone(),
            from_src,
            alias: alias.clone(),
            output_name: alias
                .clone()
                .unwrap_or_else(|| format!("{}.{}", prop_ref.var, prop_ref.prop)),
        });
    }

    let mut order_keys = Vec::new();
    if let Some(order_items) = &query.order_by {
        for item in order_items {
            let idx = match &item.expr {
                OrderExpr::Alias(name) => projections
                    .iter()
                    .position(|p| p.alias.as_deref() == Some(name.as_str())),
                OrderExpr::Prop(pr) => projections
                    .iter()
                    .position(|p| p.var == pr.var && p.prop == pr.prop),
            }
            .ok_or("ORDER BY is not on a returned property")?;
            order_keys.push((idx, item.desc));
        }
    }

    let limit = query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT);
    if limit > MAX_FAST_PATH_LIMIT {
        return Err("LIMIT too large for the fast path");
    }

    Ok(FastPlan {
        min_hops: rel.min_hops,
        max_hops: rel.max_hops,
        seed_conds,
        dst_conds,
        dst_kind: dst_node.label.as_deref().and_then(label_kind_filter),
        projections,
        order_keys,
        limit,
    })
}

/// Flatten an AND-tree of `var.prop = 'string'` equalities on
/// `name`/`symbol_uid`, splitting by endpoint variable exactly like
/// `split_where_by_var` does for the SQL path. Anything else is rejected.
fn collect_eq_conds(
    expr: &Expr,
    src_var: &str,
    dst_var: &str,
    seed_conds: &mut Vec<(&'static str, String)>,
    dst_conds: &mut Vec<(&'static str, String)>,
) -> Result<(), &'static str> {
    match expr {
        Expr::And(left, right) => {
            collect_eq_conds(left, src_var, dst_var, seed_conds, dst_conds)?;
            collect_eq_conds(right, src_var, dst_var, seed_conds, dst_conds)
        }
        Expr::Comparison {
            left,
            op: CmpOp::Eq,
            right: Value::String(value),
        } => {
            let column: &'static str = match left.prop.as_str() {
                "name" => "name",
                "symbol_uid" => "symbol_uid",
                _ => return Err("equality on a property other than name/symbol_uid"),
            };
            if left.var == src_var {
                seed_conds.push((column, value.clone()));
                Ok(())
            } else if left.var == dst_var {
                dst_conds.push((column, value.clone()));
                Ok(())
            } else {
                Err("WHERE references a variable outside the pattern")
            }
        }
        _ => Err("WHERE predicate is not a simple string equality"),
    }
}

// ── BFS execution ──────────────────────────────────

/// Per-query memo of symbol rows and outgoing call edges, so cycles and
/// diamonds never re-query the same node.
struct QueryMemo {
    neighbors: HashMap<String, Vec<String>>,
    symbols: HashMap<String, Option<serde_json::Map<String, JsonValue>>>,
}

fn run_bfs(query: &CypherQuery, plan: &FastPlan, db: &IndexDb) -> CcResult<CypherResult> {
    // Seed exactly like the CTE base case: every symbols row matching the
    // source-side WHERE conjunction. NULL UIDs are inert in the CTE (they can
    // neither expand nor join), so filter them out up front.
    let mut seed_sql = String::from("SELECT symbol_uid FROM symbols WHERE symbol_uid IS NOT NULL");
    let mut params: Vec<String> = Vec::new();
    for (column, value) in &plan.seed_conds {
        seed_sql.push_str(&format!(" AND {column} = ?{}", params.len() + 1));
        params.push(value.clone());
    }
    let seed_rows = db.query_json(&seed_sql, &params)?;
    let mut roots: Vec<String> = Vec::new();
    let mut seen_roots: HashSet<String> = HashSet::new();
    for row in &seed_rows {
        if let Some(uid) = row.get("symbol_uid").and_then(|v| v.as_str()) {
            if seen_roots.insert(uid.to_string()) {
                roots.push(uid.to_string());
            }
        }
    }

    let mut memo = QueryMemo {
        neighbors: HashMap::new(),
        symbols: HashMap::new(),
    };
    let mut rows: Vec<Vec<JsonValue>> = Vec::new();
    let mut seen_rows: HashSet<String> = HashSet::new();
    // The projected row depends only on (root, uid) — depth never reaches the
    // output. A (root, uid) pair re-reached at a deeper depth would re-emit an
    // identical row that SELECT DISTINCT discards, so skip it outright.
    let mut emitted: HashSet<(usize, String)> = HashSet::new();
    // Without ORDER BY, LIMIT can stop the traversal early (the SQL pipeline
    // does the same). With ORDER BY the full set must be enumerated first.
    let early_stop = plan.order_keys.is_empty();

    'bfs: {
        if roots.is_empty() || plan.limit == 0 {
            break 'bfs;
        }
        // Root symbol rows back every src-side projection: resolve them in
        // one batch up front.
        batch_resolve_symbols(db, &mut memo, &roots)?;
        // The parser guarantees min_hops >= 1, but a hand-built AST with
        // min_hops == 0 would make the CTE emit the seed tuples themselves.
        if plan.min_hops == 0 {
            for (root_idx, uid) in roots.iter().enumerate() {
                if !emitted.insert((root_idx, uid.clone())) {
                    continue;
                }
                emit_tuple(plan, db, &mut memo, uid, uid, &mut rows, &mut seen_rows)?;
                if early_stop && rows.len() >= plan.limit {
                    break 'bfs;
                }
            }
        }
        // Frontier of (root index, node uid) tuples for the current depth.
        // The CTE's UNION dedups (root_uid, uid, depth) tuples, so the
        // visited set is per-level and per-root: a node re-reached at a
        // DEEPER depth is a new tuple and keeps expanding (cycles re-reach
        // the root; min-depth filters still see deeper re-visits).
        let mut frontier: Vec<(usize, String)> = roots
            .iter()
            .enumerate()
            .map(|(idx, uid)| (idx, uid.clone()))
            .collect();
        for depth in 1..=plan.max_hops {
            let mut next: Vec<(usize, String)> = Vec::new();
            let mut next_seen: HashSet<(usize, String)> = HashSet::new();
            for (root_idx, uid) in &frontier {
                if !memo.neighbors.contains_key(uid.as_str()) {
                    let callees: Vec<String> = db
                        .call_edges_from_uid_lite(uid)?
                        .into_iter()
                        .map(|edge| edge.callee_uid)
                        .collect();
                    memo.neighbors.insert(uid.clone(), callees);
                }
                for callee in &memo.neighbors[uid.as_str()] {
                    if next_seen.insert((*root_idx, callee.clone())) {
                        next.push((*root_idx, callee.clone()));
                    }
                }
            }
            if depth >= plan.min_hops {
                // Only tuples whose (root, uid) pair has not been emitted at
                // a shallower depth can contribute new rows.
                let mut pending: Vec<&(usize, String)> = Vec::new();
                for tuple in &next {
                    if emitted.insert(tuple.clone()) {
                        pending.push(tuple);
                    }
                }
                // Batch-resolve this level's destination rows (IN chunks)
                // instead of one point query per node.
                let pending_uids: Vec<String> =
                    pending.iter().map(|(_, uid)| uid.clone()).collect();
                batch_resolve_symbols(db, &mut memo, &pending_uids)?;
                for (root_idx, uid) in pending {
                    emit_tuple(
                        plan,
                        db,
                        &mut memo,
                        &roots[*root_idx],
                        uid,
                        &mut rows,
                        &mut seen_rows,
                    )?;
                    if early_stop && rows.len() >= plan.limit {
                        break 'bfs;
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
    }

    if !plan.order_keys.is_empty() {
        rows.sort_by(|a, b| {
            for (idx, desc) in &plan.order_keys {
                let ord = sqlite_value_cmp(&a[*idx], &b[*idx]);
                let ord = if *desc { ord.reverse() } else { ord };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            Ordering::Equal
        });
    }
    rows.truncate(plan.limit);

    let row_count = rows.len();
    Ok(CypherResult {
        columns: plan
            .projections
            .iter()
            .map(|p| p.output_name.clone())
            .collect(),
        rows,
        row_count,
        default_limit_applied: query.limit.is_none(),
        limit: Some(plan.limit),
    })
}

/// Mirror the CTE's outer SELECT for one (root_uid, uid) tuple: INNER JOIN
/// both endpoints to `symbols`, apply destination kind/WHERE filters, project,
/// and dedup the projected row (SELECT DISTINCT).
#[allow(clippy::too_many_arguments)]
fn emit_tuple(
    plan: &FastPlan,
    db: &IndexDb,
    memo: &mut QueryMemo,
    root_uid: &str,
    dst_uid: &str,
    rows: &mut Vec<Vec<JsonValue>>,
    seen_rows: &mut HashSet<String>,
) -> CcResult<()> {
    if symbol_row(db, memo, root_uid)?.is_none() {
        return Ok(()); // source INNER JOIN drops the tuple
    }
    let Some(dst_row) = symbol_row(db, memo, dst_uid)? else {
        return Ok(()); // destination INNER JOIN drops the tuple
    };
    if let Some(kind) = plan.dst_kind {
        if dst_row.get("kind").and_then(|v| v.as_str()) != Some(kind) {
            return Ok(());
        }
    }
    for (column, value) in &plan.dst_conds {
        if dst_row.get(*column).and_then(|v| v.as_str()) != Some(value.as_str()) {
            return Ok(());
        }
    }
    // dst_row borrow ends here; re-fetch maps from the memo by key below.
    let row: Vec<JsonValue> = plan
        .projections
        .iter()
        .map(|projection| {
            let uid = if projection.from_src {
                root_uid
            } else {
                dst_uid
            };
            memo.symbols
                .get(uid)
                .and_then(|opt| opt.as_ref())
                .and_then(|map| map.get(projection.prop.as_str()))
                .cloned()
                .unwrap_or(JsonValue::Null)
        })
        .collect();
    let key = serde_json::to_string(&row).unwrap_or_default();
    if seen_rows.insert(key) {
        rows.push(row);
    }
    Ok(())
}

/// Resolve a batch of symbol rows by UID into the memo with chunked
/// `IN (...)` queries. UIDs with no symbols row are memoized as `None`.
fn batch_resolve_symbols(db: &IndexDb, memo: &mut QueryMemo, uids: &[String]) -> CcResult<()> {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for uid in uids {
        if !memo.symbols.contains_key(uid.as_str()) && seen.insert(uid.as_str()) {
            missing.push(uid.clone());
        }
    }
    for chunk in missing.chunks(SYMBOL_BATCH) {
        let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT {} FROM symbols WHERE symbol_uid IN ({})",
            SYMBOL_COLUMNS.join(", "),
            placeholders.join(",")
        );
        for value in db.query_json(&sql, chunk)? {
            if let JsonValue::Object(map) = value {
                let uid = map
                    .get("symbol_uid")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                if let Some(uid) = uid {
                    memo.symbols.insert(uid, Some(map));
                }
            }
        }
    }
    for uid in missing {
        memo.symbols.entry(uid).or_insert(None);
    }
    Ok(())
}

/// Point lookup of a symbols row by UID, memoized for the query. Returns the
/// full projectable column set so any RETURN property can be served.
fn symbol_row<'m>(
    db: &IndexDb,
    memo: &'m mut QueryMemo,
    uid: &str,
) -> CcResult<Option<&'m serde_json::Map<String, JsonValue>>> {
    if !memo.symbols.contains_key(uid) {
        let sql = format!(
            "SELECT {} FROM symbols WHERE symbol_uid = ?1",
            SYMBOL_COLUMNS.join(", ")
        );
        let result = db.query_json(&sql, &[uid.to_string()])?;
        let row = result.into_iter().next().and_then(|value| match value {
            JsonValue::Object(map) => Some(map),
            _ => None,
        });
        memo.symbols.insert(uid.to_string(), row);
    }
    Ok(memo.symbols[uid].as_ref())
}

/// SQLite cross-type ordering for the value shapes `query_json` produces:
/// NULL < numeric (INTEGER/REAL compared numerically) < TEXT (BINARY
/// collation, i.e. byte order).
fn sqlite_value_cmp(a: &JsonValue, b: &JsonValue) -> Ordering {
    fn rank(v: &JsonValue) -> u8 {
        match v {
            JsonValue::Null => 0,
            JsonValue::Number(_) => 1,
            JsonValue::String(_) => 2,
            _ => 3,
        }
    }
    match (a, b) {
        (JsonValue::Number(x), JsonValue::Number(y)) => x
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&y.as_f64().unwrap_or(0.0))
            .unwrap_or(Ordering::Equal),
        (JsonValue::String(x), JsonValue::String(y)) => x.as_bytes().cmp(y.as_bytes()),
        _ => rank(a).cmp(&rank(b)),
    }
}

// ── Tests ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cypher::executor::execute_with_options;
    use crate::cypher::{parse, tokenize};

    /// Build a tiny index DB with the given symbols and CALLS edges.
    /// Symbols: (name, kind, uid). Edges: (caller_uid, callee_uid).
    fn setup_db(
        symbols: &[(&str, &str, &str)],
        edges: &[(&str, &str)],
    ) -> (tempfile::TempDir, IndexDb) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("fastpath.db")).unwrap().0;
        {
            let conn = db.read_conn().unwrap();
            conn.execute_batch(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');",
            )
            .unwrap();
            for (i, (name, kind, uid)) in symbols.iter().enumerate() {
                conn.execute(
                    "INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) \
                     VALUES(?1,'src/x.rs',?2,?3,?4,?4,?5)",
                    rusqlite::params![format!("s{i}"), name, kind, (i + 1) as i64, uid],
                )
                .unwrap();
            }
            for (i, (from, to)) in edges.iter().enumerate() {
                conn.execute(
                    "INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) \
                     VALUES(?1,'src/x.rs','c',1,?2,?3)",
                    rusqlite::params![format!("e{i}"), from, to],
                )
                .unwrap();
            }
        }
        (tmp, db)
    }

    fn parse_query(input: &str) -> CypherQuery {
        parse(&tokenize(input).unwrap()).unwrap()
    }

    fn sorted_rows(result: &CypherResult) -> Vec<String> {
        let mut rows: Vec<String> = result
            .rows
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();
        rows.sort();
        rows
    }

    /// Run the same query through the SQL CTE path and the fast path; assert
    /// the fast path engaged and both produce identical result sets.
    fn assert_equivalent(db: &IndexDb, input: &str) -> CypherResult {
        let query = parse_query(input);
        let fast = try_execute(&query, db)
            .unwrap()
            .unwrap_or_else(|| panic!("fast path must engage for: {input}"));
        let sql = execute_with_options(&query, db, false).unwrap();
        assert_eq!(fast.columns, sql.columns, "columns diverge for: {input}");
        assert_eq!(
            sorted_rows(&fast),
            sorted_rows(&sql),
            "result sets diverge for: {input}"
        );
        assert_eq!(fast.row_count, sql.row_count, "row_count: {input}");
        assert_eq!(
            fast.default_limit_applied, sql.default_limit_applied,
            "default_limit_applied: {input}"
        );
        assert_eq!(fast.limit, sql.limit, "limit metadata: {input}");
        fast
    }

    /// Assert the gate rejects the query (the SQL path stays in charge).
    fn assert_falls_back(db: &IndexDb, input: &str) {
        let query = parse_query(input);
        assert!(
            try_execute(&query, db).unwrap().is_none(),
            "fast path must NOT engage for: {input}"
        );
    }

    fn cycle_db() -> (tempfile::TempDir, IndexDb) {
        // A -> B -> C -> A (cycle back to the root).
        setup_db(
            &[
                ("A", "function", "uA"),
                ("B", "function", "uB"),
                ("C", "function", "uC"),
            ],
            &[("uA", "uB"), ("uB", "uC"), ("uC", "uA")],
        )
    }

    fn diamond_db() -> (tempfile::TempDir, IndexDb) {
        // A -> B, A -> C, B -> D, C -> D (two paths to D), D -> E.
        setup_db(
            &[
                ("A", "function", "uA"),
                ("B", "function", "uB"),
                ("C", "function", "uC"),
                ("D", "function", "uD"),
                ("E", "function", "uE"),
            ],
            &[
                ("uA", "uB"),
                ("uA", "uC"),
                ("uB", "uD"),
                ("uC", "uD"),
                ("uD", "uE"),
            ],
        )
    }

    // ── Equivalence: divergence-risk fixtures ──────────────────

    #[test]
    fn cycle_re_reaches_root_at_depth_three() {
        let (_tmp, db) = cycle_db();
        // The CTE re-reaches A through the cycle at depth 3: tuple (uA,uA,3)
        // is distinct from the seed tuple (uA,uA,0), so 'A' appears in the
        // result. A visited-node BFS would silently drop it.
        let result = assert_equivalent(
            &db,
            "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert!(names.contains(&"A"), "cycle must re-reach root: {names:?}");
        assert_eq!(result.row_count, 3, "A, B, C: {names:?}");
    }

    #[test]
    fn cycle_with_min_depth_two() {
        let (_tmp, db) = cycle_db();
        // *2..3 from A: C at depth 2, A at depth 3. B (depth 1 only) excluded.
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*2..3]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert!(names.contains(&"C") && names.contains(&"A"), "{names:?}");
        assert!(!names.contains(&"B"), "B is depth 1 only: {names:?}");
    }

    #[test]
    fn diamond_dedups_two_paths() {
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        let d_count = result
            .rows
            .iter()
            .filter(|r| r[0].as_str() == Some("D"))
            .count();
        assert_eq!(d_count, 1, "D reachable via two paths must appear once");
        assert_eq!(result.row_count, 3, "B, C, D");
    }

    #[test]
    fn node_reachable_at_multiple_depths_with_min_two() {
        // A -> B -> C plus shortcut A -> C: C is reachable at depth 1 AND 2.
        // With *2..2 the CTE keeps the (uA,uC,2) tuple; a first-visit BFS
        // would mark C visited at depth 1 and miss it.
        let (_tmp, db) = setup_db(
            &[
                ("A", "function", "uA"),
                ("B", "function", "uB"),
                ("C", "function", "uC"),
            ],
            &[("uA", "uB"), ("uB", "uC"), ("uA", "uC")],
        );
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*2..2]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert!(names.contains(&"C"), "C re-reached at depth 2: {names:?}");
    }

    #[test]
    fn depth_range_one_to_two_on_chain() {
        let (_tmp, db) = setup_db(
            &[
                ("A", "function", "uA"),
                ("B", "function", "uB"),
                ("C", "function", "uC"),
                ("D", "function", "uD"),
            ],
            &[("uA", "uB"), ("uB", "uC"), ("uC", "uD")],
        );
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        assert_eq!(result.row_count, 2, "B, C only");
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*2..3]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        assert_eq!(result.row_count, 2, "C, D only");
    }

    #[test]
    fn traversal_continues_through_uid_missing_from_symbols() {
        // A -> ghost -> B: 'ghost' has call_edges rows but no symbols row.
        // The CTE recursion runs on the edge table alone, so B is still
        // reachable at depth 2 while ghost itself never reaches the output.
        let (_tmp, db) = setup_db(
            &[("A", "function", "uA"), ("B", "function", "uB")],
            &[("uA", "uGhost"), ("uGhost", "uB")],
        );
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert_eq!(names, vec!["B"], "ghost filtered, B reached through it");
    }

    #[test]
    fn multiple_seeds_share_a_name() {
        // Two roots named 'dup' — the CTE seeds both.
        let (_tmp, db) = setup_db(
            &[
                ("dup", "function", "u1"),
                ("dup", "function", "u2"),
                ("X", "function", "uX"),
                ("Y", "function", "uY"),
            ],
            &[("u1", "uX"), ("u2", "uY")],
        );
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'dup' RETURN a.symbol_uid, b.name",
        );
        assert_eq!(result.row_count, 2, "one row per root");
    }

    // ── Equivalence: filters, projections, ordering, limits ─────

    #[test]
    fn dst_kind_filter_from_label() {
        let (_tmp, db) = setup_db(
            &[
                ("A", "function", "uA"),
                ("B", "class", "uB"),
                ("C", "function", "uC"),
            ],
            &[("uA", "uB"), ("uB", "uC")],
        );
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b:Function) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert_eq!(names, vec!["C"], "class B filtered by :Function label");
    }

    #[test]
    fn src_label_kind_is_ignored_like_the_sql_path() {
        // Quirk parity: translate_variable_length never applies the source
        // label's kind filter. (a:Class) with a function seed still matches.
        let (_tmp, db) = setup_db(
            &[("A", "function", "uA"), ("B", "function", "uB")],
            &[("uA", "uB")],
        );
        let result = assert_equivalent(
            &db,
            "MATCH (a:Class)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        assert_eq!(result.row_count, 1, "src kind filter is not applied");
    }

    #[test]
    fn dst_where_equality_and_uid_seed() {
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..3]->(b) WHERE a.symbol_uid = 'uA' AND b.name = 'D' RETURN b.name, b.start_line",
        );
        assert_eq!(result.row_count, 1);
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE b.symbol_uid = 'uD' AND a.name = 'A' RETURN a.name, b.name",
        );
        assert_eq!(result.row_count, 1);
    }

    #[test]
    fn return_props_of_both_endpoints_with_aliases() {
        let (_tmp, db) = diamond_db();
        assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' \
             RETURN a.name AS caller, b.name, b.kind, b.start_line, b.file_path",
        );
    }

    #[test]
    fn distinct_collapses_identical_projected_rows() {
        // All reachable nodes share kind 'function': RETURN b.kind collapses
        // to a single row under the variable-length SELECT DISTINCT.
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..3]->(b) WHERE a.name = 'A' RETURN b.kind",
        );
        assert_eq!(result.row_count, 1);
    }

    #[test]
    fn order_by_with_limit_truncation() {
        let (_tmp, db) = diamond_db();
        let query =
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name ORDER BY b.name LIMIT 2";
        let parsed = parse_query(query);
        let fast = try_execute(&parsed, &db).unwrap().expect("must engage");
        let sql = execute_with_options(&parsed, &db, false).unwrap();
        // Deterministic order: compare rows positionally, not as sets.
        assert_eq!(fast.rows, sql.rows, "ordered rows must match exactly");
        assert_eq!(fast.row_count, 2);
    }

    #[test]
    fn order_by_desc_and_alias() {
        let (_tmp, db) = diamond_db();
        let query = "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' \
                     RETURN b.name AS callee ORDER BY callee DESC";
        let parsed = parse_query(query);
        let fast = try_execute(&parsed, &db).unwrap().expect("must engage");
        let sql = execute_with_options(&parsed, &db, false).unwrap();
        assert_eq!(fast.rows, sql.rows);
        let query = "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' \
                     RETURN b.name, b.start_line ORDER BY b.start_line DESC";
        let parsed = parse_query(query);
        let fast = try_execute(&parsed, &db).unwrap().expect("must engage");
        let sql = execute_with_options(&parsed, &db, false).unwrap();
        assert_eq!(fast.rows, sql.rows);
    }

    #[test]
    fn limit_without_order_by_truncates_to_a_subset() {
        // Without ORDER BY, SQL row choice under LIMIT is unspecified; assert
        // count parity and that the fast rows are a subset of the full set.
        let (_tmp, db) = diamond_db();
        let parsed =
            parse_query("MATCH (a)-[:CALLS*1..3]->(b) WHERE a.name = 'A' RETURN b.name LIMIT 2");
        let fast = try_execute(&parsed, &db).unwrap().expect("must engage");
        let sql = execute_with_options(&parsed, &db, false).unwrap();
        assert_eq!(fast.row_count, 2);
        assert_eq!(sql.row_count, 2);
        let full = execute_with_options(
            &parse_query(
                "MATCH (a)-[:CALLS*1..3]->(b) WHERE a.name = 'A' RETURN b.name LIMIT 1000",
            ),
            &db,
            false,
        )
        .unwrap();
        let full_set = sorted_rows(&full);
        for row in &fast.rows {
            let key = serde_json::to_string(row).unwrap();
            assert!(full_set.contains(&key), "fast row {key} not in full set");
        }
    }

    #[test]
    fn default_limit_metadata_matches() {
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
        );
        assert!(result.default_limit_applied);
        assert_eq!(result.limit, Some(DEFAULT_CYPHER_LIMIT));
    }

    #[test]
    fn no_matching_seed_returns_empty() {
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'nope' RETURN b.name",
        );
        assert_eq!(result.row_count, 0);
    }

    // ── Eligibility gate: conservative fallbacks ────────────────

    #[test]
    fn gate_rejects_ineligible_shapes() {
        let (_tmp, db) = diamond_db();
        for query in [
            // Non-CALLS edge kind.
            "MATCH (a)-[:IMPORTS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
            "MATCH (a)-[:CONTAINS_FILE*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
            // Reverse / undirected (SQL path ignores direction; stay out).
            "MATCH (a)<-[:CALLS*1..2]-(b) WHERE a.name = 'A' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]-(b) WHERE a.name = 'A' RETURN b.name",
            // No WHERE / no source pin.
            "MATCH (a)-[:CALLS*1..2]->(b) RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE b.name = 'D' RETURN b.name",
            // OR / NOT / non-equality predicates.
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' OR a.name = 'B' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE NOT a.name = 'A' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name =~ 'A.*' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name CONTAINS 'A' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name STARTS WITH 'A' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.start_line = 1 RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' AND degree(b) > 1 RETURN b.name",
            // Equality on a property other than name/symbol_uid.
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.file_path = 'src/x.rs' RETURN b.name",
            // Aggregations / COLLECT / whole-var returns.
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN COUNT(*)",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN COLLECT(b.name)",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b",
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name, SUM(b.start_line)",
            // ORDER BY on a non-returned property.
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name ORDER BY b.start_line",
            // Inline props (ignored by the SQL varlen path; stay out).
            "MATCH (a {name: 'A'})-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
            // Labels that do not map to the symbols table.
            "MATCH (a:File)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
            "MATCH (a)-[:CALLS*1..2]->(b:File) WHERE a.name = 'A' RETURN b.name",
            // Same variable on both endpoints.
            "MATCH (a)-[:CALLS*1..2]->(a) WHERE a.name = 'A' RETURN a.name",
            // WHERE referencing a variable outside the pattern.
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE c.name = 'A' RETURN b.name",
            // Single-hop (routed elsewhere, but the gate must still reject).
            "MATCH (a)-[:CALLS]->(b) WHERE a.name = 'A' RETURN b.name",
            // LIMIT above the fast-path ceiling (full enumeration only ties
            // the CTE, so the SQL path keeps it).
            "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name LIMIT 100000",
        ] {
            assert_falls_back(&db, query);
        }
    }

    #[test]
    fn gate_rejects_multi_clause_and_optional_match() {
        let (_tmp, db) = diamond_db();
        let query =
            parse_query("MATCH (x:Function) OPTIONAL MATCH (x)-[:CALLS*1..2]->(b) RETURN x.name");
        assert!(try_execute(&query, &db).unwrap().is_none());
        let query =
            parse_query("OPTIONAL MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name");
        assert!(try_execute(&query, &db).unwrap().is_none());
    }

    #[test]
    fn env_flag_defaults_to_enabled() {
        assert!(env_flag(None));
        assert!(env_flag(Some("1")));
        assert!(env_flag(Some("")));
        assert!(!env_flag(Some("0")));
    }

    // ── Integration: execute() routes through the fast path ─────

    #[test]
    fn execute_with_options_routes_eligible_query_identically() {
        let (_tmp, db) = cycle_db();
        let query = parse_query(
            "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = 'A' RETURN b.name",
        );
        let fast = execute_with_options(&query, &db, true).unwrap();
        let sql = execute_with_options(&query, &db, false).unwrap();
        assert_eq!(sorted_rows(&fast), sorted_rows(&sql));
        assert_eq!(fast.columns, sql.columns);
    }
}
