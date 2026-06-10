//! OutputBudget: single exit-point enforcement of MCP tool output limits.
//!
//! Centralizes the byte-cap and item-cap policies that were previously
//! scattered across individual handlers. Per-handler budget *values* keep
//! their existing source (`RepoSizeTier::output_budget` / `max_output_chars`
//! in cc-model, surfaced via `CodeIndex`); this module only owns *where* and
//! *how* they are applied at the tool boundary.
//!
//! Both dispatch surfaces route successful tool results through
//! [`finalize`]: the MCP server (`spawn_handler!` in `mcp.rs`) and the eval
//! runner (`cc-eval`'s `CodeIndexBackend::call_tool`). Semantic mid-layer
//! truncation (graph_query row envelopes, trace/flow snippet budgets, the
//! enforcement inside `context::explore_symbols` whose result is embedded
//! into larger envelopes) intentionally stays in the handlers.
//!
//! Exit-side enforcement inherently reads the budget tier *after* the
//! handler body ran (pre-C4 handlers read it before running); the tier is
//! cached on `CodeIndex`, so both points in time observe the same value.

use super::SharedCodeIndex;
use crate::tools::utf8_prefix;
use serde_json::{json, Value};

/// Exit-side enforcement policy for one MCP tool.
enum ExitPolicy {
    /// No exit-side cap: the tool's output is bounded inside the handler
    /// (semantic envelopes, snippet/char budgets) or intentionally small.
    Passthrough,
    /// Cap the serialized JSON to the tier's `max_output_chars`.
    ByteCap,
    /// Truncate a top-level array to the named handler budget's `max_items`,
    /// appending a truncation marker.
    ///
    /// Assumption: this only bites when the tool returns a top-level array.
    /// For `files`, today that is solely the `list` action — `region` and
    /// `expand` return objects, so the cap is a no-op for them. If those
    /// actions ever start returning arrays, revisit this policy.
    ItemCap { budget_handler: &'static str },
}

/// Map an MCP tool name to its exit policy.
///
/// Tools listed as `Passthrough` either bound their output internally
/// (search/explore/trace/graph_query) or return small fixed envelopes
/// (status/index/ingest_traces/adr); adding a byte cap for them here would
/// change observable behavior, so any new cap must be a deliberate decision.
fn exit_policy(tool: &str) -> ExitPolicy {
    match tool {
        "context" | "node" | "relations" | "impact" | "architecture" => ExitPolicy::ByteCap,
        "files" => ExitPolicy::ItemCap {
            budget_handler: "files",
        },
        _ => ExitPolicy::Passthrough,
    }
}

/// Apply the tool's exit policy to a successful handler result.
///
/// Errors never flow through here: callers apply this with
/// `result.map(|v| finalize(...))` so error strings are returned unchanged.
pub fn finalize(runtime: &SharedCodeIndex, tool: &str, value: Value) -> Value {
    let policy = exit_policy(tool);
    if matches!(policy, ExitPolicy::Passthrough) {
        return value;
    }

    // Single lock acquisition per finalize: every budget value used below is
    // derived from this one tier snapshot.
    let tier = match super::lock_index(runtime) {
        Ok(rt) => rt.repo_size_tier(),
        // Fail-open by design: enforcement is a protection layer, not a
        // correctness layer — if the lock were ever unavailable we prefer
        // returning the untruncated result over dropping it. Unreachable
        // today (lock_index recovers poisoned locks and never returns Err);
        // kept so the chosen failure semantics stay explicit.
        Err(_) => return value,
    };

    match policy {
        ExitPolicy::Passthrough => value,
        ExitPolicy::ByteCap => enforce_output_limit(value, tier.max_output_chars()),
        ExitPolicy::ItemCap { budget_handler } => {
            let max_items = tier.output_budget(budget_handler).max_items;
            let mut value = value;
            if let Some(arr) = value.as_array_mut() {
                if arr.len() > max_items {
                    let total = arr.len();
                    arr.truncate(max_items);
                    arr.push(json!({"_truncated": true, "_total": total, "_shown": max_items}));
                }
            }
            value
        }
    }
}

/// Cap a JSON value's serialized size, replacing oversized values with a
/// truncation envelope carrying a bounded UTF-8-safe preview.
pub(crate) fn enforce_output_limit(value: Value, max_chars: usize) -> Value {
    let serialized = serde_json::to_string(&value).unwrap_or_default();
    if serialized.len() <= max_chars {
        return value;
    }

    // Keep only a bounded UTF-8-safe preview. The old fallback inserted the
    // original `value` when the preview was not valid JSON, which defeated the
    // output budget for large strings/objects.
    let preview_budget = max_chars.saturating_sub(256);
    let preview = utf8_prefix(&serialized, preview_budget).to_string();
    let partial = serde_json::from_str::<Value>(&preview)
        .ok()
        .unwrap_or(Value::String(preview));

    json!({
        "_truncated": true,
        "_original_chars": serialized.len(),
        "_max_chars": max_chars,
        "partial": partial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── enforce_output_limit: passthrough when under limit ──────────

    #[test]
    fn enforce_output_limit_passthrough_when_small() {
        let input = json!({"key": "value"});
        let result = enforce_output_limit(input.clone(), 10000);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: truncation when over limit ────────────

    #[test]
    fn enforce_output_limit_truncates_when_over() {
        let large = json!({"data": "x".repeat(500)});
        let result = enforce_output_limit(large.clone(), 50);

        assert!(result.get("_truncated").is_some());
        assert_eq!(result["_truncated"], true);
        assert!(result["_original_chars"].as_u64().unwrap() > 50);
        assert_eq!(result["_max_chars"], 50);
        assert!(result.get("partial").is_some());
    }

    #[test]
    fn enforce_output_limit_does_not_embed_original_on_invalid_prefix() {
        let large = json!({"data": "x".repeat(10_000)});
        let result = enforce_output_limit(large, 1_000);
        let rendered = serde_json::to_string(&result).unwrap();

        assert_eq!(result["_truncated"], true);
        assert!(rendered.len() < 2_000);
        assert!(!rendered.contains(&"x".repeat(5_000)));
    }

    #[test]
    fn enforce_output_limit_handles_multibyte_boundaries() {
        let large = json!({"data": "测".repeat(2_000)});
        let result = enforce_output_limit(large, 300);

        assert_eq!(result["_truncated"], true);
        serde_json::to_string(&result).unwrap();
    }

    // ── enforce_output_limit: zero max_chars ────────────────────────

    #[test]
    fn enforce_output_limit_zero_max() {
        let input = json!({"a": 1});
        let result = enforce_output_limit(input.clone(), 0);
        // Should produce a truncated wrapper (since serialized len > 0)
        assert!(result.get("_truncated").is_some());
    }

    // ── enforce_output_limit: exact boundary ────────────────────────

    #[test]
    fn enforce_output_limit_at_exact_boundary() {
        let input = json!({"k": "v"});
        let serialized_len = serde_json::to_string(&input).unwrap().len();
        // At exact length, should pass through
        let result = enforce_output_limit(input.clone(), serialized_len);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: one less than serialized len ──────────

    #[test]
    fn enforce_output_limit_one_less_than_len() {
        let input = json!({"k": "v"});
        let serialized_len = serde_json::to_string(&input).unwrap().len();
        let result = enforce_output_limit(input, serialized_len - 1);
        assert!(result.get("_truncated").is_some());
    }

    // ── enforce_output_limit: nested structure ──────────────────────

    #[test]
    fn enforce_output_limit_nested_json() {
        let input = json!({
            "outer": {
                "inner": [1, 2, 3],
                "deep": {"value": "hello"}
            }
        });
        let serialized_len = serde_json::to_string(&input).unwrap().len();

        // Under limit: passthrough
        let result = enforce_output_limit(input.clone(), serialized_len + 100);
        assert_eq!(result, input);

        // Over limit: truncated
        let result = enforce_output_limit(input, 30);
        assert!(result.get("_truncated").is_some());
    }

    // ── enforce_output_limit: array value ───────────────────────────

    #[test]
    fn enforce_output_limit_with_array() {
        let input = json!([1, 2, 3, 4, 5]);
        let result = enforce_output_limit(input.clone(), 100000);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: string value ──────────────────────────

    #[test]
    fn enforce_output_limit_with_string() {
        let input = json!("hello world");
        let result = enforce_output_limit(input.clone(), 100000);
        assert_eq!(result, input);
    }

    // ── enforce_output_limit: null value ────────────────────────────

    #[test]
    fn enforce_output_limit_with_null() {
        let input = json!(null);
        let result = enforce_output_limit(input.clone(), 100);
        assert_eq!(result, input);
    }

    // ── finalize: unified exit policy ───────────────────────────────

    /// A minimal runtime (no index build) — empty index → Tiny tier, which is
    /// enough to exercise the exit policies with real budget values.
    fn tiny_runtime() -> (tempfile::TempDir, SharedCodeIndex) {
        let tmp = tempfile::TempDir::new().unwrap();
        let idx = crate::engine::CodeIndex::new(Some(tmp.path())).unwrap();
        (tmp, std::sync::Arc::new(std::sync::RwLock::new(idx)))
    }

    #[test]
    fn finalize_byte_caps_budgeted_tools() {
        let (_tmp, rt) = tiny_runtime();
        let max_chars = crate::handlers::lock_index(&rt)
            .unwrap()
            .repo_size_tier()
            .max_output_chars();
        let oversized = json!({"data": "x".repeat(max_chars + 1000)});

        for tool in ["context", "node", "relations", "impact", "architecture"] {
            let result = finalize(&rt, tool, oversized.clone());
            assert_eq!(result["_truncated"], true, "tool {tool} not byte-capped");
            assert_eq!(result["_max_chars"].as_u64(), Some(max_chars as u64));
        }
    }

    #[test]
    fn finalize_byte_cap_passthrough_when_under_budget() {
        let (_tmp, rt) = tiny_runtime();
        let small = json!({"callers": [], "callees": []});
        assert_eq!(finalize(&rt, "relations", small.clone()), small);
    }

    #[test]
    fn finalize_passes_through_unbudgeted_tools() {
        let (_tmp, rt) = tiny_runtime();
        let max_chars = crate::handlers::lock_index(&rt)
            .unwrap()
            .repo_size_tier()
            .max_output_chars();
        // Oversized on purpose: passthrough tools must not be byte-capped at
        // the exit, their bounding happens (or deliberately does not happen)
        // inside the handler.
        let oversized = json!({"data": "x".repeat(max_chars + 1000)});

        for tool in [
            "status",
            "index",
            "search",
            "explore",
            "trace",
            "graph_query",
            "ingest_traces",
            "adr",
        ] {
            assert_eq!(
                finalize(&rt, tool, oversized.clone()),
                oversized,
                "tool {tool} unexpectedly modified at exit"
            );
        }
    }

    #[test]
    fn finalize_item_caps_files_list_with_marker() {
        let (_tmp, rt) = tiny_runtime();
        let max_items = crate::handlers::lock_index(&rt)
            .unwrap()
            .output_budget("files")
            .max_items;
        let listing: Vec<Value> = (0..max_items + 7).map(|n| json!({"file": n})).collect();

        let result = finalize(&rt, "files", Value::Array(listing));
        let arr = result.as_array().unwrap();
        // max_items entries plus one trailing truncation marker.
        assert_eq!(arr.len(), max_items + 1);
        assert_eq!(
            arr[max_items],
            json!({"_truncated": true, "_total": max_items + 7, "_shown": max_items})
        );
    }

    #[test]
    fn finalize_item_cap_leaves_small_arrays_and_objects_alone() {
        let (_tmp, rt) = tiny_runtime();
        let small_list = json!([{"file": "a.rs"}]);
        assert_eq!(finalize(&rt, "files", small_list.clone()), small_list);

        // region/expand actions return objects: no item cap applies.
        let region = json!({"file_path": "a.rs", "content": "fn main() {}"});
        assert_eq!(finalize(&rt, "files", region.clone()), region);
    }
}
