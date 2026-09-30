use crate::{symbol::SymbolKind, Language, ParserTier};
use serde::{Deserialize, Serialize};

/// A code chunk — the unit of indexing and retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRecord {
    /// Exact indexed-input coordinates. None denotes synthetic or legacy evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::source::ChunkSource>,
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
    pub token_estimate: u32,
    pub parser_tier: ParserTier,
    pub parser_confidence: f64,
}
impl ChunkRecord {
    pub fn source_json(&self) -> crate::CcResult<Option<String>> {
        if self
            .source
            .as_ref()
            .is_some_and(|s| !s.validate(&self.text))
        {
            return Err(crate::CcError::InvalidParams(
                "invalid chunk source evidence".into(),
            ));
        }
        self.source
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(Into::into)
    }
}
