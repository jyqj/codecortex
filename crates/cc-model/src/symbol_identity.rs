//! Explicit original-source declaration association; never a display-name lookup.
use crate::{
    identity::DocumentRef,
    source::{ByteSpan, SourceIdentity},
    SymbolKind,
};
use serde::{Deserialize, Serialize};

pub const SYMBOL_IDENTITY_FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkSymbolIdentity {
    pub format_version: u32,
    pub chunk_id: String,
    pub file_path: String,
    pub document: DocumentRef,
    pub source: SourceIdentity,
    pub owner: ByteSpan,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub symbol_id: String,
    pub symbol_uid: String,
    pub name: String,
    pub kind: SymbolKind,
    pub qname: String,
}

impl ChunkSymbolIdentity {
    pub fn matches_symbol(&self, s: &crate::SymbolRecord) -> bool {
        self.symbol_id == s.symbol_id
            && self.file_path == s.file_path
            && s.symbol_uid.as_deref() == Some(&self.symbol_uid)
            && self.name == s.name
            && self.kind == s.kind
            && s.qname.as_deref() == Some(&self.qname)
            && (self.start_line, self.start_col, self.end_line, self.end_col)
                == (s.start_line, s.start_col, s.end_line, s.end_col)
    }
    pub fn valid_shape(&self) -> bool {
        self.format_version == SYMBOL_IDENTITY_FORMAT
            && !self.owner.is_empty()
            && self.owner.start < self.owner.end
            && self.owner.end <= self.source.byte_len
            && self.start_line > 0
            && self.start_line <= self.end_line
            && [&self.symbol_id, &self.symbol_uid, &self.name, &self.qname]
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 4096)
    }
}
