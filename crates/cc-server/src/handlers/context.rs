//! Context domain handlers: code-index context preparation.

use crate::engine::CodeIndex;
use std::sync::{Arc, Mutex};

pub fn search_in_context(
    runtime: Arc<Mutex<CodeIndex>>,
    query: &str,
    top_k: usize,
    intent: Option<cc_model::Intent>,
) -> Result<serde_json::Value, String> {
    let mut rt = runtime.lock().map_err(|e| e.to_string())?;
    let env = rt
        .search_in_context(query, top_k, intent)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(env).map_err(|e| e.to_string())
}

pub fn prepare_edit_region(
    runtime: Arc<Mutex<CodeIndex>>,
    file_path: &str,
    start_line: u32,
    end_line: u32,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let symbols = rt.file_symbols(file_path).map_err(|e| e.to_string())?;
    let lower_line = start_line.saturating_sub(5);
    let upper_line = end_line.saturating_add(5);
    let region_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| s.end_line >= lower_line && s.start_line <= upper_line)
        .collect();
    let project = rt.project_path.as_ref().ok_or("project not set")?;
    let db = rt.index_db().ok_or("no index database")?;
    let full_path = crate::path_guard::resolve_indexed_path_strict(project, file_path, db)?;
    let content = std::fs::read_to_string(&full_path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();
    let start = (start_line as usize).saturating_sub(1).min(lines.len());
    let end = (end_line as usize).min(lines.len());
    let region_content = if start < end {
        lines[start..end].join("\n")
    } else {
        String::new()
    };

    Ok(serde_json::json!({
        "file_path": file_path,
        "start_line": start_line,
        "end_line": end_line,
        "content": region_content,
        "symbols": serde_json::to_value(&region_symbols).unwrap_or_default(),
    }))
}

pub fn task_symbols(
    runtime: Arc<Mutex<CodeIndex>>,
    task: &str,
    max_symbols: Option<usize>,
    expand_depth: Option<usize>,
    intent: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut rt = runtime.lock().map_err(|e| e.to_string())?;
    rt.task_symbols(task, max_symbols, expand_depth, intent)
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn explore_symbols(
    runtime: Arc<Mutex<CodeIndex>>,
    symbols: &[String],
    max_callers: Option<usize>,
    max_callees: Option<usize>,
    include_source: bool,
    include_relations: bool,
    include_metrics: bool,
    outline: bool,
    max_source_per_file: Option<usize>,
) -> Result<serde_json::Value, String> {
    if symbols.is_empty() {
        return Err("missing or empty 'symbols' parameter".to_string());
    }

    let mut rt = runtime.lock().map_err(|e| e.to_string())?;
    let max_chars = rt.repo_size_tier().max_output_chars();
    let result = rt
        .explore_symbols(
            symbols,
            max_callers,
            max_callees,
            include_source,
            include_relations,
            include_metrics,
            outline,
            max_source_per_file,
        )
        .map_err(|e| e.to_string())?;
    Ok(super::facade::enforce_output_limit(result, max_chars))
}

pub fn get_symbol_source(
    runtime: Arc<Mutex<CodeIndex>>,
    symbol: &str,
    exact: bool,
    include_line_numbers: bool,
    max_chars: Option<usize>,
) -> Result<serde_json::Value, String> {
    if symbol.trim().is_empty() {
        return Err("missing or empty 'symbol' parameter".to_string());
    }
    let mut rt = runtime.lock().map_err(|e| e.to_string())?;
    rt.get_symbol_source(symbol, exact, include_line_numbers, max_chars)
        .map_err(|e| e.to_string())
}

pub fn expand_code_region(
    runtime: Arc<Mutex<CodeIndex>>,
    file_path: &str,
    start_line: u32,
    end_line: u32,
    context_lines: u32,
) -> Result<serde_json::Value, String> {
    let rt = runtime.lock().map_err(|e| e.to_string())?;
    let project = rt.project_path.as_ref().ok_or("project not set")?;
    let db = rt.index_db().ok_or("no index database")?;
    let full_path = crate::path_guard::resolve_indexed_path_strict(project, file_path, db)?;
    let content = std::fs::read_to_string(&full_path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let expanded_start = (start_line as usize)
        .saturating_sub(1)
        .saturating_sub(context_lines as usize)
        .min(total);
    let expanded_end = (end_line as usize)
        .saturating_add(context_lines as usize)
        .min(total);
    let region_content = if expanded_start < expanded_end {
        lines[expanded_start..expanded_end].join("\n")
    } else {
        String::new()
    };

    Ok(serde_json::json!({
        "file_path": file_path,
        "start_line": expanded_start + 1,
        "end_line": expanded_end,
        "total_lines": total,
        "content": region_content,
    }))
}
