//! Eval runner: CodeIndexBackend drives the REAL MCP dispatch path.
//!
//! 工具调用不再手写 `match tool {...}` 直连 handler，而是通过 in-process duplex
//! 上的 rmcp JSON-RPC client 调用 `mcp_wire::CodeCortexMcpServer`（与 stdio
//! wire path 同一份源码）。因此 corpus 的参数会经过真实的 schema 反序列化 +
//! `sanitize()` 校验 + `spawn_handler!` 派发 + output budget，schema 漂移 /
//! 新增参数 / 校验规则自动被 eval 覆盖。

use crate::mcp_wire::CodeCortexMcpServer;
use crate::types::{Assertion, EvalCase, EvalCaseResult};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_EXPECTED_SYMBOLS_RECALL_AT_5: f64 = 0.7;

// ── CodeIndexBackend ────────────────────────────────────────────────

pub struct CodeIndexBackend {
    project_path: PathBuf,
    runtime: tokio::runtime::Runtime,
    /// rmcp client end of the in-process duplex connection. `Option` only so
    /// `Drop` can take ownership for a graceful `cancel()`.
    client: Option<RunningService<RoleClient, ()>>,
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
        let backend = Self::new_unindexed(project_path)?;
        backend.build_index_report(false)?;
        Ok(backend)
    }

    /// Create a new backend without building the index yet.
    ///
    /// This is intended for eval tests that need to inspect the `IndexReport`
    /// from the first explicit full/incremental build.
    pub fn new_unindexed(project_path: &Path) -> Result<Self, String> {
        // Clean up any existing index to avoid UNIQUE constraint errors on rebuild.
        // Must remove before the server opens the project (which creates the DB).
        let codecortex_dir = project_path.join(".codecortex");
        if codecortex_dir.exists() {
            std::fs::remove_dir_all(&codecortex_dir).map_err(|e| {
                format!(
                    "failed to remove stale index at {}: {}",
                    codecortex_dir.display(),
                    e
                )
            })?;
        }

        // 确定性保障：关闭 auto_index，避免 `index` 工具触发的 file watcher
        // 在 eval 修改 fixture 文件时并发执行增量构建（与显式 build 竞争）。
        // `.codecortex.json` 是隐藏文件，scanner 的 hidden(true) 会跳过它，
        // 不影响 files_scanned 等计数断言。
        let config_path = project_path.join(".codecortex.json");
        if !config_path.exists() {
            std::fs::write(&config_path, "{\"auto_index\": {\"enabled\": false}}\n")
                .map_err(|e| format!("failed to write eval config: {}", e))?;
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| format!("failed to build tokio runtime: {}", e))?;

        // In-process duplex: real JSON-RPC framing on both ends, no stdio/child
        // process. The server side is the same `CodeCortexMcpServer` the binary
        // serves over stdio.
        let project = project_path.to_path_buf();
        let client = runtime.block_on(async {
            let server = CodeCortexMcpServer::new(Some(&project));
            let (server_io, client_io) = tokio::io::duplex(1 << 20);
            tokio::spawn(async move {
                match rmcp::serve_server(server, server_io).await {
                    Ok(service) => {
                        let _ = service.waiting().await;
                    }
                    Err(e) => tracing::warn!("eval in-process MCP server failed: {}", e),
                }
            });
            ().serve(client_io)
                .await
                .map_err(|e| format!("failed to initialize eval MCP client: {}", e))
        })?;

        Ok(Self {
            project_path: project,
            runtime,
            client: Some(client),
        })
    }

    fn client(&self) -> &RunningService<RoleClient, ()> {
        self.client
            .as_ref()
            .expect("eval MCP client is only taken in Drop")
    }

    /// Build the index and return the serialized `IndexReport`.
    /// Goes through the real `index` tool (schema → sanitize → handler).
    pub fn build_index_report(&self, full: bool) -> Result<Value, String> {
        self.call_tool("index", &serde_json::json!({ "full": full }))
    }

    /// Dispatch a tool call by name, with JSON params, through the real MCP
    /// dispatch seam (rmcp router → `Parameters<T>` schema deserialization →
    /// `sanitize()` → handler → output budget).
    ///
    /// Corpus adapter shim: corpus cases are authored project-relative, so the
    /// `index` tool's `path` param is resolved against the backend's project
    /// root (and injected when missing). All other params pass through as-is —
    /// unknown tools and schema-invalid params surface as JSON-RPC errors.
    pub fn call_tool(&self, tool: &str, params: &Value) -> Result<Value, String> {
        let mut args = match params {
            Value::Null => serde_json::Map::new(),
            Value::Object(map) => map.clone(),
            other => {
                return Err(format!("tool params must be a JSON object, got: {}", other));
            }
        };
        if tool == "index" {
            let resolved = match args.get("path").and_then(|v| v.as_str()) {
                None | Some("") | Some(".") => self.project_path.clone(),
                Some(p) if !Path::new(p).is_absolute() => self.project_path.join(p),
                Some(p) => PathBuf::from(p),
            };
            args.insert(
                "path".to_string(),
                Value::String(resolved.to_string_lossy().into_owned()),
            );
        }

        let outcome = self.runtime.block_on(
            self.client()
                .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(args)),
        );
        match outcome {
            Ok(result) => unwrap_tool_result(result),
            // JSON-RPC error: handler errors, schema validation failures
            // (invalid params), and unknown tools all land here. The display
            // string keeps the original message (e.g. "Mcp error: -32602: ...").
            Err(e) => Err(e.to_string()),
        }
    }
}

impl Drop for CodeIndexBackend {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = self.runtime.block_on(client.cancel());
        }
    }
}

/// The ONE place where the MCP content envelope is unwrapped: handlers return
/// `Json(JsonResult { result })`, which rmcp surfaces to clients as
/// `CallToolResult.structured_content = {"result": <handler value>}`. Corpus
/// assertions therefore see exactly the handler JSON a real MCP client gets.
fn unwrap_tool_result(result: CallToolResult) -> Result<Value, String> {
    if result.is_error == Some(true) {
        // Tool-level error result (not used by this server's handlers, which
        // surface errors as JSON-RPC errors — kept for protocol completeness).
        let message = result
            .content
            .iter()
            .filter_map(|c| c.raw.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if message.is_empty() {
            "tool returned an error result".to_string()
        } else {
            message
        });
    }
    let structured = result
        .structured_content
        .ok_or("tool returned no structured content")?;
    match structured {
        Value::Object(mut map) => map
            .remove("result")
            .ok_or_else(|| "structured content missing `result` envelope".to_string()),
        other => Ok(other),
    }
}

// ── Assertion checker ───────────────────────────────────────────────

pub fn check_assertion(output: &Value, assertion: &Assertion) -> bool {
    let raw_result = check_assertion_raw(output, assertion);
    if assertion.negate {
        !raw_result
    } else {
        raw_result
    }
}

/// Inner check without negate inversion.
fn check_assertion_raw(output: &Value, assertion: &Assertion) -> bool {
    match assertion.kind.as_str() {
        "output_contains" => {
            let needle = assertion.value.as_deref().unwrap_or("");
            let serialized = serde_json::to_string(output).unwrap_or_default();
            serialized.contains(needle)
        }
        "output_not_contains" => {
            // Convenience: equivalent to output_contains with negate.
            // Note: the negate field is applied on TOP of this, so if someone
            // sets both output_not_contains + negate=true it double-inverts
            // (i.e. becomes output_contains). That is intentional.
            let needle = assertion.value.as_deref().unwrap_or("");
            let serialized = serde_json::to_string(output).unwrap_or_default();
            !serialized.contains(needle)
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
        "field_equals" => {
            let field = assertion.field.as_deref().unwrap_or("");
            let expected = assertion.value.as_deref().unwrap_or("");
            match resolve_json_path(output, field) {
                Some(Value::String(s)) => s == expected,
                Some(Value::Number(n)) => n.to_string() == expected,
                Some(Value::Bool(b)) => b.to_string() == expected,
                Some(Value::Null) => expected == "null",
                _ => false,
            }
        }
        "field_matches_regex" => {
            let field = assertion.field.as_deref().unwrap_or("");
            let pattern = assertion.value.as_deref().unwrap_or("");
            let re = match regex::Regex::new(pattern) {
                Ok(r) => r,
                Err(_) => return false,
            };
            match resolve_json_path(output, field) {
                Some(Value::String(s)) => re.is_match(s),
                Some(Value::Number(n)) => re.is_match(&n.to_string()),
                Some(Value::Bool(b)) => re.is_match(&b.to_string()),
                Some(Value::Null) => re.is_match("null"),
                _ => false,
            }
        }
        "array_contains_item" => {
            // value format: "sub_field=expected_value"
            let spec = assertion.value.as_deref().unwrap_or("");
            let (sub_field, expected_val) = match spec.split_once('=') {
                Some(pair) => pair,
                None => return false,
            };
            let target = if let Some(field) = assertion.field.as_deref() {
                resolve_json_path(output, field)
            } else {
                Some(output)
            };
            match target {
                Some(Value::Array(arr)) => arr.iter().any(|item| match item.get(sub_field) {
                    Some(Value::String(s)) => s == expected_val,
                    Some(Value::Number(n)) => n.to_string() == expected_val,
                    Some(Value::Bool(b)) => b.to_string() == expected_val,
                    _ => false,
                }),
                _ => false,
            }
        }
        "expected_symbols" => {
            let (recall_at_5, _mrr) = compute_retrieval_metrics(output, assertion);
            let threshold = assertion
                .min_recall
                .unwrap_or(DEFAULT_EXPECTED_SYMBOLS_RECALL_AT_5);
            recall_at_5 >= threshold
        }
        "is_success" => {
            // Always true if we got here (the tool didn't error).
            true
        }
        _ => false,
    }
}

/// Compute Recall@5 and MRR for an expected_symbols assertion.
/// Returns (recall_at_5, mrr).
fn compute_retrieval_metrics(output: &Value, assertion: &Assertion) -> (f64, f64) {
    let expected_str = assertion.value.as_deref().unwrap_or("");
    let expected_names: Vec<&str> = expected_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if expected_names.is_empty() {
        return (0.0, 0.0);
    }

    let target = if let Some(field) = assertion.field.as_deref() {
        resolve_json_path(output, field)
    } else {
        Some(output)
    };

    let arr = match target {
        Some(Value::Array(arr)) => arr,
        _ => return (0.0, 0.0),
    };

    // Extract symbol name from each item
    let result_names: Vec<String> = arr
        .iter()
        .filter_map(|item| {
            for key in &["name", "symbol_name", "symbol"] {
                if let Some(Value::String(s)) = item.get(*key) {
                    return Some(s.clone());
                }
            }
            None
        })
        .collect();

    // Recall@5: fraction of expected names found in first 5 results
    let top5: Vec<&str> = result_names.iter().take(5).map(|s| s.as_str()).collect();
    let found_count = expected_names
        .iter()
        .filter(|name| top5.iter().any(|r| r == *name))
        .count();
    let recall_at_5 = found_count as f64 / expected_names.len() as f64;

    // MRR: 1/rank of first expected name found (1-indexed), or 0 if none
    let mrr = result_names
        .iter()
        .enumerate()
        .find_map(|(idx, name)| {
            if expected_names.contains(&name.as_str()) {
                Some(1.0 / (idx as f64 + 1.0))
            } else {
                None
            }
        })
        .unwrap_or(0.0);

    (recall_at_5, mrr)
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
    let mut recall_at_5: Option<f64> = None;
    let mut mrr: Option<f64> = None;

    let output_size_bytes = match &result {
        Ok(output) => serde_json::to_string(output).unwrap_or_default().len(),
        Err(_) => 0,
    };

    if case.expect_error {
        // ── Negative testing: we EXPECT the tool to return Err ──
        match &result {
            Err(err) => {
                // Tool errored as expected — check non-is_success assertions
                // against the error message string (wrapped as a JSON string value).
                let error_output = Value::String(err.clone());
                for assertion in &case.assertions {
                    if assertion.kind == "is_success" {
                        // is_success is irrelevant for expect_error cases; skip it.
                        assertions_passed += 1;
                        continue;
                    }
                    if check_assertion(&error_output, assertion) {
                        assertions_passed += 1;
                    } else {
                        assertions_failed.push(assertion.describe());
                    }
                }
            }
            Ok(_) => {
                assertions_failed.push("expected tool error but got success".to_string());
            }
        }
    } else {
        // ── Normal (positive) testing ──
        match &result {
            Ok(output) => {
                for assertion in &case.assertions {
                    if check_assertion(output, assertion) {
                        assertions_passed += 1;
                    } else {
                        assertions_failed.push(assertion.describe());
                    }

                    // Compute retrieval metrics for expected_symbols assertions
                    if assertion.kind == "expected_symbols" {
                        let (r, m) = compute_retrieval_metrics(output, assertion);
                        recall_at_5 = Some(r);
                        mrr = Some(m);
                    }
                }
            }
            Err(err) => {
                // If the tool errored, check if any assertion is "is_success".
                // If there are no assertions, a tool error is still a failure.
                let has_is_success = case.assertions.iter().any(|a| a.kind == "is_success");
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
    }

    let passed = assertions_failed.is_empty();

    EvalCaseResult {
        case_name: case.name.clone(),
        tool: case.tool.clone(),
        passed,
        duration_ms: elapsed.as_millis() as u64,
        output_size_bytes,
        assertions_passed,
        assertions_failed,
        error: result.err(),
        recall_at_5,
        mrr,
    }
}

// ── Run all cases ───────────────────────────────────────────────────

pub fn run_all(backend: &CodeIndexBackend, cases: &[EvalCase]) -> Vec<EvalCaseResult> {
    cases.iter().map(|case| run_case(backend, case)).collect()
}
