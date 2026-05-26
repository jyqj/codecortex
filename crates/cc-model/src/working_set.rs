use serde::{Deserialize, Serialize};

/// An entry in the working set (active files the user is working with).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSetEntry {
    pub file_path: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub source: String,
    pub freshness_seconds: Option<f64>,
}

/// A text selection in the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSelection {
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// The current state of the user's workspace/editor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub current_file: Option<String>,
    pub open_files: Vec<String>,
    pub edited_files: Vec<String>,
    pub selection: Option<WorkspaceSelection>,
    pub active_task: Option<String>,
    pub updated_at: Option<String>,
}
