//! Core domain handlers: project setup, indexing, search, symbol queries, impact.

use crate::engine::CodeIndex;
use std::sync::{Arc, RwLock};

pub fn build_index(
    runtime: Arc<RwLock<CodeIndex>>,
    full: bool,
) -> Result<serde_json::Value, String> {
    let mut rt = super::lock_index_write(&runtime)?;
    let report = rt.build_index(full).map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

pub fn index_status(runtime: Arc<RwLock<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let stats = rt.index_status().map_err(|e| e.to_string())?;
    serde_json::to_value(stats).map_err(|e| e.to_string())
}

pub fn find_symbol(
    runtime: Arc<RwLock<CodeIndex>>,
    name: &str,
    exact: bool,
    top_k: usize,
    include_metrics: bool,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.find_symbol(name, exact, top_k, include_metrics)
        .map_err(|e| e.to_string())
}

pub fn list_files(runtime: Arc<RwLock<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let files = rt.list_indexed_files().map_err(|e| e.to_string())?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

pub fn list_communities(runtime: Arc<RwLock<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.list_communities().map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn list_frameworks(runtime: Arc<RwLock<CodeIndex>>) -> Result<serde_json::Value, String> {
    use cc_index::framework_resolvers::resolver_tier_for_key;

    let rt = super::lock_index(&runtime)?;
    let rows = rt.list_frameworks().map_err(|e| e.to_string())?;

    // Enrich each framework entry with its resolver coverage tier.
    let enriched: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(key, confidence)| {
            let tier = resolver_tier_for_key(&key);
            serde_json::json!({
                "framework": key,
                "confidence": confidence,
                "resolver_tier": tier
            })
        })
        .collect();

    serde_json::to_value(enriched).map_err(|e| e.to_string())
}

pub fn index_capabilities(runtime: Arc<RwLock<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
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
    runtime: Arc<RwLock<CodeIndex>>,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.callers(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn callees(
    runtime: Arc<RwLock<CodeIndex>>,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.callees(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn analyze_impact(
    runtime: Arc<RwLock<CodeIndex>>,
    files: &[String],
    base_branch: Option<&str>,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
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
    runtime: Arc<RwLock<CodeIndex>>,
    file_path: &str,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.summarize_file(file_path).map_err(|e| e.to_string())
}

/// Show available node kinds, edge types, and their counts in the index.
pub fn graph_schema(runtime: Arc<RwLock<CodeIndex>>) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.graph_schema().map_err(|e| e.to_string())
}
