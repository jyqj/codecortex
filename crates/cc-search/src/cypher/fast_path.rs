//! Lazy-BFS fast path for variable-length `CALLS` traversals.
//!
//! Benchmark evidence (50k symbols / 200k call_edges, release):
//! the `WITH RECURSIVE` CTE in `translate_variable_length` costs 7-71 ms p50,
//! while a lazy per-node BFS over `IndexDb::call_edges_from_uid_lite` costs
//! 0.11-0.28 ms p50 for LIMIT-50 queries. This module implements that BFS for
//! a narrow, conservatively-gated query shape and mirrors the CTE's observable
//! semantics exactly. The semantics themselves are declared once in
//! `traversal_semantics.rs` and consumed by both engines through exhaustive
//! `match`es; the notes below describe this engine's mechanical mapping:
//!
//! - `DirectionHandling::IgnoreDirection` (compatibility quirk): `->`, `<-`
//!   and `--` variable-length segments all walk caller -> callee in textual
//!   pattern order, exactly like the SQL CTE.
//! - The CTE deduplicates `(root_uid, uid, depth)` tuples via `UNION`
//!   (`TupleMultiplicity::DistinctPerRootNodeDepth`), so a
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
use super::traversal_semantics::{
    CyclePolicy, DirectionHandling, ProjectionDedup, TraversalSemantics, TupleMultiplicity,
    WalkOrientation, VARLEN_TRAVERSAL,
};
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

/// Gate constants for the fast path, collected in one declaration so every
/// eligibility check references the same source of truth. Changing any value
/// widens or narrows the gate — re-run the equivalence tests in this module
/// and the ADR-0001 benchmark (`graph_traversal_bench`) before doing so.
pub(crate) struct FastPathConfig {
    /// Largest LIMIT the fast path will serve. The lazy BFS wins decisively
    /// for small-LIMIT traversals (early termination); for huge limits the
    /// full enumeration plus per-node lookups merely ties the CTE, so stay out.
    pub(crate) max_limit: usize,
    /// Edge kinds with a lazy adjacency source (`call_edges_from_uid_lite`).
    pub(crate) eligible_edge_kinds: &'static [&'static str],
    /// Columns a WHERE string equality may pin (seed and destination side).
    pub(crate) seed_eq_columns: &'static [&'static str],
    /// Columns RETURN may project (mirrors `table_json_expr` for `symbols`).
    pub(crate) projectable_columns: &'static [&'static str],
}

impl FastPathConfig {
    pub(crate) const DEFAULT: FastPathConfig = FastPathConfig {
        max_limit: 1000,
        // Single source preserved (ADR-0001 R2-D): the eligible set IS the
        // shared per-tool declaration in the graph catalog — not a third
        // copy. Catalog membership and `variable_length` support are
        // asserted by `fast_path_kinds_derive_from_catalog_declaration`
        // below and by cc-model's tool_graph_subsets tests.
        eligible_edge_kinds: cc_model::graph_catalog::tool_graph_subsets::CYPHER_FAST_PATH.kinds(),
        seed_eq_columns: &["name", "symbol_uid"],
        projectable_columns: SYMBOL_COLUMNS,
    };
}

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

#[derive(Debug)]
struct Projection {
    var: String,
    prop: String,
    from_src: bool,
    alias: Option<String>,
    output_name: String,
}

#[derive(Debug)]
struct FastPlan {
    min_hops: usize,
    max_hops: usize,
    /// Walk orientation derived from the shared semantics declaration
    /// (`traversal_semantics::VARLEN_TRAVERSAL.orient(...)`).
    walk: WalkOrientation,
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

// ── Ineligibility reasons & decision metadata ──────

/// Why a variable-length query did not take the lazy-BFS fast path. Each
/// variant maps 1:1 to a rejection point in the eligibility gate below;
/// `Display` renders a stable, agent-facing token (snapshot-locked in tests)
/// that surfaces through the `graph_query` envelope's `fast_path.reason`
/// field. The gate semantics themselves are unchanged — this enum only names
/// the decisions ADR-0001 already made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FastPathIneligibility {
    // ── Structure: MATCH / pattern shape ──
    /// Not exactly one MATCH clause.
    NotSingleMatch,
    /// The single MATCH clause is OPTIONAL.
    OptionalMatch,
    /// Comma-separated patterns inside the MATCH clause.
    MultiplePatternsInMatch,
    /// Pattern is not exactly `(src)-[rel]-(dst)` (chains, bare nodes).
    NotSingleRelationshipSegment,
    /// `*1..1` / plain single hop — routed to the single-hop translator.
    SingleHopPattern,
    /// Inline node properties `{...}` (ignored by the SQL varlen path; stay out).
    InlineNodeProperties,
    /// An endpoint label maps to a table other than `symbols`.
    LabelNotSymbolsTable { label: String },
    /// Both endpoints bind the same variable.
    SameVariableOnBothEndpoints,
    // ── Edge ──
    /// Relationship kind without a lazy adjacency source (only CALLS today).
    EdgeKindNotEligible { kind: String },
    // ── WHERE ──
    /// No WHERE clause at all, so nothing pins the seed.
    NoWhereClause,
    /// A WHERE predicate is not a `var.prop = 'string'` equality in an AND-tree.
    WhereNotSimpleEquality,
    /// A WHERE equality targets a column outside the seed set (name/symbol_uid).
    WhereOnNonSeedColumn { prop: String },
    /// A WHERE equality references a variable outside the pattern.
    WhereUnknownVariable { var: String },
    /// WHERE exists but no equality pins the source side.
    NoSeedEquality,
    // ── RETURN ──
    /// Empty RETURN clause (hand-built ASTs only; the parser rejects it).
    EmptyReturn,
    /// A RETURN item is not a simple `var.prop` (aggregates, COLLECT, vars).
    ReturnNotSimpleProperty,
    /// A RETURN property references a variable outside the pattern.
    ReturnUnknownVariable { var: String },
    /// A RETURN property is outside the symbols projection set.
    ReturnPropertyNotProjectable { prop: String },
    // ── ORDER BY / LIMIT ──
    /// ORDER BY targets an expression that is not a returned property.
    OrderByNotReturnedProperty,
    /// Effective LIMIT exceeds the fast-path ceiling.
    LimitTooLarge { requested: usize, max: usize },
}

impl std::fmt::Display for FastPathIneligibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSingleMatch => write!(f, "not_single_match"),
            Self::OptionalMatch => write!(f, "optional_match"),
            Self::MultiplePatternsInMatch => write!(f, "multiple_patterns_in_match"),
            Self::NotSingleRelationshipSegment => write!(f, "not_single_relationship_segment"),
            Self::SingleHopPattern => write!(f, "single_hop_pattern"),
            Self::InlineNodeProperties => write!(f, "inline_node_properties"),
            Self::LabelNotSymbolsTable { label } => write!(f, "label_not_symbols_table({label})"),
            Self::SameVariableOnBothEndpoints => write!(f, "same_variable_on_both_endpoints"),
            Self::EdgeKindNotEligible { kind } => write!(f, "edge_kind_not_eligible({kind})"),
            Self::NoWhereClause => write!(f, "no_where_clause"),
            Self::WhereNotSimpleEquality => write!(f, "where_not_simple_equality"),
            Self::WhereOnNonSeedColumn { prop } => write!(f, "where_on_non_seed_column({prop})"),
            Self::WhereUnknownVariable { var } => write!(f, "where_unknown_variable({var})"),
            Self::NoSeedEquality => write!(f, "no_seed_equality"),
            Self::EmptyReturn => write!(f, "empty_return"),
            Self::ReturnNotSimpleProperty => write!(f, "return_not_simple_property"),
            Self::ReturnUnknownVariable { var } => write!(f, "return_unknown_variable({var})"),
            Self::ReturnPropertyNotProjectable { prop } => {
                write!(f, "return_property_not_projectable({prop})")
            }
            Self::OrderByNotReturnedProperty => write!(f, "order_by_not_returned_property"),
            Self::LimitTooLarge { requested, max } => {
                write!(f, "limit_too_large({requested}>{max})")
            }
        }
    }
}

/// Outcome of the fast-path routing for one query, surfaced to `graph_query`
/// callers as response metadata. Deterministic mirror of the routing in
/// `execute_with_options`: the gate decision depends only on the query AST
/// and the `CODECORTEX_CYPHER_FAST_PATH` toggle, so recomputing it at the
/// response boundary reports exactly what execution did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FastPathDecision {
    /// Variable-length traversal served by the lazy BFS.
    Used,
    /// Variable-length traversal that fell back to the SQL CTE.
    Fallback(FastPathIneligibility),
    /// Variable-length traversal with `CODECORTEX_CYPHER_FAST_PATH=0`.
    DisabledByEnv,
    /// The query never routes through the fast path (single-node,
    /// single-hop, OPTIONAL-MATCH anchor, multi-rel chain, UNION).
    NotApplicable,
}

impl FastPathDecision {
    /// JSON metadata for the `graph_query` envelope. Returns `None` for
    /// `NotApplicable`: emitting `fast_path` on every single-node lookup
    /// would be pure noise — the field only appears when a variable-length
    /// traversal was in play and the used/fallback distinction is real, so
    /// its absence reads as "not a traversal", never as "fell back".
    pub fn as_metadata(&self) -> Option<JsonValue> {
        match self {
            Self::NotApplicable => None,
            Self::Used => Some(serde_json::json!({ "used": true })),
            Self::Fallback(reason) => Some(serde_json::json!({
                "used": false,
                "reason": reason.to_string(),
            })),
            Self::DisabledByEnv => Some(serde_json::json!({
                "used": false,
                "reason": "disabled(CODECORTEX_CYPHER_FAST_PATH=0)",
            })),
        }
    }
}

/// Explain whether `query` takes the lazy-BFS fast path, without executing.
pub(crate) fn decide(query: &CypherQuery) -> FastPathDecision {
    decide_with_env(query, env_enabled())
}

/// Same as `decide` with the env toggle injected (testable without mutating
/// process-global environment). Mirrors `execute_with_options` ordering: the
/// env toggle is consulted before the gate, so a disabled fast path reports
/// `DisabledByEnv` even for queries the gate would reject.
fn decide_with_env(query: &CypherQuery, enabled: bool) -> FastPathDecision {
    if !routes_to_varlen(query) {
        return FastPathDecision::NotApplicable;
    }
    if !enabled {
        return FastPathDecision::DisabledByEnv;
    }
    match build_plan(query) {
        Ok(_) => FastPathDecision::Used,
        Err(reason) => FastPathDecision::Fallback(reason),
    }
}

/// Mirror of the `execute_with_options` routing: true iff the query reaches
/// the variable-length branch where the fast path is consulted at all.
fn routes_to_varlen(query: &CypherQuery) -> bool {
    let Some(first_match) = query.match_clauses.first() else {
        return false;
    };
    let Some(pattern) = first_match.patterns.first() else {
        return false;
    };
    // `MATCH (f) OPTIONAL MATCH (f)-[...]->(g)` routes to the optional-match
    // translator before the relationship branch is ever considered.
    if super::executor::detect_two_clause_optional(query).is_some() {
        return false;
    }
    if pattern.rels.len() != 1 {
        return false; // single-node query or unsupported multi-rel chain
    }
    let rel = &pattern.rels[0];
    !(rel.min_hops == 1 && rel.max_hops == 1)
}

/// Attempt the lazy-BFS fast path. Returns `Ok(None)` when the query is not
/// eligible (the caller then runs the SQL CTE path unchanged).
pub(crate) fn try_execute(query: &CypherQuery, db: &IndexDb) -> CcResult<Option<CypherResult>> {
    let plan = match build_plan(query) {
        Ok(plan) => plan,
        Err(reason) => {
            tracing::debug!(reason = %reason, "cypher varlen fast path fallback to SQL CTE");
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
/// [ORDER BY returned properties] [LIMIT k]`. Any arrow spelling (`->`,
/// `<-`, `--`) is accepted: the shared semantics declaration says the arrow
/// is ignored for variable-length segments, so all spellings walk forward.
fn build_plan(query: &CypherQuery) -> Result<FastPlan, FastPathIneligibility> {
    let config = &FastPathConfig::DEFAULT;
    let shape = check_structure(query, config)?;
    let (seed_conds, dst_conds) = check_where(query, &shape, config)?;
    let projections = check_return(query, &shape, config)?;
    let (order_keys, limit) = check_order_limit(query, &projections, config)?;

    Ok(FastPlan {
        min_hops: shape.rel.min_hops,
        max_hops: shape.rel.max_hops,
        walk: shape.walk,
        seed_conds,
        dst_conds,
        dst_kind: shape.dst_label.and_then(label_kind_filter),
        projections,
        order_keys,
        limit,
    })
}

/// The structurally eligible pattern extracted by `check_structure`.
struct EligibleShape<'q> {
    rel: &'q RelPattern,
    src_var: &'q str,
    dst_var: &'q str,
    dst_label: Option<&'q str>,
    walk: WalkOrientation,
}

/// Structure checks: single MATCH, single variable-length CALLS segment
/// between two distinct symbols-backed endpoints, no inline props.
fn check_structure<'q>(
    query: &'q CypherQuery,
    config: &FastPathConfig,
) -> Result<EligibleShape<'q>, FastPathIneligibility> {
    if query.match_clauses.len() != 1 {
        return Err(FastPathIneligibility::NotSingleMatch);
    }
    let match_clause = &query.match_clauses[0];
    if match_clause.is_optional {
        return Err(FastPathIneligibility::OptionalMatch);
    }
    if match_clause.patterns.len() != 1 {
        return Err(FastPathIneligibility::MultiplePatternsInMatch);
    }
    let pattern = &match_clause.patterns[0];
    if pattern.nodes.len() != 2 || pattern.rels.len() != 1 {
        return Err(FastPathIneligibility::NotSingleRelationshipSegment);
    }
    let rel = &pattern.rels[0];
    if rel.min_hops == 1 && rel.max_hops == 1 {
        return Err(FastPathIneligibility::SingleHopPattern);
    }
    let kind = rel.rel_type.as_deref().unwrap_or("CALLS");
    if !config.eligible_edge_kinds.contains(&kind) {
        return Err(FastPathIneligibility::EdgeKindNotEligible {
            kind: kind.to_string(),
        });
    }
    // Compile-time tether to the shared semantics declaration: this match is
    // exhaustive on every declared rule, so adding a variant in
    // traversal_semantics.rs fails compilation here and forces this BFS to be
    // re-validated against the SQL CTE (equivalence tests below).
    match VARLEN_TRAVERSAL {
        TraversalSemantics {
            direction: DirectionHandling::IgnoreDirection,
            tuple_multiplicity: TupleMultiplicity::DistinctPerRootNodeDepth,
            cycle_policy: CyclePolicy::BoundedByMaxHops,
            projection_dedup: ProjectionDedup::DistinctRows,
        } => {}
    }
    // DirectionHandling::IgnoreDirection (compatibility quirk shared with the
    // SQL CTE): `->`, `<-` and `--` spellings all walk forward in textual
    // pattern order, so every direction is eligible here.
    let walk = VARLEN_TRAVERSAL.orient(rel.direction);

    let src_node = &pattern.nodes[0];
    let dst_node = &pattern.nodes[1];
    // Inline props are ignored by translate_variable_length; stay out of
    // that surprising territory entirely.
    if !src_node.props.is_empty() || !dst_node.props.is_empty() {
        return Err(FastPathIneligibility::InlineNodeProperties);
    }
    for node in [src_node, dst_node] {
        if let Some(label) = node.label.as_deref() {
            if label_table(label) != "symbols" {
                return Err(FastPathIneligibility::LabelNotSymbolsTable {
                    label: label.to_string(),
                });
            }
        }
    }
    // Same default aliases as translate_variable_length.
    let src_var = src_node.var.as_deref().unwrap_or("src");
    let dst_var = dst_node.var.as_deref().unwrap_or("dst");
    if src_var == dst_var {
        return Err(FastPathIneligibility::SameVariableOnBothEndpoints);
    }

    Ok(EligibleShape {
        rel,
        src_var,
        dst_var,
        dst_label: dst_node.label.as_deref(),
        walk,
    })
}

/// WHERE checks: an AND-tree of `var.prop = 'string'` equalities on the seed
/// columns, with at least one equality pinning the source side.
#[allow(clippy::type_complexity)]
fn check_where(
    query: &CypherQuery,
    shape: &EligibleShape<'_>,
    config: &FastPathConfig,
) -> Result<(Vec<(&'static str, String)>, Vec<(&'static str, String)>), FastPathIneligibility> {
    let where_clause = query
        .where_clause
        .as_ref()
        .ok_or(FastPathIneligibility::NoWhereClause)?;
    let mut seed_conds = Vec::new();
    let mut dst_conds = Vec::new();
    collect_eq_conds(
        &where_clause.expr,
        shape.src_var,
        shape.dst_var,
        config,
        &mut seed_conds,
        &mut dst_conds,
    )?;
    if seed_conds.is_empty() {
        return Err(FastPathIneligibility::NoSeedEquality);
    }
    Ok((seed_conds, dst_conds))
}

/// RETURN checks: only simple endpoint properties within the symbols
/// projection set.
fn check_return(
    query: &CypherQuery,
    shape: &EligibleShape<'_>,
    config: &FastPathConfig,
) -> Result<Vec<Projection>, FastPathIneligibility> {
    if query.return_clause.items.is_empty() {
        return Err(FastPathIneligibility::EmptyReturn);
    }
    let mut projections = Vec::with_capacity(query.return_clause.items.len());
    for item in &query.return_clause.items {
        let ReturnItem::Prop(prop_ref, alias) = item else {
            return Err(FastPathIneligibility::ReturnNotSimpleProperty);
        };
        let from_src = if prop_ref.var == shape.src_var {
            true
        } else if prop_ref.var == shape.dst_var {
            false
        } else {
            return Err(FastPathIneligibility::ReturnUnknownVariable {
                var: prop_ref.var.clone(),
            });
        };
        if !config.projectable_columns.contains(&prop_ref.prop.as_str()) {
            return Err(FastPathIneligibility::ReturnPropertyNotProjectable {
                prop: prop_ref.prop.clone(),
            });
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
    Ok(projections)
}

/// ORDER BY / LIMIT checks: ordering keys must be returned properties and the
/// effective LIMIT must stay under the fast-path ceiling.
fn check_order_limit(
    query: &CypherQuery,
    projections: &[Projection],
    config: &FastPathConfig,
) -> Result<(Vec<(usize, bool)>, usize), FastPathIneligibility> {
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
            .ok_or(FastPathIneligibility::OrderByNotReturnedProperty)?;
            order_keys.push((idx, item.desc));
        }
    }

    let limit = query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT);
    if limit > config.max_limit {
        return Err(FastPathIneligibility::LimitTooLarge {
            requested: limit,
            max: config.max_limit,
        });
    }
    Ok((order_keys, limit))
}

/// Flatten an AND-tree of `var.prop = 'string'` equalities on the configured
/// seed columns, splitting by endpoint variable exactly like
/// `split_where_by_var` does for the SQL path. Anything else is rejected.
fn collect_eq_conds(
    expr: &Expr,
    src_var: &str,
    dst_var: &str,
    config: &FastPathConfig,
    seed_conds: &mut Vec<(&'static str, String)>,
    dst_conds: &mut Vec<(&'static str, String)>,
) -> Result<(), FastPathIneligibility> {
    match expr {
        Expr::And(left, right) => {
            collect_eq_conds(left, src_var, dst_var, config, seed_conds, dst_conds)?;
            collect_eq_conds(right, src_var, dst_var, config, seed_conds, dst_conds)
        }
        Expr::Comparison {
            left,
            op: CmpOp::Eq,
            right: Value::String(value),
        } => {
            let Some(column) = config
                .seed_eq_columns
                .iter()
                .find(|col| **col == left.prop.as_str())
                .copied()
            else {
                return Err(FastPathIneligibility::WhereOnNonSeedColumn {
                    prop: left.prop.clone(),
                });
            };
            if left.var == src_var {
                seed_conds.push((column, value.clone()));
                Ok(())
            } else if left.var == dst_var {
                dst_conds.push((column, value.clone()));
                Ok(())
            } else {
                Err(FastPathIneligibility::WhereUnknownVariable {
                    var: left.var.clone(),
                })
            }
        }
        _ => Err(FastPathIneligibility::WhereNotSimpleEquality),
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
    // neither expand nor join), so filter them out up front. The typed seed
    // read enforces the same column allowlist as `FastPathConfig`'s
    // `seed_eq_columns` gate.
    let conds: Vec<(&str, &str)> = plan
        .seed_conds
        .iter()
        .map(|(column, value)| (*column, value.as_str()))
        .collect();
    let seed_uids = db.symbol_graph_reads().symbol_uids_by_eq(&conds)?;
    let mut roots: Vec<String> = Vec::new();
    let mut seen_roots: HashSet<String> = HashSet::new();
    for uid in seed_uids {
        if seen_roots.insert(uid.clone()) {
            roots.push(uid);
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
                    // Mechanical mapping of the declared walk orientation:
                    // Forward = follow call_edges caller -> callee.
                    let callees: Vec<String> = match plan.walk {
                        WalkOrientation::Forward => db
                            .reads()
                            .call_edges_from_uid_lite(uid)?
                            .into_iter()
                            .map(|edge| edge.callee_uid)
                            .collect(),
                    };
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
        // This constructor only runs when the gate accepted the query, so the
        // decision is `Used`. The routing caller (execute_with_options)
        // re-stamps `fp_decision` on this value for clarity, but filling it
        // here keeps the field self-consistent if try_execute is ever called
        // through a different entry point.
        fast_path: FastPathDecision::Used,
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

/// Convert a typed [`cc_db::index_db::SymbolRow`] into the JSON map shape the
/// projection code reads (`map.get(prop)`), matching `query_json`'s output for
/// the same SELECT: TEXT → String, INTEGER → Number, NULL → Null.
fn symbol_row_to_map(
    row: &cc_db::index_db::SymbolRow,
) -> Option<serde_json::Map<String, JsonValue>> {
    match serde_json::to_value(row) {
        Ok(JsonValue::Object(map)) => Some(map),
        _ => None,
    }
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
    for (uid, row) in db.reads().symbol_rows_by_uids(&missing)? {
        memo.symbols.insert(uid, symbol_row_to_map(&row));
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
        let key = uid.to_string();
        let row = db
            .reads()
            .symbol_rows_by_uids(std::slice::from_ref(&key))?
            .remove(uid)
            .as_ref()
            .and_then(symbol_row_to_map);
        memo.symbols.insert(key, row);
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
            let conn = crate::test_seed::seed_conn(&db);
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

    // ── Equivalence: ignored-direction spellings (`<-` / `--`) ──
    //
    // The shared semantics declaration (traversal_semantics.rs) says the
    // arrow is IGNORED for variable-length segments (compatibility quirk):
    // `->`, `<-` and `--` all walk caller -> callee in textual pattern
    // order. The fast path executes the same declaration, so all spellings
    // are eligible and must stay row-for-row equal to the SQL CTE.

    /// The three arrow spellings of the same variable-length segment.
    /// `{}` is replaced with the hop range (e.g. `:CALLS*1..3`).
    fn direction_spellings(range: &str) -> [String; 3] {
        [
            format!("-[{range}]->"),
            format!("<-[{range}]-"),
            format!("-[{range}]-"),
        ]
    }

    #[test]
    fn ignored_direction_spellings_all_match_forward() {
        let (_tmp, db) = diamond_db();
        let mut expected: Option<Vec<String>> = None;
        for arrow in direction_spellings(":CALLS*1..2") {
            let query = format!("MATCH (a){arrow}(b) WHERE a.name = 'A' RETURN b.name");
            let result = assert_equivalent(&db, &query);
            let rows = sorted_rows(&result);
            match &expected {
                None => expected = Some(rows),
                Some(forward) => assert_eq!(
                    &rows, forward,
                    "direction must be ignored (quirk parity): {query}"
                ),
            }
        }
    }

    #[test]
    fn incoming_direction_cycle_re_reaches_root() {
        let (_tmp, db) = cycle_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a:Function)<-[:CALLS*1..3]-(b:Function) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert!(names.contains(&"A"), "cycle must re-reach root: {names:?}");
        assert_eq!(result.row_count, 3, "A, B, C: {names:?}");
    }

    #[test]
    fn undirected_diamond_multiplicity_and_min_depth() {
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)-[:CALLS*1..2]-(b) WHERE a.name = 'A' RETURN b.name",
        );
        let d_count = result
            .rows
            .iter()
            .filter(|r| r[0].as_str() == Some("D"))
            .count();
        assert_eq!(d_count, 1, "D reachable via two paths must appear once");
        let result = assert_equivalent(
            &db,
            "MATCH (a)<-[:CALLS*2..3]-(b) WHERE a.name = 'A' RETURN b.name",
        );
        let names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].as_str()).collect();
        assert!(!names.contains(&"B"), "B is depth 1 only: {names:?}");
    }

    #[test]
    fn incoming_direction_distinct_collapse() {
        let (_tmp, db) = diamond_db();
        let result = assert_equivalent(
            &db,
            "MATCH (a)<-[:CALLS*1..3]-(b) WHERE a.name = 'A' RETURN b.kind",
        );
        assert_eq!(result.row_count, 1, "identical projected rows collapse");
    }

    /// Property-style sweep: for every representative fixture x query
    /// template x arrow spelling, the fast path must engage and produce the
    /// same result set as the SQL CTE.
    #[test]
    fn equivalence_sweep_over_fixtures_templates_and_directions() {
        let fixtures: Vec<(&str, (tempfile::TempDir, IndexDb))> = vec![
            ("cycle", cycle_db()),
            ("diamond", diamond_db()),
            (
                "multi-depth shortcut",
                setup_db(
                    &[
                        ("A", "function", "uA"),
                        ("B", "function", "uB"),
                        ("C", "function", "uC"),
                    ],
                    &[("uA", "uB"), ("uB", "uC"), ("uA", "uC")],
                ),
            ),
            (
                "ghost uid",
                setup_db(
                    &[("A", "function", "uA"), ("B", "function", "uB")],
                    &[("uA", "uGhost"), ("uGhost", "uB")],
                ),
            ),
        ];
        let templates = [
            "MATCH (a){ARROW}(b) WHERE a.name = 'A' RETURN b.name",
            "MATCH (a){ARROW}(b) WHERE a.name = 'A' RETURN a.name, b.name, b.kind",
            "MATCH (a){ARROW}(b:Function) WHERE a.symbol_uid = 'uA' RETURN b.name",
            "MATCH (a){ARROW}(b) WHERE a.name = 'A' RETURN b.kind",
            // Destination-side equality on top of the seed pin ('B' exists
            // in every fixture, reachable at varying depths or not at all).
            "MATCH (a){ARROW}(b) WHERE a.name = 'A' AND b.name = 'B' RETURN a.name, b.name",
        ];
        let ranges = [":CALLS*1..3", ":CALLS*2..3", ":CALLS*2..2"];
        for (fixture_name, (_tmp, db)) in &fixtures {
            for template in &templates {
                for range in &ranges {
                    for arrow in direction_spellings(range) {
                        let query = template.replace("{ARROW}", &arrow);
                        let parsed = parse_query(&query);
                        let fast = try_execute(&parsed, db).unwrap().unwrap_or_else(|| {
                            panic!("fast path must engage [{fixture_name}]: {query}")
                        });
                        let sql = execute_with_options(&parsed, db, false).unwrap();
                        assert_eq!(
                            sorted_rows(&fast),
                            sorted_rows(&sql),
                            "result sets diverge [{fixture_name}]: {query}"
                        );
                        assert_eq!(
                            fast.columns, sql.columns,
                            "columns diverge [{fixture_name}]: {query}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn incoming_direction_order_by_matches_positionally() {
        let (_tmp, db) = diamond_db();
        let query = "MATCH (a)<-[:CALLS*1..2]-(b) WHERE a.name = 'A' \
                     RETURN b.name ORDER BY b.name LIMIT 2";
        let parsed = parse_query(query);
        let fast = try_execute(&parsed, &db).unwrap().expect("must engage");
        let sql = execute_with_options(&parsed, &db, false).unwrap();
        assert_eq!(fast.rows, sql.rows, "ordered rows must match exactly");
    }

    // ── Eligibility gate: conservative fallbacks ────────────────

    #[test]
    fn gate_rejects_ineligible_shapes() {
        let (_tmp, db) = diamond_db();
        for query in [
            // Non-CALLS edge kind.
            "MATCH (a)-[:IMPORTS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
            "MATCH (a)-[:CONTAINS_FILE*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
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

    // ── Ineligibility reasons: one trigger per variant ───────────

    fn gate_reason(input: &str) -> FastPathIneligibility {
        build_plan(&parse_query(input))
            .err()
            .unwrap_or_else(|| panic!("gate must reject: {input}"))
    }

    #[test]
    fn gate_reason_covers_every_rejection_point() {
        use FastPathIneligibility as R;
        let cases: Vec<(&str, R)> = vec![
            // Structure.
            (
                "MATCH (x:Function) OPTIONAL MATCH (x)-[:CALLS*1..2]->(b) RETURN x.name",
                R::NotSingleMatch,
            ),
            (
                "OPTIONAL MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
                R::OptionalMatch,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b), (c:Function) WHERE a.name = 'A' RETURN b.name",
                R::MultiplePatternsInMatch,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b)-[:CALLS*1..2]->(c) WHERE a.name = 'A' RETURN b.name",
                R::NotSingleRelationshipSegment,
            ),
            (
                "MATCH (a)-[:CALLS]->(b) WHERE a.name = 'A' RETURN b.name",
                R::SingleHopPattern,
            ),
            (
                "MATCH (a {name: 'A'})-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
                R::InlineNodeProperties,
            ),
            (
                "MATCH (a:File)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
                R::LabelNotSymbolsTable {
                    label: "File".into(),
                },
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(a) WHERE a.name = 'A' RETURN a.name",
                R::SameVariableOnBothEndpoints,
            ),
            // Edge.
            (
                "MATCH (a)-[:IMPORTS*1..2]->(b) WHERE a.name = 'A' RETURN b.name",
                R::EdgeKindNotEligible {
                    kind: "IMPORTS".into(),
                },
            ),
            // WHERE.
            (
                "MATCH (a)-[:CALLS*1..2]->(b) RETURN b.name",
                R::NoWhereClause,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name CONTAINS 'A' RETURN b.name",
                R::WhereNotSimpleEquality,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' OR a.name = 'B' RETURN b.name",
                R::WhereNotSimpleEquality,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.start_line = 1 RETURN b.name",
                R::WhereNotSimpleEquality,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.file_path = 'src/x.rs' RETURN b.name",
                R::WhereOnNonSeedColumn {
                    prop: "file_path".into(),
                },
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE c.name = 'A' RETURN b.name",
                R::WhereUnknownVariable { var: "c".into() },
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE b.name = 'D' RETURN b.name",
                R::NoSeedEquality,
            ),
            // RETURN.
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN COUNT(*)",
                R::ReturnNotSimpleProperty,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN c.name",
                R::ReturnUnknownVariable { var: "c".into() },
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.language",
                R::ReturnPropertyNotProjectable {
                    prop: "language".into(),
                },
            ),
            // ORDER BY / LIMIT.
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name ORDER BY b.start_line",
                R::OrderByNotReturnedProperty,
            ),
            (
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name LIMIT 100000",
                R::LimitTooLarge {
                    requested: 100000,
                    max: 1000,
                },
            ),
        ];
        for (query, expected) in cases {
            assert_eq!(gate_reason(query), expected, "query: {query}");
        }
    }

    #[test]
    fn gate_reason_empty_return_via_hand_built_ast() {
        // The parser rejects an empty RETURN, so exercise the gate directly.
        let mut query =
            parse_query("MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name");
        query.return_clause.items.clear();
        assert_eq!(
            build_plan(&query).unwrap_err(),
            FastPathIneligibility::EmptyReturn
        );
    }

    /// Snapshot lock on the agent-facing reason tokens: `graph_query` callers
    /// may key behavior off these strings, so any drift must be deliberate.
    #[test]
    fn ineligibility_display_is_snapshot_locked() {
        use FastPathIneligibility as R;
        let cases: Vec<(R, &str)> = vec![
            (R::NotSingleMatch, "not_single_match"),
            (R::OptionalMatch, "optional_match"),
            (R::MultiplePatternsInMatch, "multiple_patterns_in_match"),
            (
                R::NotSingleRelationshipSegment,
                "not_single_relationship_segment",
            ),
            (R::SingleHopPattern, "single_hop_pattern"),
            (R::InlineNodeProperties, "inline_node_properties"),
            (
                R::LabelNotSymbolsTable {
                    label: "File".into(),
                },
                "label_not_symbols_table(File)",
            ),
            (
                R::SameVariableOnBothEndpoints,
                "same_variable_on_both_endpoints",
            ),
            (
                R::EdgeKindNotEligible {
                    kind: "IMPORTS".into(),
                },
                "edge_kind_not_eligible(IMPORTS)",
            ),
            (R::NoWhereClause, "no_where_clause"),
            (R::WhereNotSimpleEquality, "where_not_simple_equality"),
            (
                R::WhereOnNonSeedColumn {
                    prop: "file_path".into(),
                },
                "where_on_non_seed_column(file_path)",
            ),
            (
                R::WhereUnknownVariable { var: "c".into() },
                "where_unknown_variable(c)",
            ),
            (R::NoSeedEquality, "no_seed_equality"),
            (R::EmptyReturn, "empty_return"),
            (R::ReturnNotSimpleProperty, "return_not_simple_property"),
            (
                R::ReturnUnknownVariable { var: "c".into() },
                "return_unknown_variable(c)",
            ),
            (
                R::ReturnPropertyNotProjectable {
                    prop: "language".into(),
                },
                "return_property_not_projectable(language)",
            ),
            (
                R::OrderByNotReturnedProperty,
                "order_by_not_returned_property",
            ),
            (
                R::LimitTooLarge {
                    requested: 5000,
                    max: 1000,
                },
                "limit_too_large(5000>1000)",
            ),
        ];
        for (reason, expected) in cases {
            assert_eq!(reason.to_string(), expected);
        }
    }

    // ── Decision metadata ────────────────────────────────────────

    #[test]
    fn decision_covers_used_fallback_disabled_and_not_applicable() {
        let eligible = parse_query("MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name");
        assert_eq!(decide_with_env(&eligible, true), FastPathDecision::Used);
        assert_eq!(
            decide_with_env(&eligible, false),
            FastPathDecision::DisabledByEnv
        );

        let ineligible = parse_query("MATCH (a)-[:CALLS*1..2]->(b) RETURN b.name");
        assert_eq!(
            decide_with_env(&ineligible, true),
            FastPathDecision::Fallback(FastPathIneligibility::NoWhereClause)
        );
        // The executor consults the env toggle before the gate; the decision
        // mirrors that ordering.
        assert_eq!(
            decide_with_env(&ineligible, false),
            FastPathDecision::DisabledByEnv
        );

        // Shapes that never reach the variable-length branch.
        for query in [
            "MATCH (f:Function) RETURN f.name",
            "MATCH (a)-[:CALLS]->(b) WHERE a.name = 'A' RETURN b.name",
            "MATCH (x:Function) OPTIONAL MATCH (x)-[:CALLS]->(b) RETURN x.name",
        ] {
            assert_eq!(
                decide_with_env(&parse_query(query), true),
                FastPathDecision::NotApplicable,
                "query: {query}"
            );
        }
    }

    #[test]
    fn decision_for_query_handles_union_and_parse_errors() {
        use crate::cypher::fast_path_decision_for_query;
        // UNION sub-queries always run the SQL translation.
        assert_eq!(
            fast_path_decision_for_query(
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name \
                 UNION MATCH (c:Function) RETURN c.name"
            ),
            FastPathDecision::NotApplicable
        );
        // Parse errors have no fast-path story (execution surfaced the error).
        assert_eq!(
            fast_path_decision_for_query("this is not cypher"),
            FastPathDecision::NotApplicable
        );
        // String-level convenience agrees with the AST-level decision.
        assert_eq!(
            fast_path_decision_for_query(
                "MATCH (a)-[:CALLS*1..2]->(b) WHERE a.name = 'A' RETURN b.name LIMIT 100000"
            ),
            FastPathDecision::Fallback(FastPathIneligibility::LimitTooLarge {
                requested: 100000,
                max: 1000,
            })
        );
    }

    #[test]
    fn decision_metadata_json_shapes_are_snapshot_locked() {
        assert_eq!(
            FastPathDecision::Used.as_metadata(),
            Some(serde_json::json!({ "used": true }))
        );
        assert_eq!(
            FastPathDecision::Fallback(FastPathIneligibility::LimitTooLarge {
                requested: 5000,
                max: 1000,
            })
            .as_metadata(),
            Some(serde_json::json!({
                "used": false,
                "reason": "limit_too_large(5000>1000)",
            }))
        );
        assert_eq!(
            FastPathDecision::DisabledByEnv.as_metadata(),
            Some(serde_json::json!({
                "used": false,
                "reason": "disabled(CODECORTEX_CYPHER_FAST_PATH=0)",
            }))
        );
        assert_eq!(FastPathDecision::NotApplicable.as_metadata(), None);
    }

    // ── Gate constants vs. shared catalog declaration ────────────

    /// The fast-path eligible set must be exactly the shared declaration and
    /// stay a subset of catalog kinds with `variable_length` support: the
    /// fast path serves `*m..n` traversals, so a kind the SQL CTE cannot
    /// expand recursively must never become gate-eligible.
    #[test]
    fn fast_path_kinds_derive_from_catalog_declaration() {
        use cc_model::graph_catalog::{graph_relationship, tool_graph_subsets};

        assert_eq!(
            FastPathConfig::DEFAULT.eligible_edge_kinds,
            tool_graph_subsets::CYPHER_FAST_PATH.kinds(),
            "FastPathConfig::DEFAULT must reference the shared declaration"
        );
        for kind in FastPathConfig::DEFAULT.eligible_edge_kinds {
            let rel = graph_relationship(kind)
                .unwrap_or_else(|| panic!("fast-path kind {kind} missing from catalog"));
            assert!(
                rel.variable_length,
                "fast-path kind {kind} lacks catalog variable_length support"
            );
        }
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
