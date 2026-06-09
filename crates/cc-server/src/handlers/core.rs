//! Core domain handlers: project setup, indexing, search, symbol queries, impact.

use super::SharedCodeIndex;
use crate::engine::CodeIndex;

pub fn build_index(runtime: SharedCodeIndex, full: bool) -> Result<serde_json::Value, String> {
    // Brief read lock: clone the owned build inputs, then release.
    let inputs = {
        let rt = super::lock_index(&runtime)?;
        rt.build_inputs().map_err(|e| e.to_string())?
    };
    // Heavy prepare phase runs with NO lock held — read queries are not blocked.
    let prepared = CodeIndex::prepare_build(&inputs, full, None).map_err(|e| e.to_string())?;
    // Brief write lock: commit (write + postprocess + bookkeeping).
    let mut rt = super::lock_index_write(&runtime)?;
    let report = rt
        .commit_build(&inputs, full, None, prepared)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

pub fn index_status(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let stats = rt.index_status().map_err(|e| e.to_string())?;
    serde_json::to_value(stats).map_err(|e| e.to_string())
}

pub fn find_symbol(
    runtime: SharedCodeIndex,
    name: &str,
    exact: bool,
    top_k: usize,
    include_metrics: bool,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.find_symbol(name, exact, top_k, include_metrics)
        .map_err(|e| e.to_string())
}

pub fn list_files(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let files = rt.list_indexed_files().map_err(|e| e.to_string())?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

pub fn list_communities(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.list_communities().map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn list_frameworks(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
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

pub fn index_capabilities(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
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
    runtime: SharedCodeIndex,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.callers(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn callees(
    runtime: SharedCodeIndex,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.callees(symbol, limit).map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn analyze_impact(
    runtime: SharedCodeIndex,
    files: &[String],
    base_branch: Option<&str>,
    confidence_threshold: Option<f32>,
    result_limit: Option<usize>,
    max_nodes: Option<usize>,
    max_per_layer: Option<usize>,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let report = if files.is_empty() {
        rt.analyze_impact_capped(
            base_branch,
            confidence_threshold,
            result_limit,
            max_nodes,
            max_per_layer,
        )
    } else {
        rt.detect_impact_capped(
            files,
            confidence_threshold,
            result_limit,
            max_nodes,
            max_per_layer,
        )
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

pub fn git_changed_files(
    runtime: SharedCodeIndex,
    base_branch: Option<&str>,
) -> Result<Vec<String>, String> {
    let rt = super::lock_index(&runtime)?;
    rt.git_changed_files(base_branch).map_err(|e| e.to_string())
}

/// Get a summary of a single file.
pub fn summarize_file(
    runtime: SharedCodeIndex,
    file_path: &str,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.summarize_file(file_path).map_err(|e| e.to_string())
}

/// Show available node kinds, edge types, and their counts in the index.
pub fn graph_schema(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.graph_schema().map_err(|e| e.to_string())
}
