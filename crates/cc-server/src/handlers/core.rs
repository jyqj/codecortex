//! Core domain handlers: project setup, indexing, search, symbol queries, impact.

use crate::engine::CodeIndex;
use std::sync::{Arc, Mutex};

pub fn build_index(
    runtime: Arc<Mutex<CodeIndex>>,
    full: bool,
) -> Result<serde_json::Value, String> {
    let mut rt = runtime.lock().map_err(|e| e.to_string())?;
    let report = rt.build_index(full).map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

pub fn index_status(runtime: Arc<Mutex<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let stats = rt.index_status().map_err(|e| e.to_string())?;
    serde_json::to_value(stats).map_err(|e| e.to_string())
}

pub fn find_symbol(
    runtime: Arc<Mutex<CodeIndex>>,
    name: &str,
    exact: bool,
    top_k: usize,
    include_metrics: bool,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    rt.find_symbol(name, exact, top_k, include_metrics)
        .map_err(|e| e.to_string())
}

pub fn list_files(runtime: Arc<Mutex<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let files = rt.list_indexed_files().map_err(|e| e.to_string())?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

pub fn list_communities(runtime: Arc<Mutex<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let rows = rt.list_communities().map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn list_frameworks(runtime: Arc<Mutex<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let rows = rt.list_frameworks().map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn index_capabilities(runtime: Arc<Mutex<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let status = rt.index_status();
    let has_index = status.is_ok();
    let stats = status.ok();
    Ok(serde_json::json!({
        "has_index": has_index,
        "has_project": rt.project_path.is_some(),
        "indexed_files": stats.as_ref().map(|s| s.indexed_files).unwrap_or(0),
        "indexed_symbols": stats.as_ref().map(|s| s.indexed_symbols).unwrap_or(0),
        "capabilities": {
            "search": has_index,
            "graph": has_index,
            "impact": has_index
        }
    }))
}

pub fn callers(
    runtime: Arc<Mutex<CodeIndex>>,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let rows = rt.callers(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn callees(
    runtime: Arc<Mutex<CodeIndex>>,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let rows = rt.callees(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn analyze_impact(
    runtime: Arc<Mutex<CodeIndex>>,
    files: &[String],
    base_branch: Option<&str>,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let report = if files.is_empty() {
        rt.analyze_impact(base_branch)
    } else {
        rt.detect_impact(files)
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Get a summary of a single file.
pub fn summarize_file(
    runtime: Arc<Mutex<CodeIndex>>,
    file_path: &str,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    rt.summarize_file(file_path).map_err(|e| e.to_string())
}

/// Show available node kinds, edge types, and their counts in the index.
pub fn graph_schema(runtime: Arc<Mutex<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    rt.graph_schema().map_err(|e| e.to_string())
}

