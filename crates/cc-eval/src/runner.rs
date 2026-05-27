use crate::types::{EvalCase, EvalMetrics, EvalRun, ToolCallRecord};
use std::time::Instant;

/// Trait for an indexing+search backend that the eval runner can call.
/// Implement this against CodeIndex (in cc-server) or a mock.
pub trait EvalBackend {
    fn index_project(&mut self, project_path: &std::path::Path) -> Result<String, String>;
    fn search(&self, query: &str) -> Result<String, String>;
    fn graph_query(&self, query: &str) -> Result<String, String>;
    fn callers(&self, symbol: &str) -> Result<String, String>;
}

pub struct EvalRunner {
    pub backend: Option<Box<dyn EvalBackend>>,
}

impl EvalRunner {
    pub fn new(backend: Option<Box<dyn EvalBackend>>) -> Self {
        Self { backend }
    }

    pub fn run_case(&mut self, case: &EvalCase, with_codecortex: bool) -> Result<EvalRun, String> {
        let started = Instant::now();
        let mut metrics = EvalMetrics::default();
        let mut tool_calls = Vec::new();
        let mut missing = Vec::new();

        metrics.markers_total = case.expected_markers.len() as u32;

        if with_codecortex {
            let backend = self
                .backend
                .as_mut()
                .ok_or("no eval backend configured")?;

            // Phase 1: index
            let idx_start = Instant::now();
            let output = backend.index_project(&case.repo_path)?;
            tool_calls.push(ToolCallRecord {
                tool_name: "index".to_string(),
                duration_ms: idx_start.elapsed().as_millis() as u64,
                input_summary: case.repo_path.display().to_string(),
                output_size_bytes: output.len(),
            });
            metrics.tool_call_count += 1;
            metrics.mcp_call_count += 1;

            // Phase 2: run category-specific tool calls
            for tool_name in &case.mcp_tools {
                let tool_start = Instant::now();
                let result = match tool_name.as_str() {
                    "search" => backend.search(&case.prompt),
                    "find_route_handlers" => {
                        backend.graph_query("MATCH (r:Route) RETURN r")
                    }
                    "find_dead_code" => {
                        backend.graph_query("MATCH (f:Function) WHERE degree(f) = 0 RETURN f.name")
                    }
                    "callers" => backend.callers(&case.prompt),
                    _ => backend.search(&case.prompt),
                };
                if let Ok(output) = result {
                    tool_calls.push(ToolCallRecord {
                        tool_name: format!("codecortex {}", tool_name),
                        duration_ms: tool_start.elapsed().as_millis() as u64,
                        input_summary: case.prompt.clone(),
                        output_size_bytes: output.len(),
                    });
                    metrics.tool_call_count += 1;
                    metrics.mcp_call_count += 1;
                    metrics.token_count_input += case.prompt.len() as u64 / 4 + 1;
                    metrics.token_count_output += output.len() as u64 / 4 + 1;
                }
            }

            // Phase 3: verify expected markers via search
            for marker in &case.expected_markers {
                let search_start = Instant::now();
                let result = backend.search(marker);
                metrics.tool_call_count += 1;
                metrics.mcp_call_count += 1;
                match result {
                    Ok(output) if output.contains(marker) => {
                        metrics.markers_found += 1;
                        tool_calls.push(ToolCallRecord {
                            tool_name: "search".to_string(),
                            duration_ms: search_start.elapsed().as_millis() as u64,
                            input_summary: marker.clone(),
                            output_size_bytes: output.len(),
                        });
                    }
                    _ => {
                        missing.push(marker.clone());
                    }
                }
            }
        } else {
            // Baseline: grep
            let files = collect_files(&case.repo_path)?;
            for marker in &case.expected_markers {
                metrics.grep_count += 1;
                metrics.tool_call_count += 1;
                let mut found = false;
                for file in &files {
                    metrics.file_read_count += 1;
                    if let Ok(content) = std::fs::read_to_string(file) {
                        if content.contains(marker) {
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    metrics.markers_found += 1;
                } else {
                    missing.push(marker.clone());
                }
            }
        }

        metrics.wall_clock_ms = started.elapsed().as_millis() as u64;
        Ok(EvalRun {
            case_name: case.name.clone(),
            with_codecortex,
            metrics,
            tool_calls,
            success: missing.is_empty(),
            error: if missing.is_empty() {
                None
            } else {
                Some(format!("missing expected markers: {}", missing.join(", ")))
            },
        })
    }
}

fn collect_files(root: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == ".git" || name == "target" || name == "node_modules" || name == ".codecortex"
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    Ok(out)
}
