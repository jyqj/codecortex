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
//!
//! Limitations:
//!   - `=~` regex is approximated via SQL LIKE: only `.*`, `.+`, and `.` are converted.
//!     Complex regex patterns (character classes, alternation, anchors) are NOT supported
//!     and will produce incorrect results silently.
//!   - LIMIT defaults to 50 when omitted (standard Cypher returns all rows).
//!   - AND / OR follow standard precedence (AND binds tighter than OR).
//!   - OPTIONAL MATCH only applies to the first pattern (single-hop).
//!
//! Not supported: WITH, MERGE, CREATE, DELETE, SET

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use std::collections::HashMap;

const DEFAULT_VARLEN_MAX_HOPS: usize = 5;

// ── Token types ────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Match,
    Where,
    Return,
    OrderBy,
    Limit,
    And,
    Or,
    Not,
    As,
    Asc,
    Desc,
    Contains,
    StartsWith,
    EndsWith,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Collect,
    Distinct,
    Optional,
    // Symbols
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Dot,
    Comma,
    Arrow, // ->
    Dash,  // -
    Star,  // *
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    RegexMatch, // =~
    // Literals
    Ident(String),
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),
    // Special
    DotDot, // ..
    Eof,
}

// ── AST ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CypherQuery {
    pub match_clauses: Vec<MatchClause>,
    pub where_clause: Option<WhereClause>,
    pub return_clause: ReturnClause,
    pub order_by: Option<Vec<OrderItem>>,
    pub limit: Option<usize>,
}

impl CypherQuery {
    /// Backward-compatible accessor: returns the first (required) MATCH clause.
    pub fn match_clause(&self) -> &MatchClause {
        &self.match_clauses[0]
    }
}

#[derive(Debug, Clone)]
pub struct MatchClause {
    pub is_optional: bool,
    pub patterns: Vec<PathPattern>,
}

#[derive(Debug, Clone)]
pub struct PathPattern {
    pub nodes: Vec<NodePattern>,
    pub rels: Vec<RelPattern>,
}

#[derive(Debug, Clone)]
pub struct NodePattern {
    pub var: Option<String>,
    pub label: Option<String>,
    pub props: Vec<(String, String)>, // inline properties {name: 'value'}
}

#[derive(Debug, Clone)]
pub struct RelPattern {
    pub var: Option<String>,
    pub rel_type: Option<String>,
    pub direction: RelDirection,
    pub min_hops: usize,
    pub max_hops: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelDirection {
    Outgoing, // ->
    Incoming, // <-
    Both,     // --
}

#[derive(Debug, Clone)]
pub struct WhereClause {
    pub expr: Expr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Comparison {
        left: PropRef,
        op: CmpOp,
        right: Value,
    },
    Regex {
        left: PropRef,
        pattern: String,
    },
    Contains {
        left: PropRef,
        value: String,
    },
    StartsWith {
        left: PropRef,
        value: String,
    },
    EndsWith {
        left: PropRef,
        value: String,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    /// Degree filter: `degree(var) OP value`, `in_degree(var) OP value`, `out_degree(var) OP value`
    Degree {
        var: String,
        kind: DegreeKind,
        op: CmpOp,
        value: Value,
    },
}

/// Which degree direction to count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DegreeKind {
    /// in_degree + out_degree
    Total,
    /// Incoming edges only
    In,
    /// Outgoing edges only
    Out,
}

#[derive(Debug, Clone)]
pub struct PropRef {
    pub var: String,
    pub prop: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CmpOp {
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
}

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone)]
pub struct ReturnClause {
    pub distinct: bool,
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone)]
pub enum CollectExpr {
    Prop(PropRef),
    Var(String),
}

#[derive(Debug, Clone)]
pub enum CountArg {
    Star,          // COUNT(*)
    Var(String),   // COUNT(var)
    Prop(PropRef), // COUNT(var.prop)
}

#[derive(Debug, Clone)]
pub enum ReturnItem {
    Prop(PropRef, Option<String>),         // var.prop [AS alias]
    Count(CountArg, bool, Option<String>), // COUNT([DISTINCT] arg) [AS alias]
    /// Aggregate function: SUM/AVG/MIN/MAX on a property
    /// (func_name, PropRef, distinct, optional alias)
    Aggregate(String, PropRef, bool, Option<String>),
    Collect(CollectExpr, bool, Option<String>), // COLLECT([DISTINCT] var.prop|var) [AS alias]
    Var(String, Option<String>),                // var [AS alias] (returns all props)
}

#[derive(Debug, Clone)]
pub enum OrderExpr {
    Prop(PropRef),
    Alias(String),
}

#[derive(Debug, Clone)]
pub struct OrderItem {
    pub expr: OrderExpr,
    pub desc: bool,
}

// ── Query result ───────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct CypherResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
}

#[derive(Debug, Clone)]
struct SelectProjection {
    sql: String,
    source_key: String,
    output_name: String,
    item: ReturnItem,
}

#[derive(Debug, Clone)]
struct TranslatedQuery {
    sql: String,
    params: Vec<String>,
    projections: Vec<SelectProjection>,
}

// ── Lexer ──────────────────────────────────────────

pub fn tokenize(input: &str) -> CcResult<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        // Skip whitespace.
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }

        // Multi-char operators and arrows first.
        if pos + 1 < len {
            let two = &input[pos..pos + 2];
            match two {
                "->" => {
                    tokens.push(Token::Arrow);
                    pos += 2;
                    continue;
                }
                "<-" => {
                    // Disambiguate: this is a Dash then we handle direction in parser.
                    // Actually we treat <- as two tokens: Lt + Dash -- but for Cypher
                    // it's easier to handle in parser. Let's push as special handling.
                    // We'll let the parser deal with direction via Dash/Arrow/Lt tokens.
                    // Actually, let's just be pragmatic: push separate tokens.
                    tokens.push(Token::Lt);
                    tokens.push(Token::Dash);
                    pos += 2;
                    continue;
                }
                "=~" => {
                    tokens.push(Token::RegexMatch);
                    pos += 2;
                    continue;
                }
                "<>" => {
                    tokens.push(Token::Neq);
                    pos += 2;
                    continue;
                }
                "!=" => {
                    tokens.push(Token::Neq);
                    pos += 2;
                    continue;
                }
                "<=" => {
                    tokens.push(Token::Lte);
                    pos += 2;
                    continue;
                }
                ">=" => {
                    tokens.push(Token::Gte);
                    pos += 2;
                    continue;
                }
                ".." => {
                    tokens.push(Token::DotDot);
                    pos += 2;
                    continue;
                }
                _ => {}
            }
        }

        // Single-char symbols.
        match chars[pos] {
            '(' => {
                tokens.push(Token::LParen);
                pos += 1;
                continue;
            }
            ')' => {
                tokens.push(Token::RParen);
                pos += 1;
                continue;
            }
            '[' => {
                tokens.push(Token::LBracket);
                pos += 1;
                continue;
            }
            ']' => {
                tokens.push(Token::RBracket);
                pos += 1;
                continue;
            }
            '{' => {
                tokens.push(Token::LBrace);
                pos += 1;
                continue;
            }
            '}' => {
                tokens.push(Token::RBrace);
                pos += 1;
                continue;
            }
            ':' => {
                tokens.push(Token::Colon);
                pos += 1;
                continue;
            }
            '.' => {
                tokens.push(Token::Dot);
                pos += 1;
                continue;
            }
            ',' => {
                tokens.push(Token::Comma);
                pos += 1;
                continue;
            }
            '-' => {
                tokens.push(Token::Dash);
                pos += 1;
                continue;
            }
            '*' => {
                tokens.push(Token::Star);
                pos += 1;
                continue;
            }
            '=' => {
                tokens.push(Token::Eq);
                pos += 1;
                continue;
            }
            '<' => {
                tokens.push(Token::Lt);
                pos += 1;
                continue;
            }
            '>' => {
                tokens.push(Token::Gt);
                pos += 1;
                continue;
            }
            _ => {}
        }

        // String literals (single or double quoted).
        if chars[pos] == '\'' || chars[pos] == '"' {
            let quote = chars[pos];
            pos += 1;
            let mut s = String::new();
            while pos < len && chars[pos] != quote {
                if chars[pos] == '\\' && pos + 1 < len {
                    pos += 1; // skip backslash, take the next char literally
                    match chars[pos] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        other => s.push(other), // \', \", \\, etc.
                    }
                } else {
                    s.push(chars[pos]);
                }
                pos += 1;
            }
            if pos < len {
                pos += 1; // skip closing quote
            }
            tokens.push(Token::StringLit(s));
            continue;
        }

        // Numbers.
        if chars[pos].is_ascii_digit() {
            let start = pos;
            while pos < len && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos < len && chars[pos] == '.' && pos + 1 < len && chars[pos + 1].is_ascii_digit() {
                pos += 1;
                while pos < len && chars[pos].is_ascii_digit() {
                    pos += 1;
                }
                let s: String = chars[start..pos].iter().collect();
                let f = s
                    .parse::<f64>()
                    .map_err(|_| CcError::Search(format!("invalid float: {s}")))?;
                tokens.push(Token::FloatLit(f));
            } else {
                let s: String = chars[start..pos].iter().collect();
                let n = s
                    .parse::<i64>()
                    .map_err(|_| CcError::Search(format!("invalid integer: {s}")))?;
                tokens.push(Token::IntLit(n));
            }
            continue;
        }

        // Identifiers and keywords.
        if chars[pos].is_alphabetic() || chars[pos] == '_' {
            let start = pos;
            while pos < len && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            let upper = word.to_uppercase();

            // Check for two-word keywords.
            let token = match upper.as_str() {
                "MATCH" => Token::Match,
                "WHERE" => Token::Where,
                "RETURN" => Token::Return,
                "LIMIT" => Token::Limit,
                "AND" => Token::And,
                "OR" => Token::Or,
                "NOT" => Token::Not,
                "AS" => Token::As,
                "ASC" => Token::Asc,
                "DESC" => Token::Desc,
                "CONTAINS" => Token::Contains,
                "COUNT" => Token::Count,
                "SUM" => Token::Sum,
                "AVG" => Token::Avg,
                "MIN" => Token::Min,
                "MAX" => Token::Max,
                "COLLECT" => Token::Collect,
                "DISTINCT" => Token::Distinct,
                "OPTIONAL" => Token::Optional,
                "ORDER" => {
                    // Look ahead for BY.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let by_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[by_start..pos].iter().collect();
                    if next_word.to_uppercase() == "BY" {
                        Token::OrderBy
                    } else {
                        // Not ORDER BY, backtrack.
                        pos = saved;
                        Token::Ident(word)
                    }
                }
                "STARTS" => {
                    // Look ahead for WITH.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let w_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[w_start..pos].iter().collect();
                    if next_word.to_uppercase() == "WITH" {
                        Token::StartsWith
                    } else {
                        pos = saved;
                        Token::Ident(word)
                    }
                }
                "ENDS" => {
                    // Look ahead for WITH.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let w_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[w_start..pos].iter().collect();
                    if next_word.to_uppercase() == "WITH" {
                        Token::EndsWith
                    } else {
                        pos = saved;
                        Token::Ident(word)
                    }
                }
                "TRUE" | "FALSE" | "NULL" => Token::Ident(word),
                _ => Token::Ident(word),
            };
            tokens.push(token);
            continue;
        }

        // Skip unknown characters.
        pos += 1;
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

// ── Parser ─────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> CcResult<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Ok(tok)
        } else {
            Err(CcError::Search("unexpected end of query".into()))
        }
    }

    fn expect(&mut self, expected: &Token) -> CcResult<Token> {
        let tok = self.advance()?;
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(tok)
        } else {
            Err(CcError::Search(format!(
                "expected {:?}, got {:?}",
                expected, tok
            )))
        }
    }

    fn at(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(expected)
    }

    #[allow(dead_code)]
    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    // ── Top-level parse ────────────────────────────

    fn parse(mut self) -> CcResult<CypherQuery> {
        // First MATCH clause (required).  May be preceded by OPTIONAL.
        let mut match_clauses = Vec::new();

        let is_optional = if self.at(&Token::Optional) {
            self.advance()?;
            true
        } else {
            false
        };
        self.expect(&Token::Match)?;
        let mut mc = self.parse_match()?;
        mc.is_optional = is_optional;
        match_clauses.push(mc);

        // Additional MATCH / OPTIONAL MATCH clauses.
        loop {
            let opt = if self.at(&Token::Optional) {
                self.advance()?;
                if !self.at(&Token::Match) {
                    return Err(CcError::Search("expected MATCH after OPTIONAL".into()));
                }
                true
            } else {
                false
            };
            if self.at(&Token::Match) {
                self.advance()?;
                let mut mc = self.parse_match()?;
                mc.is_optional = opt;
                match_clauses.push(mc);
            } else {
                break;
            }
        }

        // Optional WHERE.
        let where_clause = if self.at(&Token::Where) {
            self.advance()?;
            Some(WhereClause {
                expr: self.parse_expr()?,
            })
        } else {
            None
        };

        // RETURN clause (required).
        self.expect(&Token::Return)?;
        let return_clause = self.parse_return()?;

        // Optional ORDER BY.
        let order_by = if self.at(&Token::OrderBy) {
            self.advance()?;
            Some(self.parse_order_by_items(&return_clause)?)
        } else {
            None
        };

        // Optional LIMIT.
        let limit = if self.at(&Token::Limit) {
            self.advance()?;
            match self.advance()? {
                Token::IntLit(n) => Some(n as usize),
                other => {
                    return Err(CcError::Search(format!(
                        "expected integer after LIMIT, got {:?}",
                        other
                    )))
                }
            }
        } else {
            None
        };

        Ok(CypherQuery {
            match_clauses,
            where_clause,
            return_clause,
            order_by,
            limit,
        })
    }

    /// Parse ORDER BY items, supporting both var.prop references and aliases
    /// defined in the RETURN clause.
    fn parse_order_by_items(&mut self, return_clause: &ReturnClause) -> CcResult<Vec<OrderItem>> {
        // Collect known aliases from RETURN items.
        let aliases: std::collections::HashSet<String> = return_clause
            .items
            .iter()
            .filter_map(|item| match item {
                ReturnItem::Prop(_, Some(a)) => Some(a.clone()),
                ReturnItem::Count(_, _, Some(a)) => Some(a.clone()),
                ReturnItem::Aggregate(_, _, _, Some(a)) => Some(a.clone()),
                ReturnItem::Collect(_, _, Some(a)) => Some(a.clone()),
                ReturnItem::Var(_, Some(a)) => Some(a.clone()),
                _ => None,
            })
            .collect();

        let mut items = Vec::new();
        loop {
            // Try to parse as prop_ref (var.prop).  If the next token is an
            // Ident and there is NO dot after it, treat it as an alias reference.
            let expr = if let Token::Ident(ref name) = self.peek().clone() {
                // Lookahead: is the token after the ident a Dot?
                let has_dot = self.tokens.get(self.pos + 1) == Some(&Token::Dot);
                if has_dot {
                    OrderExpr::Prop(self.parse_prop_ref()?)
                } else if aliases.contains(name) {
                    let alias = name.clone();
                    self.advance()?;
                    OrderExpr::Alias(alias)
                } else {
                    // Fall back to trying a prop_ref (will likely error if no dot).
                    OrderExpr::Prop(self.parse_prop_ref()?)
                }
            } else {
                OrderExpr::Prop(self.parse_prop_ref()?)
            };

            let desc = if self.at(&Token::Desc) {
                self.advance()?;
                true
            } else if self.at(&Token::Asc) {
                self.advance()?;
                false
            } else {
                false
            };
            items.push(OrderItem { expr, desc });
            if self.at(&Token::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }
        Ok(items)
    }

    // ── MATCH clause ───────────────────────────────

    fn parse_match(&mut self) -> CcResult<MatchClause> {
        let mut patterns = Vec::new();
        patterns.push(self.parse_path_pattern()?);

        // Support comma-separated patterns.
        while self.at(&Token::Comma) {
            self.advance()?;
            patterns.push(self.parse_path_pattern()?);
        }

        Ok(MatchClause {
            is_optional: false, // caller sets this after parsing
            patterns,
        })
    }

    fn parse_path_pattern(&mut self) -> CcResult<PathPattern> {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();

        // First node.
        nodes.push(self.parse_node_pattern()?);

        // Alternating: rel, node, rel, node...
        loop {
            if !self.at(&Token::Dash) && !self.at(&Token::Lt) {
                break;
            }
            let rel = self.parse_rel_pattern()?;
            rels.push(rel);
            nodes.push(self.parse_node_pattern()?);
        }

        Ok(PathPattern { nodes, rels })
    }

    fn parse_node_pattern(&mut self) -> CcResult<NodePattern> {
        self.expect(&Token::LParen)?;
        let mut np = NodePattern {
            var: None,
            label: None,
            props: Vec::new(),
        };

        // Optional variable name.
        if let Token::Ident(_) = self.peek() {
            if let Token::Ident(name) = self.advance()? {
                np.var = Some(name);
            }
        }

        // Optional :Label.
        if self.at(&Token::Colon) {
            self.advance()?;
            if let Token::Ident(label) = self.advance()? {
                np.label = Some(label);
            } else {
                return Err(CcError::Search("expected label after ':'".into()));
            }
        }

        // Optional inline properties {key: 'value', ...}.
        if self.at(&Token::LBrace) {
            self.advance()?;
            loop {
                if self.at(&Token::RBrace) {
                    break;
                }
                let key = match self.advance()? {
                    Token::Ident(k) => k,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected property name, got {:?}",
                            other
                        )))
                    }
                };
                self.expect(&Token::Colon)?;
                let val = match self.advance()? {
                    Token::StringLit(s) => s,
                    Token::Ident(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected property value, got {:?}",
                            other
                        )))
                    }
                };
                np.props.push((key, val));
                if self.at(&Token::Comma) {
                    self.advance()?;
                }
            }
            self.expect(&Token::RBrace)?;
        }

        self.expect(&Token::RParen)?;
        Ok(np)
    }

    fn parse_rel_pattern(&mut self) -> CcResult<RelPattern> {
        let mut rp = RelPattern {
            var: None,
            rel_type: None,
            direction: RelDirection::Outgoing,
            min_hops: 1,
            max_hops: 1,
        };

        // Determine direction prefix.
        // Patterns: -[...]-> (outgoing), <-[...]- (incoming), -[...]- (both)
        // Also: -> (shorthand outgoing), <- (shorthand incoming)

        let incoming_start = if self.at(&Token::Lt) {
            // Could be <-[...]-  or  <-[...]->
            self.advance()?; // consume <
            self.expect(&Token::Dash)?; // consume -
            true
        } else {
            // Must be - (outgoing or both)
            self.expect(&Token::Dash)?;
            false
        };

        // Check for bracket body: [...]
        if self.at(&Token::LBracket) {
            self.advance()?;

            // Optional variable name.
            if let Token::Ident(_) = self.peek() {
                if let Token::Ident(name) = self.advance()? {
                    rp.var = Some(name);
                }
            }

            // Optional :TYPE.
            if self.at(&Token::Colon) {
                self.advance()?;
                if let Token::Ident(t) = self.advance()? {
                    rp.rel_type = Some(t.to_uppercase());
                } else {
                    return Err(CcError::Search(
                        "expected relationship type after ':'".into(),
                    ));
                }
            }

            // Optional variable-length syntax:
            //   *        => 1..DEFAULT_VARLEN_MAX_HOPS
            //   *N       => N..N
            //   *min..max
            //   *..max   => 1..max
            //   *min..   => min..max(min, DEFAULT_VARLEN_MAX_HOPS)
            if self.at(&Token::Star) {
                self.advance()?;
                match self.peek() {
                    Token::RBracket => {
                        rp.min_hops = 1;
                        rp.max_hops = DEFAULT_VARLEN_MAX_HOPS;
                    }
                    Token::DotDot => {
                        self.advance()?;
                        let max = match self.advance()? {
                            Token::IntLit(n) => n as usize,
                            other => {
                                return Err(CcError::Search(format!(
                                    "expected integer after '*..', got {:?}",
                                    other
                                )))
                            }
                        };
                        rp.min_hops = 1;
                        rp.max_hops = max;
                    }
                    Token::IntLit(_) => {
                        let min = match self.advance()? {
                            Token::IntLit(n) => n as usize,
                            _ => unreachable!(),
                        };
                        if self.at(&Token::DotDot) {
                            self.advance()?;
                            let max = if self.at(&Token::RBracket) {
                                min.max(DEFAULT_VARLEN_MAX_HOPS)
                            } else {
                                match self.advance()? {
                                    Token::IntLit(n) => n as usize,
                                    other => {
                                        return Err(CcError::Search(format!(
                                            "expected integer after '..', got {:?}",
                                            other
                                        )))
                                    }
                                }
                            };
                            rp.min_hops = min;
                            rp.max_hops = max;
                        } else {
                            rp.min_hops = min;
                            rp.max_hops = min;
                        }
                    }
                    other => {
                        return Err(CcError::Search(format!(
                            "expected integer, '..', or ']' after '*', got {:?}",
                            other
                        )))
                    }
                }
                if rp.min_hops == 0 || rp.max_hops < rp.min_hops {
                    return Err(CcError::Search(format!(
                        "invalid variable-length range: *{}..{}",
                        rp.min_hops, rp.max_hops
                    )));
                }
            }

            self.expect(&Token::RBracket)?;
        }

        // Determine direction suffix.
        // After the bracket: - (no arrow) or -> (outgoing end)
        if self.at(&Token::Dash) {
            self.advance()?;
            if self.at(&Token::Gt) {
                self.advance()?;
                // Suffix is ->
                if incoming_start {
                    rp.direction = RelDirection::Both; // <-[...]->
                } else {
                    rp.direction = RelDirection::Outgoing; // -[...]->
                }
            } else {
                // Suffix is just -
                if incoming_start {
                    rp.direction = RelDirection::Incoming; // <-[...]-
                } else {
                    rp.direction = RelDirection::Both; // -[...]-
                }
            }
        } else if self.at(&Token::Arrow) {
            // Token::Arrow is ->
            self.advance()?;
            if incoming_start {
                rp.direction = RelDirection::Both;
            } else {
                rp.direction = RelDirection::Outgoing;
            }
        } else if self.at(&Token::Gt) {
            // After ] we might see > as separate token if -> was split
            self.advance()?;
            if incoming_start {
                rp.direction = RelDirection::Both;
            } else {
                rp.direction = RelDirection::Outgoing;
            }
        } else {
            // No trailing arrow means direction decided by prefix.
            if incoming_start {
                rp.direction = RelDirection::Incoming;
            } else {
                rp.direction = RelDirection::Both;
            }
        }

        Ok(rp)
    }

    // ── WHERE expression parsing ───────────────────

    /// Parse an expression with correct operator precedence: OR binds loosest, AND tighter.
    fn parse_expr(&mut self) -> CcResult<Expr> {
        let mut left = self.parse_and_expr()?;

        while self.at(&Token::Or) {
            self.advance()?;
            let right = self.parse_and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    /// Parse AND-level expressions (higher precedence than OR).
    fn parse_and_expr(&mut self) -> CcResult<Expr> {
        let mut left = self.parse_expr_atom()?;

        while self.at(&Token::And) {
            self.advance()?;
            let right = self.parse_expr_atom()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_expr_atom(&mut self) -> CcResult<Expr> {
        if self.at(&Token::Not) {
            self.advance()?;
            let inner = self.parse_expr_atom()?;
            return Ok(Expr::Not(Box::new(inner)));
        }

        if self.at(&Token::LParen) {
            self.advance()?;
            let inner = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            return Ok(inner);
        }

        // Check for degree function calls: degree(var), in_degree(var), out_degree(var)
        if let Token::Ident(ref name) = self.peek().clone() {
            let degree_kind = match name.to_lowercase().as_str() {
                "degree" => Some(DegreeKind::Total),
                "in_degree" => Some(DegreeKind::In),
                "out_degree" => Some(DegreeKind::Out),
                _ => None,
            };
            if let Some(kind) = degree_kind {
                // Lookahead: must be followed by '('
                if self.tokens.get(self.pos + 1) == Some(&Token::LParen) {
                    self.advance()?; // consume function name
                    self.expect(&Token::LParen)?;
                    let var = match self.advance()? {
                        Token::Ident(s) => s,
                        other => {
                            return Err(CcError::Search(format!(
                                "expected variable name in degree(), got {:?}",
                                other
                            )))
                        }
                    };
                    self.expect(&Token::RParen)?;
                    let op = self.parse_cmp_op()?;
                    let value = self.parse_value()?;
                    return Ok(Expr::Degree {
                        var,
                        kind,
                        op,
                        value,
                    });
                }
            }
        }

        // Must be a comparison: var.prop OP value
        let left = self.parse_prop_ref()?;

        match self.peek().clone() {
            Token::Eq => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Eq,
                    right,
                })
            }
            Token::Neq => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Neq,
                    right,
                })
            }
            Token::Lt => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Lt,
                    right,
                })
            }
            Token::Gt => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Gt,
                    right,
                })
            }
            Token::Lte => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Lte,
                    right,
                })
            }
            Token::Gte => {
                self.advance()?;
                let right = self.parse_value()?;
                Ok(Expr::Comparison {
                    left,
                    op: CmpOp::Gte,
                    right,
                })
            }
            Token::RegexMatch => {
                self.advance()?;
                let pattern = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after '=~', got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::Regex { left, pattern })
            }
            Token::Contains => {
                self.advance()?;
                let value = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after CONTAINS, got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::Contains { left, value })
            }
            Token::StartsWith => {
                self.advance()?;
                let value = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after STARTS WITH, got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::StartsWith { left, value })
            }
            Token::EndsWith => {
                self.advance()?;
                let value = match self.advance()? {
                    Token::StringLit(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected string after ENDS WITH, got {:?}",
                            other
                        )))
                    }
                };
                Ok(Expr::EndsWith { left, value })
            }
            other => Err(CcError::Search(format!(
                "expected operator after property reference, got {:?}",
                other
            ))),
        }
    }

    fn parse_prop_ref(&mut self) -> CcResult<PropRef> {
        let var = match self.advance()? {
            Token::Ident(s) => s,
            other => {
                return Err(CcError::Search(format!(
                    "expected identifier, got {:?}",
                    other
                )))
            }
        };
        self.expect(&Token::Dot)?;
        let prop = match self.advance()? {
            Token::Ident(s) => s,
            other => {
                return Err(CcError::Search(format!(
                    "expected property name, got {:?}",
                    other
                )))
            }
        };
        Ok(PropRef { var, prop })
    }

    fn parse_value(&mut self) -> CcResult<Value> {
        match self.advance()? {
            Token::StringLit(s) => Ok(Value::String(s)),
            Token::IntLit(n) => Ok(Value::Int(n)),
            Token::FloatLit(f) => Ok(Value::Float(f)),
            Token::Ident(s) if s.eq_ignore_ascii_case("true") => Ok(Value::Bool(true)),
            Token::Ident(s) if s.eq_ignore_ascii_case("false") => Ok(Value::Bool(false)),
            Token::Ident(s) if s.eq_ignore_ascii_case("null") => Ok(Value::Null),
            other => Err(CcError::Search(format!("expected value, got {:?}", other))),
        }
    }

    fn parse_cmp_op(&mut self) -> CcResult<CmpOp> {
        match self.advance()? {
            Token::Eq => Ok(CmpOp::Eq),
            Token::Neq => Ok(CmpOp::Neq),
            Token::Lt => Ok(CmpOp::Lt),
            Token::Gt => Ok(CmpOp::Gt),
            Token::Lte => Ok(CmpOp::Lte),
            Token::Gte => Ok(CmpOp::Gte),
            other => Err(CcError::Search(format!(
                "expected comparison operator, got {:?}",
                other
            ))),
        }
    }

    // ── RETURN clause ──────────────────────────────

    fn parse_return(&mut self) -> CcResult<ReturnClause> {
        // Check for RETURN DISTINCT ...
        let distinct = if self.at(&Token::Distinct) {
            self.advance()?;
            true
        } else {
            false
        };

        let mut items = Vec::new();

        loop {
            let item = self.parse_return_item()?;
            items.push(item);
            if self.at(&Token::Comma) {
                self.advance()?;
            } else {
                break;
            }
        }

        Ok(ReturnClause { distinct, items })
    }

    fn parse_return_item(&mut self) -> CcResult<ReturnItem> {
        // COUNT([DISTINCT] arg)
        if self.at(&Token::Count) {
            self.advance()?;
            self.expect(&Token::LParen)?;
            let distinct = if self.at(&Token::Distinct) {
                self.advance()?;
                true
            } else {
                false
            };
            // arg: * | var | var.prop
            let count_arg = match self.advance()? {
                Token::Star => CountArg::Star,
                Token::Ident(s) => {
                    if self.at(&Token::Dot) {
                        self.advance()?;
                        let prop = match self.advance()? {
                            Token::Ident(p) => p,
                            other => {
                                return Err(CcError::Search(format!(
                                    "expected property name in COUNT(var.prop), got {:?}",
                                    other
                                )))
                            }
                        };
                        CountArg::Prop(PropRef { var: s, prop })
                    } else {
                        CountArg::Var(s)
                    }
                }
                other => {
                    return Err(CcError::Search(format!(
                        "expected identifier or * in COUNT(), got {:?}",
                        other
                    )))
                }
            };
            self.expect(&Token::RParen)?;
            let alias = self.parse_optional_alias()?;
            return Ok(ReturnItem::Count(count_arg, distinct, alias));
        }

        // SUM/AVG/MIN/MAX([DISTINCT] var.prop)
        if self.at(&Token::Sum)
            || self.at(&Token::Avg)
            || self.at(&Token::Min)
            || self.at(&Token::Max)
        {
            let func_name = match self.peek() {
                Token::Sum => "SUM",
                Token::Avg => "AVG",
                Token::Min => "MIN",
                Token::Max => "MAX",
                _ => unreachable!(),
            };
            self.advance()?;
            self.expect(&Token::LParen)?;
            let distinct = if self.at(&Token::Distinct) {
                self.advance()?;
                true
            } else {
                false
            };
            let prop_ref = self.parse_prop_ref()?;
            self.expect(&Token::RParen)?;
            let alias = self.parse_optional_alias()?;
            return Ok(ReturnItem::Aggregate(
                func_name.to_string(),
                prop_ref,
                distinct,
                alias,
            ));
        }

        // COLLECT([DISTINCT] var.prop) or COLLECT([DISTINCT] var)
        if self.at(&Token::Collect) {
            self.advance()?;
            self.expect(&Token::LParen)?;
            let distinct = if self.at(&Token::Distinct) {
                self.advance()?;
                true
            } else {
                false
            };
            let ident = match self.advance()? {
                Token::Ident(s) => s,
                other => {
                    return Err(CcError::Search(format!(
                        "expected identifier in COLLECT(), got {:?}",
                        other
                    )))
                }
            };
            let expr = if self.at(&Token::Dot) {
                self.advance()?;
                let prop = match self.advance()? {
                    Token::Ident(s) => s,
                    other => {
                        return Err(CcError::Search(format!(
                            "expected property name in COLLECT(var.prop), got {:?}",
                            other
                        )))
                    }
                };
                CollectExpr::Prop(PropRef { var: ident, prop })
            } else {
                CollectExpr::Var(ident)
            };
            self.expect(&Token::RParen)?;
            let alias = self.parse_optional_alias()?;
            return Ok(ReturnItem::Collect(expr, distinct, alias));
        }

        // var.prop or bare var
        let ident = match self.advance()? {
            Token::Ident(s) => s,
            other => {
                return Err(CcError::Search(format!(
                    "expected identifier in RETURN, got {:?}",
                    other
                )))
            }
        };

        if self.at(&Token::Dot) {
            self.advance()?;
            let prop = match self.advance()? {
                Token::Ident(s) => s,
                other => {
                    return Err(CcError::Search(format!(
                        "expected property name after '.', got {:?}",
                        other
                    )))
                }
            };
            let alias = self.parse_optional_alias()?;
            Ok(ReturnItem::Prop(PropRef { var: ident, prop }, alias))
        } else {
            let alias = self.parse_optional_alias()?;
            Ok(ReturnItem::Var(ident, alias))
        }
    }

    fn parse_optional_alias(&mut self) -> CcResult<Option<String>> {
        if self.at(&Token::As) {
            self.advance()?;
            match self.advance()? {
                Token::Ident(s) => Ok(Some(s)),
                other => Err(CcError::Search(format!(
                    "expected alias name after AS, got {:?}",
                    other
                ))),
            }
        } else {
            Ok(None)
        }
    }
}

pub fn parse(tokens: &[Token]) -> CcResult<CypherQuery> {
    let parser = Parser::new(tokens.to_vec());
    parser.parse()
}

// ── Executor ───────────────────────────────────────

/// Edge-type to table mapping.
struct EdgeTableInfo {
    table: &'static str,
    src_col: &'static str,
    dst_col: &'static str,
    /// Whether src/dst are symbol_uid (true) or file_path (false).
    src_is_symbol: bool,
    dst_is_symbol: bool,
    /// Override the default join column on the node table side.
    /// When `None`, defaults to `symbol_uid` (if `*_is_symbol`) or `file_path`.
    src_join_key: Option<&'static str>,
    dst_join_key: Option<&'static str>,
    /// Optional SQL filter appended to WHERE clause (e.g. "call_kind = 'http'").
    /// The expression is injected verbatim with the edge alias prefix `{edge_alias}.`.
    extra_filter: Option<&'static str>,
    /// Override the full JOIN ON expression for the source/destination side.
    /// Placeholders `{src}`, `{dst}`, `{e}` are replaced with actual aliases.
    /// Empty string means "skip this JOIN entirely" (for pseudo-UID endpoints
    /// that have no corresponding node table, e.g. `dir::` prefixed UIDs).
    src_join_on: Option<&'static str>,
    dst_join_on: Option<&'static str>,
}

fn edge_table_map() -> HashMap<&'static str, EdgeTableInfo> {
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
fn validate_sql_ident(ident: &str) -> CcResult<()> {
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
fn label_table(label: &str) -> &'static str {
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
fn label_kind_filter(label: &str) -> Option<&'static str> {
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
fn expr_to_sql(expr: &Expr, params: &mut Vec<String>) -> String {
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

fn translate_single_node(query: &CypherQuery, pattern: &PathPattern) -> CcResult<TranslatedQuery> {
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

fn translate_single_hop(
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

fn translate_variable_length(
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

// ── Public API ─────────────────────────────────────

/// Validate all identifiers in a parsed query to ensure they are safe for SQL interpolation.
/// This is a defense-in-depth check: the lexer already constrains identifiers, but we
/// verify here in case identifiers are constructed outside the lexer.
fn validate_query_identifiers(query: &CypherQuery) -> CcResult<()> {
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

/// Parse and execute a Cypher query against the index database.
pub fn cypher_query(input: &str, db: &IndexDb) -> CcResult<CypherResult> {
    let tokens = tokenize(input)?;
    let query = parse(&tokens)?;
    validate_query_identifiers(&query)?;
    execute(&query, db)
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

        assert!(sql.contains("SELECT f.name FROM symbols AS f"));
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
        let sql = expr_to_sql(&expr, &mut params);
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
            sql.contains("route_nodes AS route"),
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
}
