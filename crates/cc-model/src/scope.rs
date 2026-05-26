use serde::{Deserialize, Serialize};

/// A lexical scope extracted from source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeInfo {
    pub scope_id: String,
    pub file_path: String,
    pub kind: String,
    pub name: Option<String>,
    pub parent_scope_id: Option<String>,
    pub owner_symbol_uid: Option<String>,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub bindings: Vec<ScopeBinding>,
}

/// A name binding within a scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeBinding {
    pub name: String,
    pub kind: String,
    pub symbol_uid: Option<String>,
}
