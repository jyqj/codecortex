//! V05 L3: independent literal DSL domain through actual product stdio.
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
    time::Duration,
};
const DOCS: [(&str, &str); 4] = [
    ("scope/needle.py", "def needle():\n    return 731\n"),
    ("scope/sub/needle.py", "def needle():\n    return 732\n"),
    ("scope/needle.rs", "pub fn needle() -> u32 { 733 }\n"),
    ("outside/needle.py", "def needle():\n    return 999\n"),
];
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Probe {
    endpoint: String,
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
        let held = Arc::new(AtomicBool::new(false));
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
            calls,
            events,
            task,
        }
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
        for (path, content) in DOCS {
            let file = project.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
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
        if let Some(path) = std::env::var_os("P7_V05_EVIDENCE_DIR") {
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
    let retrieval = &value["evidence_summary"]["retrieval"];
    retrieval
        .get("lanes")
        .or_else(|| retrieval.get("lane_receipts"))
        .expect("full or compact public lane receipts required")
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
async fn actual_stdio_dsl_scope_and_hydration_intersect_across_query_policies() {
    let probe = Probe::start().await;
    let session = Session::open(&probe, 5000).await;
    let cases: [(&str, &[&str]); 6] = [
        (
            "needle v18_live_scope path:scope/ lang:python",
            &["scope/needle.py", "scope/sub/needle.py"],
        ),
        (
            "needle v18_live_scope path:scope/ lang:rust",
            &["scope/needle.rs"],
        ),
        (
            "needle v18_live_scope path:scope/ path:scope/sub/ lang:python",
            &["scope/sub/needle.py"],
        ),
        ("needle v18_live_scope path:scope/ path:outside/", &[]),
        ("needle v18_live_scope lang:python lang:rust", &[]),
        ("needle v18_live_scope path:missing/ lang:python", &[]),
    ];
    for tool in ["search", "context"] {
        for (query, allowed) in cases {
            for strategy in ["local", "auto", "semantic"] {
                let before = probe.calls.load(Ordering::SeqCst);
                let mut arguments = args(tool, query, strategy);
                if tool == "search" {
                    arguments["boost_files"] = json!(["outside/needle.py"]);
                    arguments["recent_files"] = json!(["outside/needle.py"]);
                    arguments["pinned_files"] = json!(["outside/needle.py"]);
                    arguments["overlay_files"] = json!(["outside/needle.py"]);
                    arguments["file_preselect_limit"] = json!(1);
                    arguments["path_prefix"] = json!("scope/");
                }
                let result = session.call(tool, arguments).await;
                let hits = result["machine_pack"]["hits"].as_array().unwrap();
                let spans = result["spans"].as_array().unwrap();
                for hit in hits {
                    let path = hit["file_path"].as_str().unwrap();
                    assert!(allowed.contains(&path), "hit escaped independent literal DSL domain: {tool} {strategy} {query}: {path}");
                    let expected = DOCS.iter().find(|(p, _)| *p == path).unwrap().1;
                    let actual = hit["text"].as_str().unwrap();
                    assert!(
                        expected.contains(actual),
                        "hydrated text must be exact current synthetic source"
                    );
                }
                for span in spans {
                    if let Some(path) = span["file_path"].as_str() {
                        assert!(allowed.contains(&path), "span escaped DSL domain: {path}");
                    }
                }
                if allowed.is_empty() {
                    assert!(hits.is_empty());
                    assert!(spans.is_empty());
                    assert_eq!(
                        probe.calls.load(Ordering::SeqCst),
                        before,
                        "empty domain must skip query HTTP"
                    );
                } else {
                    assert!(!hits.is_empty(), "nonempty domain must remain reachable despite outside hints: {tool} {strategy} {query}");
                    if strategy != "local" {
                        assert_eq!(lane(&result)["status"], "complete");
                        assert!(lane(&result)["candidate_count"].as_u64().unwrap() > 0);
                    }
                }
                session.records.lock().unwrap().push(json!({"kind":"scope_gold","tool":tool,"strategy":strategy,"query":query,"allowed":allowed,"hit_count":hits.len(),"span_count":spans.len(),"query_posts_before":before,"query_posts_after":probe.calls.load(Ordering::SeqCst)}));
            }
        }
    }
    session.trace(&probe, "dsl-range-hydration");
    session.client.cancel().await.unwrap();
}

#[test]
fn local_lane_candidates_obey_literal_scope_and_explicit_empty_sets() {
    let root = tempfile::tempdir().unwrap();
    for (path, content) in DOCS {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let engine =
        cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let db = index.index_db().unwrap().clone();
    let mut catalog = std::collections::BTreeMap::new();
    for (path, _) in DOCS {
        for reference in cc_db::document_store::references(&db, path).unwrap() {
            catalog.insert(reference.doc_key, path);
        }
    }
    let base = cc_model::search::SearchRequest {
        query: "needle".into(),
        path_prefix: Some("scope/".into()),
        languages: Some(vec![cc_model::Language::Python]),
        boost_file_paths: Some(vec!["outside/needle.py".into()]),
        file_preselect_limit: Some(1),
        include_grep: true,
        top_k: 10,
        ..Default::default()
    };
    let cases = [
        (base.clone(), vec!["scope/needle.py", "scope/sub/needle.py"]),
        (
            cc_model::search::SearchRequest {
                file_paths: Some(vec!["scope/needle.py".into(), "outside/needle.py".into()]),
                ..base.clone()
            },
            vec!["scope/needle.py"],
        ),
        (
            cc_model::search::SearchRequest {
                file_paths: Some(vec![]),
                ..base.clone()
            },
            vec![],
        ),
        (
            cc_model::search::SearchRequest {
                languages: Some(vec![]),
                ..base.clone()
            },
            vec![],
        ),
        (
            cc_model::search::SearchRequest {
                query: "needle lang:rust".into(),
                ..base
            },
            vec![],
        ),
    ];
    let mut receipts = Vec::new();
    for (case, (request, allowed)) in cases.into_iter().enumerate() {
        let result = engine.search_with_diagnostics(&request).unwrap();
        for lane in &result.lanes {
            for candidate in &lane.candidates {
                assert!(
                    allowed.contains(
                        catalog
                            .get(&candidate.document.doc_key)
                            .expect("candidate must identify a real fixture document")
                    ),
                    "{} lane escaped independent literal domain: {}",
                    lane.lane_id,
                    candidate.document.doc_key
                );
            }
        }
        for hit in &result.hits {
            assert!(allowed.contains(&hit.file_path.as_str()));
        }
        if allowed.is_empty() {
            assert!(result.hits.is_empty());
            assert!(result.lanes.iter().all(|lane| lane.candidates.is_empty()));
        } else {
            assert!(!result.hits.is_empty());
            if case == 0 {
                for id in ["exact_symbol", "path", "lexical", "grep"] {
                    let lane = result.lanes.iter().find(|lane| lane.lane_id == id).unwrap();
                    assert!(
                        !lane.candidates.is_empty(),
                        "{id} coverage must not be vacuous"
                    );
                }
            }
        }
        receipts.push(json!({"case":case,"allowed":allowed,"request_fields":{"query":request.query,"files":request.file_paths,"languages":request.languages,"prefix":request.path_prefix},"lanes":result.lanes,"hits":result.hits,"level":"L2","semantic_or_graph_raw_lane_claim":false}));
    }
    if let Some(path) = std::env::var_os("P7_V05_EVIDENCE_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("local-lanes.json"),
            serde_json::to_vec_pretty(&receipts).unwrap(),
        )
        .unwrap();
    }
}
