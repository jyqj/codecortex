//! Production public MCP query encoding with synthetic loopback input only.
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
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    blocked: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}
impl Endpoint {
    fn new(_fault: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}/v1", listener.local_addr().unwrap());
        let healthy = Arc::new(AtomicBool::new(true));
        let blocked = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let (b, r) = (blocked.clone(), release.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, c, s) = (healthy.clone(), calls.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::Acquire) {
                let Ok((mut socket, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                let (h, c, s, b, r) = (h.clone(), c.clone(), s.clone(), b.clone(), r.clone());
                std::thread::spawn(move || {
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
                                    serde_json::from_slice::<Value>(
                                        &bytes[end + 4..end + 4 + length],
                                    )
                                    .unwrap(),
                                );
                            }
                        }
                    };
                    let Some(body) = body else {
                        return;
                    };
                    c.fetch_add(1, Ordering::AcqRel);
                    assert!(h.load(Ordering::Acquire));
                    if body["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|input| input.as_str().unwrap().contains("blocked_document"))
                    {
                        b.store(true, Ordering::Release);
                        while !r.load(Ordering::Acquire) && !s.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(2));
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
                });
            }
        });
        Self {
            address,
            calls,
            stop,
            thread: Some(thread),
            blocked,
            release,
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
    assert_eq!(
        result.is_error,
        Some(false),
        "tool={tool} result={result:?}"
    );
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn public_query_opt_in_nonempty_cache_local_empty_and_partial() {
    let cache_root = tempfile::tempdir().unwrap();
    let _cache_env = CacheEnv::install(cache_root.path());
    let endpoint = Endpoint::new("healthy");
    for opted_in in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let key = root.path().join("synthetic-key.txt");
        std::fs::write(&key, "synthetic-loopback-only").unwrap();
        let mut config = ProjectConfig::default();
        config.auto_index.enabled = false;
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.network_opt_in = true;
        config.semantic.allow_query_network = opted_in;
        config.semantic.allow_http = true;
        config.semantic.endpoint = endpoint.address.clone();
        config.semantic.api_key_ref = Some(format!("file:{}", key.display()));
        config.semantic.model_id = format!("synthetic/public-query-{opted_in}");
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.max_concurrent = 2;
        std::fs::write(
            root.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.path().join("fixture.rs"),
            "pub fn needle() -> u32 { 731 }\n",
        )
        .unwrap();
        let (server_io, client_io) = tokio::io::duplex(65536);
        let server_task = tokio::spawn(
            CodeCortexMcpServer::new(Some(root.path()))
                .unwrap()
                .serve(server_io),
        );
        let client = ().serve(client_io).await.unwrap();
        let server = server_task.await.unwrap().unwrap();
        call(&client, "index", json!({"full":true,"path":root.path()})).await;
        let status = wait_state(&client, "ready").await;
        assert_eq!(
            status["retrieval"]["query_encoding"]["network_authorized"],
            opted_in
        );
        let initial = endpoint.calls.load(Ordering::Acquire);
        for args in [
            json!({"query":"needle","retrieval_strategy":"local"}),
            json!({"query":"needle","retrieval_strategy":"semantic","path_prefix":"absent/"}),
        ] {
            call(&client, "search", args).await;
        }
        assert_eq!(
            endpoint.calls.load(Ordering::Acquire),
            initial,
            "local/empty scope cannot encode"
        );
        for tool in ["search", "context"] {
            let args = if tool == "search" {
                json!({"query":"needle","retrieval_strategy":"semantic","top_k":2})
            } else {
                json!({"task":"needle","retrieval_strategy":"semantic","max_symbols":2})
            };
            let result = call(&client, tool, args.clone()).await;
            let lane = semantic_lane(&result);
            if opted_in {
                assert_eq!(lane["status"], "complete", "{result}");
                assert!(lane["candidate_count"].as_u64().unwrap() > 0);
                assert_eq!(result["machine_pack"]["hits"][0]["file_path"], "fixture.rs");
            } else {
                assert_eq!(lane["status"], "unavailable");
                assert_eq!(lane["truncation_reason"], "query_vector_not_encoded");
            }
            let after = endpoint.calls.load(Ordering::Acquire);
            call(&client, tool, args).await;
            assert_eq!(
                endpoint.calls.load(Ordering::Acquire),
                after,
                "cache hit must make zero HTTP calls"
            );
        }
        assert_eq!(
            endpoint.calls.load(Ordering::Acquire) - initial,
            usize::from(opted_in)
        );
        if opted_in {
            std::fs::write(
                root.path().join("pending.rs"),
                "pub fn blocked_document() -> u32 { 732 }\n",
            )
            .unwrap();
            call(&client, "index", json!({"full":false,"path":root.path()})).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                while !endpoint.blocked.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            })
            .await
            .unwrap();
            let result = call(
                &client,
                "search",
                json!({"query":"needle partial","retrieval_strategy":"semantic","top_k":2}),
            )
            .await;
            let lane = semantic_lane(&result);
            assert_eq!(lane["status"], "partial", "{result}");
            assert_eq!(lane["coverage"]["complete"], false);
            assert!(lane["candidate_count"].as_u64().unwrap() > 0);
            endpoint.release.store(true, Ordering::Release);
            wait_state(&client, "ready").await;
        }
        client.cancel().await.unwrap();
        server.cancel().await.unwrap();
    }
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
