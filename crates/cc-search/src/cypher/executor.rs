use super::ast::*;
use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use std::collections::HashMap;

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
    let mut m = HashMap::new();
    m.insert(
        "CALLS",
        EdgeTableInfo {
            table: "call_edges",
            src_col: "caller_symbol_uid",
            dst_col: "callee_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "IMPORTS",
        EdgeTableInfo {
            table: "imports",
            src_col: "file_path",
            dst_col: "resolved_path",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "TESTS",
        EdgeTableInfo {
            table: "test_edges",
            src_col: "test_file_path",
            dst_col: "code_file_path",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "HANDLES",
        EdgeTableInfo {
            table: "route_edges",
            src_col: "file_path",
            dst_col: "route_path",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "ROUTES",
        EdgeTableInfo {
            table: "route_edges",
            src_col: "file_path",
            dst_col: "route_path",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "REFERENCES",
        EdgeTableInfo {
            table: "symbol_refs",
            src_col: "file_path",
            dst_col: "target_file_path",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "REFS",
        EdgeTableInfo {
            table: "symbol_refs",
            src_col: "file_path",
            dst_col: "target_file_path",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "CO_CHANGE",
        EdgeTableInfo {
            table: "co_change_edges",
            src_col: "file_a",
            dst_col: "file_b",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "DATA_FLOW",
        EdgeTableInfo {
            table: "data_flow_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "HTTP_CALLS",
        EdgeTableInfo {
            table: "http_call_edges",
            src_col: "caller_symbol_uid",
            dst_col: "normalized_path",
            src_is_symbol: true,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: Some("normalized_path"),
            extra_filter: Some("call_kind = 'http'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "ASYNC_CALLS",
        EdgeTableInfo {
            table: "http_call_edges",
            src_col: "caller_symbol_uid",
            dst_col: "normalized_path",
            src_is_symbol: true,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: Some("normalized_path"),
            extra_filter: Some("call_kind IN ('async', 'grpc')"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    // --- Semantic edges ---
    m.insert(
        "INHERITS",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'inherits'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "IMPLEMENTS",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'implements'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "DECORATES",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'decorates'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "THROWS",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'throws'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "USES_TYPE",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'uses_type'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "SEMANTIC",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: None,
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "RENDERS_COMPONENT",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'renders_component'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    // --- Hierarchical / containment edges ---
    m.insert(
        "DEFINES",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: false,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'defines'"),
            // source is pseudo-UID `file::path` -- join files via substr strip
            src_join_on: Some("{src}.file_path = SUBSTR({e}.source_symbol_uid, 7)"),
            dst_join_on: None,
        },
    );
    m.insert(
        "DEFINES_METHOD",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: true,
            dst_is_symbol: true,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'defines_method'"),
            src_join_on: None,
            dst_join_on: None,
        },
    );
    m.insert(
        "CONTAINS_FILE",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'contains_file'"),
            // source is pseudo-UID `dir::path` -- no dirs table, skip JOIN
            src_join_on: Some(""),
            // target is pseudo-UID `file::path` -- join files via substr strip
            dst_join_on: Some("{dst}.file_path = SUBSTR({e}.target_symbol_uid, 7)"),
        },
    );
    m.insert(
        "CONTAINS_MODULE",
        EdgeTableInfo {
            table: "semantic_edges",
            src_col: "source_symbol_uid",
            dst_col: "target_symbol_uid",
            src_is_symbol: false,
            dst_is_symbol: false,
            src_join_key: None,
            dst_join_key: None,
            extra_filter: Some("relation_kind = 'contains_module'"),
            // Both sides may be pseudo-UIDs -- skip JOINs, use edge columns directly
            src_join_on: Some(""),
            dst_join_on: Some(""),
        },
    );
    m
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
        "Route" => "route_nodes",
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

/// Convert a basic regex pattern to a LIKE pattern for SQLite.
fn regex_to_like(pattern: &str) -> String {
    pattern
        .replace(".*", "%")
        .replace(".+", "_%")
        .replace('.', "_")
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
pub(crate) fn expr_to_sql(expr: &Expr, params: &mut Vec<String>) -> String {
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
                    format!("{col} {sql_op} ?{idx}")
                }
                Value::Int(n) => {
                    params.push(n.to_string());
                    format!("{col} {sql_op} ?{idx}")
                }
                Value::Float(f) => {
                    params.push(f.to_string());
                    format!("{col} {sql_op} ?{idx}")
                }
                Value::Bool(b) => {
                    params.push(if *b { "1".into() } else { "0".into() });
                    format!("{col} {sql_op} ?{idx}")
                }
                Value::Null => {
                    if matches!(op, CmpOp::Eq) {
                        format!("{col} IS NULL")
                    } else {
                        format!("{col} IS NOT NULL")
                    }
                }
            }
        }
        Expr::Regex { left, pattern } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(regex_to_like(pattern));
            format!("{col} LIKE ?{idx}")
        }
        Expr::Contains { left, value } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(format!("%{value}%"));
            format!("{col} LIKE ?{idx}")
        }
        Expr::StartsWith { left, value } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(format!("{value}%"));
            format!("{col} LIKE ?{idx}")
        }
        Expr::EndsWith { left, value } => {
            let col = prop_to_sql_col(left);
            let idx = params.len() + 1;
            params.push(format!("%{value}"));
            format!("{col} LIKE ?{idx}")
        }
        Expr::And(l, r) => {
            let ls = expr_to_sql(l, params);
            let rs = expr_to_sql(r, params);
            format!("({ls} AND {rs})")
        }
        Expr::Or(l, r) => {
            let ls = expr_to_sql(l, params);
            let rs = expr_to_sql(r, params);
            format!("({ls} OR {rs})")
        }
        Expr::Not(inner) => {
            let s = expr_to_sql(inner, params);
            format!("NOT ({s})")
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
                DegreeKind::In => format!("{in_sub} {sql_op} ?{idx}"),
                DegreeKind::Out => format!("{out_sub} {sql_op} ?{idx}"),
                DegreeKind::Total => format!("({in_sub} + {out_sub}) {sql_op} ?{idx}"),
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
        "route_edges" => &[
            ("edge_id", "edge_id"),
            ("file_path", "file_path"),
            ("route_path", "route_path"),
            ("handler_name", "handler_name"),
            ("method", "method"),
            ("line", "line"),
            ("handler_symbol_uid", "handler_symbol_uid"),
            ("framework", "framework"),
            ("route_kind", "route_kind"),
            ("confidence", "confidence"),
        ],
        "route_nodes" => &[
            ("route_id", "route_id"),
            ("file_path", "file_path"),
            ("route_path", "route_path"),
            ("method", "method"),
            ("handler_symbol_uid", "handler_symbol_uid"),
            ("handler_name", "handler_name"),
            ("framework", "framework"),
            ("line", "line"),
            ("end_line", "end_line"),
            ("confidence", "confidence"),
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
                    let source_key = alias.clone().unwrap_or_else(|| pr.prop.clone());
                    let sql = match alias {
                        Some(a) => format!("{col} AS {a}"),
                        None => col,
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
    // We only support the first MATCH clause's first pattern for now.
    if query.match_clauses.is_empty() || query.match_clauses[0].patterns.is_empty() {
        return Ok(CypherResult {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
        });
    }

    let first_match = &query.match_clauses[0];
    let pattern = &first_match.patterns[0];

    let translated = if pattern.rels.is_empty() {
        // Single-node query.
        translate_single_node(query, pattern)?
    } else if pattern.rels.len() == 1 {
        let rel = &pattern.rels[0];
        if rel.min_hops == 1 && rel.max_hops == 1 {
            translate_single_hop(query, pattern, first_match.is_optional)?
        } else {
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
    Ok(CypherResult {
        columns,
        rows,
        row_count,
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

pub(crate) fn translate_single_node(query: &CypherQuery, pattern: &PathPattern) -> CcResult<TranslatedQuery> {
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
        where_parts.push(expr_to_sql(&wc.expr, &mut params));
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
    let limit = query.limit.unwrap_or(50);
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
    let distinct_kw = if query.return_clause.distinct {
        "SELECT DISTINCT"
    } else {
        "SELECT DISTINCT"
    };

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
        where_parts.push(expr_to_sql(&wc.expr, &mut params));
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
    let limit = query.limit.unwrap_or(50);
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

    // Only allow variable-length for known recursive-friendly edges.
    match edge_type_str {
        "CALLS" | "DEFINES" | "DEFINES_METHOD" | "CONTAINS_FILE" | "CONTAINS_MODULE" => {}
        other => {
            return Err(CcError::Search(format!(
                "variable-length paths only supported for CALLS/DEFINES/DEFINES_METHOD/CONTAINS_FILE/CONTAINS_MODULE edges, got {other}"
            )));
        }
    }

    let vl_table = edge_info.table;
    let vl_src_col = edge_info.src_col;
    let vl_dst_col = edge_info.dst_col;
    let vl_extra_filter = edge_info.extra_filter;

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
                (vl_table, format!("s.{vl_src_col}"), vl_src_col)
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
        split_where_by_var(&wc.expr, src_alias, dst_alias, &mut params)
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

    let mut sql = format!(
        "WITH RECURSIVE path_cte(root_uid, uid, depth) AS (\
            SELECT {seed_uid_expr}, {seed_uid_expr}, 0 FROM {seed_table} AS s WHERE {cte_where} \
            UNION ALL \
            SELECT pc.root_uid, ce.{vl_dst_col}, pc.depth + 1 \
            FROM path_cte pc \
            JOIN {vl_table} ce ON ce.{vl_src_col} = pc.uid \
            WHERE pc.depth < ?{max_param_idx}{extra_and}\
        ) \
        SELECT DISTINCT {select_cols} \
        FROM path_cte\
        {src_final_join}\
        {dst_final_join} \
        WHERE path_cte.depth >= ?{min_param_idx}"
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
    let limit = query.limit.unwrap_or(50);
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
) -> (String, Option<String>) {
    // Simple approach: convert the whole expression to SQL, then check which var it references.
    // For AND expressions, we can split the two sides.
    match expr {
        Expr::And(left, right) => {
            let (ls, ld) = split_where_by_var(left, src_var, dst_var, params);
            let (rs, rd) = split_where_by_var(right, src_var, dst_var, params);

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

            (src, dst)
        }
        _ => {
            let sql = expr_to_sql(expr, params);
            if sql.contains(&format!("{dst_var}.")) {
                ("1=1".to_string(), Some(sql))
            } else {
                (sql, None)
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
    let first_result = execute(&uq.queries[0], db)?;
    let columns = first_result.columns.clone();
    let mut all_rows: Vec<Vec<serde_json::Value>> = first_result.rows;

    // Execute remaining sub-queries and merge.
    for (i, query) in uq.queries.iter().enumerate().skip(1) {
        let result = execute(query, db)?;
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
    })
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
    Ok(())
}
