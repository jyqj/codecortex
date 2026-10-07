//! Actual parser/SQLite work at final assembly, including cached retrieval.
use cc_db::{document_store::verify_source_records_with_work, index_db::IndexDb};
use cc_model::{
    query::QueryControl,
    retrieval::HardScope,
    retrieval_cost::SqlWork,
    search::{SearchHit, SearchRequest},
    Intent,
};
use cc_search::{evidence_hydrator::EvidenceHydrator, SearchEngine};
use cc_server::engine::CodeIndex;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

fn fixture() -> (tempfile::TempDir, CodeIndex, Arc<IndexDb>, Vec<SearchHit>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    for (file, name) in [("a.py", "needle"), ("b.py", "other")] {
        std::fs::write(
            dir.path().join(file),
            format!("def {name}():\n    return 'auditmarker'\n"),
        )
        .unwrap();
    }
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap().clone();
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let hits = engine
        .search(&SearchRequest {
            query: "auditmarker".into(),
            top_k: 10,
            include_grep: false,
            ..Default::default()
        })
        .unwrap()
        .to_vec();
    let actual: std::collections::BTreeMap<_, _> = hits
        .iter()
        .map(|hit| (hit.file_path.as_str(), hit.metadata["qname"].as_str()))
        .collect();
    assert_eq!(
        actual,
        std::collections::BTreeMap::from([("a.py", Some("needle")), ("b.py", Some("other"))])
    );
    assert_eq!(hits.len(), 2);
    (dir, index, db, hits)
}

fn hydrator<'a>(db: &'a IndexDb, root: &'a std::path::Path) -> EvidenceHydrator<'a> {
    EvidenceHydrator::new(
        db,
        root,
        HardScope::default(),
        db.reads().read_generation().unwrap(),
        QueryControl::new(Duration::from_secs(10)).unwrap(),
    )
    .unwrap()
}

fn sql(value: &Value, field: &str) -> SqlWork {
    serde_json::from_value(value[field].clone()).unwrap()
}

fn assert_work(work: SqlWork, statements: usize, rows: usize) {
    assert_eq!(work.statements, statements);
    assert_eq!(work.rows, rows);
    assert!(work.vm_steps.unwrap() > 0);
}

#[test]
fn current_context_validation_is_charged_again_on_a_real_graph_cache_hit() {
    let (_dir, index, _db, _hits) = fixture();
    let handle = index.query_handle().unwrap();
    let before = index.diagnostics_info()["search_cache"].clone();
    let cold = handle
        .search_in_context("auditmarker", 10, Some(Intent::Locate))
        .unwrap();
    let middle = index.diagnostics_info()["search_cache"].clone();
    let warm = handle
        .search_in_context("auditmarker", 10, Some(Intent::Locate))
        .unwrap();
    let after = index.diagnostics_info()["search_cache"].clone();
    assert_eq!(
        middle["graph_misses"].as_u64().unwrap(),
        before["graph_misses"].as_u64().unwrap() + 1
    );
    assert_eq!(
        after["graph_hits"].as_u64().unwrap(),
        middle["graph_hits"].as_u64().unwrap() + 1
    );
    assert_eq!(after["graph_misses"], middle["graph_misses"]);
    let cold_work = &cold.evidence_summary["source_freshness"]["validation_work"];
    let warm_work = &warm.evidence_summary["source_freshness"]["validation_work"];
    // One manifest SELECT + four identity SELECTs per real declaration;
    // final projection is one separate SELECT yielding both chunks.
    assert_work(sql(cold_work, "source_records_sql"), 9, 10);
    assert_work(sql(cold_work, "candidate_projection_sql"), 1, 2);
    assert_eq!(cold_work, warm_work);
    assert_eq!(
        cold.evidence_summary["retrieval"]["cost"],
        warm.evidence_summary["retrieval"]["cost"]
    );
    assert_eq!(cold.machine_pack["hits"], warm.machine_pack["hits"]);
}

#[test]
fn repeated_validation_resets_cached_statements_and_accumulates_only_named_work() {
    let (dir, _index, db, hits) = fixture();
    let mut validator = hydrator(&db, dir.path());
    validator.hydrate(&hits).unwrap();
    let first = validator.diagnostics()["validation_work"].clone();
    validator.hydrate(&hits).unwrap();
    let second = validator.diagnostics()["validation_work"].clone();
    for field in ["source_records_sql", "candidate_projection_sql"] {
        let mut expected = sql(&first, field);
        expected.merge(expected);
        assert_eq!(sql(&second, field), expected, "{field}");
    }
    validator.hydrate(&[]).unwrap();
    assert_eq!(validator.diagnostics()["validation_work"], second);
    let mut fresh = hydrator(&db, dir.path());
    fresh.hydrate(&hits).unwrap();
    assert_eq!(fresh.diagnostics()["validation_work"], first);
}

#[test]
fn empty_and_prequery_refusal_are_zero_but_missing_locators_are_measured() {
    let (dir, _index, db, hits) = fixture();
    let mut validator = hydrator(&db, dir.path());
    assert!(validator.hydrate(&[]).unwrap().is_empty());
    assert!(validator
        .hydrate(&[hits[0].clone(), hits[0].clone()])
        .is_err());
    assert!(validator.hydrate(&vec![hits[0].clone(); 4097]).is_err());
    let value = validator.diagnostics()["validation_work"].clone();
    assert_eq!(sql(&value, "source_records_sql"), SqlWork::default());
    assert_eq!(sql(&value, "candidate_projection_sql"), SqlWork::default());
    let mut work = SqlWork::default();
    assert!(db
        .retrieval()
        .chunk_candidate_rows_by_ids_with_work(&[], &mut work)
        .unwrap()
        .is_empty());
    assert_eq!(work, SqlWork::default());

    let mut missing = hits[0].clone();
    missing.chunk_id = "missing-fixture-chunk".into();
    assert!(validator.hydrate(&[missing]).is_err());
    let value = validator.diagnostics()["validation_work"].clone();
    assert_work(sql(&value, "source_records_sql"), 1, 0);
    assert_eq!(sql(&value, "candidate_projection_sql"), SqlWork::default());
}

#[test]
fn failed_source_record_checks_keep_the_work_performed_before_rejection() {
    let (_dir, _index, db, hits) = fixture();
    let hit = &hits[0];
    let mut bad = hit.clone();
    bad.metadata["qname"] = Value::String("forged".into());
    let mut work = SqlWork::default();
    assert!(verify_source_records_with_work(&db, &[bad], &mut work).is_err());
    assert_work(work, 5, 5);

    let writer = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    // A malformed SQL value is yielded, then rejected by the identity reader.
    writer
        .execute(
            "UPDATE chunk_symbol_identity SET record_json=x'80' WHERE chunk_id=?1",
            [&hit.chunk_id],
        )
        .unwrap();
    let mut work = SqlWork::default();
    assert!(verify_source_records_with_work(&db, std::slice::from_ref(hit), &mut work).is_err());
    assert_work(work, 2, 2);

    writer
        .execute(
            "DELETE FROM chunk_symbol_identity WHERE chunk_id=?1",
            [&hit.chunk_id],
        )
        .unwrap();
    let mut work = SqlWork::default();
    assert!(verify_source_records_with_work(&db, std::slice::from_ref(hit), &mut work).is_err());
    assert_work(work, 2, 1); // The empty identity probe still executes.

    writer
        .execute(
            "UPDATE document_manifest SET record_json='{}' WHERE chunk_id=?1",
            [&hit.chunk_id],
        )
        .unwrap();
    let mut work = SqlWork::default();
    assert!(verify_source_records_with_work(&db, std::slice::from_ref(hit), &mut work).is_err());
    assert_work(work, 1, 1); // No identity read follows a rejected manifest.
}

#[test]
fn projection_failure_is_visible_and_multiple_batches_count_actual_yielded_rows() {
    let (dir, _index, db, hits) = fixture();
    let hit = &hits[0];
    let mut work = SqlWork::default();
    let ids = vec![hit.chunk_id.as_str(); 201]; // Existing IN batch size is 200.
    let rows = db
        .retrieval()
        .chunk_candidate_rows_by_ids_with_work(&ids, &mut work)
        .unwrap();
    assert_eq!(rows.len(), 2); // Duplicate IDs within one IN are one SQL row.
    assert_work(work, 2, 2);

    let writer = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    writer
        .execute(
            "UPDATE document_manifest SET reference_json='{}' WHERE chunk_id=?1",
            [&hit.chunk_id],
        )
        .unwrap();
    let mut validator = hydrator(&db, dir.path());
    assert!(validator.hydrate(std::slice::from_ref(hit)).is_err());
    let value = validator.diagnostics()["validation_work"].clone();
    assert_work(sql(&value, "source_records_sql"), 5, 5);
    assert_work(sql(&value, "candidate_projection_sql"), 1, 1);
}
