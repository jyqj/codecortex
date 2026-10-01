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
    let lanes = raw["evidence_summary"]["retrieval"]
        .get("lane_receipts")
        .or_else(|| raw["evidence_summary"]["retrieval"].get("lanes"))
        .unwrap()
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

#[test]
fn absent_basename_and_natural_language_paths_do_not_borrow_exact_doc_completeness() {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    for (name, query) in [
        ("missing", "src/missing.rs"),
        ("basename", "a.rs"),
        ("natural_language", "fix src/a.rs implementation and tests"),
        ("noncanonical", "./src/a.rs"),
    ] {
        let raw = backend
            .call_tool("search", &json!({"query":query,"top_k":24}))
            .unwrap();
        preserve(name, &raw);
        let path = lane(&raw);
        assert!(
            path["coverage"]["total_lower_bound"].as_u64().unwrap()
                >= path["candidate_count"].as_u64().unwrap()
        );
        if name != "basename" {
            assert_eq!(path["status"],"partial","broad unresolved/prose/noncanonical token domain must retain honest existing truncation: {name} {path}");
            assert_eq!(path["truncation_reason"], "path_token_limit");
        }
        if name == "missing" {
            assert!(!raw["machine_pack"]["hits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["file_path"] == "src/missing.rs"));
        }
    }
    let scoped = backend
        .call_tool(
            "search",
            &json!({"query":"src/a.rs","top_k":24,"path_prefix":"outside"}),
        )
        .unwrap();
    preserve("outside-hard-scope", &scoped);
    source_proof(&scoped, root.path());
    assert!(scoped["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["file_path"].as_str().unwrap().starts_with("outside/")));
    assert!(!scoped["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["file_path"] == "src/a.rs" || h["file_path"] == "src/ab.rs"));
}

#[test]
fn exact_document_domain_edit_delete_and_stale_source_do_not_reuse_old_bodies() {
    let root = fixture();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    let before = backend
        .call_tool("search", &json!({"query":"src/a.rs","top_k":24}))
        .unwrap();
    preserve("lifecycle-before", &before);
    std::fs::write(
        root.path().join("src/a.rs"),
        "pub fn needle() -> i32 { 777 }\n",
    )
    .unwrap();
    let stale = backend
        .call_tool("search", &json!({"query":"src/a.rs","top_k":24}))
        .unwrap();
    preserve("stale-before-reindex", &stale);
    assert_eq!(
        normalizer::mcp(&stale).unwrap().1,
        cc_eval::benchmark::schema::ResultStatus::Partial
    );
    assert_eq!(
        stale["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    assert_eq!(
        stale["evidence_summary"]["source_freshness"]["omitted_files"]["src/a.rs"],
        "stale_with_disk_change"
    );
    assert!(
        !stale["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["file_path"] == "src/a.rs"
                && h["text"].as_str().is_some_and(|s| s.contains("{ 7 }"))),
        "stale indexed doc cannot fabricate valid old source body"
    );
    let rebuild = backend
        .build_index_report_scoped(&["src/a.rs".into()])
        .unwrap();
    preserve("scoped-edit-index", &rebuild);
    let after = backend
        .call_tool("search", &json!({"query":"src/a.rs","top_k":24}))
        .unwrap();
    preserve("lifecycle-after-edit", &after);
    source_proof(&after, root.path());
    assert!(after["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["file_path"] == "src/a.rs" && h["text"].as_str().unwrap().contains("{ 777 }")));
    assert_eq!(lane(&after)["status"], "complete");
    std::fs::remove_file(root.path().join("src/a.rs")).unwrap();
    let rebuild = backend
        .build_index_report_scoped(&["src/a.rs".into()])
        .unwrap();
    preserve("scoped-delete-index", &rebuild);
    let removed = backend
        .call_tool("search", &json!({"query":"src/a.rs","top_k":24}))
        .unwrap();
    preserve("lifecycle-after-delete", &removed);
    assert!(!removed["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["file_path"] == "src/a.rs"));
    assert_eq!(lane(&removed)["status"],"partial","no exact current doc restores broad token fallback;do not fake complete missing-file domain");
}

#[test]
fn large_exact_file_is_still_truthfully_candidate_limited() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("scope")).unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"search":{"graph_weight":0.0}}"#,
    )
    .unwrap();
    let mut source = String::new();
    for n in 0..80 {
        source.push_str(&format!("pub fn bounded_part_{n}(mut x:i32)->i32 {{\n"));
        for _ in 0..100 {
            source.push_str("    x += 1;\n");
        }
        source.push_str("    x\n}\n");
    }
    std::fs::write(root.path().join("scope/large.rs"), source).unwrap();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    let raw = backend
        .call_tool("search", &json!({"query":"scope/large.rs","top_k":1}))
        .unwrap();
    preserve("largefile-cap", &raw);
    source_proof(&raw, root.path());
    let p = lane(&raw);
    assert_eq!(p["status"], "partial");
    assert_eq!(p["truncation_reason"], "candidate_limit");
    assert!(
        p["coverage"]["total_lower_bound"].as_u64().unwrap()
            > p["candidate_count"].as_u64().unwrap()
    );
    assert!(raw["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["file_path"] == "scope/large.rs"));
}

#[test]
fn actual_core_language_file_scope_and_markdown_identity_are_preserved() {
    use cc_model::{config::ProjectConfig, search::SearchRequest, Language};
    use cc_search::SearchEngine;
    use cc_server::engine::CodeIndex;
    let root = tempfile::tempdir().unwrap();
    for (path, body) in [
        ("src/a.rs", "pub fn needle() -> i32 { 7 }\n"),
        ("src/ab.rs", "pub fn neighbor() -> i32 { 81 }\n"),
        ("src/a.py", "def needle():\n    return 99\n"),
        (
            "docs/Guide.md",
            "# Guide\nCanonical document kind is retained.\n",
        ),
    ] {
        let p = root.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    let config = ProjectConfig::default();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let engine = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
    let mut req = SearchRequest {
        query: "src/a.rs".into(),
        top_k: 24,
        languages: Some(vec![Language::Python]),
        include_grep: false,
        ..Default::default()
    };
    let result = engine.search_with_diagnostics(&req).unwrap();
    core_source_proof(&result.hits, root.path());
    preserve(
        "core-language-negative",
        &json!({"hits":result.hits,"lanes":result.lanes,"scope":result.scope,"cost":result.cost}),
    );
    assert!(!result.hits.is_empty());
    assert!(result
        .hits
        .iter()
        .any(|h| h.file_path == "src/a.py" && h.text.contains("def needle():\n    return 99")));
    assert!(result.hits.iter().all(|h| h.language == Language::Python));
    let path = result.lanes.iter().find(|l| l.lane_id == "path").unwrap();
    assert_eq!(path.status, cc_model::retrieval::LaneStatus::Complete);
    assert!(path.candidate_count > 0);
    assert!(
        path.candidates.iter().all(|c| !c.exact_identity),
        "language-excluded exactdoc must use scoped fallback,not exactDoc widening"
    );
    assert!(!result.hits.iter().any(|h| h.file_path == "src/a.rs"));
    req.languages = None;
    req.file_paths = Some(vec!["src/ab.rs".into()]);
    let result = engine.search_with_diagnostics(&req).unwrap();
    core_source_proof(&result.hits, root.path());
    preserve(
        "core-file-scope-negative",
        &json!({"hits":result.hits,"lanes":result.lanes,"scope":result.scope,"cost":result.cost}),
    );
    assert!(!result.hits.is_empty());
    assert!(result
        .hits
        .iter()
        .any(|h| h.file_path == "src/ab.rs" && h.text.contains("pub fn neighbor() -> i32 { 81 }")));
    assert!(result.hits.iter().all(|h| h.file_path == "src/ab.rs"));
    let path = result.lanes.iter().find(|l| l.lane_id == "path").unwrap();
    assert_eq!(path.status, cc_model::retrieval::LaneStatus::Complete);
    assert!(path.candidate_count > 0);
    assert!(
        path.candidates.iter().all(|c| !c.exact_identity),
        "file_scope excluded target uses real scoped fallback"
    );
    req.file_paths = None;
    req.query = "docs/Guide.md".into();
    let result = engine.search_with_diagnostics(&req).unwrap();
    core_source_proof(&result.hits, root.path());
    preserve(
        "core-markdown-kind",
        &json!({"hits":result.hits,"lanes":result.lanes,"scope":result.scope,"cost":result.cost}),
    );
    let hit = result
        .hits
        .iter()
        .find(|h| h.file_path == "docs/Guide.md")
        .unwrap();
    assert_eq!(hit.language, Language::Markdown);
    assert!(hit.text.contains("# Guide"));
    assert!(
        hit.metadata.get("document").is_some(),
        "indexed document metadata retained,not invented docKind filter"
    );
    // Current schema has source_chunk for BOTH Markdown/code, not a DocKind filter.
    let db = index.index_db().unwrap();
    let connection = rusqlite::Connection::open_with_flags(
        db.admin().db_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let stored: String = connection
        .query_row(
            "SELECT record_json FROM document_manifest WHERE doc_key=?",
            [hit.metadata["document"]["doc_key"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let record: cc_model::identity::DocumentRecord = serde_json::from_str(&stored).unwrap();
    assert_eq!(record.kind, "source_chunk");
    record.validate(&hit.text).unwrap();
    assert!(cc_db::document_store::is_current(db, &record.reference).unwrap());
    preserve(
        "actual-markdown-document-record",
        &serde_json::to_value(&record).unwrap(),
    );
    req.path_prefix = Some("Docs".into());
    assert!(
        engine
            .search_with_diagnostics(&req)
            .unwrap()
            .hits
            .is_empty(),
        "case-sensitive scope cannot be widened by exact path"
    );
}

fn core_source_proof(hits: &[cc_model::search::SearchHit], root: &Path) {
    for actual in hits {
        let evidence = actual
            .metadata
            .get("source_evidence")
            .cloned()
            .filter(|v| !v.is_null());
        assert!(evidence.is_some(), "actual core source proof required");
        // Faithful adapter of ACTUAL core output, not an invented ranking/selection fixture.
        let mut hit = cc_eval::benchmark::schema::Hit {
            path: actual.file_path.clone(),
            source_evidence: evidence,
            start_line: Some(actual.start_line),
            end_line: Some(actual.end_line),
            text: Some(actual.text.clone()),
            ..Default::default()
        };
        normalizer::verify_source(&mut hit, root).unwrap();
        assert_eq!(hit.evidence_valid, Some(true));
        assert!(hit.span.is_some());
    }
}

#[test]
fn real_path_lane_sql_tripwire_proves_exact_branch_does_not_scan_token_fts() {
    use cc_model::{config::ProjectConfig, retrieval::LaneStatus, search::SearchRequest};
    use cc_search::SearchEngine;
    use cc_server::engine::CodeIndex;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/a.rs"),
        "pub fn needle() -> i32 { 7 }\n",
    )
    .unwrap();
    let config = json!({"auto_index":{"enabled":false},"search":{"lexical_weight":0.0,"exact_symbol_weight":0.0,"graph_weight":0.0,"grep_weight":0.0,"path_weight":1.0}});
    std::fs::write(root.path().join(".codecortex.json"), config.to_string()).unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap().clone();
    let conf: ProjectConfig = serde_json::from_value(config).unwrap();
    let engine = SearchEngine::new(db.clone(), &conf, None);
    // Fault confined to this one owned temporary SQLite index, never a production flag.
    let connection = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    connection
        .execute_batch("DROP TABLE file_paths_fts")
        .unwrap();
    let mut req = SearchRequest {
        query: "src/a.rs".into(),
        top_k: 24,
        include_grep: false,
        ..Default::default()
    };
    let result = engine.search_with_diagnostics(&req).unwrap();
    preserve(
        "sql-tripwire-exact-positive",
        &json!({"hits":result.hits,"lanes":result.lanes,"scope":result.scope,"cost":result.cost}),
    );
    let path = result.lanes.iter().find(|l| l.lane_id == "path").unwrap();
    assert_eq!(path.status, LaneStatus::Complete);
    assert_eq!(path.candidate_count, 1);
    assert!(result
        .hits
        .iter()
        .any(|h| h.file_path == "src/a.rs" && h.text.contains("{ 7 }")));
    core_source_proof(&result.hits, root.path());
    req.query = "src/missing.rs".into();
    match engine.search_with_diagnostics(&req) {
        Err(cc_model::CcError::Database(message)) => {
            assert!(message.contains("no such table: file_paths_fts"));
            preserve("sql-tripwire-fallback-negative",&json!({"actual_error_type":"Database","actual_error":message,"query":req.query,"not_NoMatch":true}));
        }
        Err(other) => panic!("unexpected non-database failure: {other}"),
        Ok(_) => panic!("missing canonical doc must execute tokenfts and preserve actual Database error,not Success/NoMatch"),
    }
    preserve(
        "sql-tripwire-scope",
        &json!({"claim":"exact PathLane branch does not access dropped tokenfts;missingdocfallback really fails on it","not_claimed":"wholequery/preselection has zero generic work;lexical_work is not a path SQL meter"}),
    );
}
