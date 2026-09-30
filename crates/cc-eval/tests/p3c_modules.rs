//! P3-C independent module/package truth through the real parser and SQLite.
use cc_eval::benchmark::oracle;
use cc_server::engine::CodeIndex;
use std::path::Path;
fn put(root: &Path, file: &str, text: &str) {
    let p = root.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1,"dirty_propagation_max_files":1}}"#,
    );
    d
}
#[test]
fn local_go_replace_resolves_members_across_files_and_retargets_on_config_only() {
    let d = setup();
    let root = d.path();
    put(root,"go.mod","module example.com/app\ngo 1.22\nrequire example.com/api v1.0.0\nreplace example.com/api => ./one\n");
    for dir in ["one", "two"] {
        put(
            root,
            &format!("{dir}/go.mod"),
            "module example.com/api\ngo 1.22\n",
        );
        put(
            root,
            &format!("{dir}/a.go"),
            "package service\nfunc Ping() int { return 1 }\n",
        );
        put(
            root,
            &format!("{dir}/b.go"),
            "package service\nfunc Pong() int { return 2 }\n",
        );
    }
    put(root,"main.go","package main\nimport \"example.com/api\"\nfunc Entry() int { return service.Ping() + service.Pong() }\n");
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let assert_paths = |dir: &str| {
        let facts = oracle::canonical(root).unwrap();
        for name in ["Ping", "Pong"] {
            assert!(
                facts["call_edges"]
                    .iter()
                    .any(|v| v["file_path"] == "main.go"
                        && v["callee_symbol"]
                            .as_str()
                            .is_some_and(|s| s.ends_with(name))
                        && v["target_file_path"]
                            .as_str()
                            .is_some_and(|p| p.starts_with(dir))),
                "{name}: {:?}",
                facts["call_edges"]
            );
        }
    };
    assert_paths("one/");
    put(root,"go.mod","module example.com/app\ngo 1.22\nrequire example.com/api v1.0.0\nreplace example.com/api => ./two\n");
    for _ in 0..8 {
        let r = index.build_index(false).unwrap();
        if r.resolution_freshness.complete {
            break;
        }
    }
    assert_paths("two/");
    let before = oracle::canonical(root).unwrap();
    drop(index);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    assert_eq!(before, oracle::canonical(root).unwrap());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; actual stdio subprocess"]
async fn p3c_real_mcp_go_work_only_change_resumes_across_restart() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let d = setup();
    let root = d.path();
    put(
        root,
        "app/go.mod",
        "module example.com/app\ngo 1.22\nrequire example.com/api v1.0.0\n",
    );
    for name in ["one", "two"] {
        put(
            root,
            &format!("{name}/go.mod"),
            "module example.com/api\ngo 1.22\n",
        );
        put(
            root,
            &format!("{name}/api.go"),
            "package api\nfunc Ping() int {return 1}\n",
        );
    }
    for name in ["a", "b", "c"] {
        put(
            root,
            &format!("app/{name}/use.go"),
            "package app\nimport \"example.com/api\"\nfunc Run() int {return api.Ping()}\n",
        );
    }
    let work =
        |target: &str| format!("go 1.22\nuse ./app\nreplace example.com/api => ./{target}\n");
    put(root, "go.work", &work("one"));
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
        .await
        .unwrap();
    m.call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    put(root, "go.work", &work("two"));
    let mut receipts = vec![];
    let mut done = false;
    for step in 0..12 {
        if step == 1 {
            m.close().await.unwrap();
            m = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
                .await
                .unwrap();
        }
        let r = m
            .call(
                "index",
                json!({"path":root,"full":false,"changed_paths":["go.work"]}),
            )
            .await
            .unwrap();
        assert_eq!(r["files_parsed"], 0);
        done = r["resolution_freshness"]["complete"] == true;
        let graph=m.call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE b.file_path = 'two/api.go' RETURN a.file_path AS caller LIMIT 20"})).await.unwrap();
        let n = graph["results"].as_array().unwrap().len();
        assert!(n <= 3);
        if step == 0 {
            assert!(!done);
        }
        receipts.push(json!({"report":r,"graph":graph}));
        if done {
            assert_eq!(n, 3);
            break;
        }
    }
    assert!(done, "Go work-only frontier did not converge");
    m.close().await.unwrap();
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            Path::new(&path).join("p3c-real-mcp.json"),
            serde_json::to_vec_pretty(&receipts).unwrap(),
        )
        .unwrap();
    }
    let old = oracle::canonical(root).unwrap();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    assert_eq!(old, oracle::canonical(root).unwrap());
}

#[test]
fn go_selector_and_reference_cannot_bind_same_file_leaf() {
    let d = setup();
    put(d.path(), "go.mod", "module example.com/app\ngo 1.22\n");
    put(
        d.path(),
        "api/a.go",
        "package api\nfunc Ping() int{return 1}\n",
    );
    put(d.path(),"use.go","package app\nimport \"example.com/app/api\"\nfunc Ping() int{return 2}\nfunc Entry() int{return api.Ping()}\n");
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    let f = oracle::canonical(d.path()).unwrap();
    let e = f["call_edges"]
        .iter()
        .find(|e| e["file_path"] == "use.go")
        .unwrap();
    assert_eq!(e["target_file_path"], "api/a.go");
    assert_eq!(
        e["call_kind"], "imported",
        "Go exported functions are not constructors"
    );
    let r = f["symbol_refs"]
        .iter()
        .find(|r| r["ref_id"] == e["callee_ref_id"])
        .unwrap();
    assert_eq!(r["target_file_path"], "api/a.go");
}
#[test]
fn local_go_config_dependency_is_scoped_and_rebinds_on_condition_change() {
    let d = setup();
    for m in ["first", "second"] {
        put(
            d.path(),
            &format!("{m}/go.mod"),
            &format!("module example.com/{m}\ngo 1.22\n"),
        );
        put(
            d.path(),
            &format!("{m}/api/a.go"),
            "package api\nfunc Ping() int{return 1}\n",
        );
        put(d.path(),&format!("{m}/use.go"),&format!("package app\nimport \"example.com/{m}/api\"\nfunc Entry() int{{return api.Ping()}}\n"));
    }
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    let before = oracle::canonical(d.path()).unwrap();
    assert!(!before["resolution_dependencies"]
        .iter()
        .any(|r| r["file_path"] == "first/use.go"
            && r["kind"] == "module_config"
            && r["key"] == "*"));
    put(
        d.path(),
        "second/go.mod",
        "module example.com/renamed\ngo 1.22\n",
    );
    for _ in 0..8 {
        let r = i.build_index(false).unwrap();
        if r.resolution_freshness.complete {
            break;
        }
    }
    let after = oracle::canonical(d.path()).unwrap();
    let pick = |f: &std::collections::BTreeMap<String, Vec<serde_json::Value>>| {
        f["resolution_manifests"]
            .iter()
            .find(|v| v["file_path"] == "first/use.go")
            .unwrap()
            .clone()
    };
    assert_eq!(pick(&before), pick(&after));
    put(
        d.path(),
        "first/api/a.go",
        "//go:build custom\n\npackage api\nfunc Ping() int{return 1}\n",
    );
    for _ in 0..8 {
        let r = i.build_index(false).unwrap();
        if r.resolution_freshness.complete {
            break;
        }
    }
    let f = oracle::canonical(d.path()).unwrap();
    assert!(f["call_edges"]
        .iter()
        .filter(|e| e["file_path"] == "first/use.go")
        .all(|e| e["target_file_path"].is_null()));
    i.build_index(true).unwrap();
    assert_eq!(f, oracle::canonical(d.path()).unwrap());
}

#[test]
fn external_go_import_cannot_bind_unrelated_global_function() {
    let d = setup();
    put(d.path(), "go.mod", "module example.com/app\ngo 1.22\n");
    put(d.path(),"main.go","package main\nimport remote \"example.com/absent\"\nfunc Entry() int { return remote.Ping() }\n");
    put(
        d.path(),
        "other/other.go",
        "package other\nfunc Ping() int {return 1}\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    let facts = oracle::canonical(d.path()).unwrap();
    assert!(!facts["call_edges"]
        .iter()
        .any(|e| e["file_path"] == "main.go" && !e["target_file_path"].is_null()));
    let payload = facts["resolution_manifests"]
        .iter()
        .find(|e| e["file_path"] == "main.go")
        .unwrap()["payload"]
        .as_str()
        .unwrap();
    let m: serde_json::Value = serde_json::from_str(payload).unwrap();
    assert_eq!(m["modules"][0]["status"], "external");
}
