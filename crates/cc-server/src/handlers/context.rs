//! Context domain handlers: code-index context preparation.

use super::SharedCodeIndex;
use cc_model::{CcError, CcResult};

pub fn search_in_context(
    runtime: SharedCodeIndex,
    query: &str,
    top_k: usize,
    intent: Option<cc_model::Intent>,
) -> CcResult<serde_json::Value> {
    let handle = crate::query_handle::QueryHandle::capture(&runtime)?;
    let env = handle.search_in_context(query, top_k, intent)?;
    Ok(serde_json::to_value(env)?)
}

pub fn search_in_context_with(
    runtime: SharedCodeIndex,
    query: &str,
    top_k: usize,
    intent: Option<cc_model::Intent>,
    overrides: cc_model::search::SearchRequest,
) -> CcResult<serde_json::Value> {
    let handle = crate::query_handle::QueryHandle::capture(&runtime)?;
    let env = handle.search_in_context_with(query, top_k, intent, overrides)?;
    Ok(serde_json::to_value(env)?)
}

pub async fn search_async(
    runtime: SharedCodeIndex,
    query: String,
    top_k: usize,
    intent: Option<cc_model::Intent>,
    mut overrides: cc_model::search::SearchRequest,
) -> CcResult<serde_json::Value> {
    let (handle, control) =
        crate::query_handle::capture_for_request(runtime, overrides.control.take()).await?;
    let mut cancel = control.cancel_on_drop();
    let before = handle.resolution_freshness(control.clone()).await?;
    overrides.control = Some(control.clone());
    let max_bytes = handle.tier.max_output_chars();
    let envelope = handle.search_async(query, top_k, intent, overrides).await?;
    let db = handle.db.clone();
    let value = handle
        .services
        .pool
        .run_cpu(control.clone(), move || {
            let value = serde_json::to_value(envelope)?;
            let value = super::freshness::attach_observed(
                Some(before),
                db.reads().resolution_freshness()?,
                value,
            )?;
            let value = cc_search::selection::budget::pack_value(value, max_bytes)?;
            cc_search::evidence_hydrator::validate_envelope_generation(&db, &value)?;
            Ok(value)
        })
        .await?;
    control.check()?;
    handle.engine.check_live()?;
    cancel.disarm();
    Ok(value)
}

pub async fn symbol_search_async(
    runtime: SharedCodeIndex,
    query: String,
    exact: bool,
    top_k: usize,
    path_prefix: Option<String>,
) -> CcResult<serde_json::Value> {
    let (handle, control) = crate::query_handle::capture_for_request(runtime.clone(), None).await?;
    let expected = handle.db_instance_id();
    handle
        .services
        .pool
        .run_cpu(control, move || {
            let rt = super::lock_index(&runtime)?;
            let db = rt.index_db().ok_or(CcError::IndexUnavailable)?;
            if db.admin().instance_id() != expected {
                return Err(CcError::QueryInvalidated);
            }
            let before = db.reads().resolution_freshness()?;
            let value =
                rt.graph()
                    .find_symbol_in_scope(&query, exact, top_k, path_prefix.as_deref())?;
            super::freshness::attach_observed(
                Some(before),
                db.reads().resolution_freshness()?,
                value,
            )
        })
        .await
}

pub async fn context_async(
    runtime: SharedCodeIndex,
    task: String,
    max_symbols: Option<usize>,
    include_source: bool,
    intent: Option<String>,
) -> CcResult<serde_json::Value> {
    context_async_with_strategy(runtime, task, max_symbols, include_source, intent, None).await
}

pub async fn context_async_with_strategy(
    runtime: SharedCodeIndex,
    task: String,
    max_symbols: Option<usize>,
    include_source: bool,
    intent: Option<String>,
    retrieval_strategy: Option<cc_model::query::RetrievalStrategy>,
) -> CcResult<serde_json::Value> {
    let (handle, control) = crate::query_handle::capture_for_request(runtime.clone(), None).await?;
    let mut cancel = control.cancel_on_drop();
    let owned = handle.clone();
    let before = handle.resolution_freshness(control.clone()).await?;
    let use_direct = retrieval_strategy.is_none()
        && handle.config().strategy == cc_model::query::RetrievalStrategy::Local;
    let first_task = task.clone();
    let first_intent = intent.clone();
    let direct = handle
        .services
        .pool
        .run_cpu(control.clone(), move || {
            if !use_direct {
                return Ok(None);
            }
            owned.task_symbols_direct(&first_task, max_symbols, Some(1), first_intent.as_deref())
        })
        .await?;
    let result = match direct {
        Some(value) => value,
        None => {
            let envelope = handle
                .search_async(
                    task,
                    10,
                    intent.as_deref().and_then(|s| s.parse().ok()),
                    cc_model::search::SearchRequest {
                        control: Some(control.clone()),
                        retrieval_strategy,
                        ..Default::default()
                    },
                )
                .await?;
            handle
                .services
                .pool
                .run_cpu(control.clone(), move || Ok(serde_json::to_value(envelope)?))
                .await?
        }
    };
    let max_chars = handle.tier.max_output_chars();
    let db = handle.db.clone();
    let result = handle
        .services
        .pool
        .run_cpu(control.clone(), move || {
            let result = super::facade::finish_context_with(result, include_source, |names| {
                let rt = super::lock_index(&runtime)?;
                if rt
                    .index_db()
                    .is_none_or(|current| current.admin().instance_id() != db.admin().instance_id())
                {
                    return Err(CcError::QueryInvalidated);
                }
                let details = rt
                    .graph()
                    .explore_symbols(names, &super::facade::context_source_options())?;
                Ok(super::output_budget::enforce_output_limit(
                    details, max_chars,
                ))
            })?;
            let result = super::freshness::attach_observed(
                Some(before),
                db.reads().resolution_freshness()?,
                result,
            )?;
            let result = if result
                .pointer("/machine_pack/kind")
                .and_then(serde_json::Value::as_str)
                == Some("code_index_context")
            {
                cc_search::selection::budget::pack_value(result, max_chars)?
            } else {
                super::output_budget::enforce_output_limit(result, max_chars)
            };
            cc_search::evidence_hydrator::validate_envelope_generation(&db, &result)?;
            Ok(result)
        })
        .await?;
    control.check()?;
    handle.engine.check_live()?;
    cancel.disarm();
    Ok(result)
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
    let evidence = cc_search::evidence::read_verified(
        db,
        project,
        file_path,
        cc_search::evidence::FILE_LIMIT,
    )?;
    if !evidence.is_current() {
        return Ok(
            serde_json::json!({"file_path":file_path,"content":null,"source_freshness":evidence,"retry":"reindex"}),
        );
    }
    let source_freshness = serde_json::to_value(&evidence)?;
    let content = evidence.require_text()?;
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
        "source_freshness": source_freshness,
        "source_rendering": "line-normalized",
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
    let handle = crate::query_handle::QueryHandle::capture(&runtime)?;
    handle.task_symbols(task, max_symbols, expand_depth, intent)
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
    let evidence = cc_search::evidence::read_verified(
        db,
        project,
        file_path,
        cc_search::evidence::FILE_LIMIT,
    )?;
    if !evidence.is_current() {
        return Ok(
            serde_json::json!({"file_path":file_path,"content":null,"source_freshness":evidence,"retry":"reindex"}),
        );
    }
    let source_freshness = serde_json::to_value(&evidence)?;
    let content = evidence.require_text()?;
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
        "source_freshness": source_freshness,
        "source_rendering": "line-normalized",
    }))
}
