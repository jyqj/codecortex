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
    readiness_observation: Option<Value>,
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
            readiness_observation: None,
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

const INVENTORY_PAGE_SIZE: usize = 64;
const GENERATED_CONFIG: &str = ".codecortex.json";

fn indexed_count(status: &Value) -> Result<usize> {
    status
        .get("indexed_files")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| BenchError::Protocol("status has no valid indexed_files".into()))
}

fn index_epoch(status: &Value) -> Result<u64> {
    status
        .get("resolution_freshness")
        .and_then(|v| v.get("index_epoch"))
        .and_then(Value::as_u64)
        .ok_or_else(|| BenchError::Protocol("status has no index epoch".into()))
}

fn inventory_query(after: Option<&str>) -> Result<String> {
    let filter = if let Some(path) = after {
        super::super::validation::relative_path(path)?;
        let escaped = path.replace('\'', "\\'");
        format!(" WHERE f.file_path > '{escaped}'")
    } else {
        String::new()
    };
    let query = format!(
        "MATCH (f:File){filter} RETURN f.file_path AS path, f.size AS bytes ORDER BY f.file_path LIMIT {INVENTORY_PAGE_SIZE}"
    );
    // The public graph_query handler has this existing query-size ceiling.
    // Never allow its input sanitization to silently change the cursor.
    if query.len() > 4096 {
        return Err(BenchError::Protocol("inventory cursor exceeds MCP query budget".into()));
    }
    Ok(query)
}

fn inventory_page(value: &Value, after: Option<&str>) -> Result<Vec<(String, u64)>> {
    let rows = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| BenchError::Protocol("inventory page has no results".into()))?;
    if value.get("truncated").and_then(Value::as_bool) != Some(false)
        || value.get("row_count").and_then(Value::as_u64) != Some(rows.len() as u64)
        || value.get("limit_applied").and_then(Value::as_u64) != Some(INVENTORY_PAGE_SIZE as u64)
        || rows.len() > INVENTORY_PAGE_SIZE
    {
        return Err(BenchError::Protocol("inventory page is truncated or inconsistent".into()));
    }
    let mut result = Vec::with_capacity(rows.len());
    let mut previous = after;
    for row in rows {
        let path = row
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| BenchError::Protocol("inventory row has no path".into()))?;
        super::super::validation::relative_path(path)?;
        let bytes = row
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| BenchError::Protocol("inventory row has no byte size".into()))?;
        if previous.is_some_and(|old| path <= old) {
            return Err(BenchError::Protocol("inventory cursor did not advance".into()));
        }
        previous = Some(path);
        result.push((path.to_owned(), bytes));
    }
    Ok(result)
}

fn inventory_readiness(
    files: &[FileRecord],
    observed: &std::collections::BTreeMap<String, u64>,
    before: &Value,
    after: &Value,
) -> Result<Readiness> {
    if indexed_count(before)? != observed.len()
        || indexed_count(after)? != observed.len()
        || index_epoch(before)? != index_epoch(after)?
    {
        return Err(BenchError::Protocol("indexed inventory changed during readiness".into()));
    }
    let expected: std::collections::BTreeMap<_, _> =
        files.iter().map(|f| (f.path.as_str(), f.bytes)).collect();
    if expected.len() != files.len() || expected.contains_key(GENERATED_CONFIG) {
        return Err(BenchError::Protocol("invalid source inventory for readiness".into()));
    }
    let mut ready = 0;
    for (path, size) in observed {
        // materialize writes this one control file in the otherwise isolated
        // project. It is recorded in the observation but is never an input.
        if path == GENERATED_CONFIG {
            continue;
        }
        let expected_size = expected.get(path.as_str()).ok_or_else(|| {
            BenchError::Protocol("indexed path is outside the source manifest".into())
        })?;
        if size != expected_size {
            return Err(BenchError::Protocol("indexed file byte size differs from manifest".into()));
        }
        ready += 1;
    }
    Readiness::new(files.len(), ready, 0, 0, files.len() - ready)
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
        let before = self.call("status", json!({"aspect":"index"})).await?;
        self.readiness_observation = Some(json!({
            "schema_version":1,
            "method":"public_graph_query_keyset_file_inventory",
            "page_size":INVENTORY_PAGE_SIZE,
            "generated_control_file":GENERATED_CONFIG,
            "status_before":before,
            "pages":[],
            "verification":"incomplete"
        }));
        let expected_count = indexed_count(&before)?;
        let bound = files
            .len()
            .checked_add(1)
            .ok_or_else(|| BenchError::Protocol("source inventory too large".into()))?;
        if expected_count > bound {
            return Err(BenchError::Protocol("indexed inputs exceed source manifest".into()));
        }
        index_epoch(&before)?;
        let mut observed = std::collections::BTreeMap::new();
        let mut cursor: Option<String> = None;
        loop {
            let query = inventory_query(cursor.as_deref())?;
            let page = self.call("graph_query", json!({"query":query})).await?;
            self.readiness_observation
                .as_mut()
                .expect("readiness observation initialized")["pages"]
                .as_array_mut()
                .expect("readiness pages initialized")
                .push(json!({"query":query,"result":page}));
            let entries = inventory_page(&page, cursor.as_deref())?;
            let count = entries.len();
            for (path, bytes) in entries {
                cursor = Some(path.clone());
                if observed.insert(path, bytes).is_some() || observed.len() > expected_count {
                    return Err(BenchError::Protocol("indexed inventory count or identity changed".into()));
                }
            }
            if count < INVENTORY_PAGE_SIZE {
                break;
            }
        }
        let after = self.call("status", json!({"aspect":"index"})).await?;
        let observation = self
            .readiness_observation
            .as_mut()
            .expect("readiness observation initialized");
        observation["status_after"] = after.clone();
        observation["indexed_paths_and_bytes"] = json!(observed);
        let state = inventory_readiness(files, &observed, &before, &after)?;
        observation["verification"] = json!("exact_paths_and_sizes_checked");
        observation["readiness"] = serde_json::to_value(&state)?;
        Ok(state)
    }
    fn readiness_observation(&self) -> Option<&Value> {
        self.readiness_observation.as_ref()
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

#[cfg(test)]
mod inventory_tests {
    use super::*;
    use crate::benchmark::readiness::State;
    use std::collections::BTreeMap;

    fn status(n: usize, epoch: u64) -> Value {
        json!({"indexed_files":n,"resolution_freshness":{"index_epoch":epoch}})
    }
    fn file(path: &str, bytes: u64) -> FileRecord {
        FileRecord { path:path.into(), bytes, digest:"locked elsewhere".into() }
    }
    fn page(rows: Value) -> Value {
        json!({"row_count":rows.as_array().unwrap().len(),"results":rows,
               "truncated":false,"limit_applied":INVENTORY_PAGE_SIZE})
    }

    #[test]
    fn same_count_with_wrong_path_is_not_ready() {
        let actual = BTreeMap::from([("wrong.py".to_owned(), 10)]);
        assert!(inventory_readiness(&[file("wanted.py",10)], &actual,
            &status(1,4), &status(1,4)).is_err());
    }

    #[test]
    fn missing_inputs_remain_unknown_and_generated_config_is_explicit() {
        let actual = BTreeMap::from([("a.py".to_owned(),10),
            (GENERATED_CONFIG.to_owned(),42)]);
        let state = inventory_readiness(&[file("a.py",10),file("b.py",20)], &actual,
            &status(2,4), &status(2,4)).unwrap();
        assert_eq!(state.ready,1);
        assert_eq!(state.unknown,1);
        assert_eq!(state.state,State::Unknown);
        let state = inventory_readiness(&[file("a.py",10)], &actual,
            &status(2,4), &status(2,4)).unwrap();
        assert_eq!(state.state,State::Ready);
    }

    #[test]
    fn size_count_and_epoch_drift_are_rejected() {
        let actual = BTreeMap::from([("a.py".to_owned(),10)]);
        for (files,before,after) in [
            (vec![file("a.py",11)],status(1,4),status(1,4)),
            (vec![file("a.py",10)],status(2,4),status(1,4)),
            (vec![file("a.py",10)],status(1,4),status(2,4)),
            (vec![file("a.py",10)],status(1,4),status(1,5)),
        ] {
            assert!(inventory_readiness(&files,&actual,&before,&after).is_err());
        }
    }

    #[test]
    fn truncated_duplicate_or_inconsistent_pages_are_rejected() {
        let good = page(json!([{"path":"a.py","bytes":10}]));
        assert_eq!(inventory_page(&good,None).unwrap(),vec![("a.py".to_owned(),10)]);
        let mut truncated = good.clone();
        truncated["truncated"] = json!(true);
        assert!(inventory_page(&truncated,None).is_err());
        let mut wrong_count = good.clone();
        wrong_count["row_count"] = json!(2);
        assert!(inventory_page(&wrong_count,None).is_err());
        assert!(inventory_page(&good,Some("a.py")).is_err());
        assert!(inventory_page(&page(json!([
            {"path":"a.py","bytes":10},{"path":"a.py","bytes":10}])),None).is_err());
        assert!(inventory_page(&page(json!([{"path":"../escape","bytes":10}])),None).is_err());
    }

    #[test]
    fn keyset_query_quotes_cursor_and_has_a_fixed_limit() {
        let query=inventory_query(Some("q'quoted.py")).unwrap();
        assert!(query.contains("f.file_path > 'q\\'quoted.py'"));
        assert!(query.ends_with("ORDER BY f.file_path LIMIT 64"));
        assert!(inventory_query(Some("bad\\path")).is_err());
        assert!(inventory_query(Some(&"x".repeat(4096))).is_err());
    }
}
