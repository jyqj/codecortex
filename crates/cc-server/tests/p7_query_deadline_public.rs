//! Real built-product stdio cancellation/deadline tests; toy loopback only.
#![cfg(feature = "semantic-http")]
use rmcp::{
    model::{CallToolRequestParams, ClientRequest},
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Probe {
    endpoint: String,
    held: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Probe {
    async fn start(stall_body: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let held = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (h, c, d, e) = (held.clone(), calls.clone(), closed.clone(), events.clone());
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, peer) = listener.accept().await.unwrap();
                assert!(peer.ip().is_loopback());
                let (h, c, d, e) = (h.clone(), c.clone(), d.clone(), e.clone());
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let body = loop {
                        let mut buf = [0_u8; 4096];
                        let n = socket.read(&mut buf).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buf[..n]);
                        assert!(bytes.len() < 200_000);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = std::str::from_utf8(&bytes[..end]).unwrap();
                            assert!(head.starts_with("POST /v1/embeddings HTTP/1.1"));
                            assert!(head
                                .to_ascii_lowercase()
                                .contains("authorization: bearer synthetic-public-deadline"));
                            let len: usize = head
                                .lines()
                                .find_map(|l| {
                                    let (key, v) = l.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse().unwrap())
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
                    assert_eq!(body["model"], "synthetic-public-deadline");
                    let inputs = body["input"].as_array().unwrap();
                    let query = inputs
                        .iter()
                        .any(|v| v.as_str().unwrap().contains("v18_live_"));
                    e.lock()
                        .unwrap()
                        .push(json!({"kind":"request","query":query,"body":body}));
                    if query {
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                    if query && h.load(Ordering::SeqCst) {
                        if stall_body {
                            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n{").await.unwrap();
                        }
                        let start = Instant::now();
                        let mut byte = [0_u8; 1];
                        let n = socket.read(&mut byte).await.unwrap();
                        assert_eq!(n,0,"product unexpectedly sent bytes instead of closing timed out/cancelled HTTP");
                        e.lock().unwrap().push(
                            json!({"kind":"peer_closed","elapsed_ms":start.elapsed().as_millis(),"stage":if stall_body {"body"} else {"headers"}}),
                        );
                        d.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    let data: Vec<_> = inputs
                        .iter()
                        .enumerate()
                        .map(|(i, _)| json!({"index":i,"embedding":[1.0,0.0]}))
                        .collect();
                    let response =
                        json!({"model":"synthetic-public-deadline","data":data}).to_string();
                    let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len());
                    socket.write_all(header.as_bytes()).await.unwrap();
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            endpoint,
            held,
            calls,
            closed,
            events,
            task,
        }
    }
    async fn wait(counter: &AtomicUsize, n: usize, bound: Duration) {
        tokio::time::timeout(bound, async {
            while counter.load(Ordering::SeqCst) < n {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
}
struct Session {
    root: tempfile::TempDir,
    client: RunningService<RoleClient, ()>,
    records: Mutex<Vec<Value>>,
}
impl Session {
    async fn open(probe: &Probe, semantic_ms: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("needle.py"), "def needle():\n    return 731\n").unwrap();
        let config = json!({"auto_index":{"enabled":false},"query":{"strategy":"auto","deadline_ms":8000,"semantic_timeout_ms":semantic_ms},"semantic":{"enabled":true,"network_opt_in":true,"allow_query_network":true,"allow_http":true,"endpoint":probe.endpoint,"model_id":"synthetic-public-deadline","dimensions":2,"max_input_tokens":8192,"max_batch_items":4,"api_key_ref":"env:P7_PUBLIC_DEADLINE_TOKEN"}});
        std::fs::write(project.join(".codecortex.json"), config.to_string()).unwrap();
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex")).configure(|cmd| {
                for (k, _) in std::env::vars_os() {
                    if k.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(k);
                    }
                }
                cmd.arg("mcp")
                    .arg("--project-path")
                    .arg(&project)
                    .current_dir(&project)
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .env("CODECORTEX_SEMANTIC_CACHE_ROOT", root.path().join("cache"))
                    .env("P7_PUBLIC_DEADLINE_TOKEN", "synthetic-public-deadline")
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
        let session = Self {
            root,
            client,
            records: Mutex::new(Vec::new()),
        };
        session
            .call("index", json!({"path":project,"full":true}))
            .await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let s = session
                    .call("status", json!({"aspect":"capabilities"}))
                    .await;
                if s["retrieval"]["dense_state"] == "ready" {
                    assert_eq!(s["retrieval"]["query_encoding"]["network_authorized"], true);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        session
    }
    async fn call(&self, name: &str, args: Value) -> Value {
        let reply = tokio::time::timeout(
            Duration::from_secs(10),
            self.client.call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(reply.is_error, Some(true), "{reply:?}");
        let value = reply.structured_content.unwrap()["result"].clone();
        self.records
            .lock()
            .unwrap()
            .push(json!({"kind":"call","tool":name,"args":args,"result":value}));
        value
    }
    fn trace(&self, probe: &Probe, label: &str) {
        let value = json!({"test":label,"calls":*self.records.lock().unwrap(),"http":*probe.events.lock().unwrap(),"query_calls":probe.calls.load(Ordering::SeqCst),"peer_closed":probe.closed.load(Ordering::SeqCst),"only_loopback":true,"real_credentials":false});
        if let Some(path) = std::env::var_os("P7_PUBLIC_EVIDENCE_DIR") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join(format!(
                    "{}-{}-{label}.json",
                    std::process::id(),
                    self.root.path().file_name().unwrap().to_string_lossy()
                )),
                serde_json::to_vec_pretty(&value).unwrap(),
            )
            .unwrap();
        } else {
            println!("PUBLIC_JSON {value}");
        }
    }
}
fn lane(value: &Value) -> &Value {
    value["evidence_summary"]["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["lane_id"] == "semantic")
        .unwrap()
}
fn args(tool: &str, marker: &str, strategy: &str) -> Value {
    let key = if tool == "search" { "query" } else { "task" };
    json!({key:marker,"retrieval_strategy":strategy})
}
fn fallback(value: &Value) {
    assert_eq!(lane(value)["status"], "timeout");
    assert_eq!(lane(value)["truncation_reason"], "semantic_deadline");
    assert_eq!(lane(value)["candidate_count"], 0);
    assert!(value["spans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["file_path"] == "needle.py"));
}

fn semantic_source(value: &Value) {
    assert_eq!(lane(value)["status"], "complete");
    assert!(lane(value)["candidate_count"].as_u64().unwrap() > 0);
    assert!(value["spans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["file_path"] == "needle.py"));
}

#[tokio::test]
async fn real_http_deadline_closes_transport_and_auto_keeps_local_search_and_context() {
    for (tool, stall_body) in [
        ("search", false),
        ("context", false),
        ("search", true),
        ("context", true),
    ] {
        let probe = Probe::start(stall_body).await;
        let session = Session::open(&probe, 150).await;
        let marker = format!("needle v18_live_deadline_{tool}");
        let start = Instant::now();
        let result = session.call(tool, args(tool, &marker, "auto")).await;
        assert!(
            start.elapsed() < Duration::from_millis(1500),
            "semantic work consumed the 8s parent budget"
        );
        fallback(&result);
        Probe::wait(&probe.closed, 1, Duration::from_millis(1500)).await;
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            1,
            "single HTTP attempt, no retry"
        );
        probe.held.store(false, Ordering::SeqCst);
        let healthy = session.call(tool, args(tool, &marker, "semantic")).await;
        semantic_source(&healthy);
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            2,
            "timed-out request must not prime query cache"
        );
        session.call(tool, args(tool, &marker, "semantic")).await;
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        session.trace(&probe, &format!("deadline-{tool}-body-{stall_body}"));
        session.client.cancel().await.unwrap();
    }
}
#[tokio::test]
async fn protocol_cancel_keeps_http_deadline_bounded_and_later_auto_request_can_fallback() {
    for tool in ["search", "context"] {
        let probe = Probe::start(false).await;
        let session = Session::open(&probe, 2000).await;
        let marker = format!("needle v18_live_cancel_{tool}");
        let request:ClientRequest=serde_json::from_value(json!({"method":"tools/call","params":{"name":tool,"arguments":args(tool,&marker,"auto")}})).unwrap();
        let pending = session
            .client
            .send_request_with_option(request, Default::default())
            .await
            .unwrap();
        Probe::wait(&probe.calls, 1, Duration::from_secs(1)).await;
        let cancelled = Instant::now();
        pending
            .cancel(Some("synthetic independent cancellation".into()))
            .await
            .unwrap();
        // Blocking reqwest I/O exits at its already clamped deadline;
        // cancellation fences publication without promising instant socket abort.
        // A distinct public request must still keep its own local fallback.
        let result = session.call(tool, args(tool, &marker, "auto")).await;
        fallback(&result);
        Probe::wait(&probe.closed, 2, Duration::from_millis(500)).await;
        assert!(
            cancelled.elapsed() < Duration::from_millis(3000),
            "cancelled HTTP escaped its 2s child budget into the 8s parent budget"
        );
        probe.held.store(false, Ordering::SeqCst);
        let recovered = session.call(tool, args(tool, &marker, "semantic")).await;
        semantic_source(&recovered);
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            3,
            "cancelled and timed-out encodings may not publish cache values"
        );
        session.trace(&probe, &format!("cancel-{tool}"));
        session.client.cancel().await.unwrap();
    }
}
