//! Core domain handlers: project setup, indexing, search, symbol queries, impact.

use super::SharedCodeIndex;
use crate::engine::CodeIndex;
use cc_index::IndexReport;
use cc_model::{CcError, CcResult};

pub fn build_index(runtime: SharedCodeIndex, full: bool) -> Result<serde_json::Value, String> {
    build_index_scoped(runtime, full, None)
}

/// [`build_index`] with an optional event-scoped hint from the MCP `index`
/// tool's `changed_paths`/`removed_paths` params: callers that know exactly
/// which files they touched (agents that just edited them, editor
/// integrations) get the same stat-only scan/diff path as watcher ticks.
/// The scope is a hint — every safety fallback to the full tree walk is
/// decided inside the scan/diff phase.
pub fn build_index_scoped(
    runtime: SharedCodeIndex,
    full: bool,
    scope: Option<cc_index::BuildScope>,
) -> Result<serde_json::Value, String> {
    // Per-project build gate: clone the Arc under a brief read lock and DROP
    // the read lock before blocking on the gate (lock-ordering rule: never
    // block on the gate while holding the CodeIndex RwLock). Manual builds
    // queue behind whichever build is in flight instead of racing prepares.
    let build_gate = {
        let rt = super::lock_index(&runtime)?;
        rt.build_gate()
    };
    // The gate guards no data — only build ordering — so poison is safe to recover.
    let _build_permit = build_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let report =
        run_split_build(&runtime, full, false, scope.as_ref()).map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Shared split-build driver for every gated entry point (manual `index`,
/// watcher ticks, `maybe_auto_index`): brief read lock → clone inputs →
/// lock-free prepare → write lock { stage 1: phase_write } → lock-free
/// stage 2 (postprocess/analysis compute) → write lock { stage 3: apply +
/// bookkeeping }. If a stage reports a stale build (index_epoch moved — at
/// stage 1 against the prepare snapshot, or at stage 3 against the stage-1
/// write), the whole prepare+commit sequence is re-run once. With the build
/// gate held by every entry point the retry never fires in-process; it
/// defends future ungated callers and cross-process writers. Callers must
/// already hold the build gate — stage-2 correctness (no concurrent build
/// mutating the DB between write and apply) relies on it.
///
/// `scope` carries the watcher tick's drained event set so the prepare can
/// stat/hash only those paths; the stale retry re-prepares with the SAME
/// scope (the events still describe exactly what changed — the re-prepare
/// re-reads the now-current DB state underneath them).
pub(crate) fn run_split_build(
    runtime: &SharedCodeIndex,
    full: bool,
    use_auto_file_limit: bool,
    scope: Option<&cc_index::BuildScope>,
) -> CcResult<IndexReport> {
    match split_build_once(runtime, full, use_auto_file_limit, scope) {
        Err(stale @ CcError::StalePreparedBuild { .. }) => {
            tracing::warn!("stale prepared build detected, retrying once: {}", stale);
            split_build_once(runtime, full, use_auto_file_limit, scope)
        }
        other => other,
    }
}

fn split_build_once(
    runtime: &SharedCodeIndex,
    full: bool,
    use_auto_file_limit: bool,
    scope: Option<&cc_index::BuildScope>,
) -> CcResult<IndexReport> {
    // Brief read lock: clone the owned build inputs (plus the auto-index
    // file-count gate when requested), then release.
    let (inputs, auto_file_limit) = {
        let rt = super::lock_index(runtime).map_err(CcError::Other)?;
        let limit = if use_auto_file_limit {
            Some(rt.auto_index_file_limit()?)
        } else {
            None
        };
        (rt.build_inputs()?, limit)
    };
    // Heavy prepare phase runs with NO lock held — read queries are not blocked.
    let prepared = CodeIndex::prepare_build_scoped(&inputs, full, auto_file_limit, scope)?;
    // Stage 1 — write lock scoped to `phase_write` only (the generation guard
    // runs inside, under this lock).
    let written = {
        let mut rt = super::lock_index_write(runtime).map_err(CcError::Other)?;
        rt.commit_build_write(&inputs, full, auto_file_limit, prepared)?
    };
    // Stage 2 — postprocess/analysis compute with NO lock held: signature
    // scans, synthesis passes, Louvain, git/infra/ADR analysis all read the
    // just-committed snapshot through the read pool while readers proceed.
    // The build gate (held by our caller) keeps the DB stable until stage 3.
    let staged = CodeIndex::compute_postprocess(&inputs, full, auto_file_limit, written)?;
    // Stage 3 — short write lock: apply the typed deltas + bookkeeping.
    let mut rt = super::lock_index_write(runtime).map_err(CcError::Other)?;
    rt.apply_postprocess(&inputs, full, auto_file_limit, staged)
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
    rt.graph()
        .find_symbol(name, exact, top_k, include_metrics)
        .map_err(|e| e.to_string())
}

pub fn list_files(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let files = rt.graph().list_indexed_files().map_err(|e| e.to_string())?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

pub fn list_communities(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt.graph().list_communities().map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn list_frameworks(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    use cc_index::framework_resolvers::resolver_tier_for_key;

    let rt = super::lock_index(&runtime)?;
    let rows = rt.graph().list_frameworks().map_err(|e| e.to_string())?;

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
    let rows = rt
        .graph()
        .callers(symbol, limit)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

pub fn callees(
    runtime: SharedCodeIndex,
    symbol: &str,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    let rows = rt
        .graph()
        .callees(symbol, limit)
        .map_err(|e| e.to_string())?;
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
        rt.impact().analyze_impact_capped(
            base_branch,
            confidence_threshold,
            result_limit,
            max_nodes,
            max_per_layer,
        )
    } else {
        rt.impact().detect_impact_capped(
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
    rt.impact()
        .git_changed_files(base_branch)
        .map_err(|e| e.to_string())
}

/// Get a summary of a single file.
pub fn summarize_file(
    runtime: SharedCodeIndex,
    file_path: &str,
) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.graph()
        .summarize_file(file_path)
        .map_err(|e| e.to_string())
}

/// Show available node kinds, edge types, and their counts in the index.
pub fn graph_schema(runtime: SharedCodeIndex) -> Result<serde_json::Value, String> {
    let rt = super::lock_index(&runtime)?;
    rt.graph().graph_schema().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, RwLock};
    use tempfile::TempDir;

    /// Two racing manual builds through the public handler seam must both
    /// succeed: the per-project build gate serializes them (the second waits)
    /// so concurrent prepares can never pair with interleaved commits.
    #[test]
    fn racing_manual_builds_serialize_on_the_gate() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
        let runtime: SharedCodeIndex =
            Arc::new(RwLock::new(CodeIndex::new(Some(dir.path())).unwrap()));

        let mut handles = Vec::new();
        for _ in 0..2 {
            let runtime = runtime.clone();
            handles.push(std::thread::spawn(move || build_index(runtime, false)));
        }
        for handle in handles {
            let result = handle.join().expect("build thread must not panic");
            assert!(
                result.is_ok(),
                "racing manual build failed: {:?}",
                result.err()
            );
        }

        let rt = super::super::lock_index(&runtime).unwrap();
        let stats = rt.index_status().unwrap();
        assert!(
            stats.indexed_files >= 1,
            "final index state must be consistent"
        );
        assert!(!rt.needs_initial_index());
    }

    /// Reads must proceed during stage 2 of the staged commit. Driven
    /// deterministically through the same seam `split_build_once` uses: after
    /// stage 1 releases the write lock, this test ACQUIRES the read lock and
    /// HOLDS it across the entire stage-2 compute — if compute (or anything
    /// it calls) needed the CodeIndex write lock, the `try_read` would fail
    /// or the compute would deadlock on this very thread. Stage-1 content is
    /// asserted reader-visible inside the window (the documented
    /// eventually-consistent state), and stage 3 then completes normally.
    #[test]
    fn reads_proceed_during_stage_two_compute() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
        let runtime: SharedCodeIndex =
            Arc::new(RwLock::new(CodeIndex::new(Some(dir.path())).unwrap()));

        // Hold the build gate across all three stages, as every production
        // entry point does — stage-2 correctness relies on it.
        let build_gate = {
            let rt = super::super::lock_index(&runtime).unwrap();
            rt.build_gate()
        };
        let _build_permit = build_gate.lock().unwrap();

        let inputs = {
            let rt = super::super::lock_index(&runtime).unwrap();
            rt.build_inputs().unwrap()
        };
        let prepared = CodeIndex::prepare_build(&inputs, false, None).unwrap();

        // Stage 1: write lock scoped to phase_write.
        let written = {
            let mut rt = runtime.write().unwrap();
            rt.commit_build_write(&inputs, false, None, prepared)
                .unwrap()
        };

        // Stage-2 window: the CodeIndex lock must be free for readers.
        let read_guard = runtime
            .try_read()
            .expect("read lock must be acquirable during stage 2");
        let staged = CodeIndex::compute_postprocess(&inputs, false, None, written)
            .expect("stage-2 compute must succeed while a reader holds the lock");
        assert!(
            read_guard.index_status().unwrap().indexed_files >= 1,
            "stage-1 content must be visible to concurrent readers during stage 2"
        );
        drop(read_guard);

        // Stage 3: short write lock for the apply + bookkeeping.
        let mut rt = runtime.write().unwrap();
        let report = rt.apply_postprocess(&inputs, false, None, staged).unwrap();
        assert!(report.symbols_total > 0, "staged build must index symbols");
        assert!(!rt.needs_initial_index());
    }
}
