use crate::{symbol::SymbolKind, Language};
use serde::{Deserialize, Serialize};

/// A search result from the lexical retrieval engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub chunk_id: String,
    pub file_path: String,
    pub language: Language,
    pub start_line: u32,
    pub end_line: u32,
    pub breadcrumb: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<SymbolKind>,
    pub text: String,
    pub fused_score: f64,
    pub lexical_score: f64,
    pub grep_score: f64,
    pub graph_score: f64,
    /// Final ranking score.
    ///
    /// INVARIANT: `rerank_score` is assigned exactly once, inside cc-search
    /// (including the optional graph-rerank contribution), and the hit list
    /// is sorted on it there.  Once a `SearchHit` leaves `SearchEngine`,
    /// downstream consumers must treat this field as read-only — re-scoring
    /// or re-sorting outside cc-search silently breaks ranking guarantees.
    pub rerank_score: f64,
    pub reasons: Vec<String>,
    pub source: String,
    pub lane: Option<String>,
    pub metadata: serde_json::Value,
}

/// Parameters for a search request.
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    pub query: String,
    pub top_k: usize,
    pub path_prefix: Option<String>,
    pub languages: Option<Vec<Language>>,
    pub include_grep: bool,
    pub file_paths: Option<Vec<String>>,
    pub boost_file_paths: Option<Vec<String>>,
    /// Prior conversational queries that should bias lexical/semantic retrieval.
    pub conversation_queries: Option<Vec<String>>,
    pub recent_file_paths: Option<Vec<String>>,
    pub pinned_file_paths: Option<Vec<String>>,
    pub overlay_file_paths: Option<Vec<String>>,
    pub file_preselect_limit: Option<usize>,
}
