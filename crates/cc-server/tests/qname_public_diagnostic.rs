//! Real v24 public-output baseline, to be replaced by identity-positive tests
//! after the proposed schema/association design is reviewed.
use cc_model::source::{ChunkSource, SourceSnapshot};
use cc_server::engine::CodeIndex;

#[test]
fn v24_public_hit_omits_qname_despite_exact_indexed_parser_symbol() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let text = "class Alpha:\n    def needle(self):\n        return '中文'\nclass Beta:\n    def needle(self):\n        return 'β'\n";
    std::fs::write(root.path().join("a.py"), text).unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let envelope = index
        .search()
        .search_in_context("needle", 10, None)
        .unwrap();
    let output = serde_json::to_value(&envelope).unwrap();
    if let Ok(destination) = std::env::var("CODECORTEX_QNAME_DIAGNOSTIC_OUT") {
        std::fs::write(destination, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }
    let hits = output["machine_pack"]["hits"]
        .as_array()
        .expect("public hits");
    assert!(!hits.is_empty());
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    let source = SourceSnapshot::new(text.as_bytes());
    let mut names = Vec::new();
    for hit in hits {
        if hit["symbol_name"] != "needle" {
            continue;
        }
        assert!(hit["metadata"].get("qname").is_none());
        let proof: ChunkSource =
            serde_json::from_value(hit["metadata"]["source_evidence"].clone()).unwrap();
        assert_eq!(proof.source, *source.identity());
        assert_eq!(
            source.slice(proof.span).unwrap(),
            hit["text"].as_str().unwrap()
        );
        let start = source.point(proof.owner.unwrap().start).unwrap();
        let end = source.point(proof.owner.unwrap().end).unwrap();
        let mut stmt = conn.prepare("SELECT qname FROM symbols WHERE file_path='a.py' AND start_line=?1 AND start_col=?2 AND end_line=?3 AND end_col=?4").unwrap();
        let matches: Vec<String> = stmt
            .query_map(
                rusqlite::params![start.0 as u32, start.1 as u32, end.0 as u32, end.1 as u32],
                |r| r.get(0),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(matches.len(), 1);
        names.push(matches[0].clone());
    }
    names.sort();
    assert_eq!(names, ["Alpha.needle", "Beta.needle"]);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        24
    );
}
