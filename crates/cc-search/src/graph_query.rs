//! Graph Query DSL — Cypher subset for querying the evidence graph.
//!
//! Supports a subset of Cypher syntax:
//!   MATCH (f:Function)-[:CALLS]->(g) WHERE f.name =~ '.*Handler' RETURN g.name LIMIT 10
//!   MATCH (f)-[:CALLS*1..3]->(g:Function) WHERE g.name =~ '.*Handler' RETURN f.name
//!   MATCH (f:File) WHERE f.file_path CONTAINS 'controller' RETURN f.file_path
//!
//! Internally translates to SQL JOIN queries against the index tables.

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Edge-type → table mapping
// ---------------------------------------------------------------------------

struct EdgeTableInfo {
    table: &'static str,
    src_col: &'static str,
    dst_col: &'static str,
}

fn edge_table_map() -> HashMap<&'static str, EdgeTableInfo> {
    let mut m = HashMap::new();
    m.insert(
        "CALLS",
        EdgeTableInfo {
            table: "call_edges",
            src_col: "caller_symbol_uid",
            dst_col: "callee_symbol_uid",
        },
    );
    m.insert(
        "IMPORTS",
        EdgeTableInfo {
            table: "imports",
            src_col: "file_path",
            dst_col: "resolved_path",
        },
    );
    m.insert(
        "TESTS",
        EdgeTableInfo {
            table: "test_edges",
            src_col: "test_file_path",
            dst_col: "code_file_path",
        },
    );
    m.insert(
        "ROUTES",
        EdgeTableInfo {
            table: "route_edges",
            src_col: "file_path",
            dst_col: "route_path",
        },
    );
    m.insert(
        "REFS",
        EdgeTableInfo {
            table: "symbol_refs",
            src_col: "file_path",
            dst_col: "target_file_path",
        },
    );
    m
}

fn label_table(label: &str) -> &'static str {
    match label {
        "Function" | "Class" | "Method" | "Symbol" => "symbols",
        "File" => "files",
        "Chunk" => "chunks",
        _ => "symbols",
    }
}

fn label_kind_filter(label: &str) -> Option<&'static str> {
    match label {
        "Function" => Some("function"),
        "Class" => Some("class"),
        "Method" => Some("method"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Token types and tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Keyword,   // MATCH, WHERE, RETURN, ORDER BY, LIMIT, AND, OR, NOT, AS, DESC, ASC
    Op,        // =, <>, <, >, <=, >=, =~, CONTAINS
    Arrow,     // ->, <-, -[, ]-, ]->, <-[, (, ), [, ]
    VarLen,    // *1..3
    Number,    // 42, 3.14
    StringLit, // 'foo', "bar"
    Colon,     // :
    Comma,     // ,
    Dot,       // .
    Ident,     // variable names, labels
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    value: String,
    pos: usize,
}

fn tokenize(input: &str) -> CcResult<Vec<Token>> {
    let pattern = Regex::new(concat!(
        r"(?xi)",
        r"(?P<keyword>MATCH|WHERE|RETURN|LIMIT|ORDER\s+BY|AND|OR|NOT|ASC|DESC|AS)",
        r"|(?P<op>=~|<>|!=|<=|>=|=|<|>|CONTAINS)",
        r#"|(?P<arrow>-\[|\]\->|\]\-|\->|<\-\[|<\-|\(|\)|\[|\])"#,
        r"|(?P<varlen>\*\d+\.\.\d+)",
        r"|(?P<number>\d+(?:\.\d+)?)",
        r#"|(?P<string>'[^']*'|"[^"]*")"#,
        r"|(?P<colon>:)",
        r"|(?P<comma>,)",
        r"|(?P<dot>\.)",
        r"|(?P<ident>[A-Za-z_][A-Za-z0-9_]*)",
    ))
    .map_err(|e| CcError::Search(format!("tokenizer regex error: {e}")))?;

    let mut tokens = Vec::new();
    for cap in pattern.captures_iter(input) {
        let m = cap.get(0).unwrap();
        let value = m.as_str().to_string();
        let pos = m.start();

        let kind = if cap.name("keyword").is_some() {
            TokenKind::Keyword
        } else if cap.name("op").is_some() {
            TokenKind::Op
        } else if cap.name("arrow").is_some() {
            TokenKind::Arrow
        } else if cap.name("varlen").is_some() {
            TokenKind::VarLen
        } else if cap.name("number").is_some() {
            TokenKind::Number
        } else if cap.name("string").is_some() {
            TokenKind::StringLit
        } else if cap.name("colon").is_some() {
            TokenKind::Colon
        } else if cap.name("comma").is_some() {
            TokenKind::Comma
        } else if cap.name("dot").is_some() {
            TokenKind::Dot
        } else if cap.name("ident").is_some() {
            // Check if ident is actually CONTAINS (operator)
            if value.eq_ignore_ascii_case("CONTAINS") {
                TokenKind::Op
            } else {
                TokenKind::Ident
            }
        } else {
            continue;
        };

        // Normalize keyword values to uppercase, collapse whitespace.
        let normalized_value = if kind == TokenKind::Keyword {
            let upper = value.to_uppercase();
            // "ORDER  BY" → "ORDER BY"
            let collapsed: String = upper.split_whitespace().collect::<Vec<_>>().join(" ");
            collapsed
        } else {
            value
        };

        tokens.push(Token {
            kind,
            value: normalized_value,
            pos,
        });
    }

    Ok(tokens)
}

// ---------------------------------------------------------------------------
// AST structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct NodePattern {
    variable: Option<String>,
    label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Right,
    Left,
    Both,
}

#[derive(Debug, Clone)]
struct RelPattern {
    variable: Option<String>,
    edge_type: Option<String>,
    direction: Direction,
    min_hops: Option<u32>,
    max_hops: Option<u32>,
}

impl Default for RelPattern {
    fn default() -> Self {
        Self {
            variable: None,
            edge_type: None,
            direction: Direction::Right,
            min_hops: Some(1),
            max_hops: Some(1),
        }
    }
}

#[derive(Debug, Clone)]
struct WhereClause {
    left: String,
    op: WhereOp,
    right: String,
}

#[derive(Debug, Clone, PartialEq)]
enum WhereOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    Lte,
    Gte,
    Regex,
    Contains,
    Like,
}

impl WhereOp {
    fn from_str(s: &str) -> CcResult<Self> {
        match s {
            "=" => Ok(Self::Eq),
            "<>" | "!=" => Ok(Self::NotEq),
            "<" => Ok(Self::Lt),
            ">" => Ok(Self::Gt),
            "<=" => Ok(Self::Lte),
            ">=" => Ok(Self::Gte),
            "=~" => Ok(Self::Regex),
            _ if s.eq_ignore_ascii_case("CONTAINS") => Ok(Self::Contains),
            _ if s.eq_ignore_ascii_case("LIKE") => Ok(Self::Like),
            _ => Err(CcError::Search(format!("unknown operator: {s}"))),
        }
    }
}

#[derive(Debug, Clone)]
struct ReturnField {
    expr: String,
    alias: Option<String>,
}

#[derive(Debug, Clone)]
struct QueryAST {
    node: NodePattern,
    relationship: Option<RelPattern>,
    target: Option<NodePattern>,
    where_clauses: Vec<WhereClause>,
    return_fields: Vec<ReturnField>,
    order_by: Option<(String, bool)>, // (field, ascending)
    limit: Option<u32>,
}

impl Default for QueryAST {
    fn default() -> Self {
        Self {
            node: NodePattern::default(),
            relationship: None,
            target: None,
            where_clauses: Vec::new(),
            return_fields: Vec::new(),
            order_by: None,
            limit: Some(50),
        }
    }
}

// ---------------------------------------------------------------------------
// Recursive descent parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> CcResult<&Token> {
        if self.pos < self.tokens.len() {
            let idx = self.pos;
            self.pos += 1;
            Ok(&self.tokens[idx])
        } else {
            Err(CcError::Search("unexpected end of query".into()))
        }
    }

    fn expect_value(&mut self, val: &str) -> CcResult<()> {
        let tok = self.advance()?;
        if !tok.value.eq_ignore_ascii_case(val) {
            return Err(CcError::Search(format!(
                "expected '{}', got '{}' at pos {}",
                val, tok.value, tok.pos
            )));
        }
        Ok(())
    }

    fn check_value(&self, val: &str) -> bool {
        self.peek()
            .map(|t| t.value.eq_ignore_ascii_case(val))
            .unwrap_or(false)
    }

    fn check_kind(&self, kind: &TokenKind) -> bool {
        self.peek().map(|t| &t.kind == kind).unwrap_or(false)
    }

    fn parse(mut self) -> CcResult<QueryAST> {
        let mut ast = QueryAST::default();

        // Optional MATCH keyword.
        if self.check_value("MATCH") {
            self.advance()?;
        }

        // Parse source node pattern.
        ast.node = self.parse_node_pattern()?;

        // Check for relationship.
        if let Some(tok) = self.peek() {
            if matches!(tok.value.as_str(), "-[" | "->" | "<-" | "<-[") {
                let rel = self.parse_rel_pattern()?;
                ast.relationship = Some(rel);
                ast.target = Some(self.parse_node_pattern()?);
            }
        }

        // Parse WHERE clause.
        if self.check_value("WHERE") {
            self.advance()?;
            loop {
                let clause = self.parse_where_condition()?;
                ast.where_clauses.push(clause);
                if let Some(tok) = self.peek() {
                    if tok.value.eq_ignore_ascii_case("AND") || tok.value.eq_ignore_ascii_case("OR")
                    {
                        self.advance()?;
                        continue;
                    }
                }
                break;
            }
        }

        // Parse RETURN clause.
        if self.check_value("RETURN") {
            self.advance()?;
            loop {
                let field = self.parse_return_field()?;
                ast.return_fields.push(field);
                if self.check_kind(&TokenKind::Comma) {
                    self.advance()?;
                    continue;
                }
                break;
            }
        }

        // Parse ORDER BY.
        if self.check_value("ORDER BY") {
            self.advance()?;
            let mut order_field = self.advance()?.value.clone();
            if self.check_kind(&TokenKind::Dot) {
                self.advance()?;
                let part = self.advance()?.value.clone();
                order_field = format!("{order_field}.{part}");
            }
            let ascending = if let Some(tok) = self.peek() {
                if tok.value.eq_ignore_ascii_case("DESC") {
                    self.advance()?;
                    false
                } else if tok.value.eq_ignore_ascii_case("ASC") {
                    self.advance()?;
                    true
                } else {
                    true
                }
            } else {
                true
            };
            ast.order_by = Some((order_field, ascending));
        }

        // Parse LIMIT.
        if self.check_value("LIMIT") {
            self.advance()?;
            let tok = self.advance()?;
            let n = tok
                .value
                .parse::<u32>()
                .map_err(|_| CcError::Search(format!("invalid LIMIT value: {}", tok.value)))?;
            ast.limit = Some(n);
        }

        Ok(ast)
    }

    fn parse_node_pattern(&mut self) -> CcResult<NodePattern> {
        self.expect_value("(")?;
        let mut np = NodePattern::default();

        // Optional variable name.
        if self.check_kind(&TokenKind::Ident) {
            np.variable = Some(self.advance()?.value.clone());
        }

        // Optional :Label.
        if self.check_kind(&TokenKind::Colon) {
            self.advance()?;
            np.label = Some(self.advance()?.value.clone());
        }

        self.expect_value(")")?;
        Ok(np)
    }

    fn parse_rel_pattern(&mut self) -> CcResult<RelPattern> {
        let mut rp = RelPattern::default();

        let tok = self
            .peek()
            .ok_or_else(|| CcError::Search("expected relationship pattern".into()))?;
        let start_value = tok.value.clone();

        match start_value.as_str() {
            "<-" | "<-[" => {
                rp.direction = Direction::Left;
                self.advance()?;
                if start_value == "<-" {
                    // Might have [...] next.
                    if self.check_value("[") {
                        self.advance()?;
                        self.parse_rel_body(&mut rp)?;
                        // Expect ] or ]- or ]->
                        let close = self.advance()?;
                        if close.value == "]->" {
                            // <-[...]->, direction = Both
                            rp.direction = Direction::Both;
                        }
                        // else "]" or "]-", direction stays Left
                    }
                    // Check for trailing -> making it Both if we haven't already.
                    if rp.direction == Direction::Left && self.check_value("->") {
                        self.advance()?;
                        rp.direction = Direction::Both;
                    }
                } else {
                    // <-[
                    self.parse_rel_body(&mut rp)?;
                    let close = self.advance()?;
                    if close.value == "]->" {
                        rp.direction = Direction::Both;
                    }
                    // else "]" or "]-", direction stays Left
                }
            }
            "-[" => {
                self.advance()?;
                self.parse_rel_body(&mut rp)?;
                let close = self.advance()?;
                if close.value == "]->" {
                    rp.direction = Direction::Right;
                } else {
                    // "]" or "]-" — undirected or default right
                    rp.direction = Direction::Right;
                }
            }
            "->" => {
                self.advance()?;
                rp.direction = Direction::Right;
            }
            _ => {
                return Err(CcError::Search(format!(
                    "expected relationship arrow, got '{start_value}'"
                )));
            }
        }

        Ok(rp)
    }

    fn parse_rel_body(&mut self, rp: &mut RelPattern) -> CcResult<()> {
        // Optional variable name.
        if self.check_kind(&TokenKind::Ident) {
            rp.variable = Some(self.advance()?.value.clone());
        }

        // Optional :TYPE.
        if self.check_kind(&TokenKind::Colon) {
            self.advance()?;
            rp.edge_type = Some(self.advance()?.value.to_uppercase());
        }

        // Optional *min..max.
        if self.check_kind(&TokenKind::VarLen) {
            let val = self.advance()?.value.clone();
            // val is like "*1..3"
            let inner = &val[1..]; // "1..3"
            let parts: Vec<&str> = inner.split("..").collect();
            rp.min_hops = Some(
                parts[0]
                    .parse::<u32>()
                    .map_err(|_| CcError::Search(format!("invalid min hops: {}", parts[0])))?,
            );
            if parts.len() > 1 {
                rp.max_hops = Some(
                    parts[1]
                        .parse::<u32>()
                        .map_err(|_| CcError::Search(format!("invalid max hops: {}", parts[1])))?,
                );
            } else {
                rp.max_hops = rp.min_hops;
            }
        }

        Ok(())
    }

    fn parse_where_condition(&mut self) -> CcResult<WhereClause> {
        // Parse field (possibly dotted: f.name).
        let mut field_parts = Vec::new();
        field_parts.push(self.advance()?.value.clone());
        while self.check_kind(&TokenKind::Dot) {
            self.advance()?;
            field_parts.push(self.advance()?.value.clone());
        }
        let left = field_parts.join(".");

        // Parse operator.
        let op_tok = self.advance()?;
        let op = WhereOp::from_str(&op_tok.value)?;

        // Parse value.
        let val_tok = self.advance()?;
        let right = if val_tok.kind == TokenKind::StringLit {
            // Strip surrounding quotes.
            let s = &val_tok.value;
            s[1..s.len() - 1].to_string()
        } else {
            val_tok.value.clone()
        };

        Ok(WhereClause { left, op, right })
    }

    fn parse_return_field(&mut self) -> CcResult<ReturnField> {
        let mut parts = Vec::new();
        parts.push(self.advance()?.value.clone());

        // Collect dotted parts.
        while self.check_kind(&TokenKind::Dot) {
            self.advance()?;
            parts.push(self.advance()?.value.clone());
        }

        let expr = parts.join(".");

        // Check for AS alias.
        let alias = if self.check_value("AS") {
            self.advance()?;
            Some(self.advance()?.value.clone())
        } else {
            None
        };

        Ok(ReturnField { expr, alias })
    }
}

fn parse(tokens: Vec<Token>) -> CcResult<QueryAST> {
    let parser = Parser::new(tokens);
    parser.parse()
}

// ---------------------------------------------------------------------------
// SQL translator
// ---------------------------------------------------------------------------

fn translate(ast: &QueryAST) -> CcResult<(String, Vec<String>)> {
    let mut params: Vec<String> = Vec::new();

    if ast.relationship.is_none() {
        // Single-node query.
        return translate_single_node(ast, &mut params);
    }

    let rel = ast.relationship.as_ref().unwrap();
    let edge_type_str = rel.edge_type.as_deref().unwrap_or("CALLS");
    let etm = edge_table_map();
    let edge_info = etm
        .get(edge_type_str)
        .ok_or_else(|| CcError::Search(format!("unknown edge type: {edge_type_str}")))?;

    let src_alias = ast.node.variable.as_deref().unwrap_or("src");
    let target_node = ast.target.as_ref().cloned().unwrap_or_default();
    let dst_alias = target_node.variable.as_deref().unwrap_or("dst");

    let src_table = ast
        .node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");
    let dst_table = target_node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");

    let min_hops = rel.min_hops.unwrap_or(1);
    let max_hops = rel.max_hops.unwrap_or(1);
    let is_variable_length = min_hops != 1 || max_hops != 1;

    if !is_variable_length {
        // Simple single-hop JOIN.
        return translate_single_hop(
            ast,
            &mut params,
            edge_info,
            edge_type_str,
            src_alias,
            dst_alias,
            src_table,
            dst_table,
            &target_node,
        );
    }

    // Variable-length path via recursive CTE (only CALLS supported).
    if edge_type_str != "CALLS" {
        return Err(CcError::Search(format!(
            "variable-length paths only supported for CALLS edges, got {edge_type_str}"
        )));
    }

    translate_variable_length(
        ast,
        &mut params,
        src_alias,
        dst_alias,
        &target_node,
        min_hops,
        max_hops,
    )
}

fn translate_single_node(
    ast: &QueryAST,
    params: &mut Vec<String>,
) -> CcResult<(String, Vec<String>)> {
    let table = ast
        .node
        .label
        .as_deref()
        .map(label_table)
        .unwrap_or("symbols");
    let alias = ast.node.variable.as_deref().unwrap_or("n");
    let select_cols = resolve_return_fields(&ast.return_fields, alias);

    let mut sql = format!("SELECT {select_cols} FROM {table} AS {alias}");

    let mut where_parts = build_where(&ast.where_clauses, params);
    // Kind filter from label.
    if let Some(label) = ast.node.label.as_deref() {
        if let Some(kind) = label_kind_filter(label) {
            where_parts.push(format!("{alias}.kind = ?{}", params.len() + 1));
            params.push(kind.to_string());
        }
    }

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    if let Some((field, asc)) = &ast.order_by {
        let dir = if *asc { "ASC" } else { "DESC" };
        sql.push_str(&format!(" ORDER BY {field} {dir}"));
    }

    let limit = ast.limit.unwrap_or(50);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok((sql, params.clone()))
}

#[allow(clippy::too_many_arguments)]
fn translate_single_hop(
    ast: &QueryAST,
    params: &mut Vec<String>,
    edge_info: &EdgeTableInfo,
    edge_type_str: &str,
    src_alias: &str,
    dst_alias: &str,
    src_table: &str,
    dst_table: &str,
    target_node: &NodePattern,
) -> CcResult<(String, Vec<String>)> {
    let edge_alias = "e";
    let select_cols = resolve_return_fields(&ast.return_fields, dst_alias);

    let (join_src, join_dst) = match edge_type_str {
        "CALLS" => (
            format!(
                "{src_table} AS {src_alias} ON {src_alias}.symbol_uid = {edge_alias}.{}",
                edge_info.src_col
            ),
            format!(
                "{dst_table} AS {dst_alias} ON {dst_alias}.symbol_uid = {edge_alias}.{}",
                edge_info.dst_col
            ),
        ),
        "IMPORTS" | "REFS" => (
            format!(
                "{src_table} AS {src_alias} ON {src_alias}.file_path = {edge_alias}.{}",
                edge_info.src_col
            ),
            format!(
                "{dst_table} AS {dst_alias} ON {dst_alias}.file_path = {edge_alias}.{}",
                edge_info.dst_col
            ),
        ),
        "TESTS" => (
            format!(
                "{src_table} AS {src_alias} ON {src_alias}.file_path = {edge_alias}.{}",
                edge_info.src_col
            ),
            format!(
                "{dst_table} AS {dst_alias} ON {dst_alias}.file_path = {edge_alias}.{}",
                edge_info.dst_col
            ),
        ),
        "ROUTES" => (
            format!(
                "{src_table} AS {src_alias} ON {src_alias}.file_path = {edge_alias}.{}",
                edge_info.src_col
            ),
            format!(
                "(SELECT DISTINCT route_path AS file_path FROM route_edges) AS {dst_alias} ON {dst_alias}.file_path = {edge_alias}.{}",
                edge_info.dst_col
            ),
        ),
        _ => (
            format!(
                "{src_table} AS {src_alias} ON {src_alias}.file_path = {edge_alias}.{}",
                edge_info.src_col
            ),
            format!(
                "{dst_table} AS {dst_alias} ON {dst_alias}.file_path = {edge_alias}.{}",
                edge_info.dst_col
            ),
        ),
    };

    let mut sql = format!(
        "SELECT DISTINCT {select_cols} FROM {} AS {edge_alias} JOIN {join_src} JOIN {join_dst}",
        edge_info.table
    );

    let mut where_parts = build_where(&ast.where_clauses, params);

    // Kind filters from labels.
    for (alias, label_opt) in [
        (src_alias, ast.node.label.as_deref()),
        (dst_alias, target_node.label.as_deref()),
    ] {
        if let Some(label) = label_opt {
            if let Some(kind) = label_kind_filter(label) {
                where_parts.push(format!("{alias}.kind = ?{}", params.len() + 1));
                params.push(kind.to_string());
            }
        }
    }

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    if let Some((field, asc)) = &ast.order_by {
        let dir = if *asc { "ASC" } else { "DESC" };
        sql.push_str(&format!(" ORDER BY {field} {dir}"));
    }

    let limit = ast.limit.unwrap_or(50);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok((sql, params.clone()))
}

fn translate_variable_length(
    ast: &QueryAST,
    params: &mut Vec<String>,
    src_alias: &str,
    dst_alias: &str,
    target_node: &NodePattern,
    min_hops: u32,
    max_hops: u32,
) -> CcResult<(String, Vec<String>)> {
    // Separate WHERE clauses for source vs target.
    let mut src_where_clauses = Vec::new();
    let mut dst_where_clauses = Vec::new();
    for wc in &ast.where_clauses {
        let parts: Vec<&str> = wc.left.splitn(2, '.').collect();
        if parts.len() == 2 && parts[0] == src_alias {
            src_where_clauses.push(wc.clone());
        } else if parts.len() == 2 && parts[0] == dst_alias {
            dst_where_clauses.push(wc.clone());
        }
    }

    let src_filter_parts = build_where(&src_where_clauses, params);
    let cte_where = if src_filter_parts.is_empty() {
        "1=1".to_string()
    } else {
        // Replace variable alias with 's' for the CTE base.
        src_filter_parts
            .iter()
            .map(|p| p.replace(&format!("{src_alias}."), "s."))
            .collect::<Vec<_>>()
            .join(" AND ")
    };

    let select_cols = resolve_return_fields(&ast.return_fields, dst_alias);

    let max_param_idx = params.len() + 1;
    params.push(max_hops.to_string());
    let min_param_idx = params.len() + 1;
    params.push(min_hops.to_string());

    let mut sql = format!(
        "WITH RECURSIVE path_cte(uid, depth) AS (\
            SELECT s.symbol_uid, 0 FROM symbols AS s WHERE {cte_where} \
            UNION ALL \
            SELECT ce.callee_symbol_uid, pc.depth + 1 \
            FROM path_cte pc \
            JOIN call_edges ce ON ce.caller_symbol_uid = pc.uid \
            WHERE pc.depth < ?{max_param_idx}\
        ) \
        SELECT DISTINCT {select_cols} \
        FROM path_cte \
        JOIN symbols AS {src_alias} ON 1=0 \
        JOIN symbols AS {dst_alias} ON {dst_alias}.symbol_uid = path_cte.uid \
        WHERE path_cte.depth >= ?{min_param_idx}"
    );

    let dst_filter_parts = build_where(&dst_where_clauses, params);
    for part in &dst_filter_parts {
        sql.push_str(" AND ");
        sql.push_str(part);
    }

    // Kind filter on target.
    if let Some(label) = target_node.label.as_deref() {
        if let Some(kind) = label_kind_filter(label) {
            sql.push_str(&format!(" AND {dst_alias}.kind = ?{}", params.len() + 1));
            params.push(kind.to_string());
        }
    }

    let limit = ast.limit.unwrap_or(50);
    sql.push_str(&format!(" LIMIT ?{}", params.len() + 1));
    params.push(limit.to_string());

    Ok((sql, params.clone()))
}

fn resolve_return_fields(fields: &[ReturnField], _default_alias: &str) -> String {
    if fields.is_empty() {
        return "*".to_string();
    }
    fields
        .iter()
        .map(|f| {
            let expr = if !f.expr.contains('.') {
                // Bare variable name like "g" → expand to "g.*"
                format!("{}.*", f.expr)
            } else {
                f.expr.clone()
            };
            if let Some(alias) = &f.alias {
                format!("{expr} AS {alias}")
            } else {
                expr
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_where(clauses: &[WhereClause], params: &mut Vec<String>) -> Vec<String> {
    let mut parts = Vec::new();
    for wc in clauses {
        let idx = params.len() + 1;
        match wc.op {
            WhereOp::Regex => {
                // Without the rusqlite "functions" feature, REGEXP is not available.
                // Fall back to LIKE with pattern conversion.
                parts.push(format!("{} LIKE ?{idx}", wc.left));
                params.push(regex_to_like(&wc.right));
            }
            WhereOp::Contains => {
                parts.push(format!("{} LIKE ?{idx}", wc.left));
                params.push(format!("%{}%", wc.right));
            }
            WhereOp::Like => {
                parts.push(format!("{} LIKE ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
            WhereOp::Eq => {
                parts.push(format!("{} = ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
            WhereOp::NotEq => {
                parts.push(format!("{} <> ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
            WhereOp::Lt => {
                parts.push(format!("{} < ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
            WhereOp::Gt => {
                parts.push(format!("{} > ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
            WhereOp::Lte => {
                parts.push(format!("{} <= ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
            WhereOp::Gte => {
                parts.push(format!("{} >= ?{idx}", wc.left));
                params.push(wc.right.clone());
            }
        }
    }
    parts
}

// ---------------------------------------------------------------------------
// REGEXP UDF registration (requires rusqlite "functions" feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "regexp-udf")]
fn register_regexp_function(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.create_scalar_function(
        "REGEXP",
        2,
        rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let pattern: String = ctx.get(0)?;
            let value: Option<String> = ctx.get(1)?;
            match value {
                None => Ok(false),
                Some(val) => {
                    let re = Regex::new(&pattern).map_err(|e| {
                        rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            e.to_string(),
                        )))
                    })?;
                    Ok(re.is_match(&val))
                }
            }
        },
    )
}

/// Convert a basic regex pattern to a LIKE pattern for SQLite.
/// This handles common cases: `.*` → `%`, `.+` → `_%`, anchoring, etc.
fn regex_to_like(pattern: &str) -> String {
    pattern
        .replace(".*", "%")
        .replace(".+", "_%")
        .replace('.', "_")
}

// ---------------------------------------------------------------------------
// Public API — GraphQueryEngine (backward compatible)
// ---------------------------------------------------------------------------

pub struct GraphQueryEngine {
    db: Arc<IndexDb>,
}

impl GraphQueryEngine {
    pub fn new(db: Arc<IndexDb>) -> Self {
        Self { db }
    }

    /// Execute a Cypher-subset query, returning JSON results.
    pub fn execute(&self, query: &str) -> CcResult<Vec<serde_json::Value>> {
        self.execute_query(query)
    }

    /// Execute a Cypher-subset query using the full tokenizer/parser/translator pipeline.
    pub fn execute_query(&self, query: &str) -> CcResult<Vec<serde_json::Value>> {
        let tokens = tokenize(query)?;
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let ast = parse(tokens)?;
        let (sql, params) = translate(&ast)?;

        // Register REGEXP UDF if the feature is available.
        #[cfg(feature = "regexp-udf")]
        if let Ok(conn) = self.db.read_conn() {
            let _ = register_regexp_function(&conn);
        }

        self.db
            .query_json(&sql, &params)
            .map_err(|e| CcError::Search(format!("graph query SQL error: {e} [sql={sql}]")))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_db() -> Arc<IndexDb> {
        let tmp = TempDir::new().unwrap();
        Arc::new(IndexDb::open(&tmp.path().join("test.db")).unwrap().0)
    }

    // --- Tokenizer tests ---

    #[test]
    fn test_tokenize_basic_query() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g) WHERE f.name = 'foo' RETURN g.name LIMIT 10",
        )
        .unwrap();

        // Verify we got the right token sequence.
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Keyword && t.value == "MATCH"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Ident && t.value == "f"));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Colon));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Ident && t.value == "Function"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Arrow && t.value == "-["));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Arrow && t.value == "]->"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Keyword && t.value == "WHERE"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Op && t.value == "="));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::StringLit && t.value == "'foo'"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Keyword && t.value == "RETURN"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Number && t.value == "10"));
    }

    #[test]
    fn test_tokenize_varlen() {
        let tokens = tokenize("MATCH (f)-[:CALLS*1..3]->(g) RETURN g").unwrap();
        let varlen = tokens.iter().find(|t| t.kind == TokenKind::VarLen);
        assert!(varlen.is_some());
        assert_eq!(varlen.unwrap().value, "*1..3");
    }

    #[test]
    fn test_tokenize_operators() {
        let tokens = tokenize("WHERE f.name =~ '.*Handler' AND g.score >= 5").unwrap();
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Op && t.value == "=~"));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Op && t.value == ">="));
    }

    #[test]
    fn test_tokenize_contains() {
        let tokens = tokenize("WHERE f.name CONTAINS 'test'").unwrap();
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Op && t.value == "CONTAINS"));
    }

    // --- Parser tests ---

    #[test]
    fn test_parse_match_where_return() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g:Symbol) WHERE f.name = 'main' RETURN g.name LIMIT 5",
        )
        .unwrap();
        let ast = parse(tokens).unwrap();

        assert_eq!(ast.node.variable.as_deref(), Some("f"));
        assert_eq!(ast.node.label.as_deref(), Some("Function"));
        assert!(ast.relationship.is_some());
        let rel = ast.relationship.as_ref().unwrap();
        assert_eq!(rel.edge_type.as_deref(), Some("CALLS"));
        assert_eq!(rel.direction, Direction::Right);
        assert!(ast.target.is_some());
        let target = ast.target.as_ref().unwrap();
        assert_eq!(target.variable.as_deref(), Some("g"));
        assert_eq!(target.label.as_deref(), Some("Symbol"));
        assert_eq!(ast.where_clauses.len(), 1);
        assert_eq!(ast.where_clauses[0].left, "f.name");
        assert_eq!(ast.where_clauses[0].op, WhereOp::Eq);
        assert_eq!(ast.where_clauses[0].right, "main");
        assert_eq!(ast.return_fields.len(), 1);
        assert_eq!(ast.return_fields[0].expr, "g.name");
        assert_eq!(ast.limit, Some(5));
    }

    #[test]
    fn test_parse_variable_length_path() {
        let tokens = tokenize(
            "MATCH (f)-[:CALLS*1..3]->(g:Function) WHERE g.name =~ '.*Handler' RETURN f.name",
        )
        .unwrap();
        let ast = parse(tokens).unwrap();

        assert!(ast.relationship.is_some());
        let rel = ast.relationship.as_ref().unwrap();
        assert_eq!(rel.edge_type.as_deref(), Some("CALLS"));
        assert_eq!(rel.min_hops, Some(1));
        assert_eq!(rel.max_hops, Some(3));
        assert_eq!(rel.direction, Direction::Right);
        assert!(ast.target.is_some());
        assert_eq!(
            ast.target.as_ref().unwrap().label.as_deref(),
            Some("Function")
        );
        assert_eq!(ast.where_clauses[0].op, WhereOp::Regex);
    }

    #[test]
    fn test_parse_single_node() {
        let tokens =
            tokenize("MATCH (f:File) WHERE f.file_path CONTAINS 'controller' RETURN f.file_path")
                .unwrap();
        let ast = parse(tokens).unwrap();

        assert_eq!(ast.node.variable.as_deref(), Some("f"));
        assert_eq!(ast.node.label.as_deref(), Some("File"));
        assert!(ast.relationship.is_none());
        assert!(ast.target.is_none());
        assert_eq!(ast.where_clauses[0].op, WhereOp::Contains);
    }

    #[test]
    fn test_parse_return_with_alias() {
        let tokens =
            tokenize("MATCH (f:Symbol) RETURN f.name AS symbol_name, f.file_path AS path LIMIT 10")
                .unwrap();
        let ast = parse(tokens).unwrap();

        assert_eq!(ast.return_fields.len(), 2);
        assert_eq!(ast.return_fields[0].expr, "f.name");
        assert_eq!(ast.return_fields[0].alias.as_deref(), Some("symbol_name"));
        assert_eq!(ast.return_fields[1].expr, "f.file_path");
        assert_eq!(ast.return_fields[1].alias.as_deref(), Some("path"));
    }

    #[test]
    fn test_parse_order_by() {
        let tokens =
            tokenize("MATCH (f:Symbol) RETURN f.name ORDER BY f.name DESC LIMIT 10").unwrap();
        let ast = parse(tokens).unwrap();

        assert_eq!(ast.order_by, Some(("f.name".to_string(), false)));
    }

    // --- Translator tests ---

    #[test]
    fn test_translate_single_node() {
        let tokens =
            tokenize("MATCH (f:Function) WHERE f.name = 'main' RETURN f.name LIMIT 10").unwrap();
        let ast = parse(tokens).unwrap();
        let (sql, params) = translate(&ast).unwrap();

        assert!(sql.contains("SELECT"));
        assert!(sql.contains("FROM symbols AS f"));
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("f.name = ?1"));
        assert!(sql.contains("f.kind = ?2"));
        assert!(sql.contains("LIMIT"));
        assert_eq!(params[0], "main");
        assert_eq!(params[1], "function");
    }

    #[test]
    fn test_translate_relationship_query() {
        let tokens = tokenize(
            "MATCH (f:Function)-[:CALLS]->(g:Symbol) WHERE f.name = 'main' RETURN g.name LIMIT 5",
        )
        .unwrap();
        let ast = parse(tokens).unwrap();
        let (sql, params) = translate(&ast).unwrap();

        assert!(sql.contains("call_edges"));
        assert!(sql.contains("JOIN"));
        assert!(sql.contains("symbol_uid"));
        assert!(sql.contains("f.name = ?1"));
        assert!(sql.contains("f.kind = ?2"));
        assert_eq!(params[0], "main");
        assert_eq!(params[1], "function");
    }

    #[test]
    fn test_translate_variable_length() {
        let tokens = tokenize(
            "MATCH (f)-[:CALLS*1..3]->(g:Function) WHERE f.name = 'init' RETURN g.name LIMIT 20",
        )
        .unwrap();
        let ast = parse(tokens).unwrap();
        let (sql, params) = translate(&ast).unwrap();

        assert!(sql.contains("WITH RECURSIVE"));
        assert!(sql.contains("path_cte"));
        assert!(sql.contains("callee_symbol_uid"));
        assert!(sql.contains("depth"));
        // params should include: 'init' for source filter, '3' for max_hops, '1' for min_hops,
        // 'function' for target kind, and '20' for limit.
        assert!(params.contains(&"init".to_string()));
        assert!(params.contains(&"3".to_string()));
        assert!(params.contains(&"1".to_string()));
        assert!(params.contains(&"function".to_string()));
    }

    #[test]
    fn test_translate_contains_op() {
        let tokens = tokenize(
            "MATCH (f:File) WHERE f.file_path CONTAINS 'controller' RETURN f.file_path LIMIT 10",
        )
        .unwrap();
        let ast = parse(tokens).unwrap();
        let (sql, params) = translate(&ast).unwrap();

        assert!(sql.contains("LIKE"));
        assert!(params.contains(&"%controller%".to_string()));
    }

    // --- Backward compatibility: existing tests ---

    #[test]
    fn parse_basic_calls_query() {
        let db = make_db();
        let engine = GraphQueryEngine::new(db);
        let results = engine
            .execute("MATCH (f)-[:CALLS]->(g) RETURN g LIMIT 5")
            .unwrap();
        assert!(results.is_empty()); // empty DB
    }

    #[test]
    fn parse_where_like() {
        let db = make_db();
        let engine = GraphQueryEngine::new(db);
        let results = engine
            .execute(
                "MATCH (f)-[:IMPORTS]->(g) WHERE f.file_path LIKE '%controller%' RETURN g LIMIT 10",
            )
            .unwrap();
        assert!(results.is_empty());
    }
}
