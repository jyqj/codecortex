use crate::{symbol::SymbolKind, Language, ParserTier};
use serde::{Deserialize, Serialize};

/// A code chunk — the unit of indexing and retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub chunk_id: String,
    pub file_path: String,
    pub language: Language,
    pub chunk_index: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub breadcrumb: String,
    pub text: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<SymbolKind>,
    /// f32 embedding vector (hash-based or API-based)
    pub embedding: Vec<f32>,
    pub token_estimate: u32,
    pub parser_tier: ParserTier,
    pub parser_confidence: f64,
}
