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
    /// Additive score bill explaining `rerank_score`: each entry is one
    /// `(component, amount)` contribution, and the components sum to
    /// `rerank_score` exactly (insertion order is the addition order).
    ///
    /// Component naming mirrors the reason-token vocabulary:
    /// `rrf:<lane>` for per-lane RRF fusion contributions, `overlap` for
    /// the token-overlap term, and `boost:<name>` for every rerank boost
    /// (including the post-construction `boost:dsl-name` and
    /// `boost:graph-rerank` contributions).
    ///
    /// Serde-optional for backward compatibility: absent on the wire when
    /// empty, defaults to empty when deserializing older payloads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub score_trace: Vec<(String, f64)>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Older payloads carry no `score_trace`; deserialization must default
    /// it to empty, and empty traces must stay off the wire.
    #[test]
    fn search_hit_score_trace_is_serde_backward_compatible() {
        let legacy_json = serde_json::json!({
            "chunk_id": "c1",
            "file_path": "src/a.rs",
            "language": "rust",
            "start_line": 1,
            "end_line": 2,
            "breadcrumb": "root",
            "symbol_name": null,
            "symbol_kind": null,
            "text": "fn a() {}",
            "fused_score": 0.5,
            "lexical_score": 1.0,
            "grep_score": 0.0,
            "graph_score": 0.0,
            "rerank_score": 0.7,
            "reasons": ["lexical@1"],
            "source": "index",
            "lane": null,
            "metadata": null,
        });
        let hit: SearchHit = serde_json::from_value(legacy_json).unwrap();
        assert!(hit.score_trace.is_empty());

        let serialized = serde_json::to_value(&hit).unwrap();
        assert!(
            serialized.get("score_trace").is_none(),
            "empty score_trace must be skipped during serialization"
        );

        let mut traced = hit;
        traced.score_trace = vec![("rrf:lexical".to_string(), 0.7)];
        let serialized = serde_json::to_value(&traced).unwrap();
        assert_eq!(
            serialized["score_trace"],
            serde_json::json!([["rrf:lexical", 0.7]])
        );
        let roundtrip: SearchHit = serde_json::from_value(serialized).unwrap();
        assert_eq!(roundtrip.score_trace, traced.score_trace);
    }
}
