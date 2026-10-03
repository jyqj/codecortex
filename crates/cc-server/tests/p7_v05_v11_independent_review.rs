//! Independent finite V05/V11 subset; literal gold, real source reads and stdio.
use cc_model::{search::SearchRequest, CcError};
use cc_search::SearchEngine;
use cc_server::engine::CodeIndex;
use rmcp::{
    model::CallToolRequestParams,
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    process::Stdio,
    sync::mpsc,
    time::{Duration, Instant},
};

fn trace(label: &str, value: Value) {
    if let Some(root) = std::env::var_os("P7_SCOPE_REVIEW_EVIDENCE_DIR") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(format!("{}-{label}.json", std::process::id())),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    } else {
        println!("REVIEW_JSON {label} {value}");
    }
}
fn put(root: &std::path::Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn index_fixture() -> (tempfile::TempDir, CodeIndex) {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "needle.py", "def needle():\n    return 731\n");
    put(
        root.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    );
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    (root, index)
}
fn source(engine: &SearchEngine) -> String {
    let hits = engine
        .search(&SearchRequest {
            query: "needle".into(),
            top_k: 20,
            ..Default::default()
        })
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.file_path == "needle.py"));
    hits.iter()
        .map(|h| h.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn real_source_stages_straddling_writer_commit_are_discarded_then_recovered() {
    let (root, mut index) = index_fixture();
    let db = index.index_db().unwrap().clone();
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let old = source(&engine);
    assert!(old.contains("731"));
    let (trigger, commands) = mpsc::sync_channel::<usize>(0);
    let (committed, commits) = mpsc::sync_channel(0);
    let file = root.path().join("needle.py");
    let writer = std::thread::spawn(move || {
        while let Ok(version) = commands.recv() {
            std::fs::write(&file, format!("def needle():\n    return {version}\n")).unwrap();
            index.build_index(false).unwrap();
            committed
                .send(index.index_db().unwrap().reads().read_generation().unwrap())
                .unwrap();
        }
    });
    let mut attempts = 0;
    let mut observations = Vec::new();
    let (accepted,(first,last))=engine.with_stable_generation(|before|{
  attempts+=1;let first=source(&engine);
  if attempts==1 {trigger.send(947).unwrap();let committed=commits.recv_timeout(Duration::from_secs(2)).unwrap();assert_ne!(before,committed);}
  let last=source(&engine);observations.push(json!({"attempt":attempts,"before":before,"after":db.reads().read_generation()?,"first":first,"last":last}));Ok((first,last))
 }).unwrap();
    assert_eq!(attempts, 2);
    assert!(observations[0]["first"].as_str().unwrap().contains("731"));
    assert!(observations[0]["last"].as_str().unwrap().contains("947"));
    assert!(first.contains("947") && last.contains("947"));
    assert!(!first.contains("731") && !last.contains("731"));
    assert_eq!(accepted, db.reads().read_generation().unwrap());
    drop(trigger);
    writer.join().unwrap();
    trace(
        "staged-source-generation",
        json!({"attempts":attempts,"accepted":accepted,"observations":observations,"one_reader":true,"level":"L2","public_internal_stage_hook":false}),
    );
}

#[test]
fn real_mixed_source_reads_exhaust_at_three_without_an_envelope() {
    let (root, mut index) = index_fixture();
    let db = index.index_db().unwrap().clone();
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let mut attempts = 0;
    let mut observations = Vec::new();
    let start = Instant::now();
    let result = engine.with_stable_generation(|before| {
        attempts += 1;
        let first = source(&engine);
        put(
            root.path(),
            "needle.py",
            &format!("def needle():\n    return {}\n", 947 + attempts),
        );
        index.build_index(false)?;
        let last = source(&engine);
        assert_ne!(first, last);
        assert_ne!(before, db.reads().read_generation()?);
        observations.push(json!({"attempt":attempts,"first":first,"last":last}));
        Ok((first, last))
    });
    assert!(matches!(
        result,
        Err(CcError::RetrievalChanged { attempts: 3 })
    ));
    assert_eq!(attempts, 3);
    assert!(start.elapsed() < Duration::from_secs(3));
    trace(
        "mixed-source-exhaustion",
        json!({"attempts":attempts,"observations":observations,"elapsed_ms":start.elapsed().as_millis(),"result":"RetrievalChanged(3)","level":"L2"}),
    );
}

#[test]
fn nested_real_generation_fences_have_a_finite_nine_work_attempt_bound() {
    let (root, mut index) = index_fixture();
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let mut outer_attempts = 0;
    let mut inner_attempts = 0;
    let mut observations = Vec::new();
    let start = Instant::now();
    let result = engine.with_stable_generation(|_| {
        outer_attempts += 1;
        engine.with_stable_generation(|_| {
            inner_attempts += 1;
            let first = source(&engine);
            put(root.path(), "needle.py", &format!("def needle():\n    return {}\n", 947 + inner_attempts));
            index.build_index(false)?;
            let last = source(&engine);
            assert_ne!(first, last);
            observations.push(json!({"outer":outer_attempts,"inner_total":inner_attempts,"first":first,"last":last}));
            Ok((first, last))
        })
    });
    assert!(matches!(
        result,
        Err(CcError::RetrievalChanged { attempts: 3 })
    ));
    assert_eq!(outer_attempts, 3);
    assert_eq!(inner_attempts, 9);
    assert!(start.elapsed() < Duration::from_secs(3));
    trace(
        "nested-finite-bound",
        json!({"outer_attempts":outer_attempts,"inner_attempts":inner_attempts,"elapsed_ms":start.elapsed().as_millis(),"observations":observations,"level":"L2","composed_fence_entry":true,"public_stage_hook":false,"error_attempts_is_per_fence_not_global":true}),
    );
}

#[test]
fn changed_generation_control_errors_short_circuit_and_stable_errors_survive() {
    let mut records = Vec::new();
    for kind in [
        "cancel",
        "timeout",
        "invalidated",
        "database",
        "search",
        "busy",
    ] {
        let (root, mut index) = index_fixture();
        let db = index.index_db().unwrap().clone();
        let engine = SearchEngine::new(db, &Default::default(), None);
        let mut attempts = 0;
        let result: cc_model::CcResult<(_, ())> = engine.with_stable_generation(|_| {
            attempts += 1;
            if attempts == 1 {
                put(root.path(), "needle.py", "def needle():\n    return 947\n");
                index.build_index(false)?;
            }
            Err(match kind {
                "cancel" => CcError::QueryCancelled,
                "timeout" => CcError::QueryTimedOut,
                "invalidated" => CcError::QueryInvalidated,
                "database" => CcError::Database("independent stable corruption".into()),
                "search" => CcError::Search("independent stable failure".into()),
                "busy" => CcError::QueryBusy,
                _ => unreachable!(),
            })
        });
        let expected = if matches!(kind, "cancel" | "timeout" | "invalidated") {
            1
        } else {
            2
        };
        assert_eq!(attempts, expected);
        let error = result.unwrap_err();
        assert!(match kind {
            "cancel" => matches!(error, CcError::QueryCancelled),
            "timeout" => matches!(error, CcError::QueryTimedOut),
            "invalidated" => matches!(error, CcError::QueryInvalidated),
            "database" =>
                matches!(error,CcError::Database(ref s) if s=="independent stable corruption"),
            "search" => matches!(error,CcError::Search(ref s) if s=="independent stable failure"),
            "busy" => matches!(error, CcError::QueryBusy),
            _ => false,
        });
        records.push(json!({"kind":kind,"attempts":attempts,"error":error.to_string(),"mixed_first_noncontrol_error_discarded":expected==2}));
    }
    trace("error-preservation", json!(records));
}

struct Session {
    root: tempfile::TempDir,
    client: RunningService<RoleClient, ()>,
    records: Vec<Value>,
}
impl Session {
    async fn open(docs: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        for (p, s) in docs {
            put(&project, p, s);
        }
        put(
            &project,
            ".codecortex.json",
            r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":{"strategy":"local"}}"#,
        );
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex")).configure(|cmd| {
                for (key, _) in std::env::vars_os() {
                    if key.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(key);
                    }
                }
                cmd.arg("mcp")
                    .arg("--project-path")
                    .arg(&project)
                    .current_dir(&project)
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .env("CODECORTEX_SEMANTIC_CACHE_ROOT", root.path().join("cache"))
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
        let mut session = Self {
            root,
            client,
            records: Vec::new(),
        };
        session
            .call("index", json!({"path":project,"full":true}))
            .await;
        session
    }
    async fn call(&mut self, tool: &str, args: Value) -> Value {
        let r = tokio::time::timeout(
            Duration::from_secs(10),
            self.client.call_tool(
                CallToolRequestParams::new(tool.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(r.is_error, Some(true), "{r:?}");
        let result = r.structured_content.unwrap()["result"].clone();
        self.records
            .push(json!({"tool":tool,"args":args,"result":result}));
        result
    }
}
const DOCS: [(&str, &str); 5] = [
    ("scope/a.py", "def needle():\n    return 11\n"),
    ("scope/sub/b.py", "def needle():\n    return 22\n"),
    ("scope/a.rs", "pub fn needle() -> u32 { 33 }\n"),
    ("scope_extra/a.py", "def needle():\n    return 44\n"),
    ("outside/a.py", "def needle():\n    return 55\n"),
];
fn paths(value: &Value) -> BTreeSet<&str> {
    value["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["file_path"].as_str().unwrap())
        .collect()
}
fn gold_contains_all(value: &Value, gold: &[&str]) {
    assert_eq!(
        paths(value),
        gold.iter().copied().collect(),
        "literal domain completeness, not just nonempty subset"
    );
    for hit in value["machine_pack"]["hits"].as_array().unwrap() {
        let p = hit["file_path"].as_str().unwrap();
        let expected = DOCS.iter().find(|(path, _)| *path == p).unwrap().1;
        let text = hit["text"].as_str().unwrap();
        assert!(!text.is_empty() && expected.contains(text));
        let span = &hit["metadata"]["source_evidence"]["span"];
        let start = span["start"].as_u64().unwrap() as usize;
        let end = span["end"].as_u64().unwrap() as usize;
        assert!(start < end && end <= expected.len());
        assert_eq!(
            text.as_bytes(),
            &expected.as_bytes()[start..end],
            "source must equal the independently known bytes at its declared offsets"
        );
    }
    for node in value["nodes"].as_array().unwrap() {
        if let Some(path) = node["file_path"].as_str() {
            assert!(
                gold.contains(&path),
                "packed node escaped the literal domain"
            );
        }
    }
    for span in value["spans"].as_array().unwrap() {
        assert!(gold.contains(&span["file_path"].as_str().unwrap()));
    }
}
#[tokio::test]
async fn public_local_dsl_returns_complete_literal_domain_and_warm_cache_stays_scoped() {
    let mut session = Session::open(&DOCS).await;
    let cases: [(&str, &[&str]); 7] = [
        (
            "needle path:scope/ lang:python",
            &["scope/a.py", "scope/sub/b.py"],
        ),
        (
            "needle lang:py path:scope/",
            &["scope/a.py", "scope/sub/b.py"],
        ),
        ("needle path:scope/ lang:rust", &["scope/a.rs"]),
        (
            "needle path:scope/sub/ path:scope/ lang:python",
            &["scope/sub/b.py"],
        ),
        ("needle path:scope/ path:outside/", &[]),
        ("needle lang:python lang:rust", &[]),
        ("needle path:missing/", &[]),
    ];
    for tool in ["search", "context"] {
        for (query, gold) in cases {
            let key = if tool == "search" { "query" } else { "task" };
            let mut args = json!({key:query,"retrieval_strategy":"local"});
            if tool == "search" {
                args["top_k"] = json!(30);
                args["boost_files"] = json!(["outside/a.py"]);
                args["file_preselect_limit"] = json!(1);
            } else {
                args["max_symbols"] = json!(30);
            }
            for _ in 0..2 {
                let value = session.call(tool, args.clone()).await;
                gold_contains_all(&value, gold);
            }
        }
    }
    trace(
        "public-complete-dsl",
        json!({"literal_docs":DOCS,"calls":session.records,"case_count":28,"provider_enabled":false,"complete_path_gold":true,"level":"L3"}),
    );
    session.client.cancel().await.unwrap();
}
#[tokio::test]
async fn public_bm25_contribution_matches_independent_sql_order_and_is_nonzero() {
    let strong = "src/alpha/alpha/a.py";
    let weak = "src/alpha/noise/b.py";
    let mut docs = vec![
        (
            strong.to_string(),
            "def probe():\n    return 'alpha'\n".to_string(),
        ),
        (
            weak.to_string(),
            "def probe():\n    return 'alpha'\n".to_string(),
        ),
    ];
    for n in 0..12 {
        docs.push((
            format!("src/other{n}.py"),
            "def probe():\n    return 'unrelated'\n".into(),
        ));
    }
    let refs: Vec<_> = docs.iter().map(|(p, s)| (p.as_str(), s.as_str())).collect();
    let mut session = Session::open(&refs).await;
    let value = session
        .call(
            "search",
            json!({"query":"alpha","top_k":30,"retrieval_strategy":"local"}),
        )
        .await;
    let connection = rusqlite::Connection::open_with_flags(
        session
            .root
            .path()
            .join("project/.codecortex/index.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut st=connection.prepare("SELECT file_path,bm25(files_fts,1.8,1.0) FROM files_fts WHERE files_fts MATCH 'alpha' ORDER BY 2").unwrap();
    let raw: Vec<(String, f64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let raw_score = |p: &str| raw.iter().find(|(path, _)| path == p).unwrap().1;
    assert!(raw_score(strong) < raw_score(weak));
    let contribution = |p: &str| {
        value["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["file_path"] == p)
            .unwrap()["metadata"]["stage_a_layer_scores"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v[0] == "fts-summary")
            .unwrap()[1]
            .as_f64()
            .unwrap()
    };
    let better = contribution(strong);
    let worse = contribution(weak);
    assert!(better.is_finite() && worse.is_finite() && worse>0.0 && better>worse,"stronger measured raw score must have observable strictly larger positive contribution in this unsaturated fixture");
    trace(
        "public-bm25-oracle",
        json!({"raw_sql":raw,"stronger":better,"weaker":worse,"calls":session.records,"provider_enabled":false,"final_rerank_order_claim":false}),
    );
    drop(st);
    drop(connection);
    session.client.cancel().await.unwrap();
}
