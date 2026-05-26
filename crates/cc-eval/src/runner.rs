use crate::types::{EvalCase, EvalMetrics, EvalRun, ToolCallRecord};
use std::process::Command;
use std::time::Instant;

pub struct EvalRunner {
    pub codecortex_binary: std::path::PathBuf,
    pub agent_command: String,
}

impl EvalRunner {
    pub fn new(
        codecortex_binary: impl Into<std::path::PathBuf>,
        agent_command: impl Into<String>,
    ) -> Self {
        Self {
            codecortex_binary: codecortex_binary.into(),
            agent_command: agent_command.into(),
        }
    }

    /// Run a deterministic code-index quality case.
    ///
    /// This intentionally does not spawn an LLM agent.  It is the minimal closed
    /// loop needed for regression tests: every expected marker must be discoverable
    /// either through CodeCortex search (`with_codecortex=true`) or a filesystem
    /// baseline grep (`with_codecortex=false`).  Agent subprocess benchmarking can
    /// be layered on top later without losing these stable quality assertions.
    pub fn run_case(&self, case: &EvalCase, with_codecortex: bool) -> Result<EvalRun, String> {
        let started = Instant::now();
        let mut metrics = EvalMetrics::default();
        let mut tool_calls = Vec::new();
        let mut missing = Vec::new();

        if with_codecortex && self.codecortex_binary.exists() {
            let idx_start = Instant::now();
            let output = Command::new(&self.codecortex_binary)
                .arg("index")
                .arg("--project-path")
                .arg(&case.repo_path)
                .output()
                .map_err(|e| format!("run codecortex index: {e}"))?;
            tool_calls.push(ToolCallRecord {
                tool_name: "codecortex index".to_string(),
                duration_ms: idx_start.elapsed().as_millis() as u64,
                input_summary: case.repo_path.display().to_string(),
                output_size_bytes: output.stdout.len() + output.stderr.len(),
            });
            metrics.tool_call_count += 1;
            metrics.mcp_call_count += 1;
            if !output.status.success() {
                return Ok(failed_run(
                    case,
                    with_codecortex,
                    started,
                    metrics,
                    tool_calls,
                    format!(
                        "codecortex index failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ),
                ));
            }

            for marker in &case.expected_markers {
                let search_start = Instant::now();
                let output = Command::new(&self.codecortex_binary)
                    .arg("search")
                    .arg(marker)
                    .arg("--project-path")
                    .arg(&case.repo_path)
                    .arg("--json")
                    .output()
                    .map_err(|e| format!("run codecortex search: {e}"))?;
                let combined = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                tool_calls.push(ToolCallRecord {
                    tool_name: "codecortex search".to_string(),
                    duration_ms: search_start.elapsed().as_millis() as u64,
                    input_summary: marker.clone(),
                    output_size_bytes: combined.len(),
                });
                metrics.tool_call_count += 1;
                metrics.mcp_call_count += 1;
                metrics.token_count_input += marker.len() as u64 / 4 + 1;
                metrics.token_count_output += combined.len() as u64 / 4 + 1;
                if !output.status.success() || !combined.contains(marker) {
                    missing.push(marker.clone());
                }
            }
        } else {
            // Baseline: repository-wide literal grep implemented in Rust so evals
            // do not depend on external grep/ripgrep availability.
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
                if !found {
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

fn failed_run(
    case: &EvalCase,
    with_codecortex: bool,
    started: Instant,
    mut metrics: EvalMetrics,
    tool_calls: Vec<ToolCallRecord>,
    error: String,
) -> EvalRun {
    metrics.wall_clock_ms = started.elapsed().as_millis() as u64;
    EvalRun {
        case_name: case.name.clone(),
        with_codecortex,
        metrics,
        tool_calls,
        success: false,
        error: Some(error),
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
