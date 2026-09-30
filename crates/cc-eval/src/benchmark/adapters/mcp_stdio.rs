use super::Backend;
use crate::benchmark::{
    manifest::FileRecord, readiness::Readiness, schema::SearchInput, BenchError, Result,
};
use rmcp::{
    model::CallToolRequestParams,
    service::RunningService,
    transport::{ConfigureCommandExt, TokioChildProcess},
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
pub struct McpStdio {
    client: Option<RunningService<RoleClient, ()>>,
    root: PathBuf,
    pid: Option<u32>,
    timeout: Duration,
}
impl McpStdio {
    pub async fn spawn(binary: &Path, root: &Path, timeout: Duration) -> Result<Self> {
        let binary = binary.canonicalize()?;
        let project = root.canonicalize()?;
        let transport =
            TokioChildProcess::new(tokio::process::Command::new(binary).configure(|cmd| {
                // Reject hidden benchmark configuration inherited from the developer session.
                for (key, _) in std::env::vars_os() {
                    if key.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(key);
                    }
                }
                cmd.env("HOME", &project)
                    .env("XDG_CONFIG_HOME", project.join(".config"))
                    .env("XDG_CACHE_HOME", project.join(".cache"));
                cmd.arg("mcp")
                    .arg("--project-path")
                    .arg(&project)
                    .current_dir(&project)
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .env_remove("CODECORTEX_CACHE_DIR")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
            }))?;
        let pid = transport.id();
        let client = tokio::time::timeout(timeout, ().serve(transport))
            .await
            .map_err(|_| BenchError::Timeout("MCP initialize".into()))?
            .map_err(|e| BenchError::Protocol(e.to_string()))?;
        Ok(Self {
            client: Some(client),
            root: project,
            pid,
            timeout,
        })
    }
    pub async fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        let map = args
            .as_object()
            .ok_or_else(|| BenchError::Protocol("arguments must be object".into()))?
            .clone();
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| BenchError::Protocol("MCP client closed".into()))?;
        let outcome = tokio::time::timeout(
            self.timeout,
            client.call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(map)),
        )
        .await;
        let result = match outcome {
            Err(_) => {
                let _ = self.close().await;
                return Err(BenchError::Timeout(format!("MCP {name}")));
            }
            Ok(Err(e)) => return Err(BenchError::Protocol(e.to_string())),
            Ok(Ok(r)) => r,
        };
        if result.is_error == Some(true) {
            return Err(BenchError::Tool(format!("MCP {name} returned is_error")));
        }
        let structured = result
            .structured_content
            .ok_or_else(|| BenchError::Protocol("missing structured content".into()))?;
        structured
            .get("result")
            .cloned()
            .ok_or_else(|| BenchError::Protocol("missing result envelope".into()))
    }
}
impl Backend for McpStdio {
    fn name(&self) -> &'static str {
        "mcp-stdio"
    }
    fn pid(&self) -> Option<u32> {
        self.pid
    }
    async fn prepare(&mut self, _files: &[FileRecord]) -> Result<Value> {
        self.call("index", json!({"path":self.root,"full":true}))
            .await
    }
    async fn readiness(&mut self, files: &[FileRecord]) -> Result<Readiness> {
        let status = self.call("status", json!({"aspect":"index"})).await?;
        let n = status
            .get("indexed_files")
            .and_then(Value::as_u64)
            .ok_or_else(|| BenchError::Protocol("status has no indexed_files".into()))?
            as usize;
        if n > files.len() {
            return Err(BenchError::Protocol(
                "indexed inputs exceed source manifest".into(),
            ));
        }
        Readiness::new(files.len(), n, 0, 0, files.len() - n)
    }
    async fn search(&mut self, input: &SearchInput) -> Result<Value> {
        let mut args = json!({"query":input.query,"top_k":input.top_k,"mode":"hybrid"});
        if let Some(p) = &input.path_prefix {
            args["path_prefix"] = json!(p);
        }
        self.call("search", args).await
    }
    async fn close(&mut self) -> Result<()> {
        if let Some(client) = self.client.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), client.cancel()).await;
        }
        Ok(())
    }
}
