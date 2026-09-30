//! A literal, file-order lexical baseline. It does not pretend to be BM25.
use super::Backend;
use crate::benchmark::{
    manifest::FileRecord, readiness::Readiness, schema::SearchInput, BenchError, Result,
};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
pub struct Ripgrep {
    pub root: PathBuf,
    pub timeout: Duration,
    files: Vec<String>,
}
impl Ripgrep {
    pub fn new(root: PathBuf, timeout: Duration) -> Self {
        Self {
            root,
            timeout,
            files: vec![],
        }
    }
}
impl Backend for Ripgrep {
    fn name(&self) -> &'static str {
        "rg-literal"
    }
    async fn prepare(&mut self, files: &[FileRecord]) -> Result<Value> {
        self.files = files.iter().map(|f| f.path.clone()).collect();
        self.files.sort();
        Ok(json!({"baseline":"literal file-order","files":self.files.len()}))
    }
    async fn readiness(&mut self, files: &[FileRecord]) -> Result<Readiness> {
        Readiness::new(files.len(), files.len(), 0, 0, 0)
    }
    async fn search(&mut self, input: &SearchInput) -> Result<Value> {
        let files: Vec<&String> = self
            .files
            .iter()
            .filter(|p| {
                input
                    .path_prefix
                    .as_ref()
                    .is_none_or(|prefix| p.starts_with(prefix))
            })
            .collect();
        let mut nodes = Vec::new();
        let deadline = tokio::time::Instant::now() + self.timeout;
        for batch in files.chunks(100) {
            let mut cmd = tokio::process::Command::new("rg");
            cmd.current_dir(&self.root)
                .kill_on_drop(true)
                .args([
                    "--json",
                    "--fixed-strings",
                    "--line-number",
                    "--max-count",
                    "20",
                    "--",
                ])
                .arg(&input.query)
                .args(batch);
            let out = tokio::time::timeout_at(deadline, cmd.output())
                .await
                .map_err(|_| BenchError::Timeout("rg search".into()))??;
            if !out.status.success() && out.status.code() != Some(1) {
                return Err(BenchError::Protocol("rg failed".into()));
            }
            let text = String::from_utf8(out.stdout)
                .map_err(|_| BenchError::Protocol("rg non-UTF8".into()))?;
            for line in text.lines() {
                let v: Value = serde_json::from_str(line)?;
                if v["type"] == "match" {
                    nodes.push(json!({"node_type":"SEARCH_HIT","file_path":v["data"]["path"]["text"],"start_line":v["data"]["line_number"],"end_line":v["data"]["line_number"],"text":v["data"]["lines"]["text"].as_str().unwrap_or("").trim_end_matches(['\r','\n']),"metadata":{}}));
                }
            }
            if nodes.len() >= input.top_k {
                break;
            }
        }
        nodes.truncate(input.top_k);
        Ok(json!({"nodes":nodes}))
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}
