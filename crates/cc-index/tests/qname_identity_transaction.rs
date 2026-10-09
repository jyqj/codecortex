use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;

fn unit(text: &str) -> FileWriteUnit {
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let mut outcome = ParserRegistry::new()
        .parse("a.py", text, Language::Python)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    outcome.documents =
        Some(cc_index::documents::delta::prepare(&snapshot, &outcome, &[]).unwrap());
    outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&snapshot, &outcome).unwrap();
    FileWriteUnit {
        rel_path: "a.py".into(),
        language: Language::Python,
        content_hash: snapshot.identity().content_digest.clone(),
        mtime: 0.0,
        size: text.len() as u64,
        outcome,
    }
}

#[test]
fn real_file_transaction_rejects_changed_relation_and_preserves_previous_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    let original = unit("def needle():\n    return '中文'\n");
    db.writes()
        .replace_files_batch(std::slice::from_ref(&original))
        .unwrap();
    let generation = db.reads().read_generation().unwrap();
    let mut forged = unit("def needle():\n    return 'changed'\n");
    forged.outcome.symbol_identities[0].qname = "neighbor.needle".into();
    assert!(db.writes().replace_files_batch(&[forged]).is_err());
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    let rows = db
        .retrieval()
        .chunk_rows_by_ids(&[&original.outcome.chunks[0].chunk_id], &Default::default())
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].qname.as_deref(), Some("needle"));
    assert_eq!(rows[0].text, original.outcome.chunks[0].text);

    // The same writer must recover after rollback and bind each new identity,
    // rather than retaining a previous chunk/document/payload association.
    let replacement =
        unit("def replacement_one():\n    return 1\ndef replacement_two():\n    return 2\n");
    db.writes()
        .replace_files_batch(std::slice::from_ref(&replacement))
        .unwrap();
    let replacement_generation = db.reads().read_generation().unwrap();
    assert_eq!(
        replacement_generation.index_epoch,
        generation.index_epoch + 1
    );
    drop(db);
    let (db, _) = IndexDb::open(&path).unwrap();
    assert_eq!(
        db.reads().read_generation().unwrap(),
        replacement_generation
    );
    let ids: Vec<_> = replacement
        .outcome
        .symbol_identities
        .iter()
        .map(|identity| identity.chunk_id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    let rows = db
        .retrieval()
        .chunk_rows_by_ids(&ids, &Default::default())
        .unwrap();
    assert_eq!(rows.len(), 2);
    for identity in &replacement.outcome.symbol_identities {
        let row = rows
            .iter()
            .find(|row| row.chunk_id == identity.chunk_id)
            .unwrap();
        assert_eq!(row.qname.as_deref(), Some(identity.qname.as_str()));
    }
}

#[test]
fn only_the_real_sql_surviving_declaration_gets_identity_without_candidate_deduplication() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    let original = unit("def needle():\n    return 1\ndef needle():\n    return 2\n");
    assert_eq!(original.outcome.symbols.len(), 2);
    assert_eq!(original.outcome.symbol_identities.len(), 2);
    assert_eq!(
        original.outcome.symbols[0].symbol_uid,
        original.outcome.symbols[1].symbol_uid
    );
    db.writes()
        .replace_files_batch(std::slice::from_ref(&original))
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM chunk_symbol_identity", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let ids: Vec<_> = original
        .outcome
        .chunks
        .iter()
        .map(|c| c.chunk_id.as_str())
        .collect();
    let rows = db
        .retrieval()
        .chunk_rows_by_ids(&ids, &Default::default())
        .unwrap();
    assert!(rows
        .iter()
        .any(|r| r.text.contains("return 1") && r.qname.is_none()));
    assert!(rows
        .iter()
        .any(|r| r.text.contains("return 2") && r.qname.as_deref() == Some("needle")));
}
