//! V18 public MCP fault/recovery contracts. Synthetic loopback input only.
//! Query vectors are deliberately cold: provider faults are worker faults,
//! not invented query timeout/cancel receipts (C10/C14).
#![cfg(feature = "semantic-http")]

use cc_model::config::ProjectConfig;
use cc_server::mcp::CodeCortexMcpServer;
use rmcp::{model::CallToolRequestParams, service::RunningService, RoleClient, ServiceExt};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

type Client = RunningService<RoleClient, ()>;
struct Endpoint {
    address: String,
    healthy: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Endpoint {
    fn new(fault: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}/v1", listener.local_addr().unwrap());
        let healthy = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, c, s) = (healthy.clone(), calls.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::Acquire) {
                let Ok((mut socket, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                let body = loop {
                    let n = socket.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break Some(
                                serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length])
                                    .unwrap(),
                            );
                        }
                    }
                };
                let Some(body) = body else {
                    continue;
                };
                c.fetch_add(1, Ordering::AcqRel);
                if !h.load(Ordering::Acquire) {
                    match fault {
                        "disconnect" => continue,
                        "timeout" => {
                            std::thread::sleep(Duration::from_secs(31));
                            continue;
                        }
                        "server" => {
                            let _ = socket.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            continue;
                        }
                        _ => unreachable!(),
                    }
                }
                // Exact synthetic input count; never log request bodies or auth.
                let data: Vec<_> = body["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(index, _)| json!({"index":index,"embedding":[1.0,0.5]}))
                    .collect();
                let response = json!({"model":body["model"],"data":data}).to_string();
                let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len());
                let _ = socket.write_all(header.as_bytes());
                let _ = socket.write_all(response.as_bytes());
            }
        });
        Self {
            address,
            healthy,
            calls,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
async fn call(client: &Client, tool: &str, args: Value) -> Value {
    let result = client
        .call_tool(
            CallToolRequestParams::new(tool.to_owned())
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    result.structured_content.unwrap()["result"].clone()
}
async fn wait_state(client: &Client, expected: &str) -> Value {
    let mut last = Value::Null;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let status = call(client, "status", json!({"aspect":"capabilities"})).await;
            last = status.clone();
            if status["retrieval"]["semantic_state"] == expected {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("semantic state never reached {expected}: {last}"))
}
fn semantic_lane(value: &Value) -> &Value {
    value["evidence_summary"]["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["lane_id"] == "semantic")
        .unwrap()
}
async fn assert_queries(client: &Client) {
    let local = call(
        client,
        "search",
        json!({"query":"needle","retrieval_strategy":"local","top_k":1}),
    )
    .await;
    assert_eq!(local["machine_pack"]["hits"][0]["file_path"], "fixture.rs");
    for strategy in ["auto", "semantic"] {
        for tool in ["search", "context"] {
            let args = if tool == "search" {
                json!({"query":"needle","retrieval_strategy":strategy,"top_k":1})
            } else {
                json!({"task":"needle","retrieval_strategy":strategy,"max_symbols":1})
            };
            let value = call(client, tool, args).await;
            let lane = semantic_lane(&value);
            assert_eq!(lane["status"], "unavailable");
            assert_eq!(lane["truncation_reason"], "query_vector_not_encoded");
            assert_eq!(lane["coverage"]["complete"], false);
            assert_eq!(lane["candidate_count"], 0);
            assert_eq!(value["machine_pack"]["hits"][0]["file_path"], "fixture.rs");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_server_transport_timeout_faults_and_recovery_keep_honest_public_results() {
    // This sole cache-creating test owns the process env override. The other
    // test has semantic disabled and cannot read/create a semantic cache.
    let cache_root = tempfile::tempdir().unwrap();
    let _cache_env = CacheEnv::install(cache_root.path());
    // One process-wide case avoids global gate/breaker configuration races.
    for fault in ["server", "disconnect", "timeout"] {
        let endpoint = Endpoint::new(fault);
        let root = tempfile::tempdir().unwrap();
        let key = root.path().join("synthetic-key.txt");
        std::fs::write(&key, "v18-synthetic-not-a-real-credential").unwrap();
        let mut config = ProjectConfig::default();
        config.auto_index.enabled = false;
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.network_opt_in = true;
        config.semantic.allow_http = true;
        config.semantic.endpoint = endpoint.address.clone();
        config.semantic.api_key_ref = Some(format!("file:{}", key.display()));
        config.semantic.model_id = format!("synthetic/v18-{fault}");
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        // Retry budget bounds the sequence, not the synchronous provider
        // call: production factory retains its 30s request timeout.
        config.semantic.retry_total_deadline_ms = 100;
        std::fs::write(
            root.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let source = root.path().join("fixture.rs");
        std::fs::write(&source, "pub fn needle() -> u32 { 731 }\n").unwrap();
        let (server_io, client_io) = tokio::io::duplex(65536);
        let server_task = tokio::spawn(
            CodeCortexMcpServer::new(Some(root.path()))
                .unwrap()
                .serve(server_io),
        );
        let client = ().serve(client_io).await.unwrap();
        let server = server_task.await.unwrap().unwrap();
        assert_eq!(client.list_all_tools().await.unwrap().len(), 14);
        call(&client, "index", json!({"full":true,"path":root.path()})).await;
        let failed = tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                let status = call(&client, "status", json!({"aspect":"capabilities"})).await;
                if endpoint.calls.load(Ordering::Acquire) > 0
                    && status["retrieval"]["query_pins"] == 0
                    && status["retrieval"]["semantic_pending"]
                        .as_u64()
                        .is_some_and(|n| n > 0)
                {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("failed attempt must return bounded worker pin and retain retryable work");
        assert_eq!(failed["retrieval"]["semantic_state"], "backfilling");
        assert_eq!(failed["retrieval"]["dense_state"], "partial");
        assert!(
            failed["retrieval"]["dense_published"].as_u64().unwrap()
                < failed["retrieval"]["dense_desired"].as_u64().unwrap()
        );
        // Read-only diagnostic proves the injected failure class actually reached
        // the outbox; public response assertions above/below remain product paths.
        let db = rusqlite::Connection::open_with_flags(
            root.path().join(".codecortex/index.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let reason: String = db
            .query_row(
                "SELECT last_error FROM semantic_outbox WHERE state='pending' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            reason,
            if fault == "timeout" {
                "provider timeout"
            } else {
                "provider server error"
            }
        );
        drop(db);
        let before = endpoint.calls.load(Ordering::Acquire);
        assert!(before > 0, "real transport must reach the loopback fixture");
        assert_queries(&client).await;
        assert_eq!(
            endpoint.calls.load(Ordering::Acquire),
            before,
            "status/query must not invoke provider"
        );
        endpoint.healthy.store(true, Ordering::Release);
        // New synthetic source digest supersedes delayed failed work without
        // resetting its retry budget or manipulating internal receipts/DB.
        std::fs::write(&source, "pub fn needle() -> u32 { 997 }\n").unwrap();
        call(&client, "index", json!({"full":true,"path":root.path()})).await;
        let recovered = wait_state(&client, "ready").await;
        assert_eq!(recovered["retrieval"]["dense_state"], "ready");
        assert_queries(&client).await;
        assert!(endpoint.calls.load(Ordering::Acquire) > before);
        client.cancel().await.unwrap();
        server.cancel().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unconfigured_auto_stays_local_and_explicit_semantic_is_a_public_error() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("fixture.rs"),
        "pub fn needle() -> u32 { 731 }\n",
    )
    .unwrap();
    let (server_io, client_io) = tokio::io::duplex(65536);
    let task = tokio::spawn(
        CodeCortexMcpServer::new(Some(root.path()))
            .unwrap()
            .serve(server_io),
    );
    let client = ().serve(client_io).await.unwrap();
    let server = task.await.unwrap().unwrap();
    call(&client, "index", json!({"full":true,"path":root.path()})).await;
    let status = call(&client, "status", json!({"aspect":"capabilities"})).await;
    assert_eq!(status["retrieval"]["semantic_state"], "not_configured");
    assert_eq!(status["retrieval"]["dense_state"], "disabled");
    for tool in ["search", "context"] {
        for strategy in ["local", "auto"] {
            let args = if tool == "search" {
                json!({"query":"needle","retrieval_strategy":strategy})
            } else {
                json!({"task":"needle","retrieval_strategy":strategy})
            };
            let value = call(&client, tool, args).await;
            assert_eq!(
                value["evidence_summary"]["retrieval"]["policy"]["effective"],
                "local"
            );
            assert_eq!(value["machine_pack"]["hits"][0]["file_path"], "fixture.rs");
        }
        let args = if tool == "search" {
            json!({"query":"needle","retrieval_strategy":"semantic"})
        } else {
            json!({"task":"needle","retrieval_strategy":"semantic"})
        };
        let error = client
            .call_tool(
                CallToolRequestParams::new(tool.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().to_lowercase().contains("semantic"));
    }
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
}

struct CacheEnv(Option<std::ffi::OsString>);
impl CacheEnv {
    fn install(path: &std::path::Path) -> Self {
        let old = std::env::var_os(cc_semantic::cache::CACHE_ROOT_ENV);
        std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, path);
        Self(old)
    }
}
impl Drop for CacheEnv {
    fn drop(&mut self) {
        if let Some(old) = self.0.take() {
            std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, old);
        } else {
            std::env::remove_var(cc_semantic::cache::CACHE_ROOT_ENV);
        }
    }
}
