//! Transaction, single-lease reads, corruption and rebuild-boundary contracts.
use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{
    public_surface::{PublicSurface, SurfaceKnowledge},
    Language, ParseOutcome,
};
fn unit(path: &str) -> FileWriteUnit {
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Rust,
        content_hash: "hash".into(),
        mtime: 1.0,
        size: 0,
        outcome: ParseOutcome {
            public_surface: PublicSurface::new("rust", path, "rust-declared-v1"),
            ..Default::default()
        },
    }
}
#[test]
fn surface_roundtrip_shares_file_epoch_and_cascades_on_delete() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open_with_read_pool_size(&d.path().join("index.db"), 1)
        .unwrap()
        .0;
    let before = db.reads().generation().unwrap();
    let a = unit("a.rs");
    let mut b = unit("b.rs");
    b.outcome.public_surface = PublicSurface::default();
    db.writes().replace_files_batch(&[a.clone(), b]).unwrap();
    let after = db.reads().generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    let rows = db
        .reads()
        .public_surfaces(&["a.rs".into(), "b.rs".into(), "missing.rs".into()])
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows["a.rs"], a.outcome.public_surface);
    assert_eq!(rows["b.rs"].knowledge, SurfaceKnowledge::Unknown);
    assert_eq!(rows["b.rs"].fingerprint(), None);
    db.writes().remove_files_batch(&["a.rs".into()]).unwrap();
    assert!(db
        .reads()
        .public_surfaces(&["a.rs".into()])
        .unwrap()
        .is_empty());
}
#[test]
fn a_bad_surface_rolls_back_the_whole_file_batch() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&d.path().join("index.db")).unwrap().0;
    db.writes().replace_files_batch(&[unit("old.rs")]).unwrap();
    let before = db.reads().generation().unwrap();
    let mut bad = unit("bad.rs");
    bad.outcome.public_surface.module = "other.rs".into();
    assert!(db
        .writes()
        .replace_files_batch(&[unit("a.rs"), bad])
        .is_err());
    assert_eq!(db.reads().generation().unwrap(), before);
    assert!(db
        .reads()
        .public_surfaces(&["a.rs".into(), "bad.rs".into()])
        .unwrap()
        .is_empty());
    assert_eq!(
        db.reads().list_file_paths().unwrap(),
        vec!["old.rs".to_string()]
    );
}
#[test]
fn corrupt_fingerprint_and_new_format_fail_closed() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("index.db");
    let db = IndexDb::open(&path).unwrap().0;
    db.writes().replace_files_batch(&[unit("a.rs")]).unwrap();
    let seed = rusqlite::Connection::open(&path).unwrap();
    seed.execute("UPDATE public_surfaces SET fingerprint='wrong'", [])
        .unwrap();
    assert!(db.reads().public_surfaces(&["a.rs".into()]).is_err());
    let mut future = unit("future.rs");
    future.outcome.public_surface.format_version = 999;
    assert!(db.writes().replace_files_batch(&[future]).is_err());
}
#[test]
fn nonempty_surface_roundtrip_canonicalizes_declaration_order() {
    use cc_model::public_surface::{SurfaceEntry, SurfaceToken, VisibilityDomain};
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open_with_read_pool_size(&d.path().join("index.db"), 1)
        .unwrap()
        .0;
    let mut file = unit("api.rs");
    for name in ["z", "a", "z"] {
        file.outcome.public_surface.entries.push(SurfaceEntry {
            qualified_name: name.into(),
            exported_name: name.into(),
            kind: "function".into(),
            visibility: VisibilityDomain::Exported,
            signature: vec![SurfaceToken {
                kind: "literal".into(),
                text: "a b".into(),
            }],
            conditions: vec![],
        });
    }
    file.outcome.public_surface.normalize();
    let expected = file.outcome.public_surface.clone();
    file.outcome.public_surface.entries.reverse();
    db.writes().replace_files_batch(&[file]).unwrap();
    let stored = db.reads().public_surfaces(&["api.rs".into()]).unwrap();
    assert_eq!(stored["api.rs"], expected);
    assert_eq!(
        stored["api.rs"]
            .entries
            .iter()
            .map(|e| e.qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "z"]
    );
    assert_eq!(stored["api.rs"].entries[0].signature[0].text, "a b");
    assert_eq!(stored["api.rs"].fingerprint(), expected.fingerprint());
}

#[test]
fn batched_surface_reads_cross_placeholder_boundary_without_losing_empty_unknown() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open_with_read_pool_size(&d.path().join("index.db"), 1)
        .unwrap()
        .0;
    let units: Vec<_> = (0..600).map(|i| unit(&format!("f{i}.rs"))).collect();
    let paths: Vec<_> = units.iter().map(|u| u.rel_path.clone()).collect();
    db.writes().replace_files_batch(&units).unwrap();
    let rows = db.reads().public_surfaces(&paths).unwrap();
    assert_eq!(rows.len(), 600);
    assert!(rows
        .values()
        .all(|s| s.knowledge == SurfaceKnowledge::KnownEmpty && s.fingerprint().is_some()));
}

#[test]
fn previous_schema_cannot_reuse_absent_surface_evidence() {
    for previous in [7u32, 8u32, 9u32, 10u32, 11u32] {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("index.db");
        {
            let db = IndexDb::open(&path).unwrap().0;
            db.writes().replace_files_batch(&[unit("old.rs")]).unwrap();
        }
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", previous).unwrap();
            assert_eq!(
                cc_db::index_migrate::migrate_index_db(&c).unwrap(),
                cc_db::index_migrate::SchemaStatus::Mismatch { stored: previous }
            );
        }
        let (db, status) = IndexDb::open(&path).unwrap();
        // Both P1 and unfinished P2 draft evidence must cross a rebuild boundary.
        assert_eq!(status, cc_db::index_migrate::SchemaStatus::Initialized);
        assert_eq!(
            db.reads().schema_version().unwrap(),
            cc_db::index_migrate::CURRENT_SCHEMA_VERSION
        );
        assert!(db.reads().list_file_paths().unwrap().is_empty());
    }
}
