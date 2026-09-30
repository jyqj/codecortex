//! Actual parser/SQLite and public-process source tests, with independently authored bytes.
use cc_eval::benchmark::oracle;
use cc_model::source::ChunkSource;
use cc_server::engine::CodeIndex;
use rusqlite::Connection;
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text.as_bytes()).unwrap();
}
fn chunks(root: &Path, path: &str, expected: &str) -> Vec<String> {
    let db = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let mut statement=db.prepare("SELECT text,text_encoding,source_json,start_line,end_line FROM chunks WHERE file_path=? ORDER BY chunk_index").unwrap();
    let rows = statement
        .query_map([path], |row| {
            Ok((
                cc_db::index_db::read_chunk_text_with_encoding(row, 0, 1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, u32>(4)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let mut position = 0;
    let mut receipts = vec![];
    for (text, json, first, last) in rows {
        let source: ChunkSource = serde_json::from_str(&json).unwrap();
        assert!(source.validate(&text));
        assert_eq!(source.span.start, position);
        assert_eq!(&expected[source.span.start..source.span.end], text);
        assert_eq!(
            source.source.content_digest,
            blake3::hash(expected.as_bytes()).to_hex().as_str()
        );
        assert!(first > 0 && last >= first);
        position = source.span.end;
        receipts.push(json);
    }
    assert_eq!(position, expected.len());
    receipts
}
#[test]
fn original_chunks_survive_compression_dirty_reload_reopen_and_full_rebuild() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false}}"#,
    );
    let source =
        "from provider import value\r\n\r\ndef entry(x):\r\n    return value(x)  # 中文\r\n";
    let compressed = format!(
        "def compressible():\r\n{}",
        "    text = 'repeated payload with original CRLF and 中文'\r\n".repeat(40)
    );
    put(root, "compressed.py", &compressed);
    put(root, "consumer.py", source);
    put(root, "provider.py", "def value(x):\n    return x\n");
    let mut index = CodeIndex::new(Some(root)).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.parse_errors.is_empty());
    let before = chunks(root, "consumer.py", source);
    chunks(root, "compressed.py", &compressed);
    let db = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    assert!(
        db.query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path='compressed.py' AND text_encoding='zstd'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap()
            > 0
    );
    drop(db);
    put(
        root,
        "provider.py",
        "def value(x, extra=1):\n    return x + extra\n",
    );
    index.build_index(false).unwrap();
    assert_eq!(chunks(root, "consumer.py", source), before);
    index.close();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(false).unwrap();
    let incremental = oracle::canonical(root).unwrap();
    index.build_index(true).unwrap();
    assert_eq!(oracle::canonical(root).unwrap(), incremental);
    assert_eq!(chunks(root, "consumer.py", source), before);
    let changed = source.replace("entry(x)", "entry(x, mode=1)");
    put(root, "consumer.py", &changed);
    index.build_index(false).unwrap();
    let after = chunks(root, "consumer.py", &changed);
    assert_ne!(after, before);
    let incremental = oracle::canonical(root).unwrap();
    index.build_index(true).unwrap();
    assert_eq!(oracle::canonical(root).unwrap(), incremental);
}
#[test]
fn source_evidence_and_text_agree_for_all_languages_and_sfc() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let fixtures = [
        (
            "a.ts",
            "/** 文档 */\r\nexport function marker() { return 42; }\r\n",
        ),
        ("lib.rs", "/// 文档\r\npub fn marker() -> i32 { 42 }\r\n"),
        ("a.py", "def marker():\r\n    return 42\r\n"),
        ("a.go", "package app\r\nfunc Marker() int { return 42 }\r\n"),
        (
            "View.vue",
            "<template>你好</template>\r\n<script>function marker(){return 42;}</script>\r\n",
        ),
    ];
    for (path, text) in fixtures {
        put(root, path, text);
    }
    let mut index = CodeIndex::new(Some(root)).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.parse_errors.is_empty(), "{:?}", report.parse_errors);
    for (path, text) in fixtures {
        chunks(root, path, text);
    }
}
#[test]
fn byte_evidence_checks_raw_coordinates_and_rejects_tampering_without_legacy_fallback() {
    use cc_eval::benchmark::normalizer;
    use cc_model::source::{ByteSpan, SourceSnapshot};
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let text = "prefix 甲; value();\r\nnext\r\n";
    put(root.path(), "a.ts", text);
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let start = text.find("value()").unwrap();
    let end = text.find("next").unwrap();
    let proof = ChunkSource {
        source: snapshot.identity().clone(),
        span: ByteSpan { start, end },
        slice_digest: snapshot.slice_digest(ByteSpan { start, end }).unwrap(),
        boundary: "statement".into(),
        owner: None,
        signature: None,
    };
    let node = json!({"file_path":"a.ts","symbol_name":"value","start_line":1,"end_line":1,"text":&text[start..end],"metadata":{"source_evidence":proof}});
    let check = |node: serde_json::Value| {
        let (mut hits, _) = normalizer::mcp(&json!({"machine_pack":{"hits":[node]}})).unwrap();
        assert!(hits[0].source_evidence.is_some());
        normalizer::verify_source(&mut hits[0], root.path()).unwrap();
        hits.remove(0)
    };
    let valid = check(node.clone());
    assert_eq!(valid.evidence_valid, Some(true));
    assert_eq!(valid.span.unwrap().end, end as u64);
    for field in ["snapshot_id", "content_digest"] {
        let mut bad = node.clone();
        bad["metadata"]["source_evidence"]["source"][field] = json!("0".repeat(64));
        assert_eq!(check(bad).evidence_valid, Some(false));
    }
    for (field, value) in [("start", start + 1), ("end", text.len() + 1)] {
        let mut bad = node.clone();
        bad["metadata"]["source_evidence"]["span"][field] = json!(value);
        assert_eq!(check(bad).evidence_valid, Some(false));
    }
    let mut bad = node.clone();
    bad["end_line"] = json!(2);
    assert_eq!(check(bad).evidence_valid, Some(false));
    let mut bad = node.clone();
    bad["text"] = json!("value();");
    assert_eq!(check(bad).evidence_valid, Some(false));
    let mut bad = node.clone();
    bad["metadata"]["source_evidence"] = json!({"invalid":true});
    assert_eq!(check(bad).evidence_valid, Some(false));
    put(root.path(), "a.ts", &text.replace("next", "NEXT"));
    assert_eq!(check(node).evidence_valid, Some(false));
}
#[test]
fn legacy_normalized_line_proof_remains_separate_and_strict() {
    use cc_eval::benchmark::{normalizer, schema::Hit};
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "a.ts", "a\r\nb\r\n");
    let mut hit = Hit {
        path: "a.ts".into(),
        start_line: Some(1),
        end_line: Some(2),
        text: Some("a\nb".into()),
        ..Default::default()
    };
    normalizer::verify_source(&mut hit, root.path()).unwrap();
    assert_eq!(hit.evidence_valid, Some(true));
    hit.text = Some("b".into());
    normalizer::verify_source(&mut hit, root.path()).unwrap();
    assert_eq!(hit.evidence_valid, Some(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit product binary; actual MCP subprocess and its isolated SQLite"]
async fn real_mcp_indexes_original_bytes_and_source_coordinates() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source =
        "/** Exact original source. */\r\nexport function sourceMarker() { return '甲'; }\r\n";
    put(root, "marker.ts", source);
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut mcp = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
        .await
        .unwrap();
    let report = mcp
        .call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    let evidence = chunks(root, "marker.ts", source);
    let query=mcp.call("graph_query",json!({"query":"MATCH (s:Symbol) WHERE s.name = 'sourceMarker' RETURN s.file_path AS file LIMIT 10"})).await.unwrap();
    assert!(query["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["file"] == "marker.ts"));
    let search = mcp
        .call(
            "search",
            json!({"query":"sourceMarker","mode":"hybrid","top_k":5}),
        )
        .await
        .unwrap();
    let (mut hits, _) = cc_eval::benchmark::normalizer::mcp(&search).unwrap();
    assert!(!hits.is_empty(), "{search}");
    for hit in &mut hits {
        assert!(
            hit.source_evidence.is_some(),
            "public source evidence missing: {search}"
        );
        cc_eval::benchmark::normalizer::verify_source(hit, root).unwrap();
        assert_eq!(
            hit.evidence_valid,
            Some(true),
            "public bytes failed: {hit:?}"
        );
    }
    mcp.close().await.unwrap();
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(Path::new(&path).join("p4a-mcp.json"),serde_json::to_vec_pretty(&json!({"index":report,"graph":query,"search":search,"verified_hits":hits,"chunk_source":evidence,"scope":"real public index/graph/search with raw-byte proof validation plus separate read-only SQLite checks"})).unwrap()).unwrap();
    }
}
