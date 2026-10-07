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

/// Single exit for async search responses after the query's own optimistic
/// fence accepted the envelope: attach the dispatch-seam freshness projection
/// (with accepted-generation refinement) and apply the output budget.
///
/// There is deliberately no fence-external generation hard-check here. After
/// fence acceptance the envelope is single-generation self-consistent; a
/// generation change inside the serialization window is a freshness fact the
/// projection above annotates explicitly (`changed_during_query`, or the
/// accepted-generation observation when the fence recovered onto the new
/// generation) — it is not a single-attempt retryable error. The retained
/// contract (`cc_model` error.rs: never return or cache mixed-generation
/// results) is untouched.
pub(crate) fn finalize_search_response(
    db: &cc_db::index_db::IndexDb,
    before: Option<cc_model::freshness::ResolutionFreshness>,
    value: serde_json::Value,
    max_bytes: usize,
) -> CcResult<serde_json::Value> {
    let accepted = super::freshness::accepted_generation_of(&value);
    let value = super::freshness::attach_observed(
        before,
        db.reads().resolution_freshness()?,
        accepted,
        value,
    )?;
    cc_search::selection::budget::pack_value(value, max_bytes)
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
            finalize_search_response(&db, Some(before), value, max_bytes)
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
            let accepted = super::freshness::accepted_generation_of(&value);
            super::freshness::attach_observed(
                Some(before),
                db.reads().resolution_freshness()?,
                accepted,
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
            let accepted = super::freshness::accepted_generation_of(&result);
            let result = super::freshness::attach_observed(
                Some(before),
                db.reads().resolution_freshness()?,
                accepted,
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
            // Same P0 contract as finalize_search_response: no fence-external
            // single-shot generation hard-check after acceptance — the
            // freshness projection above owns the annotation.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, RwLock};

    /// Build an indexed project and return its shared handle plus db.
    fn indexed_db() -> (
        tempfile::TempDir,
        Arc<RwLock<crate::engine::CodeIndex>>,
        Arc<cc_db::index_db::IndexDb>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "pub fn a() {}\n").unwrap();
        let index = Arc::new(RwLock::new(
            crate::engine::CodeIndex::new(Some(dir.path())).unwrap(),
        ));
        index.write().unwrap().build_index(true).unwrap();
        let db = index.read().unwrap().index_db().unwrap().clone();
        (dir, index, db)
    }

    /// Minimal envelope carrying the fence-accepted generation exactly where
    /// the hydrator embeds it (`evidence_summary/source_freshness/generation`),
    /// shaped to pass `selection::budget::pack_value`.
    fn envelope_with_accepted_generation(
        generation: cc_model::generation::ReadGeneration,
    ) -> serde_json::Value {
        serde_json::json!({
            "token_budget": 4096,
            "nodes": [],
            "spans": [],
            "machine_pack": {"kind": "code_index_context", "hits": []},
            "evidence_summary": {
                "source_freshness": {
                    "generation": serde_json::to_value(generation).unwrap(),
                },
            },
        })
    }

    // P0: a full rebuild committing inside the serialization window (after the
    // query's fence accepted the envelope) is a freshness fact to annotate,
    // not a fence-external single-shot hard error. The response must succeed
    // and honestly report changed_during_query with complete=false.
    #[test]
    fn serialized_window_generation_change_is_annotated_not_a_hard_error() {
        let (_dir, index, db) = indexed_db();
        let accepted = db.reads().read_generation().unwrap();
        let before = cc_model::freshness::ResolutionFreshness::ready(accepted.index_epoch);
        let value = envelope_with_accepted_generation(accepted);
        // The commit lands between fence acceptance and response finalization.
        std::fs::write(_dir.path().join("b.rs"), "pub fn b() {}\n").unwrap();
        index.write().unwrap().build_index(false).unwrap();
        let value = finalize_search_response(&db, Some(before), value, 1_000_000).unwrap();
        assert_eq!(value["resolution_freshness"]["complete"], false);
        assert_eq!(
            value["resolution_freshness"]["status"],
            "changed_during_query"
        );
        assert_eq!(
            value["resolution_freshness"]["reason"],
            "index_generation_changed_during_query"
        );
    }

    // P2: when the outer fence recovered onto the post-change generation, the
    // result is fresh — it must not be flagged incomplete, and the observed
    // dispatch-window change must still be recorded explicitly.
    #[test]
    fn fence_recovered_result_is_complete_with_observed_change() {
        let (_dir, _index, db) = indexed_db();
        let accepted = db.reads().read_generation().unwrap();
        let before =
            cc_model::freshness::ResolutionFreshness::ready(accepted.index_epoch.saturating_sub(1));
        let value = envelope_with_accepted_generation(accepted);
        let value = finalize_search_response(&db, Some(before), value, 1_000_000).unwrap();
        assert_eq!(value["resolution_freshness"]["complete"], true);
        assert_eq!(
            value["resolution_freshness"]["dispatch_observed_generation_change"]
                ["from_index_epoch"],
            accepted.index_epoch.saturating_sub(1)
        );
        assert_eq!(
            value["resolution_freshness"]["dispatch_observed_generation_change"]["to_index_epoch"],
            accepted.index_epoch
        );
    }
}
