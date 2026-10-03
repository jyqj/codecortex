//! Independent public identity review; caches and transactions are fixture-owned.
use cc_model::source::{ChunkSource, SourceSnapshot};
use cc_server::engine::CodeIndex;
fn fixture(text: &str) -> (tempfile::TempDir, CodeIndex) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("micro.py"), text).unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    (root, index)
}
#[test]
fn decorated_class_identity_is_proved_without_losing_members() {
    let text = "@decorate\nclass Outer:\n    @decorate\n    class Inner:\n        def pulse(self): return 1\n";
    let (_root, index) = fixture(text);
    let source = SourceSnapshot::new(text.as_bytes());
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    for (query, qname, kind) in [
        ("Outer", "Outer", "class"),
        ("Inner", "Outer.Inner", "class"),
        ("pulse", "Outer.Inner.pulse", "method"),
    ] {
        let output = index.search().search_in_context(query, 20, None).unwrap();
        let hits = output.machine_pack["hits"].as_array().unwrap();
        let classes: Vec<_> = hits.iter().filter(|h| h["symbol_name"] == query).collect();
        assert!(!classes.is_empty());
        for hit in classes {
            assert_eq!(hit["symbol_kind"], kind);
            assert_eq!(hit["metadata"]["qname"], qname);
            let proof: ChunkSource =
                serde_json::from_value(hit["metadata"]["source_evidence"].clone()).unwrap();
            assert_eq!(proof.source, *source.identity());
            assert_eq!(
                source.slice(proof.span).unwrap(),
                hit["text"].as_str().unwrap()
            );
            let start = source.point(proof.owner.unwrap().start).unwrap();
            let end = source.point(proof.owner.unwrap().end).unwrap();
            let count: i64 = conn.query_row("SELECT count(*) FROM symbols WHERE file_path='micro.py' AND start_line=?1 AND start_col=?2 AND end_line=?3 AND end_col=?4 AND qname=?5 AND kind=?6", rusqlite::params![start.0 as u32,start.1 as u32,end.0 as u32,end.1 as u32,qname,kind], |r| r.get(0)).unwrap();
            assert_eq!(count, 1);
            let persisted: i64 = conn
                .query_row(
                    "SELECT count(*) FROM chunk_symbol_identity WHERE chunk_id=?1 AND json_extract(record_json, '$.qname')=?2",
                    rusqlite::params![hit["chunk_id"].as_str().unwrap(), qname],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(persisted, 1);
        }
    }
}
#[test]
fn split_method_local_function_and_replaced_same_name_have_exact_sql_survivors() {
    let mut text = String::from("# leading 中文\r\nclass Cedar:\r\n    @decorate\r\n    async def pulse(self):\r\n        def leaf():\r\n            return 'β'\r\n        total = 0\r\n");
    for i in 0..260 {
        text.push_str(&format!("        total += {i}\r\n"));
    }
    text.push_str("        return total + leaf()\r\nclass Willow:\r\n    def pulse(self): return '中'\r\ndef duplicate(): return 'first'\r\ndef duplicate(): return 'second'\r\n");
    let (_root, index) = fixture(&text);
    let source = SourceSnapshot::new(text.as_bytes());
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    let mut cedar = 0;
    for query in ["pulse", "leaf", "duplicate"] {
        let output = index.search().search_in_context(query, 30, None).unwrap();
        let hits = output.machine_pack["hits"].as_array().unwrap();
        assert!(hits.iter().any(|h| h["symbol_name"] == query));
        for h in hits.iter().filter(|h| h["symbol_name"] == query) {
            let p: ChunkSource =
                serde_json::from_value(h["metadata"]["source_evidence"].clone()).unwrap();
            assert_eq!(p.source, *source.identity());
            assert_eq!(source.slice(p.span).unwrap(), h["text"].as_str().unwrap());
            if query == "duplicate" && h["text"].as_str().unwrap().contains("'first'") {
                assert!(h["metadata"].get("qname").is_none());
                continue;
            }
            let q = h["metadata"]["qname"]
                .as_str()
                .expect("proved surviving declaration");
            if q == "Cedar.pulse" {
                cedar += 1;
            }
            if query == "leaf" {
                assert_eq!(q, "Cedar.pulse.leaf");
                assert_eq!(h["symbol_kind"], "function");
            }
            let start = source.point(p.owner.unwrap().start).unwrap();
            let end = source.point(p.owner.unwrap().end).unwrap();
            let count: i64 = conn.query_row("SELECT count(*) FROM symbols WHERE file_path='micro.py' AND start_line=?1 AND start_col=?2 AND end_line=?3 AND end_col=?4 AND qname=?5",rusqlite::params![start.0 as u32,start.1 as u32,end.0 as u32,end.1 as u32,q], |r| r.get(0)).unwrap();
            assert_eq!(count, 1);
            let persisted: i64 = conn
                .query_row(
                    "SELECT count(*) FROM chunk_symbol_identity WHERE chunk_id=?1",
                    [h["chunk_id"].as_str().unwrap()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(persisted, 1);
        }
    }
    assert!(cedar > 1, "actual split body must be tested");
}
