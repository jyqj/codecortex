//! V18 public-parameter acceptance through the built product's real stdio MCP.
//! Fixtures are synthetic, endpoints (when enabled) are loopback only. No live
//! credential/provider/corpus certification, and no injected query receipts.
use rmcp::{
    model::CallToolRequestParams,
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, process::Stdio, time::Duration};

struct Session {
    _root: tempfile::TempDir,
    client: RunningService<RoleClient, ()>,
    trace_sequence: std::sync::atomic::AtomicUsize,
}
impl Session {
    async fn open(strategy: Option<&str>, semantic: Option<Value>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(
            project.join("needle.py"),
            "def needle():\n    return 731\n\ndef neighbor():\n    return needle()\n",
        )
        .unwrap();
        let mut config = json!({"auto_index":{"enabled":false}});
        if let Some(strategy) = strategy {
            config["query"] = json!({"strategy":strategy});
        }
        if let Some(semantic) = semantic {
            config["semantic"] = semantic;
        }
        std::fs::write(project.join(".codecortex.json"), config.to_string()).unwrap();
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex")).configure(|cmd| {
                for (key, _) in std::env::vars_os() {
                    if key.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(key);
                    }
                }
                // Public dummy marker, resolved only for our loopback mock.
                // No real credential is inherited or read.
                if config.pointer("/semantic/api_key_ref")
                    == Some(&json!("env:P7_V18_LOOPBACK_TOKEN"))
                {
                    assert!(config["semantic"]["endpoint"]
                        .as_str()
                        .unwrap()
                        .starts_with("http://127.0.0.1:"));
                    cmd.env("P7_V18_LOOPBACK_TOKEN", "synthetic-v18-marker");
                } else {
                    cmd.env_remove("P7_V18_LOOPBACK_TOKEN");
                }
                cmd.arg("mcp")
                    .arg("--project-path")
                    .arg(&project)
                    .current_dir(&project)
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .env(
                        "CODECORTEX_SEMANTIC_CACHE_ROOT",
                        root.path().join("semantic-cache"),
                    )
                    .env_remove("OPENAI_API_KEY")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
            }),
        )
        .unwrap();
        let client = tokio::time::timeout(Duration::from_secs(10), ().serve(transport))
            .await
            .unwrap()
            .unwrap();
        Self {
            _root: root,
            client,
            trace_sequence: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn trace(&self, kind: &str, value: Value) {
        if let Some(root) = std::env::var_os("P7_V18_EVIDENCE_DIR") {
            let root = std::path::PathBuf::from(root);
            std::fs::create_dir_all(&root).unwrap();
            let sequence = self
                .trace_sequence
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let session = self._root.path().file_name().unwrap().to_string_lossy();
            let record = json!({"test":std::thread::current().name(),"kind":kind,"data":value});
            let path = root.join(format!(
                "{}-{}-{sequence:03}-{kind}.json",
                std::process::id(),
                session
            ));
            std::fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
            println!("V18_TRACE {}", path.display());
        } else {
            println!("V18_JSON {}", value);
        }
    }
    async fn call(&self, name: &str, args: Value) -> Value {
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            self.client.call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(
            response.is_error,
            Some(true),
            "tool {name} failed: {response:?}"
        );
        let value = response.structured_content.unwrap()["result"].clone();
        self.trace("call", json!({"tool":name,"args":args,"result":value}));
        value
    }
    async fn invalid(&self, name: &str, args: Value, schema_error: bool) {
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            self.client.call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap();
        if schema_error {
            // rmcp 2.2 returns schema-deserialization failures as tool errors;
            // sanitizer failures are JSON-RPC -32602 (MCP_TOOLS.md).
            let response = result.unwrap();
            assert_eq!(response.is_error, Some(true));
            assert!(response.structured_content.is_none());
            let message = serde_json::to_value(&response.content).unwrap();
            assert!(message
                .to_string()
                .contains("failed to deserialize parameters"));
            self.trace(
                "schema_error",
                json!({"tool":name,"args":args,"is_error":true,"content":message}),
            );
        } else {
            let error = result.unwrap_err();
            let rmcp::ServiceError::McpError(error) = error else {
                panic!("not a protocol parameter error: {error}");
            };
            assert_eq!(error.code.0, -32602);
            self.trace(
                "sanitize_error",
                json!({"tool":name,"args":args,"error_code":error.code.0,"message":error.message}),
            );
        }
    }
    async fn index(&self) {
        self.call(
            "index",
            json!({"path":self._root.path().join("project"),"full":true}),
        )
        .await;
    }
    async fn status(&self) -> Value {
        self.call("status", json!({"aspect":"capabilities"})).await
    }
    async fn close(self) {
        self.client.cancel().await.unwrap();
    }
}
fn policy(value: &Value) -> &Value {
    value
        .pointer("/evidence_summary/retrieval/policy")
        .expect("actual unified-query policy receipt")
}

#[tokio::test]
async fn advertised_schema_keeps_fourteen_tools_and_optional_strategy_defaults() {
    let session = Session::open(None, None).await;
    let tools = session.client.list_tools(None).await.unwrap().tools;
    let names: BTreeSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "status",
            "index",
            "search",
            "context",
            "node",
            "explore",
            "trace",
            "relations",
            "impact",
            "architecture",
            "files",
            "graph_query",
            "ingest_traces",
            "adr"
        ])
    );
    for (name, required) in [("search", "query"), ("context", "task")] {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        let schema = serde_json::to_value(&tool.input_schema).unwrap();
        assert!(schema["properties"]["retrieval_strategy"].is_object());
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!(required)));
        assert!(!schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("retrieval_strategy")));
        assert_eq!(schema["additionalProperties"], false);
        let description = schema["properties"]["retrieval_strategy"]["description"]
            .as_str()
            .unwrap();
        for strategy in ["local", "auto", "semantic"] {
            assert!(description.contains(strategy));
        }
        if name == "search" {
            assert_eq!(schema["properties"]["mode"]["default"], "hybrid");
            assert_eq!(schema["properties"]["top_k"]["default"], 10);
        }
        session.trace("schema", json!({"tool":name,"schema":schema}));
    }
    session.index().await;
    let result = session.call("search", json!({"query":"needle"})).await;
    assert_eq!(policy(&result)["requested"], "local");
    assert_eq!(policy(&result)["effective"], "local");
    let status = session.status().await;
    assert_eq!(status["retrieval"]["default_strategy"], "local");
    assert_eq!(status["retrieval"]["default_effective_strategy"], "local");
    assert_eq!(status["retrieval"]["semantic_state"], "not_configured");
    session.close().await;
}

#[tokio::test]
async fn malformed_strategy_and_symbol_incompatibility_are_public_parameter_errors() {
    let session = Session::open(None, None).await;
    let before = session.status().await;
    assert!(before["retrieval"]["generation"].is_object());
    for (tool, key) in [("search", "query"), ("context", "task")] {
        for bad in [
            json!("dense"),
            json!("AUTO"),
            json!(""),
            json!(1),
            json!({}),
            json!([]),
        ] {
            session
                .invalid(
                    tool,
                    json!({key:"needle","retrieval_strategy":bad}),
                    !bad.is_string(),
                )
                .await;
        }
        session
            .invalid(
                tool,
                json!({key:"needle","retrieval_stratgey":"auto"}),
                true,
            )
            .await;
        session
            .invalid(
                tool,
                json!({key:"needle","endpoint":"https://provider.invalid"}),
                true,
            )
            .await;
    }
    session
        .invalid("search", json!({"query":"needle","mode":"semantic"}), false)
        .await;
    session
        .invalid("search", json!({"query":"needle","top_k":"10"}), true)
        .await;
    for strategy in ["auto", "semantic"] {
        session
            .invalid(
                "search",
                json!({"query":"needle","mode":"symbol","retrieval_strategy":strategy}),
                false,
            )
            .await;
    }
    session
        .invalid("status", json!({"aspect":"semantic"}), false)
        .await;
    session.invalid("status", json!({"aspect":1}), true).await;
    let after = session.status().await;
    assert_eq!(
        before["retrieval"]["generation"],
        after["retrieval"]["generation"]
    );
    assert_eq!(after["retrieval"]["index_state"], "empty");
    session.close().await;
}

#[tokio::test]
async fn symbol_mode_preserves_legacy_array_and_sanitized_limits() {
    // A non-local project default cannot replace explicit legacy symbol mode.
    let session = Session::open(Some("auto"), None).await;
    session.index().await;
    for strategy in [None, Some(Value::Null), Some(json!("local"))] {
        let mut args = json!({"query":"needle","mode":"symbol","exact":true,"top_k":0});
        if let Some(strategy) = strategy {
            args["retrieval_strategy"] = strategy;
        }
        let result = session.call("search", args).await;
        let symbols = result.as_array().expect("original symbol array shape");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0]["name"], "needle");
    }
    session.close().await;
}

#[tokio::test]
async fn omitted_null_and_explicit_overrides_inherit_project_strategy_through_queries() {
    let session = Session::open(Some("auto"), None).await;
    session.index().await;
    let status = session.status().await;
    assert_eq!(status["retrieval"]["default_strategy"], "auto");
    assert_eq!(status["retrieval"]["default_effective_strategy"], "local");
    for (tool, key) in [("search", "query"), ("context", "task")] {
        for strategy in [
            None,
            Some(Value::Null),
            Some(json!("auto")),
            Some(json!("local")),
        ] {
            let mut args = json!({key:"needle"});
            let requested = if strategy == Some(json!("local")) {
                "local"
            } else {
                "auto"
            };
            if let Some(strategy) = strategy {
                args["retrieval_strategy"] = strategy;
            }
            let result = session.call(tool, args).await;
            assert_eq!(policy(&result)["requested"], requested);
            assert_eq!(policy(&result)["effective"], "local");
        }
    }
    let after = session.status().await;
    assert_eq!(
        after["retrieval"]["default_strategy"],
        status["retrieval"]["default_strategy"]
    );
    assert_eq!(
        after["retrieval"]["query_coverage"]["state"],
        "not_measured"
    );
    session.close().await;
}

#[cfg(feature = "semantic")]
fn enabled_config() -> Value {
    json!({"enabled":true,"network_opt_in":false,"model_id":"synthetic-v18-contract","dimensions":2,"max_input_tokens":8192,"max_batch_items":4})
}

#[cfg(feature = "semantic")]
#[tokio::test]
async fn enabled_semantic_strategy_reaches_real_wired_query_and_read_only_status() {
    let session = Session::open(Some("semantic"), Some(enabled_config())).await;
    session.index().await;
    let before = session.status().await;
    assert_eq!(before["retrieval"]["default_strategy"], "semantic");
    assert_eq!(
        before["retrieval"]["default_effective_strategy"],
        "semantic"
    );
    assert_eq!(
        before["retrieval"]["semantic_state"],
        "port_attached_unverified"
    );
    assert_eq!(
        before["retrieval"]["dense_reason"],
        "semantic_no_active_space"
    );
    assert!(before["retrieval"]["semantic_pending"].is_null());
    assert_eq!(before["retrieval"]["dense_state"], "disabled");
    for (tool, key) in [("search", "query"), ("context", "task")] {
        for strategy in [
            None,
            Some(Value::Null),
            Some(json!("semantic")),
            Some(json!("auto")),
            Some(json!("local")),
        ] {
            let mut args = json!({key:"needle"});
            let requested = strategy
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or("semantic");
            let expected = requested.to_owned();
            if let Some(strategy) = strategy {
                args["retrieval_strategy"] = strategy;
            }
            let result = session.call(tool, args).await;
            assert_eq!(policy(&result)["requested"], expected);
            assert_eq!(policy(&result)["effective"], expected);
            // Public packing may project verbose obligations away. Actual
            // executed lane receipts remain mandatory and distinguish routing.
            let retrieval = &result["evidence_summary"]["retrieval"];
            let lanes = retrieval["lanes"]
                .as_array()
                .or_else(|| retrieval["lane_receipts"].as_array())
                .unwrap();
            assert_eq!(
                lanes.iter().any(|l| l["lane_id"] == "semantic"),
                expected != "local"
            );
        }
    }
    let after = session.status().await;
    assert_eq!(
        before["retrieval"]["semantic_pending"],
        after["retrieval"]["semantic_pending"]
    );
    assert_eq!(
        after["retrieval"]["semantic_state"],
        "port_attached_unverified"
    );
    assert_eq!(
        after["retrieval"]["query_coverage"]["scope"],
        "per_query_not_global"
    );
    session.close().await;
}

#[cfg(feature = "semantic-http")]
mod loopback {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    struct MockEndpoint {
        endpoint: String,
        calls: Arc<AtomicUsize>,
        release_first: Arc<tokio::sync::Semaphore>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for MockEndpoint {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl MockEndpoint {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let release_first = Arc::new(tokio::sync::Semaphore::new(0));
            let gate = release_first.clone();
            let task = tokio::spawn(async move {
                loop {
                    let (mut stream, peer) = listener.accept().await.unwrap();
                    assert!(peer.ip().is_loopback());
                    let mut bytes = Vec::new();
                    let request = loop {
                        let mut buffer = [0_u8; 4096];
                        let n = stream.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() < 200_000);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = std::str::from_utf8(&bytes[..end]).unwrap();
                            assert!(head.starts_with("POST /v1/embeddings HTTP/1.1"));
                            assert!(head
                                .to_ascii_lowercase()
                                .contains("authorization: bearer synthetic-v18-marker"));
                            let len: usize = head
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")?
                                        .trim()
                                        .parse()
                                        .ok()
                                })
                                .unwrap();
                            if bytes.len() >= end + 4 + len {
                                break serde_json::from_slice::<Value>(
                                    &bytes[end + 4..end + 4 + len],
                                )
                                .unwrap();
                            }
                        }
                    };
                    assert_eq!(request["model"], "synthetic-v18-contract");
                    assert_eq!(request["encoding_format"], "float");
                    let inputs = request["input"].as_array().unwrap();
                    assert!(!inputs.is_empty());
                    let data: Vec<_> = inputs
                        .iter()
                        .enumerate()
                        .map(|(i, _)| json!({"index":i,"embedding":[1.0,0.0]}))
                        .collect();
                    if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                        gate.acquire().await.unwrap().forget();
                    }
                    let body = json!({"model":"synthetic-v18-contract","data":data,"usage":{"prompt_tokens":inputs.len(),"total_tokens":inputs.len()}}).to_string();
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    stream.write_all(header.as_bytes()).await.unwrap();
                    stream.write_all(body.as_bytes()).await.unwrap();
                    stream.shutdown().await.unwrap();
                }
            });
            Self {
                endpoint,
                calls,
                release_first,
                task,
            }
        }
    }

    #[tokio::test]
    async fn ready_publication_status_keeps_request_policy_and_per_query_coverage_distinct() {
        let mock = MockEndpoint::start().await;
        let mut config = enabled_config();
        config["network_opt_in"] = json!(true);
        config["allow_http"] = json!(true);
        config["endpoint"] = json!(mock.endpoint);
        config["api_key_ref"] = json!("env:P7_V18_LOOPBACK_TOKEN");
        let session = Session::open(Some("auto"), Some(config)).await;
        session.index().await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let backfilling = loop {
            let status = session.status().await;
            if mock.calls.load(Ordering::SeqCst) > 0 {
                break status;
            }
            assert!(
                !mock.task.is_finished(),
                "loopback mock exited before request"
            );
            assert!(
                tokio::time::Instant::now() < deadline,
                "worker did not contact loopback"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(backfilling["retrieval"]["semantic_state"], "backfilling");
        assert_eq!(backfilling["retrieval"]["dense_state"], "partial");
        assert!(
            backfilling["retrieval"]["semantic_pending"]
                .as_u64()
                .unwrap()
                > 0
        );
        mock.release_first.add_permits(1);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let ready = loop {
            let status = session.status().await;
            if status["retrieval"]["semantic_state"] == "ready"
                && status["retrieval"]["dense_state"] == "ready"
            {
                break status;
            }
            assert!(!mock.task.is_finished(), "loopback mock exited");
            assert!(
                tokio::time::Instant::now() < deadline,
                "publication did not become ready: {status}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(ready["retrieval"]["semantic_pending"], 0);
        assert_eq!(ready["retrieval"]["semantic_failed"], 0);
        assert!(ready["retrieval"]["dense_desired"].as_u64().unwrap() > 0);
        assert_eq!(
            ready["retrieval"]["dense_published"],
            ready["retrieval"]["dense_desired"]
        );
        assert_eq!(ready["retrieval"]["default_strategy"], "auto");
        assert_eq!(ready["retrieval"]["default_effective_strategy"], "auto");
        assert_eq!(
            ready["retrieval"]["query_coverage"]["state"],
            "not_measured"
        );
        let calls = mock.calls.load(Ordering::SeqCst);
        assert!(calls > 0);
        // Only parameter propagation is asserted here. The production query
        // path intentionally does not encode cache-miss vectors inline. Ready
        // publication therefore never certifies an arbitrary query's coverage.
        for (tool, key) in [("search", "query"), ("context", "task")] {
            for strategy in ["local", "auto", "semantic"] {
                let result = session
                    .call(tool, json!({key:"needle","retrieval_strategy":strategy}))
                    .await;
                assert_eq!(policy(&result)["requested"], strategy);
                assert_eq!(policy(&result)["effective"], strategy);
            }
        }
        let after = session.status().await;
        assert_eq!(after["retrieval"]["semantic_state"], "ready");
        assert_eq!(
            after["retrieval"]["query_coverage"]["state"],
            "not_measured"
        );
        let all = session.call("status", json!({"aspect":"all"})).await;
        for key in [
            "default_strategy",
            "default_effective_strategy",
            "semantic_state",
            "dense_state",
            "dense_published",
            "dense_desired",
            "query_coverage",
        ] {
            assert_eq!(
                all["capabilities"]["retrieval"][key], after["retrieval"][key],
                "status(all/capabilities) disagree at {key}"
            );
        }
        assert_eq!(
            mock.calls.load(Ordering::SeqCst),
            calls,
            "queries/status unexpectedly contacted provider"
        );
        session.trace("loopback", json!({"embedding_http_calls":calls,"only_loopback":true,"synthetic_vectors":true,"query_http_calls":0,"real_credentials":false}));
        session.close().await;
    }
}

// New independently opted-in user query encoding. These cases are separately
// filtered; the frozen PR33 parameter matrix is not re-run as new coverage.
mod query_network_contract {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    struct Probe {
        endpoint: String,
        requests: Arc<Mutex<Vec<Value>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Probe {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let observed = requests.clone();
            let task = tokio::spawn(async move {
                loop {
                    let (mut stream, peer) = listener.accept().await.unwrap();
                    assert!(peer.ip().is_loopback());
                    let mut bytes = Vec::new();
                    let request = loop {
                        let mut buffer = [0_u8; 4096];
                        let n = stream.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() < 200_000);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = std::str::from_utf8(&bytes[..end]).unwrap();
                            assert!(head.starts_with("POST /v1/embeddings HTTP/1.1"));
                            assert!(head
                                .to_ascii_lowercase()
                                .contains("authorization: bearer synthetic-v18-marker"));
                            let len: usize = head
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")?
                                        .trim()
                                        .parse()
                                        .ok()
                                })
                                .unwrap();
                            if bytes.len() >= end + 4 + len {
                                break serde_json::from_slice::<Value>(
                                    &bytes[end + 4..end + 4 + len],
                                )
                                .unwrap();
                            }
                        }
                    };
                    assert_eq!(request["model"], "synthetic-v18-contract");
                    assert_eq!(request["encoding_format"], "float");
                    let inputs = request["input"].as_array().unwrap();
                    assert!(!inputs.is_empty());
                    assert!(inputs.iter().all(Value::is_string));
                    observed.lock().unwrap().push(request.clone());
                    let data: Vec<_> = inputs
                        .iter()
                        .enumerate()
                        .map(|(i, _)| json!({"index":i,"embedding":[1.0,0.0]}))
                        .collect();
                    let body = json!({"model":"synthetic-v18-contract","data":data,"usage":{"prompt_tokens":inputs.len(),"total_tokens":inputs.len()}}).to_string();
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    stream.write_all(header.as_bytes()).await.unwrap();
                    stream.write_all(body.as_bytes()).await.unwrap();
                    stream.shutdown().await.unwrap();
                }
            });
            Self {
                endpoint,
                requests,
                task,
            }
        }
        fn query_calls(&self) -> usize {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| {
                    r["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|t| t.as_str().unwrap().contains("v18_query_"))
                })
                .count()
        }
        fn document_calls(&self) -> usize {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| {
                    r["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|t| !t.as_str().unwrap().contains("v18_query_"))
                })
                .count()
        }
        fn config(&self) -> Value {
            json!({"enabled":true,"network_opt_in":true,"allow_query_network":true,
                   "model_id":"synthetic-v18-contract","dimensions":2,"max_input_tokens":8192,
                   "max_batch_items":4,"allow_http":true,"endpoint":self.endpoint,
                   "api_key_ref":"env:P7_V18_LOOPBACK_TOKEN"})
        }
        fn trace(&self, session: &Session) {
            session.trace("query_network_probe", json!({"query_http_calls":self.query_calls(),
                "document_http_calls":self.document_calls(),"requests":*self.requests.lock().unwrap(),
                "only_loopback":true,"real_credentials":false,"synthetic_vectors":true}));
        }
    }
    fn encoding_status(status: &Value) -> &Value {
        status
            .pointer("/retrieval/query_encoding")
            .expect("owner's actual status interface")
    }
    async fn published(session: &Session, probe: &Probe) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let status = session.status().await;
            if status["retrieval"]["dense_state"] == "ready" {
                return;
            }
            assert!(!probe.task.is_finished(), "loopback probe terminated");
            assert!(
                tokio::time::Instant::now() < deadline,
                "document publication not ready: {status}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    fn semantic_lane(value: &Value) -> Option<&Value> {
        let retrieval = value.pointer("/evidence_summary/retrieval")?;
        retrieval["lanes"]
            .as_array()
            .or_else(|| retrieval["lane_receipts"].as_array())?
            .iter()
            .find(|l| l["lane_id"] == "semantic")
    }

    #[test]
    fn config_bool_is_default_false_and_rejects_non_boolean_values() {
        let default = serde_json::to_value(cc_model::config::ProjectConfig::default()).unwrap();
        assert_eq!(default["semantic"]["allow_query_network"], false);
        for (input, expected) in [
            (json!({}), false),
            (json!({"allow_query_network":false}), false),
            (json!({"allow_query_network":true}), true),
        ] {
            let config: cc_model::config::ProjectConfig =
                serde_json::from_value(json!({"semantic":input})).unwrap();
            assert_eq!(
                serde_json::to_value(config).unwrap()["semantic"]["allow_query_network"],
                expected
            );
        }
        for invalid in [json!("true"), json!(1), Value::Null, json!([]), json!({})] {
            assert!(serde_json::from_value::<cc_model::config::ProjectConfig>(
                json!({"semantic":{"allow_query_network":invalid}})
            )
            .is_err());
        }
    }

    #[tokio::test]
    async fn each_missing_gate_keeps_cold_requests_off_the_network() {
        let probe = Probe::start().await;
        let mut cases = Vec::new();
        for (name, key, value, reason) in [
            (
                "disabled",
                "enabled",
                Some(json!(false)),
                "semantic_disabled",
            ),
            (
                "network_off",
                "network_opt_in",
                Some(json!(false)),
                "network_opt_in_required",
            ),
            (
                "omitted",
                "allow_query_network",
                None,
                "query_network_opt_in_required",
            ),
            (
                "false",
                "allow_query_network",
                Some(json!(false)),
                "query_network_opt_in_required",
            ),
        ] {
            let mut config = probe.config();
            if let Some(value) = value {
                config[key] = value;
            } else {
                config.as_object_mut().unwrap().remove(key);
            }
            cases.push((name, config, reason));
        }
        if !cfg!(feature = "semantic-http") {
            cases.push((
                "feature_off",
                probe.config(),
                "semantic_http_feature_required",
            ));
        }
        for (name, config, reason) in cases {
            let old_network = config["network_opt_in"] == true;
            let enabled = config["enabled"] == true;
            let opt_in = config["allow_query_network"] == true;
            let session = Session::open(Some("auto"), Some(config)).await;
            session.index().await;
            if cfg!(feature = "semantic-http") && enabled && old_network {
                published(&session, &probe).await;
            }
            let status = session.status().await;
            let encoding = encoding_status(&status);
            assert_eq!(encoding["configured_opt_in"], opt_in);
            assert_eq!(encoding["network_authorized"], false);
            assert_eq!(encoding["reason"], reason);
            assert_eq!(encoding["request_scope"], "nonlocal_nonempty_only");
            for (tool, key) in [("search", "query"), ("context", "task")] {
                let marker = format!("v18_query_gate_{name}_{tool}");
                let result = session
                    .call(tool, json!({key:marker,"retrieval_strategy":"auto"}))
                    .await;
                if cfg!(feature = "semantic") && enabled {
                    let lane = semantic_lane(&result).expect("actual cold semantic lane");
                    assert_eq!(lane["candidate_count"], 0);
                    assert_eq!(lane["status"], "unavailable");
                    assert_eq!(lane["truncation_reason"], "query_vector_not_encoded");
                }
                assert_eq!(probe.query_calls(), 0, "{name}/{tool} bypassed a gate");
            }
            probe.trace(&session);
            session.close().await;
        }
    }

    #[tokio::test]
    async fn mcp_input_cannot_enable_or_bypass_configuration_authority() {
        let probe = Probe::start().await;
        let mut config = probe.config();
        config["allow_query_network"] = json!(false);
        let session = Session::open(Some("auto"), Some(config)).await;
        let tools = session.client.list_tools(None).await.unwrap().tools;
        assert_eq!(tools.len(), 14);
        for tool in tools {
            assert!(!tool.input_schema["properties"]
                .as_object()
                .unwrap()
                .contains_key("allow_query_network"));
        }
        session.index().await;
        if cfg!(feature = "semantic-http") {
            published(&session, &probe).await;
        }
        for (tool, key) in [("search", "query"), ("context", "task")] {
            session.invalid(tool, json!({key:format!("v18_query_bypass_{tool}"),"retrieval_strategy":"auto","allow_query_network":true}), true).await;
        }
        assert_eq!(probe.query_calls(), 0);
        assert_eq!(
            encoding_status(&session.status().await)["network_authorized"],
            false
        );
        probe.trace(&session);
        session.close().await;
    }

    #[tokio::test]
    async fn denied_authority_local_and_empty_scope_make_no_query_calls() {
        let probe = Probe::start().await;
        let mut config = probe.config();
        config["allow_query_network"] = json!(false);
        let session = Session::open(Some("auto"), Some(config)).await;
        session.index().await;
        if cfg!(feature = "semantic-http") {
            published(&session, &probe).await;
        }
        assert_eq!(
            encoding_status(&session.status().await)["reason"],
            "query_network_opt_in_required"
        );
        for (tool, key) in [("search", "query"), ("context", "task")] {
            session.call(tool, json!({key:format!("v18_query_denied_local_{tool}"),"retrieval_strategy":"local"})).await;
        }
        let empty = session.call("search", json!({"query":"v18_query_denied_empty_scope","retrieval_strategy":"auto","path_prefix":"absent/"})).await;
        assert_eq!(empty["evidence_summary"]["search_hits"], 0);
        assert_eq!(probe.query_calls(), 0);
        probe.trace(&session);
        session.close().await;
    }

    // Requires the designated service worker's real producer wiring before
    // execution. No manual query cache priming is permitted in this test.
    #[cfg(feature = "semantic-http")]
    #[tokio::test]
    async fn explicit_authority_encodes_public_search_and_context_and_reuses_cache() {
        let probe = Probe::start().await;
        let session = Session::open(Some("auto"), Some(probe.config())).await;
        session.index().await;
        published(&session, &probe).await;
        let status = session.status().await;
        assert_eq!(encoding_status(&status)["configured_opt_in"], true);
        assert_eq!(encoding_status(&status)["network_authorized"], true);
        assert!(encoding_status(&status)["reason"].is_null());
        assert!(probe.document_calls() > 0);
        for (tool, key, marker) in [
            ("search", "query", "v18_query_search_marker"),
            ("context", "task", "v18_query_context_marker"),
        ] {
            let before = probe.query_calls();
            let args = json!({key:marker,"retrieval_strategy":"semantic"});
            let first = session.call(tool, args.clone()).await;
            let lane = semantic_lane(&first).unwrap();
            assert_eq!(lane["status"], "complete");
            assert!(lane["candidate_count"].as_u64().unwrap() > 0);
            assert!(first["evidence_summary"]["search_hits"].as_u64().unwrap() > 0);
            assert!(first["spans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["file_path"] == "needle.py"));
            assert_eq!(
                probe.query_calls(),
                before + 1,
                "public {tool} did not encode its own unique marker"
            );
            let warm = session.call(tool, args).await;
            assert!(
                semantic_lane(&warm).unwrap()["candidate_count"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            assert_eq!(probe.query_calls(), before + 1, "cache hit encoded twice");
        }
        probe.trace(&session);
        session.close().await;
    }

    #[cfg(feature = "semantic-http")]
    #[tokio::test]
    async fn local_empty_scope_errors_and_reloaded_revocation_never_encode() {
        let probe = Probe::start().await;
        let session = Session::open(Some("auto"), Some(probe.config())).await;
        session.index().await;
        published(&session, &probe).await;
        assert_eq!(
            encoding_status(&session.status().await)["network_authorized"],
            true,
            "zero-traffic cases require a genuinely attached encoder"
        );
        for (tool, key) in [("search", "query"), ("context", "task")] {
            session
                .call(
                    tool,
                    json!({key:format!("v18_query_local_{tool}"),"retrieval_strategy":"local"}),
                )
                .await;
            session
                .invalid(
                    tool,
                    json!({key:format!("v18_query_invalid_{tool}"),"retrieval_strategy":"invalid"}),
                    false,
                )
                .await;
        }
        session.call("search", json!({"query":"v18_query_empty_scope","retrieval_strategy":"semantic","path_prefix":"absent/"})).await;
        assert_eq!(probe.query_calls(), 0);
        probe.trace(&session);
        session.close().await;
        // Re-open a fresh product session after an operator revokes authority.
        // This claims config reload semantics, not undocumented hot reload.
        let mut revoked = probe.config();
        revoked["allow_query_network"] = json!(false);
        let session = Session::open(Some("auto"), Some(revoked)).await;
        session.index().await;
        published(&session, &probe).await;
        for (tool, key) in [("search", "query"), ("context", "task")] {
            session
                .call(
                    tool,
                    json!({key:format!("v18_query_revoked_{tool}"),"retrieval_strategy":"auto"}),
                )
                .await;
        }
        assert_eq!(probe.query_calls(), 0);
        assert_eq!(
            encoding_status(&session.status().await)["reason"],
            "query_network_opt_in_required"
        );
        probe.trace(&session);
        session.close().await;
    }
}
