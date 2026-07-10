//! Context domain handlers: code-index context preparation.

use super::SharedCodeIndex;
use cc_model::{CcError, CcResult};

pub fn search_in_context(
    runtime: SharedCodeIndex,
    query: &str,
    top_k: usize,
    intent: Option<cc_model::Intent>,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let env = rt.search().search_in_context(query, top_k, intent)?;
    Ok(serde_json::to_value(env)?)
}

pub fn search_in_context_with(
    runtime: SharedCodeIndex,
    query: &str,
    top_k: usize,
    intent: Option<cc_model::Intent>,
    overrides: cc_model::search::SearchRequest,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let env = rt
        .search()
        .search_in_context_with(query, top_k, intent, overrides)?;
    Ok(serde_json::to_value(env)?)
}

pub fn prepare_edit_region(
    runtime: SharedCodeIndex,
    file_path: &str,
    start_line: u32,
    end_line: u32,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let symbols = rt.graph().file_symbols(file_path)?;
    let lower_line = start_line.saturating_sub(5);
    let upper_line = end_line.saturating_add(5);
    let region_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| s.end_line >= lower_line && s.start_line <= upper_line)
        .collect();
    let project = rt.project_path.as_ref().ok_or(CcError::ProjectNotSet)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let full_path = crate::path_guard::resolve_indexed_path_strict(project, file_path, db)?;
    let content = std::fs::read_to_string(&full_path)?;
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
    runtime: SharedCodeIndex,
    task: &str,
    max_symbols: Option<usize>,
    expand_depth: Option<usize>,
    intent: Option<&str>,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    rt.search()
        .task_symbols(task, max_symbols, expand_depth, intent)
}

pub fn explore_symbols(
    runtime: SharedCodeIndex,
    symbols: &[String],
    opts: &crate::engine_query::ExploreOptions,
) -> CcResult<serde_json::Value> {
    if symbols.is_empty() {
        return Err(CcError::InvalidParams(
            "missing or empty 'symbols' parameter".to_string(),
        ));
    }

    let rt = super::lock_index(&runtime)?;
    let max_chars = rt.repo_size_tier().max_output_chars();
    let result = rt.graph().explore_symbols(symbols, opts)?;
    // Mid-layer cap (not exit-only): this result is also embedded as
    // `symbol_details` inside handle_context / handle_node envelopes, where
    // it must already be bounded before assembly.
    Ok(super::output_budget::enforce_output_limit(
        result, max_chars,
    ))
}

pub fn get_symbol_source(
    runtime: SharedCodeIndex,
    symbol: &str,
    exact: bool,
    include_line_numbers: bool,
    max_chars: Option<usize>,
) -> CcResult<serde_json::Value> {
    if symbol.trim().is_empty() {
        return Err(CcError::InvalidParams(
            "missing or empty 'symbol' parameter".to_string(),
        ));
    }
    let rt = super::lock_index(&runtime)?;
    rt.graph()
        .get_symbol_source(symbol, exact, include_line_numbers, max_chars)
}

pub fn expand_code_region(
    runtime: SharedCodeIndex,
    file_path: &str,
    start_line: u32,
    end_line: u32,
    context_lines: u32,
) -> CcResult<serde_json::Value> {
    let rt = super::lock_index(&runtime)?;
    let project = rt.project_path.as_ref().ok_or(CcError::ProjectNotSet)?;
    let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
    let full_path = crate::path_guard::resolve_indexed_path_strict(project, file_path, db)?;
    let content = std::fs::read_to_string(&full_path)?;
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
