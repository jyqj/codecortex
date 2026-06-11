use super::ast::*;
use super::traversal_semantics::{
    CyclePolicy, ProjectionDedup, TupleMultiplicity, WalkOrientation, VARLEN_TRAVERSAL,
};
use cc_db::index_db::IndexDb;
use cc_model::graph_catalog::graph_relationships;
use cc_model::{CcError, CcResult};
use std::collections::HashMap;

/// Default LIMIT applied when a Cypher query omits an explicit LIMIT clause.
pub(crate) const DEFAULT_CYPHER_LIMIT: usize = 50;

// ── Executor ───────────────────────────────────────

/// Edge-type to table mapping.
pub(crate) struct EdgeTableInfo {
    pub(crate) table: &'static str,
    pub(crate) src_col: &'static str,
    pub(crate) dst_col: &'static str,
    /// Whether src/dst are symbol_uid (true) or file_path (false).
    pub(crate) src_is_symbol: bool,
    pub(crate) dst_is_symbol: bool,
    /// Override the default join column on the node table side.
    /// When `None`, defaults to `symbol_uid` (if `*_is_symbol`) or `file_path`.
    pub(crate) src_join_key: Option<&'static str>,
    pub(crate) dst_join_key: Option<&'static str>,
    /// Optional SQL filter appended to WHERE clause (e.g. "call_kind = 'http'").
    /// The expression is injected verbatim with the edge alias prefix `{edge_alias}.`.
    pub(crate) extra_filter: Option<&'static str>,
    /// Override the full JOIN ON expression for the source/destination side.
    /// Placeholders `{src}`, `{dst}`, `{e}` are replaced with actual aliases.
    /// Empty string means "skip this JOIN entirely" (for pseudo-UID endpoints
    /// that have no corresponding node table, e.g. `dir::` prefixed UIDs).
    pub(crate) src_join_on: Option<&'static str>,
    pub(crate) dst_join_on: Option<&'static str>,
}

pub(crate) fn edge_table_map() -> HashMap<&'static str, EdgeTableInfo> {
    graph_relationships()
        .iter()
        .map(|rel| {
            (
                rel.edge,
                EdgeTableInfo {
                    table: rel.table,
                    src_col: rel.source.column,
                    dst_col: rel.destination.column,
                    src_is_symbol: rel.source.is_symbol,
                    dst_is_symbol: rel.destination.is_symbol,
                    src_join_key: rel.source.join_key,
                    dst_join_key: rel.destination.join_key,
                    extra_filter: rel.extra_filter,
                    src_join_on: rel.source.join_on,
                    dst_join_on: rel.destination.join_on,
                },
            )
        })
        .collect()
}

/// Validate that a SQL identifier (table alias, column name) only contains safe characters.
/// Returns Ok(()) if valid, Err if it contains anything other than [a-zA-Z0-9_].
pub(crate) fn validate_sql_ident(ident: &str) -> CcResult<()> {
    if ident.is_empty() {
        return Err(CcError::Search("empty SQL identifier".into()));
    }
    if ident.chars().all(|c| c.is_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err(CcError::Search(format!(
            "unsafe SQL identifier: {:?}",
            ident
        )))
    }
}

/// Map node label to DB table name.
pub(crate) fn label_table(label: &str) -> &'static str {
    match label {
        "Function" | "Class" | "Method" | "Module" | "Interface" | "Enum" | "Type" | "Variable"
        | "Symbol" => "symbols",
        "File" => "files",
        "Chunk" => "chunks",
        "Route" => "routes",
        _ => "symbols",
    }
}

/// Map node label to a kind filter value (for the `symbols.kind` column).
pub(crate) fn label_kind_filter(label: &str) -> Option<&'static str> {
    match label {
        "Function" => Some("function"),
        "Class" => Some("class"),
        "Method" => Some("method"),
        "Module" => Some("module"),
        "Interface" => Some("interface"),
        "Enum" => Some("enum"),
        "Type" => Some("type_alias"),
        "Variable" => Some("variable"),
        // Route maps to route_nodes table (no kind column), so no kind filter needed.
        "Route" => None,
        _ => None,
    }
}

/// Validate that a regex pattern only uses features that can be faithfully
/// converted to SQL LIKE.  Legacy helper retained for test coverage of the
/// old LIKE-based path. Production `=~` now uses SQLite REGEXP directly.
#[cfg(test)]
pub(crate) fn validate_regex_for_like(pattern: &str) -> Result<(), String> {
    // Unsupported character-class / group / anchor tokens.
    let unsupported: &[(&str, &str)] = &[
        ("[", "character class [...]"),
        ("]", "character class [...]"),
        ("^", "anchor ^"),
        ("$", "anchor $"),
        ("|", "alternation |"),
        ("{", "quantifier {n,m}"),
        ("}", "quantifier {n,m}"),
        ("(?", "non-capturing/lookahead group (?...)"),
        ("+?", "lazy quantifier +?"),
        ("*?", "lazy quantifier *?"),
        ("\\d", "shorthand class \\d"),
        ("\\D", "shorthand class \\D"),
        ("\\w", "shorthand class \\w"),
        ("\\W", "shorthand class \\W"),
        ("\\s", "shorthand class \\s"),
        ("\\S", "shorthand class \\S"),
        ("\\b", "word boundary \\b"),
        ("\\B", "word boundary \\B"),
        ("\\1", "back-reference \\1"),
        ("\\2", "back-reference \\2"),
        ("\\3", "back-reference \\3"),
    ];

    for (token, description) in unsupported {
        if pattern.contains(token) {
            return Err(format!(
                "regex pattern contains unsupported feature for LIKE conversion: {description}. \
                 Only `.*`, `.+`, `.` (single char), and literal characters are supported in =~ patterns."
            ));
        }
    }

    Ok(())
}

/// Map a property reference to its SQL column, given the variable's alias and table.
/// Safety: the lexer constrains Ident tokens to `[a-zA-Z_][a-zA-Z0-9_]*`,
/// so `prop.var` and `prop.prop` are guaranteed safe for SQL interpolation.
/// The `validate_sql_ident` check below is a defense-in-depth assertion.
fn prop_to_sql_col(prop: &PropRef) -> String {
    debug_assert!(
        validate_sql_ident(&prop.var).is_ok(),
        "unsafe var: {}",
        prop.var
    );
    debug_assert!(
        validate_sql_ident(&prop.prop).is_ok(),
        "unsafe prop: {}",
        prop.prop
    );
    format!("{}.{}", prop.var, prop.prop)
}

/// Build a SQL WHERE fragment and collect params from a single Expr node.
pub(crate) fn expr_to_sql(expr: &Expr, params: &mut Vec<String>) -> CcResult<String> {
    match expr {
        Expr::Comparison { left, op, right } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            let sql_op = match op {
                CmpOp::Eq => "=",
                CmpOp::Neq => "<>",
                CmpOp::Lt => "<",
                CmpOp::Gt => ">",
                CmpOp::Lte => "<=",
                CmpOp::Gte => ">=",
            };
            match right {
                Value::String(s) => {
                    params.push(s.clone());
                    Ok(format!("{col} {sql_op} ?{idx}"))
                }
                Value::Int(n) => {
                    params.push(n.to_string());
                    Ok(format!("{col} {sql_op} ?{idx}"))
                }
                Value::Float(f) => {
                    params.push(f.to_string());
                    Ok(format!("{col} {sql_op} ?{idx}"))
                }
                Value::Bool(b) => {
                    params.push(if *b { "1".into() } else { "0".into() });
                    Ok(format!("{col} {sql_op} ?{idx}"))
                }
                Value::Null => {
                    if matches!(op, CmpOp::Eq) {
                        Ok(format!("{col} IS NULL"))
                    } else {
                        Ok(format!("{col} IS NOT NULL"))
                    }
                }
            }
        }
        Expr::Regex { left, pattern } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(pattern.clone());
            Ok(format!("{col} REGEXP ?{idx}"))
        }
        Expr::Contains { left, value } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(format!("%{value}%"));
            Ok(format!("{col} LIKE ?{idx}"))
        }
        Expr::StartsWith { left, value } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(format!("{value}%"));
            Ok(format!("{col} LIKE ?{idx}"))
        }
        Expr::EndsWith { left, value } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(format!("%{value}"));
            Ok(format!("{col} LIKE ?{idx}"))
        }
        Expr::And(l, r) => {
            let ls = expr_to_sql(l, params)?;
            let rs = expr_to_sql(r, params)?;
            Ok(format!("({ls} AND {rs})"))
        }
        Expr::Or(l, r) => {
            let ls = expr_to_sql(l, params)?;
            let rs = expr_to_sql(r, params)?;
            Ok(format!("({ls} OR {rs})"))
        }
        Expr::Not(inner) => {
            let s = expr_to_sql(inner, params)?;
            Ok(format!("NOT ({s})"))
        }
        Expr::Degree {
            var,
            kind,
            op,
            value,
        } => {
            let sql_op = match op {
                CmpOp::Eq => "=",
                CmpOp::Neq => "<>",
                CmpOp::Lt => "<",
                CmpOp::Gt => ">",
                CmpOp::Lte => "<=",
                CmpOp::Gte => ">=",
            };
            let idx = params.len() + 1;
            match value {
                Value::Int(n) => params.push(n.to_string()),
                Value::Float(f) => params.push(f.to_string()),
                _ => params.push("0".to_string()),
            }
            // Generate SQL subquery counting call_edges for the variable.
            // The variable alias is used as-is (safe: Ident tokens are validated).
            let in_sub = format!(
                "(SELECT COUNT(*) FROM call_edges WHERE callee_symbol_uid = {var}.symbol_uid)"
            );
            let out_sub = format!(
                "(SELECT COUNT(*) FROM call_edges WHERE caller_symbol_uid = {var}.symbol_uid)"
            );
            match kind {
                DegreeKind::In => Ok(format!("{in_sub} {sql_op} ?{idx}")),
                DegreeKind::Out => Ok(format!("{out_sub} {sql_op} ?{idx}")),
                DegreeKind::Total => Ok(format!("({in_sub} + {out_sub}) {sql_op} ?{idx}")),
            }
        }
    }
}

fn table_json_expr(table: &str, alias: &str) -> CcResult<String> {
    let pairs: &[(&str, &str)] = match table {
        "symbols" => &[
            ("symbol_id", "symbol_id"),
            ("symbol_uid", "symbol_uid"),
            ("name", "name"),
            ("kind", "kind"),
            ("file_path", "file_path"),
            ("container", "container"),
            ("start_line", "start_line"),
            ("end_line", "end_line"),
            ("qname", "qname"),
            ("signature", "signature"),
        ],
        "files" => &[
            ("file_path", "file_path"),
            ("language", "language"),
            ("size", "size"),
            ("parser_tier", "parser_tier"),
            ("indexed_at", "indexed_at"),
            ("summary", "summary"),
        ],
        "chunks" => &[
            ("chunk_id", "chunk_id"),
            ("file_path", "file_path"),
            ("language", "language"),
            ("chunk_index", "chunk_index"),
            ("start_line", "start_line"),
            ("end_line", "end_line"),
            ("breadcrumb", "breadcrumb"),
            ("symbol_name", "symbol_name"),
            ("symbol_kind", "symbol_kind"),
            ("token_estimate", "token_estimate"),
        ],
        "imports" => &[
            ("file_path", "file_path"),
            ("import_string", "import_string"),
            ("resolved_path", "resolved_path"),
            ("imported_name", "imported_name"),
            ("alias", "alias"),
            ("is_namespace", "is_namespace"),
            ("is_default", "is_default"),
            ("is_reexport", "is_reexport"),
        ],
        "symbol_refs" => &[
            ("ref_id", "ref_id"),
            ("file_path", "file_path"),
            ("symbol_name", "symbol_name"),
            ("container", "container"),
            ("ref_kind", "ref_kind"),
            ("line", "line"),
            ("column_no", "column_no"),
            ("target_symbol_id", "target_symbol_id"),
            ("target_file_path", "target_file_path"),
            ("target_symbol_uid", "target_symbol_uid"),
            ("ref_name", "ref_name"),
            ("resolution_kind", "resolution_kind"),
        ],
        "call_edges" => &[
            ("edge_id", "edge_id"),
            ("file_path", "file_path"),
            ("caller_symbol", "caller_symbol"),
            ("callee_symbol", "callee_symbol"),
            ("line", "line"),
            ("target_symbol_id", "target_symbol_id"),
            ("target_file_path", "target_file_path"),
            ("caller_symbol_uid", "caller_symbol_uid"),
            ("callee_symbol_uid", "callee_symbol_uid"),
            ("dispatch_kind", "dispatch_kind"),
            ("call_kind", "call_kind"),
            ("resolution_kind", "resolution_kind"),
        ],
        "test_edges" => &[
            ("edge_id", "edge_id"),
            ("test_file_path", "test_file_path"),
            ("code_file_path", "code_file_path"),
            ("reason", "reason"),
            ("confidence", "confidence"),
        ],
        "routes" => &[
            ("edge_id", "edge_id"),
            ("route_id", "route_id"),
            ("file_path", "file_path"),
            ("route_path", "route_path"),
            ("handler_name", "handler_name"),
            ("method", "method"),
            ("line", "line"),
            ("end_line", "end_line"),
            ("handler_symbol_uid", "handler_symbol_uid"),
            ("framework", "framework"),
            ("route_kind", "route_kind"),
            ("normalized_path", "normalized_path"),
            ("confidence", "confidence"),
            ("resolution_strategy", "resolution_strategy"),
            ("resolution_confidence", "resolution_confidence"),
        ],
        "data_flow_edges" => &[
            ("edge_id", "edge_id"),
            ("file_path", "file_path"),
            ("source_symbol_uid", "source_symbol_uid"),
            ("target_symbol_uid", "target_symbol_uid"),
            ("flow_kind", "flow_kind"),
            ("line", "line"),
            ("confidence", "confidence"),
        ],
        "co_change_edges" => &[
            ("edge_id", "edge_id"),
            ("file_a", "file_a"),
            ("file_b", "file_b"),
            ("co_change_count", "co_change_count"),
            ("total_commits_a", "total_commits_a"),
            ("total_commits_b", "total_commits_b"),
            ("confidence", "confidence"),
        ],
        other => {
            return Err(CcError::Search(format!(
                "COLLECT/RETURN of full rows is not supported for table {other}"
            )))
        }
    };

    let args = pairs
        .iter()
        .map(|(k, col)| format!("'{k}', {alias}.{col}"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("json_object({args})"))
}

fn build_projections(
    items: &[ReturnItem],
    alias_tables: &HashMap<String, String>,
) -> CcResult<Vec<SelectProjection>> {
    items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let fallback = format!("__cc_col_{idx}");
            let projection = match item {
                ReturnItem::Prop(pr, alias) => {
                    let col = prop_to_sql_col(pr);
                    // Use "var__prop" as source_key to avoid collisions when
                    // multiple RETURN items share the same property name
                    // (e.g. RETURN a.name, b.name).
                    let default_key = format!("{}_{}", pr.var, pr.prop);
                    let source_key = alias.clone().unwrap_or_else(|| default_key.clone());
                    let sql = match alias {
                        Some(a) => format!("{col} AS {a}"),
                        None => format!("{col} AS {default_key}"),
                    };
                    SelectProjection {
                        sql,
                        source_key,
                        output_name: alias
                            .clone()
                            .unwrap_or_else(|| format!("{}.{}", pr.var, pr.prop)),
                        item: item.clone(),
                    }
                }
                ReturnItem::Count(count_arg, distinct, alias) => {
                    let source_key = alias.clone().unwrap_or_else(|| fallback.clone());
                    let dist = if *distinct { "DISTINCT " } else { "" };
                    let expr = match count_arg {
                        CountArg::Star => "COUNT(*)".to_string(),
                        CountArg::Var(var) => format!("COUNT({dist}{var}.rowid)"),
                        CountArg::Prop(pr) => {
                            let col = prop_to_sql_col(pr);
                            format!("COUNT({dist}{col})")
                        }
                    };
                    let default_name = match count_arg {
                        CountArg::Star => "COUNT(*)".to_string(),
                        CountArg::Var(var) => format!("COUNT({var})"),
                        CountArg::Prop(pr) => format!("COUNT({}.{})", pr.var, pr.prop),
                    };
                    SelectProjection {
                        sql: format!("{expr} AS {source_key}"),
                        source_key,
                        output_name: alias.clone().unwrap_or(default_name),
                        item: item.clone(),
                    }
                }
                ReturnItem::Aggregate(func, pr, distinct, alias) => {
                    let col = prop_to_sql_col(pr);
                    let source_key = alias.clone().unwrap_or_else(|| fallback.clone());
                    let dist = if *distinct { "DISTINCT " } else { "" };
                    let expr = format!("{func}({dist}{col})");
                    let output_name = alias
                        .clone()
                        .unwrap_or_else(|| format!("{func}({}.{})", pr.var, pr.prop));
                    SelectProjection {
                        sql: format!("{expr} AS {source_key}"),
                        source_key,
                        output_name,
                        item: item.clone(),
                    }
                }
                ReturnItem::Collect(expr, distinct, alias) => {
                    let source_key = alias.clone().unwrap_or_else(|| fallback.clone());
                    let inner = match expr {
                        CollectExpr::Prop(pr) => prop_to_sql_col(pr),
                        CollectExpr::Var(var) => {
                            let table = alias_tables.get(var).ok_or_else(|| {
                                CcError::Search(format!("unknown variable in COLLECT(): {var}"))
                            })?;
                            table_json_expr(table, var)?
                        }
                    };
                    let output_name = alias.clone().unwrap_or_else(|| match expr {
                        CollectExpr::Prop(pr) => format!("COLLECT({}.{})", pr.var, pr.prop),
                        CollectExpr::Var(var) => format!("COLLECT({var})"),
                    });
                    // COLLECT(DISTINCT x) -> use a subquery for distinct values
                    let agg_expr = if *distinct {
                        format!("json_group_array(DISTINCT {inner})")
                    } else {
                        format!("json_group_array({inner})")
                    };
                    SelectProjection {
                        sql: format!("{agg_expr} AS {source_key}"),
                        source_key,
                        output_name,
                        item: item.clone(),
                    }
                }
                ReturnItem::Var(var, alias) => {
                    let source_key = alias.clone().unwrap_or_else(|| fallback.clone());
                    let table = alias_tables.get(var).ok_or_else(|| {
                        CcError::Search(format!("unknown variable in RETURN: {var}"))
                    })?;
                    let expr = table_json_expr(table, var)?;
                    SelectProjection {
                        sql: format!("{expr} AS {source_key}"),
                        source_key,
                        output_name: alias.clone().unwrap_or_else(|| var.clone()),
                        item: item.clone(),
                    }
                }
            };
            Ok(projection)
        })
        .collect()
}

/// Build a GROUP BY clause if the projections contain any aggregate items
/// (COUNT, SUM, AVG, MIN, MAX, COLLECT) alongside non-aggregate items.
/// Returns an empty string when no GROUP BY is needed.
fn build_group_by_clause(projections: &[SelectProjection]) -> String {
    let has_agg = projections.iter().any(|p| {
        matches!(
            &p.item,
            ReturnItem::Count(_, _, _)
                | ReturnItem::Aggregate(_, _, _, _)
                | ReturnItem::Collect(_, _, _)
        )
    });
    if !has_agg {
        return String::new();
    }
    let group_cols: Vec<String> = projections
        .iter()
        .filter_map(|p| match &p.item {
            ReturnItem::Prop(pr, _) => Some(prop_to_sql_col(pr)),
            ReturnItem::Var(var, _) => Some(format!("{}.rowid", var)),
            _ => None,
        })
        .collect();
    if group_cols.is_empty() {
        return String::new();
    }
    format!(" GROUP BY {}", group_cols.join(", "))
}

pub fn execute(query: &CypherQuery, db: &IndexDb) -> CcResult<CypherResult> {
    execute_with_options(query, db, true)
}

/// Detect the special case `MATCH (f:Label) OPTIONAL MATCH (f)-[:R]->(g) ...`
/// — anchor on the required source node so it survives even with no matching
/// edge/target. Shared by the routing in `execute_with_options` and by the
/// fast-path decision metadata (`fast_path::decide`), so the two cannot drift.
pub(crate) fn detect_two_clause_optional(query: &CypherQuery) -> Option<&MatchClause> {
    let first_match = query.match_clauses.first()?;
    let pattern = first_match.patterns.first()?;
    (query.match_clauses.len() == 2
        && !first_match.is_optional
        && pattern.rels.is_empty()
        && first_match.patterns.len() == 1)
        .then(|| &query.match_clauses[1])
        .filter(|m1| m1.is_optional && m1.patterns.len() == 1 && m1.patterns[0].rels.len() == 1)
        .filter(|m1| {
            let anchor_var = pattern.nodes[0].var.as_deref();
            anchor_var.is_some() && m1.patterns[0].nodes[0].var.as_deref() == anchor_var
        })
}

/// Execute with explicit control over the lazy-BFS fast path
/// (`allow_fast_path = false` forces the SQL translation, used by UNION
/// sub-queries and by equivalence tests).
pub(crate) fn execute_with_options(
    query: &CypherQuery,
    db: &IndexDb,
    allow_fast_path: bool,
) -> CcResult<CypherResult> {
    // We only support the first MATCH clause's first pattern for now.
    if query.match_clauses.is_empty() || query.match_clauses[0].patterns.is_empty() {
        return Ok(CypherResult {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            default_limit_applied: false,
            limit: None,
        });
    }

    let first_match = &query.match_clauses[0];
    let pattern = &first_match.patterns[0];

    let two_clause_optional = detect_two_clause_optional(query);

    let translated = if let Some(m1) = two_clause_optional {
        translate_optional_match(query, pattern.nodes[0].label.as_deref(), &m1.patterns[0])?
    } else if pattern.rels.is_empty() {
        // Single-node query.
        translate_single_node(query, pattern)?
    } else if pattern.rels.len() == 1 {
        let rel = &pattern.rels[0];
        if rel.min_hops == 1 && rel.max_hops == 1 {
            translate_single_hop(query, pattern, first_match.is_optional)?
        } else {
            if allow_fast_path && super::fast_path::env_enabled() {
                if let Some(result) = super::fast_path::try_execute(query, db)? {
                    return Ok(result);
                }
            }
            translate_variable_length(query, pattern)?
        }
    } else {
        return Err(CcError::Search(
            "multi-hop chains with different edge types are not yet supported; \
             use variable-length paths (e.g. *1..3) for same-type traversal"
                .into(),
        ));
    };

    let columns: Vec<String> = translated
        .projections
        .iter()
        .map(|p| p.output_name.clone())
        .collect();

    // Execute the SQL.
    let json_rows = db
        .reads()
        .query_json(&translated.sql, &translated.params)
        .map_err(|e| CcError::Search(format!("cypher SQL error: {e} [sql={}]", translated.sql)))?;

    // Convert JSON objects to row arrays.
    let rows: Vec<Vec<serde_json::Value>> = json_rows
        .iter()
        .map(|obj| {
            translated
                .projections
                .iter()
                .map(|projection| {
                    let val = obj
                        .get(&projection.source_key)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    match &projection.item {
                        ReturnItem::Var(_, _) | ReturnItem::Collect(_, _, _) => match val {
                            serde_json::Value::String(ref s) => {
                                serde_json::from_str::<serde_json::Value>(s)
                                    .unwrap_or_else(|_| serde_json::Value::String(s.clone()))
                            }
                            serde_json::Value::Null
                                if matches!(&projection.item, ReturnItem::Collect(_, _, _)) =>
                            {
                                serde_json::Value::Array(Vec::new())
                            }
                            other => other,
                        },
                        _ => val,
                    }
                })
                .collect()
        })
        .collect();

    let row_count = rows.len();
    let default_limit_applied = query.limit.is_none();
    let limit = Some(query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT));
    Ok(CypherResult {
        columns,
        rows,
        row_count,
        default_limit_applied,
        limit,
    })
}

/// Convert an OrderItem to its SQL representation.
fn order_item_to_sql(oi: &OrderItem) -> String {
    let col = match &oi.expr {
        OrderExpr::Prop(pr) => prop_to_sql_col(pr),
        OrderExpr::Alias(name) => name.clone(),
    };
    let dir = if oi.desc { "DESC" } else { "ASC" };
    format!("{col} {dir}")
}

/// Build the SELECT keyword with optional DISTINCT.
fn select_keyword(return_clause: &ReturnClause) -> &'static str {
    if return_clause.distinct {
        "SELECT DISTINCT"
    } else {
        "SELECT"
    }
}

pub(crate) fn translate_single_node(
    query: &CypherQuery,
    pattern: &PathPattern,
) -> CcResult<TranslatedQuery> {
    let node = &pattern.nodes[0];
    let alias = node.var.as_deref().unwrap_or("n");
    let table = node.label.as_deref().map(label_table).unwrap_or("symbols");

    let alias_tables = HashMap::from([(alias.to_string(), table.to_string())]);
    let projections = build_projections(&query.return_clause.items, &alias_tables)?;
    let select_cols = projections
        .iter()
        .map(|p| p.sql.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut params: Vec<String> = Vec::new();

    let kw = select_keyword(&query.return_clause);
    let mut sql = format!("{kw} {select_cols} FROM {table} AS {alias}");

    let mut where_parts: Vec<String> = Vec::new();

    // Kind filter from label.
    if let Some(label) = node.label.as_deref() {
        if let Some(kind) = label_kind_filter(label) {
            where_parts.push(format!("{alias}.kind = ?{}", params.len() + 1));
            params.push(kind.to_string());
        }
    }

    // Inline properties.
    for (key, val) in &node.props {
        where_parts.push(format!("{alias}.{key} = ?{}", params.len() + 1));
        params.push(val.clone());
    }

    // WHERE clause.
    if let Some(wc) = &query.where_clause {
        where_parts.push(expr_to_sql(&wc.expr, &mut params)?);
    }

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    // GROUP BY (when aggregates are present).
    sql.push_str(&build_group_by_clause(&projections));

    // ORDER BY.
    if let Some(order_items) = &query.order_by {
        let parts: Vec<String> = order_items.iter().map(order_item_to_sql).collect();
        sql.push_str(" ORDER BY ");
        sql.push_str(&parts.join(", "));
    }

    // LIMIT.
    let limit = query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok(TranslatedQuery {
        sql,
        params,
        projections,
    })
}

pub(crate) fn translate_single_hop(
    query: &CypherQuery,
    pattern: &PathPattern,
    is_optional: bool,
) -> CcResult<TranslatedQuery> {
    let src_node = &pattern.nodes[0];
    let dst_node = &pattern.nodes[1];
    let rel = &pattern.rels[0];

    let edge_type_str = rel.rel_type.as_deref().unwrap_or("CALLS");
    let etm = edge_table_map();
    let edge_info = etm
        .get(edge_type_str)
        .ok_or_else(|| CcError::Search(format!("unknown edge type: {edge_type_str}")))?;

    let src_alias = src_node.var.as_deref().unwrap_or("src");
    let dst_alias = dst_node.var.as_deref().unwrap_or("dst");
    let src_table = src_node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");
    let dst_table = dst_node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");
    let edge_alias = "e";

    let alias_tables = HashMap::from([
        (src_alias.to_string(), src_table.to_string()),
        (dst_alias.to_string(), dst_table.to_string()),
        (edge_alias.to_string(), edge_info.table.to_string()),
    ]);
    let projections = build_projections(&query.return_clause.items, &alias_tables)?;
    let select_cols = projections
        .iter()
        .map(|p| p.sql.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut params: Vec<String> = Vec::new();

    // Build JOIN based on whether the edge connects by symbol_uid or file_path,
    // with optional explicit join key override or full ON-expression override.
    // When a join_on override is an empty string, the JOIN for that side is skipped
    // entirely (for pseudo-UID endpoints that have no corresponding node table).
    let skip_src_join = matches!(edge_info.src_join_on, Some(""));
    let skip_dst_join = matches!(edge_info.dst_join_on, Some(""));

    let join_kw = if is_optional { "LEFT JOIN" } else { "JOIN" };
    let distinct_kw = "SELECT DISTINCT";

    let mut sql = format!(
        "{distinct_kw} {select_cols} FROM {} AS {edge_alias}",
        edge_info.table
    );

    if !skip_src_join {
        let on_clause = if let Some(tmpl) = edge_info.src_join_on {
            // Full ON-expression override with placeholder substitution.
            tmpl.replace("{src}", src_alias)
                .replace("{dst}", dst_alias)
                .replace("{e}", edge_alias)
        } else {
            let src_join_col = edge_info
                .src_join_key
                .unwrap_or(if edge_info.src_is_symbol {
                    "symbol_uid"
                } else {
                    "file_path"
                });
            format!(
                "{src_alias}.{src_join_col} = {edge_alias}.{}",
                edge_info.src_col
            )
        };
        sql.push_str(&format!(
            " {join_kw} {src_table} AS {src_alias} ON {on_clause}"
        ));
    }

    if !skip_dst_join {
        let on_clause = if let Some(tmpl) = edge_info.dst_join_on {
            tmpl.replace("{src}", src_alias)
                .replace("{dst}", dst_alias)
                .replace("{e}", edge_alias)
        } else {
            let dst_join_col = edge_info
                .dst_join_key
                .unwrap_or(if edge_info.dst_is_symbol {
                    "symbol_uid"
                } else {
                    "file_path"
                });
            format!(
                "{dst_alias}.{dst_join_col} = {edge_alias}.{}",
                edge_info.dst_col
            )
        };
        sql.push_str(&format!(
            " {join_kw} {dst_table} AS {dst_alias} ON {on_clause}"
        ));
    }

    let mut where_parts: Vec<String> = Vec::new();

    // Kind filters from labels (skip for sides whose JOIN was omitted).
    for (alias, label_opt, skipped) in [
        (src_alias, src_node.label.as_deref(), skip_src_join),
        (dst_alias, dst_node.label.as_deref(), skip_dst_join),
    ] {
        if skipped {
            continue;
        }
        if let Some(label) = label_opt {
            if let Some(kind) = label_kind_filter(label) {
                where_parts.push(format!("{alias}.kind = ?{}", params.len() + 1));
                params.push(kind.to_string());
            }
        }
    }

    // Edge-level extra filter (e.g. call_kind discrimination).
    if let Some(filter) = edge_info.extra_filter {
        where_parts.push(format!("{edge_alias}.{filter}"));
    }

    // Inline properties.
    for node in &pattern.nodes {
        let na = node.var.as_deref().unwrap_or("n");
        for (key, val) in &node.props {
            where_parts.push(format!("{na}.{key} = ?{}", params.len() + 1));
            params.push(val.clone());
        }
    }

    // WHERE clause.
    if let Some(wc) = &query.where_clause {
        where_parts.push(expr_to_sql(&wc.expr, &mut params)?);
    }

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    // GROUP BY (when aggregates are present).
    sql.push_str(&build_group_by_clause(&projections));

    // ORDER BY.
    if let Some(order_items) = &query.order_by {
        let parts: Vec<String> = order_items.iter().map(order_item_to_sql).collect();
        sql.push_str(" ORDER BY ");
        sql.push_str(&parts.join(", "));
    }

    // LIMIT.
    let limit = query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok(TranslatedQuery {
        sql,
        params,
        projections,
    })
}

/// Translate `MATCH (f:Label) OPTIONAL MATCH (f)-[:R]->(g) ...` with the source
/// node as the anchor, so `f` rows are preserved even when no edge/target exists.
///
/// `anchor_label` carries the label from the required MATCH clause (the optional
/// clause may repeat the source variable without its label). The target-side kind
/// filter and any target-referencing WHERE predicates are placed in the LEFT JOIN
/// `ON` clause rather than the outer WHERE, otherwise NULL target rows would be
/// discarded and the OPTIONAL semantics lost.
pub(crate) fn translate_optional_match(
    query: &CypherQuery,
    anchor_label: Option<&str>,
    pattern: &PathPattern,
) -> CcResult<TranslatedQuery> {
    let src_node = &pattern.nodes[0];
    let dst_node = &pattern.nodes[1];
    let rel = &pattern.rels[0];

    let edge_type_str = rel.rel_type.as_deref().unwrap_or("CALLS");
    let etm = edge_table_map();
    let edge_info = etm
        .get(edge_type_str)
        .ok_or_else(|| CcError::Search(format!("unknown edge type: {edge_type_str}")))?;

    // Anchored form needs real node tables on both sides; pseudo-UID endpoints
    // (custom or skipped joins) fall back to the edge-anchored single hop.
    if edge_info.src_join_on.is_some() || edge_info.dst_join_on.is_some() {
        return translate_single_hop(query, pattern, true);
    }

    let src_alias = src_node.var.as_deref().unwrap_or("src");
    let dst_alias = dst_node.var.as_deref().unwrap_or("dst");
    let edge_alias = "e";

    let src_label = src_node.label.as_deref().or(anchor_label);
    let src_table = src_label.map(label_table).unwrap_or("symbols");
    let dst_table = dst_node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");

    let alias_tables = HashMap::from([
        (src_alias.to_string(), src_table.to_string()),
        (dst_alias.to_string(), dst_table.to_string()),
        (edge_alias.to_string(), edge_info.table.to_string()),
    ]);
    let projections = build_projections(&query.return_clause.items, &alias_tables)?;
    let select_cols = projections
        .iter()
        .map(|p| p.sql.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut params: Vec<String> = Vec::new();

    let src_join_col = edge_info
        .src_join_key
        .unwrap_or(if edge_info.src_is_symbol {
            "symbol_uid"
        } else {
            "file_path"
        });
    let dst_join_col = edge_info
        .dst_join_key
        .unwrap_or(if edge_info.dst_is_symbol {
            "symbol_uid"
        } else {
            "file_path"
        });

    // Edge LEFT JOIN ON (source link + edge-level filter).
    let mut edge_on = format!(
        "{edge_alias}.{} = {src_alias}.{src_join_col}",
        edge_info.src_col
    );
    if let Some(filter) = edge_info.extra_filter {
        edge_on.push_str(&format!(" AND {edge_alias}.{filter}"));
    }

    // Target LEFT JOIN ON (target link + target kind filter — kept out of WHERE).
    let mut dst_on = format!(
        "{dst_alias}.{dst_join_col} = {edge_alias}.{}",
        edge_info.dst_col
    );
    if let Some(label) = dst_node.label.as_deref() {
        if let Some(kind) = label_kind_filter(label) {
            dst_on.push_str(&format!(" AND {dst_alias}.kind = ?{}", params.len() + 1));
            params.push(kind.to_string());
        }
    }

    // Source-side filters go to the outer WHERE.
    let mut where_parts: Vec<String> = Vec::new();
    if let Some(label) = src_label {
        if let Some(kind) = label_kind_filter(label) {
            where_parts.push(format!("{src_alias}.kind = ?{}", params.len() + 1));
            params.push(kind.to_string());
        }
    }
    for (key, val) in &src_node.props {
        where_parts.push(format!("{src_alias}.{key} = ?{}", params.len() + 1));
        params.push(val.clone());
    }

    // Split the query WHERE: source predicates → WHERE, target predicates → JOIN ON.
    if let Some(wc) = &query.where_clause {
        let (src_where, dst_where) =
            split_where_by_var(&wc.expr, src_alias, dst_alias, &mut params)?;
        if !src_where.is_empty() && src_where != "1=1" {
            where_parts.push(src_where);
        }
        if let Some(dw) = dst_where {
            dst_on.push_str(&format!(" AND {dw}"));
        }
    }

    // Target inline props also belong in the JOIN ON to preserve OPTIONAL semantics.
    for (key, val) in &dst_node.props {
        dst_on.push_str(&format!(" AND {dst_alias}.{key} = ?{}", params.len() + 1));
        params.push(val.clone());
    }

    let mut sql = format!("SELECT DISTINCT {select_cols} FROM {src_table} AS {src_alias}");
    sql.push_str(&format!(
        " LEFT JOIN {} AS {edge_alias} ON {edge_on}",
        edge_info.table
    ));
    sql.push_str(&format!(
        " LEFT JOIN {dst_table} AS {dst_alias} ON {dst_on}"
    ));

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    sql.push_str(&build_group_by_clause(&projections));

    if let Some(order_items) = &query.order_by {
        let parts: Vec<String> = order_items.iter().map(order_item_to_sql).collect();
        sql.push_str(" ORDER BY ");
        sql.push_str(&parts.join(", "));
    }

    let limit = query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok(TranslatedQuery {
        sql,
        params,
        projections,
    })
}

pub(crate) fn translate_variable_length(
    query: &CypherQuery,
    pattern: &PathPattern,
) -> CcResult<TranslatedQuery> {
    let src_node = &pattern.nodes[0];
    let dst_node = &pattern.nodes[1];
    let rel = &pattern.rels[0];

    let edge_type_str = rel.rel_type.as_deref().unwrap_or("CALLS");

    let etm = edge_table_map();
    let edge_info = etm.get(edge_type_str).ok_or_else(|| {
        CcError::Search(format!(
            "variable-length paths not supported for edge type: {edge_type_str}"
        ))
    })?;

    // Only allow variable-length for catalog-declared recursive-friendly edges.
    if !cc_model::graph_catalog::graph_relationship(edge_type_str)
        .is_some_and(|rel| rel.variable_length)
    {
        let supported = graph_relationships()
            .iter()
            .filter(|rel| rel.variable_length)
            .map(|rel| rel.edge)
            .collect::<Vec<_>>()
            .join("/");
        return Err(CcError::Search(format!(
            "variable-length paths only supported for {supported} edges, got {edge_type_str}"
        )));
    }

    let vl_table = edge_info.table;
    let vl_src_col = edge_info.src_col;
    let vl_dst_col = edge_info.dst_col;
    let vl_extra_filter = edge_info.extra_filter;

    // Traversal semantics come from the shared declaration consumed by both
    // this CTE translation and the lazy-BFS fast path (see
    // traversal_semantics.rs). Each `match` on a declared rule below is this
    // engine's mechanical mapping to SQL. A new variant on the multiplicity/
    // cycle/dedup rules fails compilation in both engines directly; direction
    // is constrained indirectly: a new `DirectionHandling` variant breaks
    // `orient()` and the fast-path gate, and reaches this translation only
    // through the `WalkOrientation` that `orient()` returns.
    let semantics = &VARLEN_TRAVERSAL;
    // DirectionHandling::IgnoreDirection (compatibility quirk): every arrow
    // spelling walks the edge table from its source column to its
    // destination column, in textual pattern order.
    let (walk_from_col, walk_to_col) = match semantics.orient(rel.direction) {
        WalkOrientation::Forward => (vl_src_col, vl_dst_col),
    };

    let src_alias = src_node.var.as_deref().unwrap_or("src");
    let dst_alias = dst_node.var.as_deref().unwrap_or("dst");
    let min_hops = rel.min_hops;
    let max_hops = rel.max_hops;

    let skip_src_join = matches!(edge_info.src_join_on, Some(""));
    let skip_dst_join = matches!(edge_info.dst_join_on, Some(""));

    // Determine the seed table and UID column for the CTE base case.
    // For edges whose source is a pseudo-UID (e.g. DEFINES with `file::path`),
    // seed from `files` and synthesise the pseudo-UID so the recursive JOIN works.
    let (seed_table, seed_uid_expr, _seed_join_col): (&str, String, &str) =
        if let Some(tmpl) = edge_info.src_join_on {
            if tmpl.is_empty() {
                // No node table for source -- seed directly from the edge table.
                (vl_table, format!("s.{walk_from_col}"), walk_from_col)
            } else {
                // Custom JOIN expression -- source is pseudo-UID backed by files table.
                // Seed from files, synthesise pseudo-UID = 'file::' || file_path.
                ("files", "'file::' || s.file_path".to_string(), "file_path")
            }
        } else if edge_info.src_is_symbol {
            ("symbols", "s.symbol_uid".to_string(), "symbol_uid")
        } else {
            ("files", "s.file_path".to_string(), "file_path")
        };

    // Determine final-result join for the source side.
    let src_table = src_node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or(seed_table);
    let dst_table = dst_node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");

    let alias_tables = HashMap::from([
        (src_alias.to_string(), src_table.to_string()),
        (dst_alias.to_string(), dst_table.to_string()),
    ]);
    let projections = build_projections(&query.return_clause.items, &alias_tables)?;
    let select_cols = projections
        .iter()
        .map(|p| p.sql.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut params: Vec<String> = Vec::new();

    // Separate WHERE conditions for source vs target variables.
    let (src_where, dst_where) = if let Some(wc) = &query.where_clause {
        split_where_by_var(&wc.expr, src_alias, dst_alias, &mut params)?
    } else {
        ("1=1".to_string(), None)
    };

    // If src_where came back empty from the split, use 1=1.
    let cte_where = if src_where.is_empty() {
        "1=1".to_string()
    } else {
        // Replace var alias with 's' for the CTE base query.
        src_where.replace(&format!("{src_alias}."), "s.")
    };

    let max_param_idx = params.len() + 1;
    params.push(max_hops.to_string());
    let min_param_idx = params.len() + 1;
    params.push(min_hops.to_string());

    // Build extra filter clause for the recursive step.
    let extra_and = match vl_extra_filter {
        Some(f) => format!(" AND ce.{f}"),
        None => String::new(),
    };

    // Build the final JOIN clauses for source and destination sides.
    let src_final_join = if skip_src_join {
        String::new()
    } else if let Some(tmpl) = edge_info.src_join_on {
        // Custom ON: replace {src} with alias, {e} not applicable here -- join via root_uid.
        let on_expr = tmpl.replace("{src}", src_alias).replace("{e}", "path_cte");
        // The CTE stores the original pseudo-UID in root_uid; rewrite to match.
        // For DEFINES: `f.file_path = SUBSTR(path_cte.root_uid, 7)` works since root_uid
        // is the pseudo-UID we seeded.
        let on_rewritten = on_expr.replace(
            &format!("SUBSTR(path_cte.{vl_src_col}, 7)"),
            "SUBSTR(path_cte.root_uid, 7)",
        );
        format!(" JOIN {src_table} AS {src_alias} ON {on_rewritten}")
    } else {
        let join_col = if edge_info.src_is_symbol {
            "symbol_uid"
        } else {
            "file_path"
        };
        format!(" JOIN {src_table} AS {src_alias} ON {src_alias}.{join_col} = path_cte.root_uid")
    };

    let dst_final_join = if skip_dst_join {
        String::new()
    } else if let Some(tmpl) = edge_info.dst_join_on {
        let on_expr = tmpl.replace("{dst}", dst_alias).replace("{e}", "path_cte");
        let on_rewritten = on_expr.replace(
            &format!("SUBSTR(path_cte.{vl_dst_col}, 7)"),
            "SUBSTR(path_cte.uid, 7)",
        );
        format!(" JOIN {dst_table} AS {dst_alias} ON {on_rewritten}")
    } else {
        let join_col = if edge_info.dst_is_symbol {
            "symbol_uid"
        } else {
            "file_path"
        };
        format!(" JOIN {dst_table} AS {dst_alias} ON {dst_alias}.{join_col} = path_cte.uid")
    };

    // Reachability semantics (not path enumeration): the working set is
    // bounded by O(nodes x max_depth) instead of O(distinct paths). This
    // removes the per-path `visited` string and its quadratic LIKE
    // cycle-guard. The trade-off is that variable-length results report the
    // *set of reachable nodes* within the hop range and no longer carry path
    // multiplicity (e.g. COUNT(*) counts nodes, not paths).
    //
    // TupleMultiplicity::DistinctPerRootNodeDepth: `UNION` (never UNION ALL)
    // dedups (root_uid, uid, depth) tuples in the recursion.
    let recursion_set_op = match semantics.tuple_multiplicity {
        TupleMultiplicity::DistinctPerRootNodeDepth => "UNION",
    };
    // CyclePolicy::BoundedByMaxHops: the depth cap is the only cycle guard.
    let depth_cap = match semantics.cycle_policy {
        CyclePolicy::BoundedByMaxHops => format!("pc.depth < CAST(?{max_param_idx} AS INTEGER)"),
    };
    // ProjectionDedup::DistinctRows: identical projected rows collapse.
    let projection_select = match semantics.projection_dedup {
        ProjectionDedup::DistinctRows => "SELECT DISTINCT",
    };
    let mut sql = format!(
        "WITH RECURSIVE path_cte(root_uid, uid, depth) AS (\
            SELECT {seed_uid_expr}, {seed_uid_expr}, 0 FROM {seed_table} AS s WHERE {cte_where} \
            {recursion_set_op} \
            SELECT pc.root_uid, ce.{walk_to_col}, pc.depth + 1 \
            FROM path_cte pc \
            JOIN {vl_table} ce ON ce.{walk_from_col} = pc.uid \
            WHERE {depth_cap}{extra_and}\
        ) \
        {projection_select} {select_cols} \
        FROM path_cte\
        {src_final_join}\
        {dst_final_join} \
        WHERE path_cte.depth >= CAST(?{min_param_idx} AS INTEGER)"
    );

    // Target-side WHERE.
    if let Some(dw) = &dst_where {
        sql.push_str(" AND ");
        sql.push_str(dw);
    }

    // Kind filter on target (skip when dst JOIN was omitted).
    if !skip_dst_join {
        if let Some(label) = dst_node.label.as_deref() {
            if let Some(kind) = label_kind_filter(label) {
                sql.push_str(&format!(" AND {dst_alias}.kind = ?{}", params.len() + 1));
                params.push(kind.to_string());
            }
        }
    }

    // GROUP BY (when aggregates are present).
    sql.push_str(&build_group_by_clause(&projections));

    // ORDER BY.
    if let Some(order_items) = &query.order_by {
        let parts: Vec<String> = order_items.iter().map(order_item_to_sql).collect();
        sql.push_str(" ORDER BY ");
        sql.push_str(&parts.join(", "));
    }

    // LIMIT.
    let limit = query.limit.unwrap_or(DEFAULT_CYPHER_LIMIT);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok(TranslatedQuery {
        sql,
        params,
        projections,
    })
}

/// Split a WHERE expression into source-side and destination-side SQL fragments.
/// Returns (src_sql, Option<dst_sql>).
#[allow(clippy::only_used_in_recursion)]
fn split_where_by_var(
    expr: &Expr,
    src_var: &str,
    dst_var: &str,
    params: &mut Vec<String>,
) -> CcResult<(String, Option<String>)> {
    // Simple approach: convert the whole expression to SQL, then check which var it references.
    // For AND expressions, we can split the two sides.
    match expr {
        Expr::And(left, right) => {
            let (ls, ld) = split_where_by_var(left, src_var, dst_var, params)?;
            let (rs, rd) = split_where_by_var(right, src_var, dst_var, params)?;

            let src_parts: Vec<&str> = [&ls, &rs]
                .iter()
                .filter(|s| !s.is_empty() && **s != "1=1")
                .map(|s| s.as_str())
                .collect();
            let src = if src_parts.is_empty() {
                "1=1".to_string()
            } else {
                src_parts.join(" AND ")
            };

            let dst_parts: Vec<&String> = [&ld, &rd].iter().filter_map(|o| o.as_ref()).collect();
            let dst = if dst_parts.is_empty() {
                None
            } else {
                Some(
                    dst_parts
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(" AND "),
                )
            };

            Ok((src, dst))
        }
        _ => {
            let sql = expr_to_sql(expr, params)?;
            if sql.contains(&format!("{dst_var}.")) {
                Ok(("1=1".to_string(), Some(sql)))
            } else {
                Ok((sql, None))
            }
        }
    }
}

/// Execute a `CypherUnionQuery` by running each sub-query and merging results.
pub fn execute_union(uq: &CypherUnionQuery, db: &IndexDb) -> CcResult<CypherResult> {
    if uq.queries.len() == 1 {
        return execute(&uq.queries[0], db);
    }

    // Execute the first sub-query to establish columns.
    // UNION branches stay on the SQL path (the fast path is gated to plain
    // single-query traversals only).
    let first_result = execute_with_options(&uq.queries[0], db, false)?;
    let columns = first_result.columns.clone();
    let default_limit_applied = first_result.default_limit_applied;
    let limit = first_result.limit;
    let mut all_rows: Vec<Vec<serde_json::Value>> = first_result.rows;

    // Execute remaining sub-queries and merge.
    for (i, query) in uq.queries.iter().enumerate().skip(1) {
        let result = execute_with_options(query, db, false)?;
        let is_all = uq.union_all[i - 1]; // union_all[0] = between query 0 and 1

        if is_all {
            // UNION ALL: just append.
            all_rows.extend(result.rows);
        } else {
            // UNION: dedup by serializing rows.
            use std::collections::HashSet;
            let mut seen: HashSet<String> = HashSet::new();
            // Add existing rows to the set.
            for row in &all_rows {
                seen.insert(serde_json::to_string(row).unwrap_or_default());
            }
            // Deduplicate existing rows first (in case first query itself has dupes).
            let mut deduped = Vec::new();
            let mut seen_dedup: HashSet<String> = HashSet::new();
            for row in all_rows.drain(..) {
                let key = serde_json::to_string(&row).unwrap_or_default();
                if seen_dedup.insert(key) {
                    deduped.push(row);
                }
            }
            all_rows = deduped;
            // Now add new rows, deduplicating.
            let mut seen_final: HashSet<String> = HashSet::new();
            for row in &all_rows {
                seen_final.insert(serde_json::to_string(row).unwrap_or_default());
            }
            for row in result.rows {
                let key = serde_json::to_string(&row).unwrap_or_default();
                if seen_final.insert(key) {
                    all_rows.push(row);
                }
            }
        }
    }

    let row_count = all_rows.len();
    Ok(CypherResult {
        columns,
        rows: all_rows,
        row_count,
        default_limit_applied,
        limit,
    })
}

/// Recursively validate all identifiers referenced in a WHERE expression.
/// Covers PropRef.var/prop (used in Comparison, Regex, Contains, StartsWith, EndsWith)
/// and Degree.var — both of which get interpolated directly into SQL strings.
fn validate_expr_identifiers(expr: &Expr) -> CcResult<()> {
    match expr {
        Expr::Comparison { left, .. }
        | Expr::Regex { left, .. }
        | Expr::Contains { left, .. }
        | Expr::StartsWith { left, .. }
        | Expr::EndsWith { left, .. } => {
            validate_sql_ident(&left.var)?;
            validate_sql_ident(&left.prop)?;
        }
        Expr::Degree { var, .. } => {
            validate_sql_ident(var)?;
        }
        Expr::And(lhs, rhs) | Expr::Or(lhs, rhs) => {
            validate_expr_identifiers(lhs)?;
            validate_expr_identifiers(rhs)?;
        }
        Expr::Not(inner) => {
            validate_expr_identifiers(inner)?;
        }
    }
    Ok(())
}

/// Validate all identifiers in a parsed query to ensure they are safe for SQL interpolation.
/// This is a defense-in-depth check: the lexer already constrains identifiers, but we
/// verify here in case identifiers are constructed outside the lexer.
pub(crate) fn validate_query_identifiers(query: &CypherQuery) -> CcResult<()> {
    for mc in &query.match_clauses {
        for pat in &mc.patterns {
            for node in &pat.nodes {
                if let Some(v) = &node.var {
                    validate_sql_ident(v)?;
                }
                if let Some(l) = &node.label {
                    validate_sql_ident(l)?;
                }
                for (k, _) in &node.props {
                    validate_sql_ident(k)?;
                }
            }
            for rel in &pat.rels {
                if let Some(v) = &rel.var {
                    validate_sql_ident(v)?;
                }
                if let Some(t) = &rel.rel_type {
                    validate_sql_ident(t)?;
                }
            }
        }
    }
    for item in &query.return_clause.items {
        match item {
            ReturnItem::Prop(pr, alias) => {
                validate_sql_ident(&pr.var)?;
                validate_sql_ident(&pr.prop)?;
                if let Some(a) = alias {
                    validate_sql_ident(a)?;
                }
            }
            ReturnItem::Count(count_arg, _, alias) => {
                match count_arg {
                    CountArg::Star => {}
                    CountArg::Var(v) => validate_sql_ident(v)?,
                    CountArg::Prop(pr) => {
                        validate_sql_ident(&pr.var)?;
                        validate_sql_ident(&pr.prop)?;
                    }
                }
                if let Some(a) = alias {
                    validate_sql_ident(a)?;
                }
            }
            ReturnItem::Aggregate(_, pr, _, alias) => {
                validate_sql_ident(&pr.var)?;
                validate_sql_ident(&pr.prop)?;
                if let Some(a) = alias {
                    validate_sql_ident(a)?;
                }
            }
            ReturnItem::Collect(expr, _, alias) => {
                match expr {
                    CollectExpr::Prop(pr) => {
                        validate_sql_ident(&pr.var)?;
                        validate_sql_ident(&pr.prop)?;
                    }
                    CollectExpr::Var(v) => validate_sql_ident(v)?,
                }
                if let Some(a) = alias {
                    validate_sql_ident(a)?;
                }
            }
            ReturnItem::Var(v, alias) => {
                validate_sql_ident(v)?;
                if let Some(a) = alias {
                    validate_sql_ident(a)?;
                }
            }
        }
    }
    if let Some(order_items) = &query.order_by {
        for oi in order_items {
            match &oi.expr {
                OrderExpr::Prop(pr) => {
                    validate_sql_ident(&pr.var)?;
                    validate_sql_ident(&pr.prop)?;
                }
                OrderExpr::Alias(a) => validate_sql_ident(a)?,
            }
        }
    }
    if let Some(where_clause) = &query.where_clause {
        validate_expr_identifiers(&where_clause.expr)?;
    }
    Ok(())
}
