use cc_db::index_db::IndexDb;
use cc_index::Indexer;
use cc_model::{config::IndexingConfig, Language};
use cc_parsers::{go::GoParser, traits::FileParser};
use std::{collections::HashSet, sync::Arc};

fn check(name: &str, source: &str, expected: &[String], old_duplicates: Option<usize>) {
    let old = std::env::var_os("INDEPENDENT_OLD").is_some();
    let parser = GoParser::new();
    let out = parser.parse(name, source, Language::Go).unwrap();
    let again = GoParser::new().parse(name, source, Language::Go).unwrap();
    assert_eq!(
        serde_json::to_value(&out).unwrap(),
        serde_json::to_value(&again).unwrap()
    );
    let mut actual: Vec<_> = out
        .call_edges
        .iter()
        .map(|c| c.callee_symbol.clone())
        .collect();
    actual.sort();
    let mut expected = expected.to_vec();
    expected.sort();
    assert_eq!(actual, expected, "lost/extra calls in {name}");
    // Compare every semantic field and non-call record to PR92; only call identity/span fields may change.
    let mut normalized = serde_json::to_value(&out).unwrap();
    normalized["independent_direct_call_identity"] = serde_json::to_value(
        out.call_edges
            .iter()
            .filter(|c| c.receiver_expr.is_none())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for (key, fields) in [
        (
            "call_edges",
            vec![
                "edge_id",
                "callee_ref_id",
                "line",
                "start_col",
                "end_line",
                "end_col",
            ],
        ),
        (
            "symbol_refs",
            vec!["ref_id", "line", "column", "ref_end_line", "ref_end_col"],
        ),
    ] {
        for record in normalized[key].as_array_mut().unwrap() {
            if key == "call_edges" || record["ref_kind"] == "call" {
                for field in &fields {
                    record.as_object_mut().unwrap().remove(*field);
                }
            }
        }
    }
    let oracle = std::path::PathBuf::from(std::env::var("INDEPENDENT_EVIDENCE").unwrap())
        .join(format!("{name}.base-semantics.json"));
    if old {
        std::fs::write(&oracle, serde_json::to_vec_pretty(&normalized).unwrap()).unwrap();
    } else {
        let baseline: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&oracle).unwrap()).unwrap();
        assert_eq!(
            normalized, baseline,
            "semantic changes beyond call identity/span"
        );
    }
    let ids: HashSet<_> = out.call_edges.iter().map(|c| &c.edge_id).collect();
    let call_refs: Vec<_> = out
        .symbol_refs
        .iter()
        .filter(|r| r.ref_kind == "call")
        .collect();
    assert_eq!(call_refs.len(), actual.len(), "lost call references");
    let ref_ids: HashSet<_> = call_refs.iter().map(|r| &r.ref_id).collect();
    let mut groups = std::collections::HashMap::new();
    for c in &out.call_edges {
        *groups.entry(&c.edge_id).or_insert(0usize) += 1;
    }
    let duplicates = groups.values().filter(|&&n| n > 1).count();
    if old {
        if let Some(n) = old_duplicates {
            assert_eq!(duplicates, n);
        }
    } else {
        assert_eq!(ids.len(), actual.len(), "duplicate call IDs");
        assert_eq!(ref_ids.len(), actual.len(), "duplicate ref IDs");
        for c in &out.call_edges {
            let lines: Vec<_> = source.split('\n').collect();
            assert_eq!(c.end_line, Some(c.line));
            let token =
                &lines[c.line as usize - 1].as_bytes()[c.start_col as usize..c.end_col as usize];
            assert_eq!(token, c.callee_symbol.as_bytes(), "wrong span {c:?}");
            let matching: Vec<_> = call_refs
                .iter()
                .filter(|r| Some(&r.ref_id) == c.callee_ref_id.as_ref())
                .collect();
            assert_eq!(matching.len(), 1);
            let r = matching[0];
            assert_eq!(
                (
                    &r.symbol_name,
                    r.line,
                    r.column,
                    r.ref_end_line,
                    r.ref_end_col
                ),
                (
                    &c.callee_symbol,
                    c.line,
                    c.start_col,
                    c.end_line,
                    Some(c.end_col)
                )
            );
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(name), source).unwrap();
    let dbpath = dir.path().join("index.sqlite3");
    let (db, _) = IndexDb::open(&dbpath).unwrap();
    let config = IndexingConfig::default();
    let indexer = Indexer::new(Arc::new(db), dir.path(), &config);
    let result = indexer.build_index(dir.path(), true);
    if old && duplicates > 0 {
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("conflicting duplicate call site")
                || error.contains("conflicting duplicate symbol_ref site"),
            "{error}"
        );
    } else {
        result.unwrap();
        let conn = rusqlite::Connection::open(&dbpath).unwrap();
        let calls: i64 = conn
            .query_row("SELECT COUNT(*) FROM call_edges", [], |r| r.get(0))
            .unwrap();
        let refs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbol_refs WHERE ref_kind='call'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(calls, actual.len() as i64, "index dropped calls");
        assert_eq!(refs, actual.len() as i64, "index dropped refs");
        let dangling: i64 = conn.query_row("SELECT COUNT(*) FROM call_edges c LEFT JOIN symbol_refs r ON c.callee_ref_id=r.ref_id WHERE r.ref_id IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(dangling, 0, "index lost call/ref links");
        indexer.build_index(dir.path(), true).unwrap();
        let repeat_calls: i64 = conn
            .query_row("SELECT COUNT(*) FROM call_edges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(repeat_calls, calls);
    }
    println!(
        "EVIDENCE {}",
        serde_json::json!({"case":name,"calls":actual.len(),"call_refs":call_refs.len(),"all_refs":out.symbol_refs.len(),"duplicate_groups":duplicates,"unique_calls":ids.len(),"unique_call_refs":ref_ids.len(),"stable_reparse":true,"exact_spans_checked":!old,"mode":if old {"base"} else {"fixed"}})
    );
}
fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}
#[test]
fn deep_nested_same_start() {
    let mut expr = "obj".to_string();
    for _ in 0..128 {
        expr.push_str(".Step()");
    }
    check(
        "deep.go",
        &format!("package p\nfunc f() {{ {expr} }}\n"),
        &vec!["Step".into(); 128],
        Some(1),
    );
}
#[test]
fn arguments_method_indexed_generic_controls() {
    check("controls.go", "package p\nfunc f() { a.B(C()).B(D()); arr[I()].Run().Run(); Box[int]{}.Build().Build(); pkg.Make[int]().Next().Next(); r.Method() }\n", &names(&["B","B","C","D","I","Run","Run","Build","Build","Make","Next","Next","Method"]), None);
}
#[test]
fn unicode_multiline_and_direct_targets() {
    check(
        "unicode.go",
        "package p\nfunc Leaf() {}\nfunc f() { 对象.方法().\n 方法(); Leaf(); Leaf(); x.Leaf() }\n",
        &names(&["方法", "方法", "Leaf", "Leaf", "Leaf"]),
        None,
    );
    let src = "package p\nfunc Leaf() {}\nfunc f() { Leaf(); x.Leaf() }\n";
    let out = GoParser::new()
        .parse("target.go", src, Language::Go)
        .unwrap();
    assert!(out
        .call_edges
        .iter()
        .find(|c| c.receiver_expr.is_none())
        .unwrap()
        .target_symbol_id
        .is_some());
    assert!(out
        .call_edges
        .iter()
        .find(|c| c.receiver_expr.is_some())
        .unwrap()
        .target_symbol_id
        .is_none());
}
#[test]
fn locked_gin_context_preserves_256_calls() {
    let src = std::fs::read_to_string(std::env::var("INDEPENDENT_GIN").unwrap()).unwrap();
    let oracle_path = std::env::var("INDEPENDENT_NAMES").unwrap();
    let expected: Vec<String> = if std::env::var_os("INDEPENDENT_OLD").is_some() {
        let out = GoParser::new()
            .parse("context.go", &src, Language::Go)
            .unwrap();
        let names: Vec<_> = out
            .call_edges
            .iter()
            .map(|c| c.callee_symbol.clone())
            .collect();
        std::fs::write(&oracle_path, serde_json::to_vec_pretty(&names).unwrap()).unwrap();
        names
    } else {
        serde_json::from_slice(&std::fs::read(&oracle_path).unwrap()).unwrap()
    };
    assert_eq!(expected.len(), 256);
    check("context.go", &src, &expected, Some(13));
}
