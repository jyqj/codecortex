//! P1-A red/green contracts through public search and real MCP transports.
use cc_model::{search::SearchRequest, Language};
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path, time::Duration};

fn put(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}
fn fixture() -> (tempfile::TempDir, CodeIndex) {
    let dir = tempfile::tempdir().unwrap();
    for path in ["src/api/one.py", "src/other.py", "tests/one.py"] {
        put(dir.path(), path, "def needle():\n    return 7\n");
    }
    put(
        dir.path(),
        "src/native.rs",
        "pub fn needle() -> i32 { 7 }\n",
    );
    put(dir.path(), "noise.py", "def other():\n    return 0\n");
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    (dir, index)
}
fn paths(index: &CodeIndex, query: &str, request: SearchRequest) -> BTreeSet<String> {
    let envelope = index
        .search()
        .search_in_context_with(query, 30, None, request)
        .unwrap();
    envelope.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["file_path"].as_str().unwrap().to_owned())
        .collect()
}
fn expected(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|s| s.to_string()).collect()
}
#[test]
fn p1a_caller_path_and_dsl_are_intersected() {
    let (_dir, index) = fixture();
    let req = SearchRequest {
        path_prefix: Some("src/".into()),
        ..Default::default()
    };
    assert_eq!(
        paths(&index, "needle path:src/api/", req),
        expected(&["src/api/one.py"])
    );
}
#[test]
fn p1a_disjoint_path_constraints_are_empty() {
    let (_dir, index) = fixture();
    assert!(paths(
        &index,
        "needle path:tests/",
        SearchRequest {
            path_prefix: Some("src/".into()),
            ..Default::default()
        }
    )
    .is_empty());
}
#[test]
fn p1a_repeated_dsl_paths_cannot_widen() {
    let (_dir, index) = fixture();
    assert_eq!(
        paths(
            &index,
            "needle path:src/api/ path:src/",
            SearchRequest::default()
        ),
        expected(&["src/api/one.py"])
    );
    assert!(paths(
        &index,
        "needle path:src/ path:tests/",
        SearchRequest::default()
    )
    .is_empty());
}
#[test]
fn p1a_language_parameter_and_dsl_are_intersected() {
    let (_dir, index) = fixture();
    let req = SearchRequest {
        languages: Some(vec![Language::Python, Language::Rust]),
        ..Default::default()
    };
    assert_eq!(
        paths(&index, "needle lang:py", req),
        expected(&["src/api/one.py", "src/other.py", "tests/one.py"])
    );
    assert!(paths(
        &index,
        "needle lang:rust",
        SearchRequest {
            languages: Some(vec![Language::Python]),
            ..Default::default()
        }
    )
    .is_empty());
}
#[test]
fn p1a_repeated_languages_intersect_and_aliases_agree() {
    let (_dir, index) = fixture();
    assert!(paths(
        &index,
        "needle lang:python lang:rust",
        SearchRequest::default()
    )
    .is_empty());
    assert_eq!(
        paths(
            &index,
            "needle lang:py lang:python",
            SearchRequest::default()
        ),
        expected(&["src/api/one.py", "src/other.py", "tests/one.py"])
    );
}
#[test]
fn p1a_unknown_or_empty_language_is_not_unrestricted() {
    let (_dir, index) = fixture();
    for query in ["needle lang:not-a-language", "needle lang:", "needle path:"] {
        let result =
            index
                .search()
                .search_in_context_with(query, 30, None, SearchRequest::default());
        assert!(
            result.is_err(),
            "malformed hard filter must fail explicitly: {query}"
        );
    }
}
#[test]
fn p1a_files_language_and_path_combine_without_widening() {
    let (_dir, index) = fixture();
    let req = SearchRequest {
        file_paths: Some(vec![
            "src/api/one.py".into(),
            "src/native.rs".into(),
            "tests/one.py".into(),
        ]),
        path_prefix: Some("src/".into()),
        ..Default::default()
    };
    assert_eq!(
        paths(&index, "needle lang:python", req),
        expected(&["src/api/one.py"])
    );
}
#[test]
fn p1a_empty_scope_cannot_alias_an_empty_soft_hint_in_cache() {
    let (_dir, index) = fixture();
    let warm = paths(
        &index,
        "needle",
        SearchRequest {
            boost_file_paths: Some(vec![]),
            ..Default::default()
        },
    );
    assert!(!warm.is_empty());
    assert!(
        paths(
            &index,
            "needle",
            SearchRequest {
                file_paths: Some(vec![]),
                ..Default::default()
            }
        )
        .is_empty(),
        "hard empty list must not reuse unrestricted cached result"
    );
    assert!(paths(
        &index,
        "needle",
        SearchRequest {
            languages: Some(vec![]),
            ..Default::default()
        }
    )
    .is_empty());
    assert_eq!(paths(&index, "needle", SearchRequest::default()), warm);
}
#[test]
fn p1a_empty_hard_lists_are_also_empty_in_sql() {
    let (_dir, index) = fixture();
    let db = index.index_db().unwrap();
    for scope in [
        cc_db::ChunkScope {
            file_paths: Some(vec![]),
            ..Default::default()
        },
        cc_db::ChunkScope {
            languages: Some(vec![]),
            ..Default::default()
        },
    ] {
        assert!(db
            .retrieval()
            .fts_chunk_candidates("needle", &scope, 100)
            .unwrap()
            .is_empty());
    }
}
#[test]
fn p1a_soft_hints_do_not_mutate_the_callers_request() {
    let (_dir, index) = fixture();
    let req = SearchRequest {
        pinned_file_paths: Some(vec!["noise.py".into()]),
        file_preselect_limit: Some(1),
        ..Default::default()
    };
    assert!(!paths(&index, "needle", req.clone()).is_empty());
    assert_eq!(req.file_paths, None);
    assert_eq!(req.path_prefix, None);
}
#[test]
fn p1a_graph_evidence_obeys_the_same_hard_scope() {
    let dir = tempfile::tempdir().unwrap();
    put(
        dir.path(),
        "pkg/provider.py",
        "def needle():\n    return 7\n",
    );
    put(
        dir.path(),
        "outside.py",
        "from pkg.provider import needle\n\ndef caller():\n    return needle()\n",
    );
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let env = index
        .search()
        .search_in_context_with("needle path:pkg/", 30, None, SearchRequest::default())
        .unwrap();
    assert!(!env.machine_pack["hits"].as_array().unwrap().is_empty());
    assert!(
        env.nodes
            .iter()
            .all(|n| n.file_path.as_deref().is_none_or(|p| p.starts_with("pkg/"))),
        "graph sidecar escaped scope"
    );
}
fn wire_hits(v: &Value) -> &[Value] {
    v["machine_pack"]["hits"].as_array().unwrap()
}

#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY; execute explicitly for old/new product"]
async fn p1a_stdio_soft_hints_and_path_intersection() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let bin = std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product binary");
    let dir = tempfile::tempdir().unwrap();
    put(
        dir.path(),
        ".codecortex.json",
        "{\"auto_index\":{\"enabled\":false}}",
    );
    put(
        dir.path(),
        "src/api/target.py",
        "def rareSymbol():\n    return 17\n",
    );
    put(dir.path(), "src/noise.py", "def other():\n    return 0\n");
    let mut client = McpStdio::spawn(Path::new(&bin), dir.path(), Duration::from_secs(30))
        .await
        .unwrap();
    client
        .call("index", json!({"path":dir.path(),"full":true}))
        .await
        .unwrap();
    let v=client.call("search",json!({"query":"rareSymbol","mode":"hybrid","top_k":10,"boost_files":["src/noise.py"],"recent_files":["src/noise.py"],"pinned_files":["src/noise.py"],"file_preselect_limit":1})).await.unwrap();
    let target_present = wire_hits(&v)
        .iter()
        .any(|h| h["file_path"] == "src/api/target.py");
    let v2 = client
        .call(
            "search",
            json!({"query":"rareSymbol path:tests/","path_prefix":"src/","top_k":10}),
        )
        .await
        .unwrap();
    client.close().await.unwrap();
    assert!(
        target_present,
        "MCP soft hints removed indexed exact target: {v}"
    );
    assert!(
        wire_hits(&v2).is_empty(),
        "MCP disjoint scopes widened: {v2}"
    );
}
#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY; execute explicitly for old/new product"]
async fn p1a_stdio_bm25_contribution_is_monotonic() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let bin = std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product binary");
    let dir = tempfile::tempdir().unwrap();
    put(
        dir.path(),
        ".codecortex.json",
        "{\"auto_index\":{\"enabled\":false}}",
    );
    let strong = "src/alpha/alpha/a.py";
    let weak = "src/alpha/noise/b.py";
    for p in [strong, weak] {
        put(dir.path(), p, "def probe():\n    return 'alpha'\n");
    }
    for i in 0..12 {
        put(
            dir.path(),
            &format!("src/other{i}.py"),
            "def probe():\n    return 'unrelated'\n",
        );
    }
    let mut client = McpStdio::spawn(Path::new(&bin), dir.path(), Duration::from_secs(30))
        .await
        .unwrap();
    client
        .call("index", json!({"path":dir.path(),"full":true}))
        .await
        .unwrap();
    let v = client
        .call("search", json!({"query":"alpha","top_k":30}))
        .await
        .unwrap();
    let c = rusqlite::Connection::open_with_flags(
        dir.path().join(".codecortex/index.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut st=c.prepare("SELECT file_path,bm25(files_fts,1.8,1.0) FROM files_fts WHERE files_fts MATCH 'alpha' ORDER BY 2").unwrap();
    let raw: Vec<(String, f64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let raw_score = |p: &str| raw.iter().find(|(path, _)| path == p).unwrap().1;
    assert!(
        raw_score(strong) < raw_score(weak),
        "fixture must have stronger raw BM25: {raw:?}"
    );
    let contribution = |p: &str| {
        let h = wire_hits(&v).iter().find(|h| h["file_path"] == p).unwrap();
        h["metadata"]["stage_a_layer_scores"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry[0] == "fts-summary")
            .unwrap()[1]
            .as_f64()
            .unwrap()
    };
    let good = contribution(strong);
    let poor = contribution(weak);
    println!("P1A_MCP_BM25 raw={raw:?}, stronger={good}, weaker={poor}");
    client.close().await.unwrap();
    assert!(
        good >= poor,
        "stronger FTS match must not get lower MCP contribution"
    );
}
