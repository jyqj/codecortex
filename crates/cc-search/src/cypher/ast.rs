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
    Union,    // UNION keyword
    UnionAll, // UNION ALL (two-word keyword)
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

/// A compound query formed by one or more sub-queries joined with UNION / UNION ALL.
#[derive(Debug, Clone)]
pub struct CypherUnionQuery {
    pub queries: Vec<CypherQuery>,
    /// For each boundary between consecutive queries: `true` = UNION ALL, `false` = UNION (dedup).
    /// Length = queries.len() - 1.
    pub union_all: Vec<bool>,
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
    /// True when the query had no explicit LIMIT and the default LIMIT was applied.
    pub default_limit_applied: bool,
    /// The LIMIT value that actually took effect (explicit or default), if any.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct SelectProjection {
    pub(crate) sql: String,
    pub(crate) source_key: String,
    pub(crate) output_name: String,
    pub(crate) item: ReturnItem,
}

#[derive(Debug, Clone)]
pub(crate) struct TranslatedQuery {
    pub(crate) sql: String,
    pub(crate) params: Vec<String>,
    pub(crate) projections: Vec<SelectProjection>,
}
