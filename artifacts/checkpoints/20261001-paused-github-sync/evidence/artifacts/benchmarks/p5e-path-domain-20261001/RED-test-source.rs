//! True indexed thousand-file hotspot through public MCP, never a hand-written DTO.
use cc_eval::{benchmark::normalizer, runner::CodeIndexBackend};
use serde_json::{json, Value};
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    for (path, body) in [
        ("src/a.rs", "pub fn needle() -> i32 { 7 }\n"),
        ("src/ab.rs", "pub fn prefix_neighbor() -> i32 { 81 }\n"),
        (
            "outside/src/a.rs",
            "pub fn forbidden_neighbor() -> i32 { 999999 }\n",
        ),
    ] {
        let p = root.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    for n in 0..997 {
        std::fs::write(
            root.path().join(format!("src/corpus_{n:04}.rs")),
            format!("pub fn corpus_{n:04}() -> usize {{ {n} }}\n"),
        )
        .unwrap();
    }
    root
}
fn preserve(case: &str, value: &Value) {
    if let Ok(dir) = std::env::var("P5E_PATH_DOMAIN_EVIDENCE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{case}.json"));
        assert!(!p.exists(), "immutable path evidence");
        std::fs::write(p, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }
}
fn source_proof(raw: &Value, root: &Path) {
    let (mut hits, _) = normalizer::mcp(raw).unwrap();
    assert!(!hits.is_empty());
    for hit in &mut hits {
        normalizer::verify_source(hit, root).unwrap();
        assert_eq!(hit.evidence_valid, Some(true));
        assert!(hit.span.as_ref().is_some_and(|s| s.end > s.start));
    }
}
fn lane(raw: &Value) -> &Value {
    let lanes = raw["evidence_summary"]["retrieval"]["lane_receipts"]
        .as_array()
        .unwrap();
    let path: Vec<_> = lanes.iter().filter(|l| l["lane_id"] == "path").collect();
    assert_eq!(path.len(), 1);
    path[0]
}
#[test]
fn canonical_exact_indexed_path_avoids_unrelated_thousand_file_token_inventory() {
    let root = fixture();
    let backend = CodeIndexBackend::new_unindexed(root.path()).unwrap();
    let build = backend.build_index_report(true).unwrap();
    preserve("initial-build", &build);
    assert_eq!(build["files_scanned"], 1000);
    assert_eq!(build["files_parsed"], 1000);
    assert_eq!(build["files_skipped"], 0);
    assert_eq!(build["document_changes"]["files_projected"], 1000);
    let raw = backend
        .call_tool(
            "search",
            &json!({"query":"src/a.rs","top_k":24,"mode":"hybrid"}),
        )
        .unwrap();
    preserve("canonical-thousand-files-before-assert", &raw);
    source_proof(&raw, root.path());
    assert!(raw["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["file_path"] == "src/a.rs"
            && h["text"]
                .as_str()
                .unwrap()
                .contains("pub fn needle() -> i32 { 7 }")));
    let path = lane(&raw);
    assert_eq!(path["status"],"complete","exact current indexed doc must restrict PathLane domain,not silently claim broad token inventory complete: {path}");
    assert_eq!(path["coverage"]["complete"], true);
    assert_eq!(path["candidate_count"], 1);
    assert_eq!(path["coverage"]["total_lower_bound"], 1);
    assert!(path["truncation_reason"].is_null());
}
