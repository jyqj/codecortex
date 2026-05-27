//! Eval runner: CodeIndexBackend wraps CodeIndex and dispatches to facade handlers.

use crate::types::{Assertion, EvalCase, EvalCaseResult};
use cc_server::engine::CodeIndex;
use cc_server::handlers::{context, core, facade, graph};
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ── CodeIndexBackend ────────────────────────────────────────────────

pub struct CodeIndexBackend {
    runtime: Arc<Mutex<CodeIndex>>,
}

impl CodeIndexBackend {
    /// Create a new backend, pointing at a project directory.
    /// Removes any stale index, then builds fresh.
    ///
    /// **Safety note**: This intentionally deletes the `.codecortex` directory
    /// under `project_path` before rebuilding. This is by design for fixture /
    /// eval mode where we always want a clean index. Do NOT use this constructor
    /// against a user's real project directory — it will destroy their cached index.
    pub fn new(project_path: &Path) -> Result<Self, String> {
        // Clean up any existing index to avoid UNIQUE constraint errors on rebuild.
        // Must remove before CodeIndex::new() which creates the DB.
        let codecortex_dir = project_path.join(".codecortex");
        if codecortex_dir.exists() {
            std::fs::remove_dir_all(&codecortex_dir).map_err(|e| {
                format!("failed to remove stale index at {}: {}", codecortex_dir.display(), e)
            })?;
        }

        let mut index = CodeIndex::new(Some(project_path)).map_err(|e| e.to_string())?;
        // Build index (full=false since DB is fresh — avoids any full-rebuild edge cases)
        index.build_index(false).map_err(|e| e.to_string())?;
        let rt = Arc::new(Mutex::new(index));
        Ok(Self { runtime: rt })
    }

    /// Dispatch a tool call by name, with JSON params.
    pub fn call_tool(&self, tool: &str, params: &Value) -> Result<Value, String> {
        let rt = self.runtime.clone();
        match tool {
            "status" => {
                let aspect = params
                    .get("aspect")
                    .and_then(|v| v.as_str())
                    .unwrap_or("all");
                facade::handle_status(rt, aspect)
            }
            "index" => {
                let full = params
                    .get("full")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                core::build_index(rt, full)
            }
            "search" => {
                let query = params
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mode = params
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hybrid");
                let top_k = params
                    .get("top_k")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;
                let exact = params
                    .get("exact")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                match mode {
                    "symbol" => core::find_symbol(rt, query, exact, top_k, false),
                    _ => context::search_in_context(rt, query, top_k, None),
                }
            }
            "context" => {
                let task = params
                    .get("task")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let max_symbols = params
                    .get("max_symbols")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);
                let include_source = params
                    .get("include_source")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let intent = params.get("intent").and_then(|v| v.as_str());
                facade::handle_context(rt, task, max_symbols, include_source, intent)
            }
            "node" => {
                let symbol = params
                    .get("symbol")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let include = params
                    .get("include")
                    .and_then(|v| v.as_str())
                    .unwrap_or("trail");
                facade::handle_node(rt, symbol, include)
            }
            "explore" => {
                let symbols: Vec<String> = params
                    .get("symbols")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let mode = params
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("symbols");
                let include_source = params
                    .get("include_source")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let outline = params
                    .get("outline")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let max_depth = params
                    .get("max_depth")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;
                match mode {
                    "flow" => graph::explore_flow(
                        rt,
                        &symbols,
                        max_depth,
                        include_source,
                        None,
                        None,
                        None,
                        None,
                    ),
                    _ => context::explore_symbols(
                        rt,
                        &symbols,
                        None,
                        None,
                        include_source,
                        false,
                        false,
                        outline,
                        None,
                    ),
                }
            }
            "trace" => {
                let from = params
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let to = params.get("to").and_then(|v| v.as_str()).unwrap_or("");
                let max_depth = params
                    .get("max_depth")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;
                let include_source = params
                    .get("include_source")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let source_mode = params.get("source_mode").and_then(|v| v.as_str());
                graph::trace_path(rt, from, to, max_depth, include_source, None, source_mode)
            }
            "relations" => {
                let symbol = params
                    .get("symbol")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("both");
                let limit = params
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as usize;
                let direction = params
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("both");
                facade::handle_relations(rt, symbol, kind, limit, direction)
            }
            "impact" => {
                let scope = params
                    .get("scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("changes");
                let files: Vec<String> = params
                    .get("files")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let base_branch = params.get("base_branch").and_then(|v| v.as_str());
                let granularity = params
                    .get("granularity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("file");
                let file_path = params.get("file_path").and_then(|v| v.as_str());
                let limit = params
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as usize;
                facade::handle_impact(rt, scope, &files, base_branch, granularity, file_path, limit)
            }
            "architecture" => {
                let aspect = params
                    .get("aspect")
                    .and_then(|v| v.as_str())
                    .unwrap_or("overview");
                let filter = params.get("filter").and_then(|v| v.as_str());
                let limit = params
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as usize;
                facade::handle_architecture(rt, aspect, filter, limit)
            }
            "files" => {
                let action = params
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("list");
                let path = params.get("path").and_then(|v| v.as_str());
                let start_line = params
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                let end_line = params
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                let context_lines = params
                    .get("context_lines")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as u32;
                facade::handle_files(rt, action, path, start_line, end_line, context_lines)
            }
            "graph_query" => {
                let query = params
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                graph::graph_query(rt, query)
            }
            "ingest_traces" => {
                let traces: Vec<Value> = params
                    .get("traces")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                facade::handle_ingest_traces(rt, &traces)
            }
            "adr" => {
                let action = params
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("list");
                let adr_id = params.get("adr_id").and_then(|v| v.as_str());
                let title = params.get("title").and_then(|v| v.as_str());
                let status = params.get("status").and_then(|v| v.as_str());
                let context = params.get("context").and_then(|v| v.as_str());
                let decision = params.get("decision").and_then(|v| v.as_str());
                facade::handle_adr(rt, action, adr_id, title, status, context, decision)
            }
            unknown => Err(format!("unknown tool: {}", unknown)),
        }
    }
}

// ── Assertion checker ───────────────────────────────────────────────

pub fn check_assertion(output: &Value, assertion: &Assertion) -> bool {
    match assertion.kind.as_str() {
        "output_contains" => {
            let needle = assertion.value.as_deref().unwrap_or("");
            let serialized = serde_json::to_string(output).unwrap_or_default();
            serialized.contains(needle)
        }
        "field_exists" => {
            let field = assertion.field.as_deref().unwrap_or("");
            resolve_json_path(output, field).is_some()
        }
        "min_results" => {
            let min: usize = assertion
                .value
                .as_deref()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            let target = if let Some(field) = assertion.field.as_deref() {
                resolve_json_path(output, field)
            } else {
                Some(output)
            };
            match target {
                Some(Value::Array(arr)) => arr.len() >= min,
                // If the value exists but isn't an array, count it as 1 item
                Some(_) => min <= 1,
                None => false,
            }
        }
        "is_success" => {
            // Always true if we got here (the tool didn't error).
            true
        }
        _ => false,
    }
}

/// Simple dot-separated JSON path resolver.
/// Supports: "foo.bar.baz", "foo.0.bar" (array index).
fn resolve_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for segment in path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            Value::Array(arr) => {
                let idx: usize = segment.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

// ── Run a single case ───────────────────────────────────────────────

pub fn run_case(backend: &CodeIndexBackend, case: &EvalCase) -> EvalCaseResult {
    let start = Instant::now();
    let result = backend.call_tool(&case.tool, &case.params);
    let elapsed = start.elapsed();

    let mut assertions_passed = 0;
    let mut assertions_failed = Vec::new();

    match &result {
        Ok(output) => {
            for assertion in &case.assertions {
                if check_assertion(output, assertion) {
                    assertions_passed += 1;
                } else {
                    assertions_failed.push(assertion.describe());
                }
            }
        }
        Err(err) => {
            // If the tool errored, check if any assertion is "is_success".
            // If there are no assertions, a tool error is still a failure.
            let has_is_success = case
                .assertions
                .iter()
                .any(|a| a.kind == "is_success");
            if has_is_success || case.assertions.is_empty() {
                assertions_failed.push(format!("tool error: {}", err));
            } else {
                // All assertions automatically fail on tool error
                for assertion in &case.assertions {
                    assertions_failed.push(format!(
                        "{} (tool errored: {})",
                        assertion.describe(),
                        err
                    ));
                }
            }
        }
    }

    let passed = assertions_failed.is_empty();

    EvalCaseResult {
        case_name: case.name.clone(),
        tool: case.tool.clone(),
        passed,
        duration_ms: elapsed.as_millis() as u64,
        assertions_passed,
        assertions_failed,
        error: result.err(),
    }
}

// ── Run all cases ───────────────────────────────────────────────────

pub fn run_all(backend: &CodeIndexBackend, cases: &[EvalCase]) -> Vec<EvalCaseResult> {
    cases.iter().map(|case| run_case(backend, case)).collect()
}
