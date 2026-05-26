use serde::{Deserialize, Serialize};

/// A diagnostic (warning, error, etc.) associated with a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRecord {
    pub diagnostic_id: String,
    pub file_path: String,
    pub severity: String,
    pub message: String,
    pub line: u32,
    pub end_line: Option<u32>,
    pub source: String,
    pub code: Option<String>,
    pub confidence: f64,
    pub symbol_uid: Option<String>,
}

/// A literal value indexed from source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteralRecord {
    pub literal_id: String,
    pub file_path: String,
    pub literal: String,
    pub literal_kind: String,
    pub line: u32,
    pub container: Option<String>,
    pub confidence: f64,
    /// Symbol UID of the enclosing function/method.
    pub enclosing_symbol_uid: Option<String>,
    /// Dot-separated key path for config values (e.g., "database.host").
    pub key_path: Option<String>,
}
