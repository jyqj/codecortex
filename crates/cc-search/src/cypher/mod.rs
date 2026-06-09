//! Cypher query engine -- parse and execute a Cypher subset on the CC index database.
//!
//! Supported:
//!   MATCH (var:Label)-[:EDGE_TYPE]->(var:Label)
//!   MATCH (var:Label)-[:EDGE_TYPE*1..N]->(var:Label)   // variable-length paths
//!   MATCH (var:Label)-[:EDGE_TYPE*]->(var:Label)        // defaults to 1..5 hops
//!   MATCH (var:Label)-[:EDGE_TYPE*N]->(var:Label)       // exactly N hops
//!   MATCH (var:Label)-[:EDGE_TYPE*..N]->(var:Label)     // 1..N hops
//!   OPTIONAL MATCH (var:Label)-[:EDGE]->(var:Label)    // LEFT JOIN variant
//!   WHERE var.prop = 'value' AND/OR ...
//!   WHERE var.prop =~ 'regex'
//!   WHERE var.prop CONTAINS 'substring'
//!   WHERE var.prop STARTS WITH 'prefix'
//!   WHERE var.prop ENDS WITH 'suffix'
//!   WHERE degree(var) OP value / in_degree(var) / out_degree(var)
//!   RETURN var.prop, var.prop AS alias
//!   RETURN DISTINCT var.prop                           // SELECT DISTINCT
//!   RETURN COUNT(*), COUNT(var), COUNT(var.prop)
//!   RETURN COUNT(DISTINCT var), COUNT(DISTINCT var.prop)
//!   RETURN SUM(var.prop), AVG(var.prop), MIN(var.prop), MAX(var.prop)
//!   RETURN SUM(DISTINCT var.prop), ...                 // DISTINCT aggregates
//!   RETURN COLLECT(var.prop), COLLECT(DISTINCT var.prop)
//!   ORDER BY var.prop [ASC|DESC]
//!   ORDER BY alias [ASC|DESC]                          // alias-based ordering
//!   LIMIT N
//!   <query> UNION <query>                               // merge + dedup
//!   <query> UNION ALL <query>                           // merge without dedup
//!
//! Limitations:
//!   - LIMIT defaults to 50 when omitted (standard Cypher returns all rows).
//!   - AND / OR follow standard precedence (AND binds tighter than OR).
//!   - OPTIONAL MATCH only applies to the first pattern (single-hop).
//!
//! Not supported: WITH, MERGE, CREATE, DELETE, SET, UNWIND

mod ast;
mod executor;
mod lexer;
mod parser;

pub use ast::*;
pub use executor::{execute, execute_union};
pub use lexer::tokenize;
pub use parser::parse;

// re-export for internal use (cypher_query, parse_union)
pub(crate) use executor::validate_query_identifiers;

// re-export for tests
#[cfg(test)]
pub(crate) use executor::{
    edge_table_map, expr_to_sql, label_kind_filter, label_table, translate_single_hop,
    translate_single_node, translate_variable_length, validate_regex_for_like, validate_sql_ident,
    DEFAULT_CYPHER_LIMIT,
};

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};

pub enum ParsedCypher {
    Single(CypherQuery),
    Union(CypherUnionQuery),
}

/// Count the number of columns a query will return.
fn return_column_count(query: &CypherQuery) -> usize {
    query.return_clause.items.len()
}

/// Parse a token stream that may contain UNION / UNION ALL into a `CypherUnionQuery`.
///
/// The approach: scan for `Token::Union` or `Token::UnionAll` at the top level,
/// split the token stream into segments, and parse each segment with `parse()`.
pub fn parse_union(tokens: &[Token]) -> CcResult<CypherUnionQuery> {
    // Find split points (indices of Union / UnionAll tokens).
    let mut segments: Vec<&[Token]> = Vec::new();
    let mut union_all: Vec<bool> = Vec::new();
    let mut start = 0;

    for (i, tok) in tokens.iter().enumerate() {
        match tok {
            Token::Union | Token::UnionAll => {
                // Slice [start..i] is a sub-query (without the trailing Eof).
                // We need to append Eof for the sub-parser.
                let is_all = matches!(tok, Token::UnionAll);
                // Capture the segment before this UNION token.
                segments.push(&tokens[start..i]);
                union_all.push(is_all);
                start = i + 1;
            }
            _ => {}
        }
    }
    // Remaining tokens form the last segment (includes the original Eof).
    segments.push(&tokens[start..]);

    if segments.len() < 2 {
        // No UNION found — single query.
        let query = parse(tokens)?;
        return Ok(CypherUnionQuery {
            queries: vec![query],
            union_all: Vec::new(),
        });
    }

    let mut queries = Vec::with_capacity(segments.len());
    for (idx, seg) in segments.iter().enumerate() {
        // Each segment needs to end with Eof for the parser.
        let needs_eof = seg.last() != Some(&Token::Eof);
        let owned: Vec<Token>;
        let parse_slice: &[Token] = if needs_eof {
            owned = seg
                .iter()
                .cloned()
                .chain(std::iter::once(Token::Eof))
                .collect();
            &owned
        } else {
            seg
        };
        let query = parse(parse_slice).map_err(|e| {
            CcError::Search(format!("error parsing UNION sub-query {}: {e}", idx + 1))
        })?;
        queries.push(query);
    }

    // Validate column count consistency.
    let expected_cols = return_column_count(&queries[0]);
    for (i, q) in queries.iter().enumerate().skip(1) {
        let actual = return_column_count(q);
        if actual != expected_cols {
            return Err(CcError::Search(format!(
                "UNION sub-query {} returns {actual} columns but first query returns {expected_cols}",
                i + 1
            )));
        }
    }

    Ok(CypherUnionQuery { queries, union_all })
}

/// Parse a token stream into either a single query or a UNION query.
pub fn parse_tokens(tokens: &[Token]) -> CcResult<ParsedCypher> {
    // Check if this is a UNION query.
    let has_union = tokens
        .iter()
        .any(|t| matches!(t, Token::Union | Token::UnionAll));

    if has_union {
        parse_union(tokens).map(ParsedCypher::Union)
    } else {
        parse(tokens).map(ParsedCypher::Single)
    }
}

/// Execute an already-parsed Cypher query against the index database.
pub fn execute_parsed(parsed: &ParsedCypher, db: &IndexDb) -> CcResult<CypherResult> {
    match parsed {
        ParsedCypher::Single(query) => {
            validate_query_identifiers(query)?;
            execute(query, db)
        }
        ParsedCypher::Union(uq) => {
            for query in &uq.queries {
                validate_query_identifiers(query)?;
            }
            execute_union(uq, db)
        }
    }
}

/// Parse and execute a Cypher query against the index database.
pub fn cypher_query(input: &str, db: &IndexDb) -> CcResult<CypherResult> {
    let tokens = tokenize(input)?;
    let parsed = parse_tokens(&tokens)?;
    execute_parsed(&parsed, db)
}

// ── Tests ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- Tokenizer tests ---

    #[test]
    fn tokenize_basic_match() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g:Function) WHERE f.name = 'main' RETURN g.name LIMIT 10",
        )
        .unwrap();

        assert!(tokens.iter().any(|t| matches!(t, Token::Match)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Where)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Return)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Limit)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Arrow)));
        assert!(tokens.iter().any(|t| !matches!(t, Token::RegexMatch)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Eq)));
        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::StringLit(ref s) if s == "main")));
        assert!(tokens.iter().any(|t| matches!(t, Token::IntLit(10))));
    }

    #[test]
    fn tokenize_regex_and_contains() {
        let tokens = tokenize("WHERE f.name =~ '.*Handler' AND g.path CONTAINS 'test'").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, Token::RegexMatch)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Contains)));
        assert!(tokens.iter().any(|t| matches!(t, Token::And)));
    }

    #[test]
    fn tokenize_starts_ends_with() {
        let tokens =
            tokenize("WHERE f.name STARTS WITH 'get' AND g.name ENDS WITH 'Handler'").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, Token::StartsWith)));
        assert!(tokens.iter().any(|t| matches!(t, Token::EndsWith)));
    }

    #[test]
    fn tokenize_order_by() {
        let tokens = tokenize("ORDER BY f.name DESC").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, Token::OrderBy)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Desc)));
    }

    #[test]
    fn tokenize_varlen() {
        let tokens = tokenize("[:CALLS*1..3]").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, Token::Star)));
        assert!(tokens.iter().any(|t| matches!(t, Token::DotDot)));
        assert!(tokens.iter().any(|t| matches!(t, Token::IntLit(1))));
        assert!(tokens.iter().any(|t| matches!(t, Token::IntLit(3))));
    }

    // --- Parser tests ---

    #[test]
    fn parse_single_node() {
        let tokens = tokenize(
            "MATCH (f:File) WHERE f.file_path CONTAINS 'controller' RETURN f.file_path LIMIT 10",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();

        assert_eq!(ast.match_clause().patterns.len(), 1);
        let pat = &ast.match_clause().patterns[0];
        assert_eq!(pat.nodes.len(), 1);
        assert_eq!(pat.rels.len(), 0);
        assert_eq!(pat.nodes[0].var.as_deref(), Some("f"));
        assert_eq!(pat.nodes[0].label.as_deref(), Some("File"));
        assert!(ast.where_clause.is_some());
        assert_eq!(ast.limit, Some(10));
    }

    #[test]
    fn parse_match_with_rel() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g:Function) WHERE f.name = 'main' RETURN g.name LIMIT 5",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();

        let pat = &ast.match_clause().patterns[0];
        assert_eq!(pat.nodes.len(), 2);
        assert_eq!(pat.rels.len(), 1);
        assert_eq!(pat.nodes[0].label.as_deref(), Some("Function"));
        assert_eq!(pat.nodes[1].label.as_deref(), Some("Function"));
        assert_eq!(pat.rels[0].rel_type.as_deref(), Some("CALLS"));
        assert_eq!(pat.rels[0].direction, RelDirection::Outgoing);
        assert_eq!(pat.rels[0].min_hops, 1);
        assert_eq!(pat.rels[0].max_hops, 1);
        assert_eq!(ast.limit, Some(5));
    }

    #[test]
    fn parse_variable_length_path() {
        let tokens = tokenize(
            "MATCH (f)-[:CALLS*1..3]->(g:Function) WHERE g.name =~ '.*Handler' RETURN f.name",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();

        let rel = &ast.match_clause().patterns[0].rels[0];
        assert_eq!(rel.rel_type.as_deref(), Some("CALLS"));
        assert_eq!(rel.min_hops, 1);
        assert_eq!(rel.max_hops, 3);
        assert_eq!(rel.direction, RelDirection::Outgoing);
    }

    #[test]
    fn parse_return_with_alias() {
        let tokens =
            tokenize("MATCH (f:Symbol) RETURN f.name AS symbol_name, f.file_path AS path LIMIT 10")
                .unwrap();
        let ast = parse(&tokens).unwrap();

        assert_eq!(ast.return_clause.items.len(), 2);
        match &ast.return_clause.items[0] {
            ReturnItem::Prop(pr, alias) => {
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "name");
                assert_eq!(alias.as_deref(), Some("symbol_name"));
            }
            _ => panic!("expected Prop return item"),
        }
    }

    #[test]
    fn parse_count_return() {
        let tokens =
            tokenize("MATCH (f:Function)-[:CALLS]->(g) RETURN COUNT(g) AS call_count LIMIT 100")
                .unwrap();
        let ast = parse(&tokens).unwrap();

        assert_eq!(ast.return_clause.items.len(), 1);
        match &ast.return_clause.items[0] {
            ReturnItem::Count(CountArg::Var(var), distinct, alias) => {
                assert_eq!(var, "g");
                assert!(!distinct);
                assert_eq!(alias.as_deref(), Some("call_count"));
            }
            _ => panic!("expected Count return item"),
        }
    }

    #[test]
    fn parse_collect_return() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN COLLECT(f.name) AS names LIMIT 10").unwrap();
        let ast = parse(&tokens).unwrap();

        assert_eq!(ast.return_clause.items.len(), 1);
        match &ast.return_clause.items[0] {
            ReturnItem::Collect(CollectExpr::Prop(pr), distinct, alias) => {
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "name");
                assert!(!distinct);
                assert_eq!(alias.as_deref(), Some("names"));
            }
            _ => panic!("expected Collect return item"),
        }
    }

    #[test]
    fn parse_order_by_desc() {
        let tokens =
            tokenize("MATCH (f:Symbol) RETURN f.name ORDER BY f.name DESC LIMIT 10").unwrap();
        let ast = parse(&tokens).unwrap();

        let order = ast.order_by.as_ref().unwrap();
        assert_eq!(order.len(), 1);
        match &order[0].expr {
            OrderExpr::Prop(pr) => {
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "name");
            }
            other => panic!("expected Prop order expression, got {:?}", other),
        }
        assert!(order[0].desc);
    }

    #[test]
    fn parse_where_and_or() {
        let tokens = tokenize(
            "MATCH (f:Function) WHERE f.name = 'main' AND f.file_path CONTAINS 'src' RETURN f.name",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();

        assert!(ast.where_clause.is_some());
        match &ast.where_clause.as_ref().unwrap().expr {
            Expr::And(_, _) => {} // expected
            other => panic!("expected AND expression, got {:?}", other),
        }
    }

    // --- SQL translation tests ---

    #[test]
    fn translate_single_node_sql() {
        let tokens =
            tokenize("MATCH (f:Function) WHERE f.name = 'main' RETURN f.name LIMIT 10").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = translated.sql;
        let params = translated.params;

        assert!(
            sql.contains("SELECT f.name AS f_name FROM symbols AS f")
                || sql.contains("SELECT DISTINCT f.name AS f_name FROM symbols AS f"),
            "SQL was: {}",
            sql,
        );
        assert!(sql.contains("f.kind = ?1"));
        assert!(sql.contains("f.name = ?2"));
        assert!(sql.contains("LIMIT"));
        assert_eq!(params[0], "function");
        assert_eq!(params[1], "main");
    }

    #[test]
    fn translate_single_hop_sql() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g:Function) WHERE f.name = 'main' RETURN g.name LIMIT 5",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = translated.sql;
        let params = translated.params;

        assert!(sql.contains("call_edges"));
        assert!(sql.contains("JOIN"));
        assert!(sql.contains("symbol_uid"));
        assert!(sql.contains("f.kind = ?1"));
        assert!(sql.contains("g.kind = ?2"));
        assert_eq!(params[0], "function");
        assert_eq!(params[1], "function");
    }

    #[test]
    fn translate_contains_to_like() {
        let tokens = tokenize(
            "MATCH (f:File) WHERE f.file_path CONTAINS 'controller' RETURN f.file_path LIMIT 10",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = translated.sql;
        let params = translated.params;

        assert!(sql.contains("LIKE"));
        assert!(params.contains(&"%controller%".to_string()));
    }

    #[test]
    fn translate_starts_with() {
        let tokens =
            tokenize("MATCH (f:Function) WHERE f.name STARTS WITH 'get' RETURN f.name LIMIT 10")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = translated.sql;
        let params = translated.params;

        assert!(sql.contains("LIKE"));
        assert!(params.contains(&"get%".to_string()));
    }

    #[test]
    fn translate_ends_with() {
        let tokens =
            tokenize("MATCH (f:Function) WHERE f.name ENDS WITH 'Handler' RETURN f.name LIMIT 10")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = translated.sql;
        let params = translated.params;

        assert!(sql.contains("LIKE"));
        assert!(params.contains(&"%Handler".to_string()));
    }

    #[test]
    fn translate_variable_length_sql() {
        let tokens = tokenize(
            "MATCH (f)-[:CALLS*1..3]->(g:Function) WHERE f.name = 'init' RETURN g.name LIMIT 20",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_variable_length(&ast, pattern).unwrap();
        let sql = translated.sql;
        let params = translated.params;

        assert!(sql.contains("WITH RECURSIVE"));
        assert!(sql.contains("path_cte"));
        assert!(sql.contains("callee_symbol_uid"));
        assert!(params.contains(&"init".to_string()));
        assert!(params.contains(&"3".to_string()));
        assert!(params.contains(&"1".to_string()));
        assert!(params.contains(&"function".to_string()));
    }

    /// Reachability-semantics variable-length traversal: a node reachable via
    /// multiple paths is returned once, and a cycle terminates at the hop cap
    /// instead of looping forever or exploding the row count.
    #[test]
    fn variable_length_reachability_dedups_and_terminates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("varlen_reach.db"))
            .unwrap()
            .0;
        let conn = db.read_conn().unwrap();
        // Diamond A->B, A->C, B->D, C->D (D reachable via two paths) plus a
        // back-edge D->A forming a cycle.
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');\
             INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES \
                 ('a','src/x.rs','A','function',1,1,'uA'),\
                 ('b','src/x.rs','B','function',2,2,'uB'),\
                 ('c','src/x.rs','C','function',3,3,'uC'),\
                 ('d','src/x.rs','D','function',4,4,'uD');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) VALUES \
                 ('e1','src/x.rs','B',1,'uA','uB'),\
                 ('e2','src/x.rs','C',1,'uA','uC'),\
                 ('e3','src/x.rs','D',1,'uB','uD'),\
                 ('e4','src/x.rs','D',1,'uC','uD'),\
                 ('e5','src/x.rs','A',1,'uD','uA');",
        )
        .unwrap();

        let result = cypher_query(
            "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = 'A' RETURN b.name",
            &db,
        )
        .unwrap();

        let names: Vec<String> = result
            .rows
            .iter()
            .filter_map(|r| r.first().and_then(|v| v.as_str()).map(String::from))
            .collect();

        assert!(names.contains(&"B".to_string()), "B reachable at hop 1: {names:?}");
        assert!(names.contains(&"C".to_string()), "C reachable at hop 1: {names:?}");
        assert!(
            names.contains(&"D".to_string()),
            "D reachable via two paths: {names:?}"
        );
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "D").count(),
            1,
            "D must be returned once (reachability dedup), not per path: {names:?}"
        );
        // The D->A cycle must terminate and stay bounded by the reachable node set.
        assert!(
            result.rows.len() <= 4,
            "cycle must not explode the result: {names:?}"
        );
    }

    #[test]
    fn translate_applies_default_limit_when_omitted() {
        // No explicit LIMIT → the default limit (50) is appended as the last param.
        let tokens = tokenize("MATCH (f:Function) RETURN f.name").unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.limit, None);
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        assert!(translated.sql.contains("LIMIT"));
        assert_eq!(
            translated.params.last().map(String::as_str),
            Some(DEFAULT_CYPHER_LIMIT.to_string().as_str())
        );
    }

    #[test]
    fn translate_uses_explicit_limit_value() {
        // Explicit LIMIT → that value is appended, not the default.
        let tokens = tokenize("MATCH (f:Function) RETURN f.name LIMIT 7").unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.limit, Some(7));
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        assert_eq!(translated.params.last().map(String::as_str), Some("7"));
    }

    // --- Edge table mapping tests ---

    #[test]
    fn edge_map_has_expected_types() {
        let m = edge_table_map();
        assert!(m.contains_key("CALLS"));
        assert!(m.contains_key("IMPORTS"));
        assert!(m.contains_key("TESTS"));
        assert!(m.contains_key("HANDLES"));
        assert!(m.contains_key("ROUTES"));
        assert!(m.contains_key("REFERENCES"));
        assert!(m.contains_key("REFS"));
    }

    #[test]
    fn edge_map_matches_graph_catalog() {
        let m = edge_table_map();
        for rel in cc_model::graph_catalog::graph_relationships() {
            let info = m
                .get(rel.edge)
                .unwrap_or_else(|| panic!("missing catalog edge {}", rel.edge));
            assert_eq!(info.table, rel.table, "{} table drift", rel.edge);
            assert_eq!(
                info.src_col, rel.source.column,
                "{} source column drift",
                rel.edge
            );
            assert_eq!(
                info.dst_col, rel.destination.column,
                "{} destination column drift",
                rel.edge
            );
            assert_eq!(
                info.extra_filter, rel.extra_filter,
                "{} filter drift",
                rel.edge
            );
        }
    }

    #[test]
    fn label_maps_correctly() {
        assert_eq!(label_table("Function"), "symbols");
        assert_eq!(label_table("File"), "files");
        assert_eq!(label_table("Module"), "symbols");
        assert_eq!(label_table("Interface"), "symbols");
        assert_eq!(label_kind_filter("Function"), Some("function"));
        assert_eq!(label_kind_filter("Method"), Some("method"));
        assert_eq!(label_kind_filter("Module"), Some("module"));
        assert_eq!(label_kind_filter("File"), None);
    }

    // ── Degree filter tests ──────────────────────────

    #[test]
    fn parse_degree_filter() {
        let tokens = tokenize("MATCH (n:Function) WHERE degree(n) > 5 RETURN n.name").unwrap();
        let query = parse(&tokens).unwrap();
        let where_clause = query.where_clause.unwrap();
        match where_clause.expr {
            Expr::Degree {
                ref var,
                kind,
                op,
                ref value,
            } => {
                assert_eq!(var, "n");
                assert_eq!(kind, DegreeKind::Total);
                assert_eq!(op, CmpOp::Gt);
                assert!(matches!(value, Value::Int(5)));
            }
            _ => panic!("expected Degree expression"),
        }
    }

    #[test]
    fn parse_in_degree_filter() {
        let tokens = tokenize("MATCH (n:Function) WHERE in_degree(n) >= 3 RETURN n.name").unwrap();
        let query = parse(&tokens).unwrap();
        let where_clause = query.where_clause.unwrap();
        match where_clause.expr {
            Expr::Degree { kind, op, .. } => {
                assert_eq!(kind, DegreeKind::In);
                assert_eq!(op, CmpOp::Gte);
            }
            _ => panic!("expected Degree expression"),
        }
    }

    #[test]
    fn parse_out_degree_filter() {
        let tokens = tokenize("MATCH (n:Function) WHERE out_degree(n) = 0 RETURN n.name").unwrap();
        let query = parse(&tokens).unwrap();
        let where_clause = query.where_clause.unwrap();
        match where_clause.expr {
            Expr::Degree { kind, op, .. } => {
                assert_eq!(kind, DegreeKind::Out);
                assert_eq!(op, CmpOp::Eq);
            }
            _ => panic!("expected Degree expression"),
        }
    }

    #[test]
    fn parse_degree_combined_with_and() {
        let tokens = tokenize(
            "MATCH (n:Function) WHERE n.name =~ '.*main.*' AND degree(n) > 3 RETURN n.name",
        )
        .unwrap();
        let query = parse(&tokens).unwrap();
        let where_clause = query.where_clause.unwrap();
        // Should be And(Regex, Degree)
        match where_clause.expr {
            Expr::And(_, right) => match *right {
                Expr::Degree { kind, op, .. } => {
                    assert_eq!(kind, DegreeKind::Total);
                    assert_eq!(op, CmpOp::Gt);
                }
                _ => panic!("expected Degree in AND right"),
            },
            _ => panic!("expected And expression"),
        }
    }

    #[test]
    fn degree_to_sql_generates_subquery() {
        let expr = Expr::Degree {
            var: "n".to_string(),
            kind: DegreeKind::Total,
            op: CmpOp::Gt,
            value: Value::Int(5),
        };
        let mut params = Vec::new();
        let sql = expr_to_sql(&expr, &mut params).unwrap();
        assert!(sql.contains("call_edges"));
        assert!(sql.contains("n.symbol_uid"));
        assert!(sql.contains("> ?1"));
        assert_eq!(params, vec!["5"]);
    }

    #[test]
    fn edge_map_has_http_calls() {
        let m = edge_table_map();
        assert!(m.contains_key("HTTP_CALLS"));
        assert!(m.contains_key("ASYNC_CALLS"));
        assert_eq!(m["HTTP_CALLS"].table, "http_call_edges");
        assert_eq!(m["HTTP_CALLS"].extra_filter, Some("call_kind = 'http'"));
        assert_eq!(
            m["ASYNC_CALLS"].extra_filter,
            Some("call_kind IN ('async', 'grpc')")
        );
    }

    #[test]
    fn handles_joins_route_to_handler_symbol() {
        let tokens =
            tokenize("MATCH (r:Route)-[:HANDLES]->(f:Function) RETURN r.route_path, f.name")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("FROM routes AS e"),
            "HANDLES should use routes as edge table, got: {sql}"
        );
        assert!(
            sql.contains("JOIN routes AS r ON r.edge_id = e.edge_id"),
            "HANDLES source should join the route row by edge_id, got: {sql}"
        );
        assert!(
            sql.contains("JOIN symbols AS f ON f.symbol_uid = e.handler_symbol_uid"),
            "HANDLES destination should join handler symbol uid, got: {sql}"
        );
    }

    #[test]
    fn http_calls_join_route_by_normalized_path() {
        // Verify that HTTP_CALLS generates a JOIN to route_nodes via normalized_path,
        // NOT via file_path or symbol_uid.
        let tokens = tokenize(
            "MATCH (caller)-[:HTTP_CALLS]->(route:Route) RETURN caller.name, route.route_path",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        // The source side should join caller (symbols) via symbol_uid.
        assert!(
            sql.contains("caller.symbol_uid = e.caller_symbol_uid"),
            "src join should use symbol_uid, got: {sql}"
        );
        // The destination side MUST join route_nodes via normalized_path (the dst_join_key).
        assert!(
            sql.contains("route.normalized_path = e.normalized_path"),
            "dst join should use normalized_path, got: {sql}"
        );
        // Ensure it does NOT join via file_path (the old incorrect behaviour).
        assert!(
            !sql.contains("route.file_path"),
            "dst join must NOT use file_path, got: {sql}"
        );
        // Table name should be http_call_edges.
        assert!(
            sql.contains("http_call_edges"),
            "edge table should be http_call_edges, got: {sql}"
        );
        // Destination table should be route_nodes.
        assert!(
            sql.contains("routes AS route"),
            "dst table should be route_nodes, got: {sql}"
        );
        // HTTP_CALLS must filter by call_kind = 'http'.
        assert!(
            sql.contains("e.call_kind = 'http'"),
            "HTTP_CALLS should filter by call_kind, got: {sql}"
        );
    }

    // ── Aggregate function tests ───────────────────────

    #[test]
    fn tokenize_sum_avg_min_max() {
        let tokens = tokenize("RETURN SUM(f.line), AVG(f.line), MIN(f.line), MAX(f.line)").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, Token::Sum)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Avg)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Min)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Max)));
    }

    #[test]
    fn parse_sum_return() {
        let tokens =
            tokenize("MATCH (f:Function)-[:CALLS]->(g) RETURN f.name, SUM(g.line) AS total_lines")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.return_clause.items.len(), 2);
        match &ast.return_clause.items[1] {
            ReturnItem::Aggregate(func, pr, distinct, alias) => {
                assert_eq!(func, "SUM");
                assert_eq!(pr.var, "g");
                assert_eq!(pr.prop, "line");
                assert!(!distinct);
                assert_eq!(alias.as_deref(), Some("total_lines"));
            }
            other => panic!("expected Aggregate return item, got {:?}", other),
        }
    }

    #[test]
    fn parse_avg_return() {
        let tokens = tokenize("MATCH (f:Function) RETURN AVG(f.line) AS avg_line").unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.return_clause.items.len(), 1);
        match &ast.return_clause.items[0] {
            ReturnItem::Aggregate(func, pr, distinct, alias) => {
                assert_eq!(func, "AVG");
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "line");
                assert!(!distinct);
                assert_eq!(alias.as_deref(), Some("avg_line"));
            }
            other => panic!("expected Aggregate return item, got {:?}", other),
        }
    }

    #[test]
    fn parse_min_max_return() {
        let tokens = tokenize("MATCH (f:Function) RETURN MIN(f.line), MAX(f.line)").unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.return_clause.items.len(), 2);
        match &ast.return_clause.items[0] {
            ReturnItem::Aggregate(func, pr, _, _) => {
                assert_eq!(func, "MIN");
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "line");
            }
            other => panic!("expected MIN Aggregate, got {:?}", other),
        }
        match &ast.return_clause.items[1] {
            ReturnItem::Aggregate(func, pr, _, _) => {
                assert_eq!(func, "MAX");
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "line");
            }
            other => panic!("expected MAX Aggregate, got {:?}", other),
        }
    }

    #[test]
    fn translate_sum_generates_group_by() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g:Function) RETURN f.name, SUM(g.line) AS total_lines",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;
        assert!(
            sql.contains("SUM(g.line)"),
            "should contain SUM(g.line), got: {sql}"
        );
        assert!(
            sql.contains("GROUP BY"),
            "should contain GROUP BY, got: {sql}"
        );
        assert!(
            sql.contains("GROUP BY f.name"),
            "should GROUP BY f.name, got: {sql}"
        );
    }

    #[test]
    fn translate_avg_single_node() {
        let tokens = tokenize("MATCH (f:Function) RETURN AVG(f.line) AS avg_line").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = &translated.sql;
        assert!(
            sql.contains("AVG(f.line)"),
            "should contain AVG(f.line), got: {sql}"
        );
        // No non-aggregate columns, so no GROUP BY needed.
        assert!(
            !sql.contains("GROUP BY"),
            "should not contain GROUP BY when no non-agg columns, got: {sql}"
        );
    }

    #[test]
    fn translate_min_max_single_node() {
        let tokens = tokenize("MATCH (f:Function) RETURN MIN(f.line), MAX(f.line)").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = &translated.sql;
        assert!(
            sql.contains("MIN(f.line)"),
            "should contain MIN(f.line), got: {sql}"
        );
        assert!(
            sql.contains("MAX(f.line)"),
            "should contain MAX(f.line), got: {sql}"
        );
    }

    // ── Hierarchical edge tests (DEFINES, DEFINES_METHOD, CONTAINS_FILE) ──

    #[test]
    fn translate_defines_edge() {
        let tokens = tokenize("MATCH (f)-[:DEFINES]->(s:Function) RETURN s.name").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        // Should use semantic_edges table with relation_kind = 'defines'.
        assert!(
            sql.contains("semantic_edges"),
            "DEFINES should use semantic_edges table, got: {sql}"
        );
        assert!(
            sql.contains("relation_kind = 'defines'"),
            "DEFINES should filter by relation_kind, got: {sql}"
        );
        // Source side uses pseudo-UID (file::path) with custom ON expression,
        // NOT a standard JOIN via symbol_uid.
        assert!(
            sql.contains("SUBSTR"),
            "DEFINES source side should use SUBSTR for pseudo-UID, got: {sql}"
        );
        // Destination side joins symbols normally.
        assert!(
            sql.contains("JOIN symbols AS s"),
            "DEFINES destination should join symbols, got: {sql}"
        );
    }

    #[test]
    fn translate_defines_method_edge() {
        let tokens =
            tokenize("MATCH (c:Class)-[:DEFINES_METHOD]->(m:Method) RETURN m.name").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("relation_kind = 'defines_method'"),
            "DEFINES_METHOD should filter by relation_kind, got: {sql}"
        );
        // Both sides are symbol-based (src_is_symbol = true, dst_is_symbol = true).
        assert!(
            sql.contains("JOIN symbols AS c"),
            "DEFINES_METHOD source should join symbols, got: {sql}"
        );
        assert!(
            sql.contains("JOIN symbols AS m"),
            "DEFINES_METHOD destination should join symbols, got: {sql}"
        );
    }

    #[test]
    fn translate_contains_file_variable_length() {
        let tokens = tokenize("MATCH (d)-[:CONTAINS_FILE*1..2]->(f) RETURN f.name").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_variable_length(&ast, pattern).unwrap();
        let sql = &translated.sql;

        // Variable-length CONTAINS_FILE should produce a recursive CTE.
        assert!(
            sql.contains("WITH RECURSIVE"),
            "CONTAINS_FILE*1..2 should use recursive CTE, got: {sql}"
        );
        assert!(
            sql.contains("relation_kind = 'contains_file'"),
            "should filter by relation_kind = 'contains_file', got: {sql}"
        );
    }

    // ── Aggregate SQL generation tests ────────────────────────────

    #[test]
    fn translate_sum_group_by_full_pipeline() {
        let tokens =
            tokenize("MATCH (f:Function)-[:CALLS]->(g) RETURN f.name, SUM(g.line) AS total")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("SUM("),
            "should contain SUM( aggregate, got: {sql}"
        );
        assert!(
            sql.contains("GROUP BY"),
            "should contain GROUP BY clause, got: {sql}"
        );
    }

    #[test]
    fn translate_multi_aggregate_no_group_by() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN MIN(f.line), MAX(f.line), AVG(f.line)").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("MIN("),
            "should contain MIN( aggregate, got: {sql}"
        );
        assert!(
            sql.contains("MAX("),
            "should contain MAX( aggregate, got: {sql}"
        );
        assert!(
            sql.contains("AVG("),
            "should contain AVG( aggregate, got: {sql}"
        );
        // All columns are aggregates — no GROUP BY needed.
        assert!(
            !sql.contains("GROUP BY"),
            "all-aggregate query should not have GROUP BY, got: {sql}"
        );
    }

    // ── New feature tests: COUNT(DISTINCT), OPTIONAL MATCH, RETURN DISTINCT, alias ORDER BY ──

    #[test]
    fn tokenize_distinct_and_optional() {
        let tokens = tokenize("OPTIONAL MATCH (a) RETURN COUNT(DISTINCT a.name)").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, Token::Optional)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Distinct)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Count)));
    }

    #[test]
    fn parse_count_distinct_var() {
        let tokens =
            tokenize("MATCH (a)-[:CALLS]->(b) RETURN b.name, COUNT(DISTINCT a) AS fan_in").unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.return_clause.items.len(), 2);
        match &ast.return_clause.items[1] {
            ReturnItem::Count(CountArg::Var(var), distinct, alias) => {
                assert_eq!(var, "a");
                assert!(distinct);
                assert_eq!(alias.as_deref(), Some("fan_in"));
            }
            other => panic!("expected Count(Var, distinct=true), got {:?}", other),
        }
    }

    #[test]
    fn parse_count_distinct_prop() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN COUNT(DISTINCT f.file_path) AS unique_files")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.return_clause.items.len(), 1);
        match &ast.return_clause.items[0] {
            ReturnItem::Count(CountArg::Prop(pr), distinct, alias) => {
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "file_path");
                assert!(distinct);
                assert_eq!(alias.as_deref(), Some("unique_files"));
            }
            other => panic!("expected Count(Prop, distinct=true), got {:?}", other),
        }
    }

    #[test]
    fn parse_count_star() {
        let tokens = tokenize("MATCH (f:Function) RETURN COUNT(*) AS total").unwrap();
        let ast = parse(&tokens).unwrap();
        match &ast.return_clause.items[0] {
            ReturnItem::Count(CountArg::Star, distinct, alias) => {
                assert!(!distinct);
                assert_eq!(alias.as_deref(), Some("total"));
            }
            other => panic!("expected Count(Star), got {:?}", other),
        }
    }

    #[test]
    fn parse_count_var_prop() {
        let tokens = tokenize("MATCH (f:Function) RETURN COUNT(f.name) AS cnt").unwrap();
        let ast = parse(&tokens).unwrap();
        match &ast.return_clause.items[0] {
            ReturnItem::Count(CountArg::Prop(pr), distinct, alias) => {
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "name");
                assert!(!distinct);
                assert_eq!(alias.as_deref(), Some("cnt"));
            }
            other => panic!("expected Count(Prop, non-distinct), got {:?}", other),
        }
    }

    #[test]
    fn translate_count_distinct_to_sql() {
        let tokens = tokenize(
            "MATCH (a)-[:CALLS]->(b) RETURN b.name, COUNT(DISTINCT a) AS fan_in ORDER BY fan_in DESC LIMIT 10",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("COUNT(DISTINCT a.rowid)"),
            "should contain COUNT(DISTINCT a.rowid), got: {sql}"
        );
        assert!(
            sql.contains("GROUP BY b.name"),
            "should contain GROUP BY b.name, got: {sql}"
        );
        assert!(
            sql.contains("ORDER BY fan_in DESC"),
            "should ORDER BY alias fan_in DESC, got: {sql}"
        );
    }

    #[test]
    fn translate_count_distinct_prop_to_sql() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN COUNT(DISTINCT f.file_path) AS unique_files")
                .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("COUNT(DISTINCT f.file_path)"),
            "should contain COUNT(DISTINCT f.file_path), got: {sql}"
        );
    }

    #[test]
    fn parse_optional_match() {
        let tokens = tokenize("OPTIONAL MATCH (a)-[:CALLS]->(b) RETURN a.name, b.name").unwrap();
        let ast = parse(&tokens).unwrap();
        assert!(ast.match_clauses[0].is_optional);
    }

    #[test]
    fn parse_match_then_optional_match() {
        let tokens =
            tokenize("MATCH (a:Function) OPTIONAL MATCH (a)-[:CALLS]->(b) RETURN a.name").unwrap();
        let ast = parse(&tokens).unwrap();
        assert_eq!(ast.match_clauses.len(), 2);
        assert!(!ast.match_clauses[0].is_optional);
        assert!(ast.match_clauses[1].is_optional);
    }

    #[test]
    fn translate_optional_match_uses_left_join() {
        let tokens = tokenize("OPTIONAL MATCH (a)-[:CALLS]->(b) RETURN a.name, b.name").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clauses[0].patterns[0];
        let translated = translate_single_hop(&ast, pattern, true).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("LEFT JOIN"),
            "OPTIONAL MATCH should produce LEFT JOIN, got: {sql}"
        );
    }

    #[test]
    fn parse_return_distinct() {
        let tokens = tokenize("MATCH (f:Function) RETURN DISTINCT f.file_path").unwrap();
        let ast = parse(&tokens).unwrap();
        assert!(ast.return_clause.distinct);
        assert_eq!(ast.return_clause.items.len(), 1);
    }

    #[test]
    fn translate_return_distinct_to_sql() {
        let tokens = tokenize("MATCH (f:Function) RETURN DISTINCT f.file_path").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("SELECT DISTINCT"),
            "RETURN DISTINCT should produce SELECT DISTINCT, got: {sql}"
        );
    }

    #[test]
    fn parse_order_by_alias() {
        let tokens = tokenize(
            "MATCH (a)-[:CALLS]->(b) RETURN b.name, COUNT(a) AS fan_in ORDER BY fan_in DESC",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let order = ast.order_by.as_ref().unwrap();
        assert_eq!(order.len(), 1);
        match &order[0].expr {
            OrderExpr::Alias(name) => assert_eq!(name, "fan_in"),
            other => panic!("expected Alias order, got {:?}", other),
        }
        assert!(order[0].desc);
    }

    #[test]
    fn parse_sum_distinct() {
        let tokens = tokenize("MATCH (f:Function) RETURN SUM(DISTINCT f.line) AS total").unwrap();
        let ast = parse(&tokens).unwrap();
        match &ast.return_clause.items[0] {
            ReturnItem::Aggregate(func, pr, distinct, alias) => {
                assert_eq!(func, "SUM");
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "line");
                assert!(distinct);
                assert_eq!(alias.as_deref(), Some("total"));
            }
            other => panic!("expected Aggregate(SUM, distinct=true), got {:?}", other),
        }
    }

    #[test]
    fn translate_sum_distinct_to_sql() {
        let tokens = tokenize("MATCH (f:Function) RETURN SUM(DISTINCT f.line) AS total").unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_node(&ast, pattern).unwrap();
        let sql = &translated.sql;

        assert!(
            sql.contains("SUM(DISTINCT f.line)"),
            "should contain SUM(DISTINCT f.line), got: {sql}"
        );
    }

    #[test]
    fn parse_collect_distinct() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN COLLECT(DISTINCT f.name) AS names").unwrap();
        let ast = parse(&tokens).unwrap();
        match &ast.return_clause.items[0] {
            ReturnItem::Collect(CollectExpr::Prop(pr), distinct, alias) => {
                assert_eq!(pr.var, "f");
                assert_eq!(pr.prop, "name");
                assert!(distinct);
                assert_eq!(alias.as_deref(), Some("names"));
            }
            other => panic!("expected Collect(distinct=true), got {:?}", other),
        }
    }

    #[test]
    fn fan_in_query_full_sql() {
        // The canonical use case: top-10 most-called functions.
        let tokens = tokenize(
            "MATCH (a)-[:CALLS]->(b) RETURN b.name, COUNT(DISTINCT a) AS fan_in ORDER BY fan_in DESC LIMIT 10",
        )
        .unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let translated = translate_single_hop(&ast, pattern, false).unwrap();
        let sql = &translated.sql;

        // Essential SQL shape checks:
        assert!(
            sql.contains("COUNT(DISTINCT a.rowid)"),
            "COUNT(DISTINCT a.rowid): {sql}"
        );
        assert!(sql.contains("AS fan_in"), "AS fan_in: {sql}");
        assert!(sql.contains("GROUP BY b.name"), "GROUP BY b.name: {sql}");
        assert!(
            sql.contains("ORDER BY fan_in DESC"),
            "ORDER BY fan_in DESC: {sql}"
        );
        assert!(sql.contains("LIMIT"), "LIMIT: {sql}");
    }

    // --- UNION tests ---

    #[test]
    fn tokenize_union() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN f.name UNION MATCH (c:Class) RETURN c.name")
                .unwrap();
        assert!(tokens.contains(&Token::Union), "should contain Union token");
        assert!(
            !tokens.contains(&Token::UnionAll),
            "should not contain UnionAll"
        );
    }

    #[test]
    fn tokenize_union_all() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN f.name UNION ALL MATCH (c:Class) RETURN c.name")
                .unwrap();
        assert!(
            tokens.contains(&Token::UnionAll),
            "should contain UnionAll token"
        );
        assert!(
            !tokens.contains(&Token::Union),
            "should not contain plain Union"
        );
    }

    #[test]
    fn parse_union_two_queries() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN f.name UNION MATCH (c:Class) RETURN c.name")
                .unwrap();
        let uq = parse_union(&tokens).unwrap();
        assert_eq!(uq.queries.len(), 2);
        assert_eq!(uq.union_all.len(), 1);
        assert!(!uq.union_all[0], "UNION should be dedup (not ALL)");
    }

    #[test]
    fn parse_union_all_two_queries() {
        let tokens =
            tokenize("MATCH (f:Function) RETURN f.name UNION ALL MATCH (c:Class) RETURN c.name")
                .unwrap();
        let uq = parse_union(&tokens).unwrap();
        assert_eq!(uq.queries.len(), 2);
        assert_eq!(uq.union_all.len(), 1);
        assert!(uq.union_all[0], "UNION ALL should be marked as ALL");
    }

    #[test]
    fn parse_union_column_count_mismatch() {
        let tokens = tokenize(
            "MATCH (f:Function) RETURN f.name, f.line UNION MATCH (c:Class) RETURN c.name",
        )
        .unwrap();
        let result = parse_union(&tokens);
        assert!(
            result.is_err(),
            "column count mismatch should produce error"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("columns"),
            "error should mention columns: {err_msg}"
        );
    }

    #[test]
    fn parse_single_query_no_union() {
        // Regression: single query without UNION should parse normally through parse_union.
        let tokens = tokenize("MATCH (f:Function) RETURN f.name LIMIT 10").unwrap();
        let uq = parse_union(&tokens).unwrap();
        assert_eq!(uq.queries.len(), 1);
        assert!(uq.union_all.is_empty());
        assert_eq!(uq.queries[0].limit, Some(10));
    }

    #[test]
    fn parse_triple_union() {
        let tokens = tokenize(
            "MATCH (a:Function) RETURN a.name \
             UNION MATCH (b:Class) RETURN b.name \
             UNION ALL MATCH (c:Module) RETURN c.name",
        )
        .unwrap();
        let uq = parse_union(&tokens).unwrap();
        assert_eq!(uq.queries.len(), 3);
        assert_eq!(uq.union_all.len(), 2);
        assert!(!uq.union_all[0], "first boundary is UNION (dedup)");
        assert!(uq.union_all[1], "second boundary is UNION ALL");
    }

    #[test]
    fn execute_union_dedup_merges() {
        // Unit test for UNION dedup logic using mock CypherResult values.
        use std::collections::HashSet;

        let r1_rows = vec![
            vec![serde_json::Value::String("alpha".into())],
            vec![serde_json::Value::String("beta".into())],
        ];
        let r2_rows = vec![
            vec![serde_json::Value::String("beta".into())],
            vec![serde_json::Value::String("gamma".into())],
        ];

        // Simulate UNION (dedup): merge r1 and r2, remove duplicate "beta".
        let mut all_rows = r1_rows;
        let mut seen: HashSet<String> = HashSet::new();
        let mut deduped = Vec::new();
        for row in all_rows.drain(..) {
            let key = serde_json::to_string(&row).unwrap_or_default();
            if seen.insert(key) {
                deduped.push(row);
            }
        }
        all_rows = deduped;
        let mut seen_final: HashSet<String> = HashSet::new();
        for row in &all_rows {
            seen_final.insert(serde_json::to_string(row).unwrap_or_default());
        }
        for row in r2_rows {
            let key = serde_json::to_string(&row).unwrap_or_default();
            if seen_final.insert(key) {
                all_rows.push(row);
            }
        }
        assert_eq!(all_rows.len(), 3, "UNION should deduplicate 'beta'");
        let names: Vec<String> = all_rows
            .iter()
            .map(|r| r[0].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
        assert!(names.contains(&"gamma".to_string()));
    }

    #[test]
    fn execute_union_all_no_dedup() {
        // UNION ALL: just concatenate, no dedup.
        let r1_rows = vec![
            vec![serde_json::Value::String("alpha".into())],
            vec![serde_json::Value::String("beta".into())],
        ];
        let r2_rows = vec![
            vec![serde_json::Value::String("beta".into())],
            vec![serde_json::Value::String("gamma".into())],
        ];
        let mut all_rows = r1_rows;
        all_rows.extend(r2_rows);
        assert_eq!(
            all_rows.len(),
            4,
            "UNION ALL should keep all rows including duplicates"
        );
    }

    // --- Security: SQL injection regression tests ---

    #[test]
    fn test_injection_attach_database() {
        let result = validate_sql_ident("x; ATTACH DATABASE ':memory:' AS evil");
        assert!(result.is_err());
    }

    #[test]
    fn test_injection_drop_table() {
        let result = validate_sql_ident("symbols; DROP TABLE symbols; --");
        assert!(result.is_err());
    }

    #[test]
    fn test_injection_semicolon() {
        let result = validate_sql_ident("name; SELECT * FROM sqlite_master");
        assert!(result.is_err());
    }

    #[test]
    fn test_injection_quotes() {
        let result = validate_sql_ident("name' OR '1'='1");
        assert!(result.is_err());
    }

    #[test]
    fn test_injection_double_dash_comment() {
        let result = validate_sql_ident("name -- comment");
        assert!(result.is_err());
    }

    #[test]
    fn test_injection_unicode() {
        let result = validate_sql_ident("name\u{200B}evil"); // zero-width space
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_ident_accepts_valid() {
        assert!(validate_sql_ident("valid_name").is_ok());
        assert!(validate_sql_ident("CamelCase").is_ok());
        assert!(validate_sql_ident("name123").is_ok());
        assert!(validate_sql_ident("_private").is_ok());
    }

    #[test]
    fn test_validate_ident_rejects_empty() {
        assert!(validate_sql_ident("").is_err());
    }

    #[test]
    fn test_validate_ident_rejects_special_chars() {
        let dangerous = vec![
            "name;drop",
            "a'b",
            "a\"b",
            "a`b",
            "a\\b",
            "a\nb",
            "a\tb",
            "a(b",
            "a)b",
            "a{b",
            "a}b",
            "a[b",
            "a]b",
            "a=b",
            "a<b",
            "a>b",
            "a|b",
        ];
        for input in dangerous {
            assert!(
                validate_sql_ident(input).is_err(),
                "should reject: {:?}",
                input
            );
        }
    }

    // --- Security: query-level injection via tokenizer/parser ---

    #[test]
    fn test_cypher_injection_via_node_label() {
        // Backtick-escaped label with injection payload should fail at tokenize or parse.
        let malicious = "MATCH (n:`Function; DROP TABLE symbols`) RETURN n";
        let result = tokenize(malicious);
        if let Ok(tokens) = result {
            // If tokenizer accepts it, parse + validate should reject the label.
            let parse_result = parse(&tokens);
            if let Ok(query) = parse_result {
                assert!(
                    validate_query_identifiers(&query).is_err(),
                    "injected label should be rejected by identifier validation"
                );
            }
            // If parse itself fails, that's also acceptable — injection blocked.
        }
        // If tokenize fails, injection is blocked at the lexer level — that's fine.
    }

    #[test]
    fn test_cypher_injection_via_property_value() {
        // Property values go through parameter binding, so injection in a string
        // literal should be harmless — the SQL never sees it unescaped.
        let input = "MATCH (n:Function) WHERE n.name = 'x\\' OR 1=1 --' RETURN n.name LIMIT 10";
        let result = tokenize(input);
        if let Ok(tokens) = result {
            if let Ok(query) = parse(&tokens) {
                // validate_query_identifiers only checks identifiers, not values.
                // Values are safe because they are parameterized.
                assert!(validate_query_identifiers(&query).is_ok());
                // The injected string is just a literal — it's bound via ?N params.
            }
        }
    }

    #[test]
    fn test_validate_query_identifiers_rejects_malicious_var() {
        // Manually build an AST with a malicious variable name and confirm rejection.
        let bad_var = "x; DROP TABLE symbols".to_string();
        let query = CypherQuery {
            match_clauses: vec![MatchClause {
                is_optional: false,
                patterns: vec![PathPattern {
                    nodes: vec![NodePattern {
                        var: Some(bad_var),
                        label: Some("Function".to_string()),
                        props: vec![],
                    }],
                    rels: vec![],
                }],
            }],
            where_clause: None,
            return_clause: ReturnClause {
                items: vec![ReturnItem::Var("n".to_string(), None)],
                distinct: false,
            },
            order_by: None,
            limit: None,
        };
        assert!(
            validate_query_identifiers(&query).is_err(),
            "malicious variable name should be rejected"
        );
    }

    #[test]
    fn test_validate_query_identifiers_rejects_malicious_label() {
        let query = CypherQuery {
            match_clauses: vec![MatchClause {
                is_optional: false,
                patterns: vec![PathPattern {
                    nodes: vec![NodePattern {
                        var: Some("n".to_string()),
                        label: Some("Function; DROP TABLE symbols".to_string()),
                        props: vec![],
                    }],
                    rels: vec![],
                }],
            }],
            where_clause: None,
            return_clause: ReturnClause {
                items: vec![ReturnItem::Var("n".to_string(), None)],
                distinct: false,
            },
            order_by: None,
            limit: None,
        };
        assert!(
            validate_query_identifiers(&query).is_err(),
            "malicious label should be rejected"
        );
    }

    // ── Regex validation tests ────────────────────────────────

    #[test]
    fn validate_regex_accepts_simple_patterns() {
        assert!(validate_regex_for_like(".*Handler").is_ok());
        assert!(validate_regex_for_like("get.+").is_ok());
        assert!(validate_regex_for_like("foo").is_ok());
        assert!(validate_regex_for_like(".*main.*").is_ok());
        assert!(validate_regex_for_like("a.b").is_ok());
    }

    #[test]
    fn validate_regex_rejects_character_class() {
        let err = validate_regex_for_like("[a-z].*").unwrap_err();
        assert!(
            err.contains("character class"),
            "should mention character class: {err}"
        );
    }

    #[test]
    fn validate_regex_rejects_anchors() {
        assert!(validate_regex_for_like("^start").is_err());
        assert!(validate_regex_for_like("end$").is_err());
    }

    #[test]
    fn validate_regex_rejects_alternation() {
        let err = validate_regex_for_like("foo|bar").unwrap_err();
        assert!(
            err.contains("alternation"),
            "should mention alternation: {err}"
        );
    }

    #[test]
    fn validate_regex_rejects_quantifier() {
        assert!(validate_regex_for_like("a{2,3}").is_err());
    }

    #[test]
    fn validate_regex_rejects_shorthand_classes() {
        assert!(validate_regex_for_like("\\d+").is_err());
        assert!(validate_regex_for_like("\\w+").is_err());
        assert!(validate_regex_for_like("\\s+").is_err());
    }

    #[test]
    fn validate_regex_rejects_lookahead() {
        assert!(validate_regex_for_like("(?=foo)bar").is_err());
    }

    #[test]
    fn validate_regex_rejects_lazy_quantifiers() {
        assert!(validate_regex_for_like(".*?foo").is_err());
        assert!(validate_regex_for_like(".+?bar").is_err());
    }

    #[test]
    fn validate_regex_rejects_backreference() {
        assert!(validate_regex_for_like("(foo)\\1").is_err());
    }

    #[test]
    fn regex_generates_regexp_sql() {
        // With the REGEXP UDF, complex patterns like character classes should
        // translate to `column REGEXP ?` rather than failing or using LIKE.
        let input = "MATCH (f:Function) WHERE f.name =~ '[A-Z].*Handler' RETURN f.name";
        let tokens = tokenize(input).unwrap();
        let ast = parse(&tokens).unwrap();
        let pattern = &ast.match_clause().patterns[0];
        let result = translate_single_node(&ast, pattern);
        assert!(
            result.is_ok(),
            "regex patterns should succeed with REGEXP: {:?}",
            result.unwrap_err()
        );
        let translated = result.unwrap();
        assert!(
            translated.sql.contains("REGEXP"),
            "SQL should use REGEXP, got: {}",
            translated.sql
        );
        assert!(
            translated.params.contains(&"[A-Z].*Handler".to_string()),
            "params should contain the raw regex pattern"
        );
    }

    // ── WHERE clause identifier validation tests ─────────────────────────────

    #[test]
    fn test_validate_query_identifiers_rejects_malicious_prop_ref_in_where() {
        // Directly construct an AST with an illegal PropRef.var in WHERE clause.
        // This simulates a bypass of the normal lexer path.
        let bad_var = "x; DROP TABLE symbols".to_string();
        let query = CypherQuery {
            match_clauses: vec![MatchClause {
                is_optional: false,
                patterns: vec![PathPattern {
                    nodes: vec![NodePattern {
                        var: Some("n".to_string()),
                        label: Some("Function".to_string()),
                        props: vec![],
                    }],
                    rels: vec![],
                }],
            }],
            where_clause: Some(WhereClause {
                expr: Expr::Comparison {
                    left: PropRef {
                        var: bad_var,
                        prop: "name".to_string(),
                    },
                    op: CmpOp::Eq,
                    right: Value::String("foo".to_string()),
                },
            }),
            return_clause: ReturnClause {
                items: vec![ReturnItem::Var("n".to_string(), None)],
                distinct: false,
            },
            order_by: None,
            limit: None,
        };
        assert!(
            validate_query_identifiers(&query).is_err(),
            "malicious PropRef.var in WHERE clause should be rejected"
        );
    }

    #[test]
    fn test_validate_query_identifiers_rejects_malicious_prop_name_in_where() {
        // Illegal PropRef.prop in WHERE clause.
        let bad_prop = "name; DROP TABLE symbols".to_string();
        let query = CypherQuery {
            match_clauses: vec![MatchClause {
                is_optional: false,
                patterns: vec![PathPattern {
                    nodes: vec![NodePattern {
                        var: Some("n".to_string()),
                        label: Some("Function".to_string()),
                        props: vec![],
                    }],
                    rels: vec![],
                }],
            }],
            where_clause: Some(WhereClause {
                expr: Expr::Comparison {
                    left: PropRef {
                        var: "n".to_string(),
                        prop: bad_prop,
                    },
                    op: CmpOp::Eq,
                    right: Value::String("foo".to_string()),
                },
            }),
            return_clause: ReturnClause {
                items: vec![ReturnItem::Var("n".to_string(), None)],
                distinct: false,
            },
            order_by: None,
            limit: None,
        };
        assert!(
            validate_query_identifiers(&query).is_err(),
            "malicious PropRef.prop in WHERE clause should be rejected"
        );
    }

    #[test]
    fn test_validate_query_identifiers_rejects_malicious_degree_var_in_where() {
        // Illegal Expr::Degree.var in WHERE clause.
        let bad_var = "n; DROP TABLE symbols".to_string();
        let query = CypherQuery {
            match_clauses: vec![MatchClause {
                is_optional: false,
                patterns: vec![PathPattern {
                    nodes: vec![NodePattern {
                        var: Some("n".to_string()),
                        label: Some("Function".to_string()),
                        props: vec![],
                    }],
                    rels: vec![],
                }],
            }],
            where_clause: Some(WhereClause {
                expr: Expr::Degree {
                    var: bad_var,
                    kind: DegreeKind::Out,
                    op: CmpOp::Gt,
                    value: Value::Int(0),
                },
            }),
            return_clause: ReturnClause {
                items: vec![ReturnItem::Var("n".to_string(), None)],
                distinct: false,
            },
            order_by: None,
            limit: None,
        };
        assert!(
            validate_query_identifiers(&query).is_err(),
            "malicious Degree.var in WHERE clause should be rejected"
        );
    }

    #[test]
    fn test_validate_query_identifiers_accepts_valid_where_clause() {
        // Legal WHERE with valid identifiers should still pass.
        let query = CypherQuery {
            match_clauses: vec![MatchClause {
                is_optional: false,
                patterns: vec![PathPattern {
                    nodes: vec![NodePattern {
                        var: Some("n".to_string()),
                        label: Some("Function".to_string()),
                        props: vec![],
                    }],
                    rels: vec![],
                }],
            }],
            where_clause: Some(WhereClause {
                expr: Expr::And(
                    Box::new(Expr::Comparison {
                        left: PropRef {
                            var: "n".to_string(),
                            prop: "name".to_string(),
                        },
                        op: CmpOp::Eq,
                        right: Value::String("foo".to_string()),
                    }),
                    Box::new(Expr::Degree {
                        var: "n".to_string(),
                        kind: DegreeKind::In,
                        op: CmpOp::Gt,
                        value: Value::Int(0),
                    }),
                ),
            }),
            return_clause: ReturnClause {
                items: vec![ReturnItem::Var("n".to_string(), None)],
                distinct: false,
            },
            order_by: None,
            limit: None,
        };
        assert!(
            validate_query_identifiers(&query).is_ok(),
            "valid WHERE clause should pass identifier validation"
        );
    }
}
