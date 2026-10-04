//! Independent database authority fixtures; no public gold or author fixture cache.
use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;

fn unit(text: &str) -> FileWriteUnit {
    let source = SourceSnapshot::new(text.as_bytes());
    let mut outcome = ParserRegistry::new()
        .parse("review.py", text, Language::Python)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    outcome.documents = Some(cc_index::documents::delta::prepare(&source, &outcome, &[]).unwrap());
    outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&source, &outcome).unwrap();
    FileWriteUnit {
        rel_path: "review.py".into(),
        language: Language::Python,
        content_hash: source.identity().content_digest.clone(),
        mtime: 0.0,
        size: text.len() as u64,
        outcome,
    }
}
fn ids(unit: &FileWriteUnit) -> Vec<&str> {
    unit.outcome
        .chunks
        .iter()
        .map(|c| c.chunk_id.as_str())
        .collect()
}
const TEXT: &str = "# UTF8 雪\r\nclass North:\r\n    def beacon(self):\r\n        return '灯'\r\n\r\nclass South:\r\n    def beacon(self):\r\n        return '海'\r\n";

#[test]
fn independent_ambiguity_omission_and_sql_survivor_with_measured_selects() {
    let mut original = unit(TEXT);
    let north = original
        .outcome
        .symbols
        .iter()
        .find(|s| s.qname.as_deref() == Some("North.beacon"))
        .unwrap()
        .clone();
    original.outcome.symbols.push(north);
    let source = SourceSnapshot::new(TEXT.as_bytes());
    original.outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&source, &original.outcome).unwrap();
    assert!(!original
        .outcome
        .symbol_identities
        .iter()
        .any(|i| i.qname == "North.beacon"));
    let dir = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&dir.path().join("db.sqlite3")).unwrap().0;
    db.writes()
        .replace_files_batch(&[original.clone()])
        .unwrap();
    let hydrated = db
        .retrieval()
        .chunk_rows_by_ids_with_work(&ids(&original), &Default::default())
        .unwrap();
    let present = hydrated.rows.iter().filter(|r| r.qname.is_some()).count();
    let absent = hydrated.rows.len() - present;
    assert!(hydrated
        .rows
        .iter()
        .any(|r| r.symbol_name.as_deref() == Some("beacon") && r.qname.is_none()));
    assert_eq!(hydrated.work.sql.statements, 1 + present * 4 + absent);
    assert!(hydrated.work.sql.vm_steps.unwrap() > 0);
    println!(
        "hydration rows={} admitted={} absent={} measured={:?}",
        hydrated.rows.len(),
        present,
        absent,
        hydrated.work.sql
    );
    let repeated = unit("def beacon():\n    return 'first'\ndef beacon():\n    return 'second'\n");
    assert_eq!(repeated.outcome.symbol_identities.len(), 2);
    db.writes()
        .replace_files_batch(&[repeated.clone()])
        .unwrap();
    let rows = db
        .retrieval()
        .chunk_rows_by_ids(&ids(&repeated), &Default::default())
        .unwrap();
    assert!(rows
        .iter()
        .any(|r| r.text.contains("first") && r.qname.is_none()));
    assert!(rows
        .iter()
        .any(|r| r.text.contains("second") && r.qname.as_deref() == Some("beacon")));
}

#[test]
fn independent_dirty_only_preserves_authority_and_changed_symbol_fails_closed() {
    let original = unit(TEXT);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite3");
    let db = IndexDb::open(&path).unwrap().0;
    db.writes()
        .replace_files_batch(&[original.clone()])
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let records = || {
        conn.prepare("SELECT record_json FROM chunk_symbol_identity ORDER BY chunk_id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let before = records();
    db.writes()
        .replace_reresolved_edges_only(&[original.clone()])
        .unwrap();
    assert_eq!(records(), before);
    db.retrieval()
        .chunk_rows_by_ids(&ids(&original), &Default::default())
        .unwrap();
    let mut dirty = original.clone();
    dirty
        .outcome
        .symbols
        .iter_mut()
        .find(|s| s.qname.as_deref() == Some("North.beacon"))
        .unwrap()
        .qname = Some("Forged.beacon".into());
    db.writes().replace_reresolved_edges_only(&[dirty]).unwrap();
    assert_eq!(records(), before);
    assert!(db
        .retrieval()
        .chunk_rows_by_ids(&ids(&original), &Default::default())
        .is_err());
}

#[test]
fn independent_write_relation_rollback_and_fk_disabled_delete_trigger() {
    let original = unit(TEXT);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite3");
    let db = IndexDb::open(&path).unwrap().0;
    db.writes()
        .replace_files_batch(&[original.clone()])
        .unwrap();
    let generation = db.reads().read_generation().unwrap();
    let mut bad = unit(&TEXT.replace("灯", "焰"));
    bad.outcome.symbol_identities[0].document.doc_version = "broken".into();
    assert!(db.writes().replace_files_batch(&[bad]).is_err());
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    db.retrieval()
        .chunk_rows_by_ids(&ids(&original), &Default::default())
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=OFF; DELETE FROM chunks WHERE file_path='review.py';")
        .unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM chunk_symbol_identity", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn independent_original_byte_points_partial_legacy_no_symbol_and_utf8_owner_omit() {
    let original = unit(TEXT);
    let source = SourceSnapshot::new(TEXT.as_bytes());
    for i in &original.outcome.symbol_identities {
        assert_eq!(
            source.point(i.owner.start).unwrap(),
            (i.start_line as usize, i.start_col as usize)
        );
        assert_eq!(
            source.point(i.owner.end).unwrap(),
            (i.end_line as usize, i.end_col as usize)
        );
        assert!(source.slice(i.owner).unwrap().contains(&i.name));
    }
    for mode in 0..3 {
        let mut outcome = original.outcome.clone();
        match mode {
            0 => outcome.source_structure = None,
            1 => outcome.source_structure.as_mut().unwrap().complete = false,
            _ => outcome.symbols.clear(),
        }
        assert!(
            cc_index::documents::symbol_identity::prepare(&source, &outcome)
                .unwrap()
                .is_empty()
        );
    }
    let mut invalid = original.outcome.clone();
    let interior = TEXT.find('灯').unwrap() + 1;
    for c in &mut invalid.chunks {
        if let Some(p) = &mut c.source {
            p.owner = Some(cc_model::source::ByteSpan {
                start: interior,
                end: interior + 1,
            });
        }
    }
    assert!(
        cc_index::documents::symbol_identity::prepare(&source, &invalid)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn independent_hydration_snapshot_survives_concurrent_real_replacements() {
    let variants = [
        unit(TEXT),
        unit(&TEXT.replace("North", "Northern").replace("灯", "焰")),
    ];
    let dir = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&dir.path().join("db.sqlite3")).unwrap().0;
    db.writes()
        .replace_files_batch(&[variants[0].clone()])
        .unwrap();
    let all_ids = variants
        .iter()
        .flat_map(|u| ids(u).into_iter().map(str::to_owned))
        .collect::<Vec<_>>();
    let expected = variants
        .iter()
        .flat_map(|u| {
            u.outcome
                .symbol_identities
                .iter()
                .map(|i| (i.chunk_id.clone(), i.qname.clone(), i.source.clone()))
        })
        .collect::<Vec<_>>();
    let db = std::sync::Arc::new(db);
    let reader_db = db.clone();
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let reader_start = start.clone();
    let reader = std::thread::spawn(move || {
        let ids = all_ids.iter().map(String::as_str).collect::<Vec<_>>();
        reader_start.wait();
        let mut checked = 0;
        for _ in 0..80 {
            let result = reader_db
                .retrieval()
                .chunk_rows_by_ids(&ids, &Default::default())
                .unwrap();
            for r in result {
                if let Some(qname) = r.qname {
                    let expected = expected
                        .iter()
                        .find(|e| {
                            e.0 == r.chunk_id && e.2 == r.source_evidence.as_ref().unwrap().source
                        })
                        .unwrap();
                    assert_eq!(qname, expected.1);
                    assert_eq!(r.source_evidence.unwrap().source, expected.2);
                    checked += 1;
                }
            }
        }
        checked
    });
    start.wait();
    for i in 0..30 {
        db.writes()
            .replace_files_batch(&[variants[i % 2].clone()])
            .unwrap();
    }
    assert!(reader.join().unwrap() > 0);
}

#[test]
fn independent_actual_pooled_sqlite_snapshot_pins_relations_until_transaction_end() {
    let dir = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&dir.path().join("db.sqlite3")).unwrap().0;
    let original = unit(TEXT);
    let changed = unit(&TEXT.replace("North", "Northern"));
    db.writes().replace_files_batch(&[original]).unwrap();
    let pooled = db.read_conn().unwrap();
    let snapshot = pooled.unchecked_transaction().unwrap();
    let read = |conn: &rusqlite::Connection| {
        conn.query_row("SELECT i.record_json,s.qname,f.content_hash,c.source_json,d.doc_version FROM chunk_symbol_identity i JOIN symbols s ON s.symbol_id=i.symbol_id JOIN chunks c ON c.chunk_id=i.chunk_id JOIN files f ON f.file_path=i.file_path JOIN document_manifest d ON d.chunk_id=i.chunk_id WHERE s.name='beacon' ORDER BY s.qname LIMIT 1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).unwrap()
    };
    let before = read(&snapshot);
    db.writes().replace_files_batch(&[changed]).unwrap();
    assert_eq!(
        read(&snapshot),
        before,
        "writer commit must not mix a pinned SQLite read snapshot"
    );
    snapshot.commit().unwrap();
    let after = read(&pooled);
    assert_ne!(before, after);
    assert_eq!(before.1, "North.beacon");
    assert_eq!(after.1, "Northern.beacon");
}

#[test]
fn independent_leading_docs_whitespace_and_same_name_neighbor_are_byte_bound() {
    use cc_model::source::ByteSpan;
    let text = "\n\n# attached beacon documentation 雪\ndef beacon():\n    return '灯'\n\n\n# second declaration\ndef beacon():\n    return '海'\n";
    let parsed = unit(text);
    let source = SourceSnapshot::new(text.as_bytes());
    let identity = parsed
        .outcome
        .symbol_identities
        .iter()
        .find(|i| i.qname == "beacon")
        .unwrap();
    let boundary = parsed
        .outcome
        .source_structure
        .as_ref()
        .unwrap()
        .boundaries
        .iter()
        .find(|b| b.span == identity.owner)
        .unwrap();
    let leading = boundary
        .leading_comment
        .expect("real parser must explicitly attach leading comment");
    let prototype = parsed
        .outcome
        .chunks
        .iter()
        .find(|c| c.chunk_id == identity.chunk_id)
        .unwrap()
        .source
        .as_ref()
        .unwrap();
    let check = |span: ByteSpan| {
        let mut proof = prototype.clone();
        proof.span = span;
        proof.slice_digest = source.slice_digest(span).unwrap();
        let slice = source.slice(span).unwrap();
        assert!(proof.validate(slice));
        let owns = boundary.owns_chunk(&proof, slice);
        let mut candidate = parsed.outcome.clone();
        let mut chunk = candidate
            .chunks
            .iter()
            .find(|c| c.chunk_id == identity.chunk_id)
            .unwrap()
            .clone();
        chunk.text = slice.to_owned();
        chunk.source = Some(proof);
        candidate.chunks = vec![chunk];
        candidate.documents =
            Some(cc_index::documents::delta::prepare(&source, &candidate, &[]).unwrap());
        let prepared = cc_index::documents::symbol_identity::prepare(&source, &candidate).unwrap();
        assert_eq!(
            !prepared.is_empty(),
            owns,
            "prepare must use explicit byte ownership for {span:?}"
        );
        owns
    };
    assert!(check(leading));
    assert!(check(ByteSpan {
        start: leading.start - 2,
        end: identity.owner.end + 2
    }));
    let neighbor = parsed
        .outcome
        .symbol_identities
        .iter()
        .find(|i| i.qname == "beacon" && i.owner.start > identity.owner.start)
        .unwrap();
    assert!(!check(neighbor.owner));
    assert!(!check(ByteSpan {
        start: identity.owner.start,
        end: neighbor.owner.end
    }));
}
