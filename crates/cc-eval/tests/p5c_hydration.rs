//! Final evidence checks use real parser/SQLite/source bytes, never a provider oracle.
use cc_model::{
    query::QueryControl,
    retrieval::HardScope,
    search::{SearchHit, SearchRequest},
    CcError, ContextNode, Intent, Language, NodeType, Role,
};
use cc_search::{
    evidence_hydrator::{validate_envelope_generation, EvidenceHydrator},
    SearchEngine,
};
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::{sync::Arc, time::Duration};

fn fixture() -> (
    tempfile::TempDir,
    CodeIndex,
    Arc<cc_db::index_db::IndexDb>,
    Vec<SearchHit>,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "// 中文\r\npub fn needle() -> &'static str { \"α\" }\r\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b.py"),
        "def needle():\n    return 'beta'\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap().clone();
    let engine = SearchEngine::new(db.clone(), &Default::default(), None);
    let hits = engine
        .search(&SearchRequest {
            query: "needle".into(),
            top_k: 10,
            ..Default::default()
        })
        .unwrap()
        .to_vec();
    assert!(hits.iter().any(|h| h.file_path == "a.rs"));
    (dir, index, db, hits)
}
fn control() -> QueryControl {
    QueryControl::new(Duration::from_secs(10)).unwrap()
}

#[test]
fn final_hits_preserve_original_utf8_crlf_proofs_and_metadata() {
    let (dir, _index, db, hits) = fixture();
    let generation = db.reads().read_generation().unwrap();
    let mut hydrator =
        EvidenceHydrator::new(&db, dir.path(), HardScope::default(), generation, control())
            .unwrap();
    let verified = hydrator.hydrate(&hits).unwrap();
    assert_eq!(verified.len(), hits.len());
    for (a, b) in hits.iter().zip(&verified) {
        assert_eq!(a.text, b.text);
        assert_eq!(a.rerank_score, b.rerank_score);
        assert_eq!(a.metadata["document"], b.metadata["document"]);
        assert_eq!(a.metadata["source_evidence"], b.metadata["source_evidence"]);
    }
    assert_eq!(hydrator.diagnostics()["partial"], false);
    hydrator.finish().unwrap();
}

#[test]
fn no_lane_may_substitute_final_identity_coordinates_or_scope() {
    let (dir, _index, db, hits) = fixture();
    let original = hits.iter().find(|h| h.file_path == "a.rs").unwrap().clone();
    for n in 0..8 {
        let mut hit = original.clone();
        match n {
            0 => {
                hit.metadata.as_object_mut().unwrap().remove("document");
            }
            1 => hit.metadata["document"]["doc_version"] = json!("0".repeat(64)),
            2 => hit.metadata["source_evidence"]["span"]["start"] = json!(999999),
            3 => hit.language = Language::Python,
            4 => hit.file_path = "b.py".into(),
            5 => hit.text.push('!'),
            6 => hit.start_line += 1,
            _ => hit.metadata["source_evidence"]["source"]["snapshot_id"] = json!("0".repeat(64)),
        }
        let mut hydrator = EvidenceHydrator::new(
            &db,
            dir.path(),
            HardScope::default(),
            db.reads().read_generation().unwrap(),
            control(),
        )
        .unwrap();
        assert!(
            hydrator.hydrate(&[hit]).is_err(),
            "accepted invalid final evidence case {n}"
        );
    }
    for scope in [
        HardScope {
            file_paths: Some(vec![]),
            ..Default::default()
        },
        HardScope {
            file_paths: Some(vec!["b.py".into()]),
            ..Default::default()
        },
        HardScope {
            languages: Some(vec![Language::Python]),
            ..Default::default()
        },
    ] {
        let mut hydrator = EvidenceHydrator::new(
            &db,
            dir.path(),
            scope,
            db.reads().read_generation().unwrap(),
            control(),
        )
        .unwrap();
        assert!(hydrator.hydrate(std::slice::from_ref(&original)).is_err());
    }
}

#[test]
fn disk_changes_and_deletions_are_explicit_omissions_not_new_source() {
    let (dir, _index, db, hits) = fixture();
    let original = hits.iter().find(|h| h.file_path == "a.rs").unwrap().clone();
    for deleted in [false, true] {
        if deleted {
            std::fs::remove_file(dir.path().join("a.rs")).unwrap();
        } else {
            std::fs::write(dir.path().join("a.rs"), "pub fn changed() {}\n").unwrap();
        }
        let mut hydrator = EvidenceHydrator::new(
            &db,
            dir.path(),
            HardScope::default(),
            db.reads().read_generation().unwrap(),
            control(),
        )
        .unwrap();
        assert!(hydrator
            .hydrate(std::slice::from_ref(&original))
            .unwrap()
            .is_empty());
        assert_eq!(hydrator.diagnostics()["partial"], true);
        assert_eq!(
            hydrator.diagnostics()["omitted_files"]["a.rs"],
            if deleted {
                "deleted"
            } else {
                "stale_with_disk_change"
            }
        );
    }
}

#[test]
fn swap_same_epoch_and_cancel_reject_final_publication() {
    let (dir, _index, db, hits) = fixture();
    let generation = db.reads().read_generation().unwrap();
    let mut hydrator =
        EvidenceHydrator::new(&db, dir.path(), HardScope::default(), generation, control())
            .unwrap();
    hydrator.hydrate(&hits).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "UPDATE metadata SET value=lower(hex(randomblob(16))) WHERE key='index_incarnation'",
        [],
    )
    .unwrap();
    assert!(matches!(
        hydrator.finish(),
        Err(CcError::RetrievalChanged { .. })
    ));
    let ctl = control();
    let mut cancelled = EvidenceHydrator::new(
        &db,
        dir.path(),
        HardScope::default(),
        db.reads().read_generation().unwrap(),
        ctl.clone(),
    )
    .unwrap();
    ctl.cancel();
    assert!(matches!(
        cancelled.hydrate(&hits),
        Err(CcError::QueryCancelled)
    ));
}

#[test]
fn warmed_search_cannot_hide_deleted_manifest_or_forged_snapshot() {
    let (dir, index, db, hits) = fixture();
    index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .unwrap();
    let chosen = hits.iter().find(|h| h.file_path == "a.rs").unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    let mut forged = chosen.clone();
    forged.metadata["source_evidence"]["source"]["snapshot_id"] = json!("f".repeat(64));
    conn.execute(
        "UPDATE chunks SET source_json=? WHERE chunk_id=?",
        rusqlite::params![
            forged.metadata["source_evidence"].to_string(),
            chosen.chunk_id
        ],
    )
    .unwrap();
    let mut hydrator = EvidenceHydrator::new(
        &db,
        dir.path(),
        HardScope::default(),
        db.reads().read_generation().unwrap(),
        control(),
    )
    .unwrap();
    assert!(hydrator.hydrate(&[forged]).is_err());
    conn.execute(
        "DELETE FROM document_manifest WHERE chunk_id=?",
        [&chosen.chunk_id],
    )
    .unwrap();
    assert!(index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .is_err());
}

#[test]
fn warm_context_revalidates_the_full_record_not_only_its_reference_mirror() {
    let (_dir, index, db, _) = fixture();
    let value = index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .unwrap();
    let id = value.machine_pack["hits"][0]["chunk_id"].as_str().unwrap();
    let before = db.reads().read_generation().unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "UPDATE document_manifest SET record_json='{}' WHERE chunk_id=?",
        [id],
    )
    .unwrap();
    assert_eq!(before, db.reads().read_generation().unwrap());
    assert!(index
        .search()
        .search_in_context("needle", 1, Some(Intent::Locate))
        .is_err());
}

#[test]
fn graph_nodes_and_post_handler_metadata_keep_the_same_scope_and_generation() {
    let (dir, index, db, _) = fixture();
    let scope = HardScope {
        file_paths: Some(vec!["a.rs".into()]),
        ..Default::default()
    };
    let mut hydrator = EvidenceHydrator::new(
        &db,
        dir.path(),
        scope,
        db.reads().read_generation().unwrap(),
        control(),
    )
    .unwrap();
    let mut node = ContextNode::new(
        "edge".into(),
        NodeType::CallEdge,
        Role::Neighbor,
        "Caller".into(),
        "relation description".into(),
    );
    node.file_path = Some("b.py".into());
    assert!(hydrator.graph_current(&node).is_err());
    node.file_path = Some("a.rs".into());
    assert!(hydrator.graph_current(&node).unwrap());
    node.span_kind = Some("indexed_chunk".into());
    assert!(
        hydrator.graph_current(&node).is_err(),
        "unproven source slice is not a graph description"
    );
    let value = serde_json::to_value(
        index
            .search()
            .search_in_context("needle", 1, Some(Intent::Locate))
            .unwrap(),
    )
    .unwrap();
    validate_envelope_generation(&db, &value).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO metadata(key,value) VALUES('evidence_epoch','99')",
        [],
    )
    .unwrap();
    assert!(matches!(
        validate_envelope_generation(&db, &value),
        Err(CcError::RetrievalChanged { .. })
    ));
}
