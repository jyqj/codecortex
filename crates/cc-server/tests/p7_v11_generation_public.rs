//! V11 L3: real product stdio rejects a recall crossing a real rebuild.
//! Synthetic loopback only; helper derived from preserved PR44 deadline fixture.
#![cfg(feature = "semantic-http")]
use rmcp::{
    model::CallToolRequestParams,
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
    events: Arc<Mutex<Vec<Value>>>,
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
        let held = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (h, c, e) = (held.clone(), calls.clone(), events.clone());
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, peer) = listener.accept().await.unwrap();
                assert!(peer.ip().is_loopback());
                let (h, c, e) = (h.clone(), c.clone(), e.clone());
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
                    while query && h.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(2)).await;
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
        let config = json!({"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":{"strategy":"auto","deadline_ms":8000,"semantic_timeout_ms":semantic_ms},"semantic":{"enabled":true,"network_opt_in":true,"allow_query_network":true,"allow_http":true,"endpoint":probe.endpoint,"model_id":"synthetic-public-deadline","dimensions":2,"max_input_tokens":8192,"max_batch_items":4,"api_key_ref":"env:P7_PUBLIC_DEADLINE_TOKEN"}});
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
        let value = json!({"test":label,"calls":*self.records.lock().unwrap(),"http":*probe.events.lock().unwrap(),"query_calls":probe.calls.load(Ordering::SeqCst),"only_loopback":true,"real_credentials":false});
        if let Some(path) = std::env::var_os("P7_V11_EVIDENCE_DIR") {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_http_crossing_rebuild_is_retryable_conflict_not_mixed_source() {
    for tool in ["search", "context"] {
        let probe = Probe::start().await;
        let session = Arc::new(Session::open(&probe, 5000).await);
        let before = session
            .call("status", json!({"aspect":"capabilities"}))
            .await;
        let marker = format!("needle v18_live_generation_{tool}");
        let old = session.call(tool, args(tool, &marker, "local")).await;
        assert!(old["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["text"].as_str().unwrap().contains("731")));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        let query_session = session.clone();
        let request_args = args(tool, &marker, "semantic");
        let pending = tokio::spawn(async move {
            query_session
                .client
                .call_tool(
                    CallToolRequestParams::new(tool.to_owned())
                        .with_arguments(request_args.as_object().unwrap().clone()),
                )
                .await
        });
        Probe::wait(&probe.calls, 1, Duration::from_secs(1)).await;
        assert!(
            !pending.is_finished(),
            "barrier must hold successful query HTTP"
        );
        std::fs::write(
            session.root.path().join("project/needle.py"),
            "def needle():\n    return 947\n",
        )
        .unwrap();
        let rebuild_start = Instant::now();
        session
            .call(
                "index",
                json!({"path":session.root.path().join("project"),"full":false}),
            )
            .await;
        let after = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let status = session
                    .call("status", json!({"aspect":"capabilities"}))
                    .await;
                if status["retrieval"]["dense_state"] == "ready" {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(rebuild_start.elapsed() < Duration::from_secs(3));
        assert_ne!(
            before["retrieval"]["generation"],
            after["retrieval"]["generation"]
        );
        assert!(
            !pending.is_finished(),
            "actual rebuild and one-reader status must progress before network release"
        );
        probe.held.store(false, Ordering::SeqCst);
        let conflict = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap();
        let wire = match conflict {
            Err(rmcp::ServiceError::McpError(error)) => serde_json::to_value(error).unwrap(),
            other => panic!("cross-generation HTTP must produce explicit RPC conflict, not any envelope: {other:?}"),
        };
        assert_eq!(wire["code"], -32603);
        assert_eq!(wire["data"]["retryable"], true);
        assert_eq!(
            wire["message"],
            "index changed during retrieval after 1 attempts; retry the query"
        );
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            1,
            "generation conflict must not restart provider IO internally"
        );
        let stable = session.call(tool, args(tool, &marker, "semantic")).await;
        assert_eq!(lane(&stable)["status"], "complete");
        assert!(lane(&stable)["candidate_count"].as_u64().unwrap() > 0);
        let spans = stable["spans"].as_array().unwrap();
        assert!(!spans.is_empty());
        assert!(spans.iter().any(|s| s["file_path"] == "needle.py"));
        let hits = stable["machine_pack"]["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(hits
            .iter()
            .any(|h| h["file_path"] == "needle.py" && h["text"].as_str().unwrap().contains("947")));
        assert!(hits
            .iter()
            .all(|h| !h["text"].as_str().unwrap().contains("731")));
        assert_eq!(
            stable["evidence_summary"]["source_freshness"]["generation"],
            after["retrieval"]["generation"]
        );
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1, "query vector remains reusable across document generations; final source envelope must not be cached across them");
        let local = session.call(tool, args(tool, &marker, "local")).await;
        assert!(local["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["text"].as_str().unwrap().contains("947")));
        assert!(local["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| !h["text"].as_str().unwrap().contains("731")));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        session.records.lock().unwrap().push(json!({"kind":"generation_conflict","before":before["retrieval"]["generation"],"after":after["retrieval"]["generation"],"wire":wire,"reader_pool":1,"provider_attempts":1}));
        session.trace(&probe, &format!("generation-{tool}"));
        Arc::try_unwrap(session)
            .ok()
            .unwrap()
            .client
            .cancel()
            .await
            .unwrap();
    }
}

#[test]
fn real_rebuild_fence_has_three_attempt_ceiling_and_stable_recovery() {
    for fail_work in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("needle.py");
        std::fs::write(&file, "def needle():\n    return 0\n").unwrap();
        std::fs::write(
            root.path().join(".codecortex.json"),
            r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
        )
        .unwrap();
        let mut index = cc_server::engine::CodeIndex::new(Some(root.path())).unwrap();
        index.build_index(true).unwrap();
        let db = index.index_db().unwrap().clone();
        let engine = cc_search::SearchEngine::new(db.clone(), &Default::default(), None);
        let mut attempts = 0;
        let start = Instant::now();
        let exhausted: cc_model::CcResult<_> = engine.with_stable_generation(|before| {
            attempts += 1;
            std::fs::write(&file, format!("def needle():\n    return {attempts}\n")).unwrap();
            index.build_index(false)?;
            assert_ne!(before, db.reads().read_generation()?);
            if fail_work {
                Err(cc_model::CcError::Search(
                    "synthetic work failed in changed generation".into(),
                ))
            } else {
                Ok(before.index_epoch)
            }
        });
        assert!(matches!(
            exhausted,
            Err(cc_model::CcError::RetrievalChanged { attempts: 3 })
        ));
        assert_eq!(attempts, 3);
        assert!(start.elapsed() < Duration::from_secs(3));
        let mut recovered_attempts = 0;
        let (accepted, work_epoch) = engine
            .with_stable_generation(|before| {
                recovered_attempts += 1;
                if recovered_attempts == 1 {
                    std::fs::write(&file, "def needle():\n    return 947\n").unwrap();
                    index.build_index(false)?;
                }
                Ok(before.index_epoch)
            })
            .unwrap();
        assert_eq!(recovered_attempts, 2);
        assert_eq!(accepted.index_epoch, work_epoch);
        assert_eq!(accepted, db.reads().read_generation().unwrap());
        for short_circuit in [
            cc_model::CcError::QueryCancelled,
            cc_model::CcError::QueryTimedOut,
            cc_model::CcError::Database("synthetic stable corruption".into()),
        ] {
            let expected = short_circuit.to_string();
            let mut error = Some(short_circuit);
            let mut calls = 0;
            let result: cc_model::CcResult<(cc_model::generation::ReadGeneration, ())> = engine
                .with_stable_generation(|_| {
                    calls += 1;
                    Err(error.take().expect("stable/cancelled work must not retry"))
                });
            assert_eq!(calls, 1);
            assert_eq!(result.unwrap_err().to_string(), expected);
        }
        let receipt = json!({"test":"real_rebuild_fence", "failed_work":fail_work,"exhausted_attempts":attempts,"recovered_attempts":recovered_attempts,"short_circuit_attempts":1,"accepted":accepted,"reader_pool":1,"level":"L2","direct_fence_entry":true});
        if let Some(path) = std::env::var_os("P7_V11_EVIDENCE_DIR") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join(format!("fence-{fail_work}.json")),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
        }
    }
}
