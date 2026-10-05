//! Fresh-owned synthetic C++ persistence only. No old shared or user cache is read.
use cc_db::{
    index_db::{FileWriteUnit, IndexDb},
    index_migrate::SchemaStatus,
};
use cc_index::Indexer;
use cc_model::{config::IndexingConfig, source::SourceSnapshot, Language, StableId, SymbolKind};
use cc_parsers::ParserRegistry;
use std::{path::Path, sync::Arc};

const SOURCE: &str = "namespace left { namespace shared {\r\nint leaf() { return 1; }\r\n} }\r\nnamespace right { namespace shared {\r\nint leaf() { return 2; }\r\n} }\r\n";
const FILE: &str = "taxonomy.cpp";

fn unit(text: &str) -> FileWriteUnit {
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let mut outcome = ParserRegistry::new()
        .parse(FILE, text, Language::Cpp)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    outcome.documents =
        Some(cc_index::documents::delta::prepare(&snapshot, &outcome, &[]).unwrap());
    outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&snapshot, &outcome).unwrap();
    FileWriteUnit {
        rel_path: FILE.into(),
        language: Language::Cpp,
        content_hash: snapshot.identity().content_digest.clone(),
        mtime: 0.0,
        size: text.len() as u64,
        outcome,
    }
}

fn leaf_rows(path: &Path) -> Vec<(String, String, String)> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT qname,kind,symbol_uid FROM symbols WHERE name='leaf' ORDER BY qname")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn assert_public_identity_rows(db: &IndexDb, path: &Path, expected: &[&str]) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT chunk_id FROM chunks WHERE symbol_name='leaf' ORDER BY chunk_index")
        .unwrap();
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let refs: Vec<_> = ids.iter().map(String::as_str).collect();
    let rows = db
        .retrieval()
        .chunk_rows_by_ids(&refs, &Default::default())
        .unwrap();
    let mut qnames: Vec<_> = rows.iter().filter_map(|r| r.qname.as_deref()).collect();
    qnames.sort_unstable();
    qnames.dedup();
    assert_eq!(qnames, expected);
    for row in rows {
        if let Some(qname) = row.qname {
            assert!(expected.contains(&qname.as_str()));
            assert_eq!(row.symbol_kind.as_deref(), Some("function"));
        }
    }
}

#[test]
fn fresh_owned_sql_preserves_both_namespace_function_identities() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    let data = unit(SOURCE);
    let leaves: Vec<_> = data
        .outcome
        .symbols
        .iter()
        .filter(|s| s.name == "leaf")
        .collect();
    assert_eq!(leaves.len(), 2);
    assert_ne!(leaves[0].symbol_uid, leaves[1].symbol_uid);
    for symbol in leaves {
        assert!(data
            .outcome
            .symbol_identities
            .iter()
            .any(|i| i.matches_symbol(symbol)));
    }
    db.writes().replace_files_batch(&[data]).unwrap();
    assert_eq!(leaf_rows(&path).len(), 2);
    assert_public_identity_rows(&db, &path, &["left::shared::leaf", "right::shared::leaf"]);
    drop(db);
    let (reopened, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    assert_public_identity_rows(
        &reopened,
        &path,
        &["left::shared::leaf", "right::shared::leaf"],
    );
}

#[test]
fn fresh_owned_sql_rejects_stale_method_uid_without_advancing_generation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    db.writes().replace_files_batch(&[unit(SOURCE)]).unwrap();
    let before = db.reads().read_generation().unwrap();
    let expected = leaf_rows(&path);
    let mut forged = unit(SOURCE);
    let identity = forged
        .outcome
        .symbol_identities
        .iter_mut()
        .find(|i| i.name == "leaf")
        .unwrap();
    identity.symbol_uid = StableId::symbol_uid(FILE, "shared::leaf", "method", Some("int leaf()"));
    assert!(db.writes().replace_files_batch(&[forged]).is_err());
    assert_eq!(db.reads().read_generation().unwrap(), before);
    assert_eq!(leaf_rows(&path), expected);
}

#[test]
fn fresh_owned_v25_rebuild_reparses_unchanged_source_then_reopen_rename_delete() {
    let root = tempfile::tempdir().unwrap();
    let source_path = root.path().join(FILE);
    std::fs::write(&source_path, SOURCE).unwrap();
    let original_metadata = std::fs::metadata(&source_path).unwrap();
    let path = root.path().join(".codecortex/index.sqlite3");
    let (old, _) = IndexDb::open(&path).unwrap();
    let mut legacy = unit(SOURCE);
    legacy.mtime = original_metadata
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    // Explicitly synthesize v25's observed fields in this new test-owned DB.
    // This is not a copied historical cache or an original program execution.
    for symbol in &mut legacy.outcome.symbols {
        if symbol.name == "leaf" {
            symbol.kind = SymbolKind::Method;
            symbol.qname = Some("shared::leaf".into());
            symbol.symbol_uid = Some(StableId::symbol_uid(
                FILE,
                "shared::leaf",
                "method",
                symbol.signature.as_deref(),
            ));
        }
    }
    legacy.outcome.symbol_identities = cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(SOURCE.as_bytes()),
        &legacy.outcome,
    )
    .unwrap();
    old.writes().replace_files_batch(&[legacy]).unwrap();
    let old_rows = leaf_rows(&path);
    assert_eq!(
        old_rows.len(),
        1,
        "legacy UNIQUE UID collision really persists one row"
    );
    assert_eq!(old_rows[0].1, "method");
    let old_generation = old.reads().read_generation().unwrap();
    drop(old);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 25u32).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            25
        );
    }
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(
        status,
        SchemaStatus::Initialized,
        "semantic parser change must invalidate owned v25 cache even with unchanged source"
    );
    assert_eq!(
        db.reads().schema_version().unwrap(),
        cc_db::index_migrate::CURRENT_SCHEMA_VERSION
    );
    let generation = db.reads().read_generation().unwrap();
    assert!(generation.index_epoch > old_generation.index_epoch);
    assert!(generation.evidence_epoch > old_generation.evidence_epoch);
    assert_ne!(generation.incarnation, old_generation.incarnation);
    let db = Arc::new(db);
    let config = IndexingConfig {
        max_concurrent_parse: Some(1),
        dispatch_synthesis: false,
        ..Default::default()
    };
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &config);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        1
    );
    assert_eq!(std::fs::read(&source_path).unwrap(), SOURCE.as_bytes());
    assert_eq!(
        std::fs::metadata(&source_path).unwrap().modified().unwrap(),
        original_metadata.modified().unwrap()
    );
    let new_rows = leaf_rows(&path);
    assert_eq!(new_rows.len(), 2);
    assert!(new_rows
        .iter()
        .all(|r| r.1 == "function" && r.2 != old_rows[0].2));
    assert_public_identity_rows(&db, &path, &["left::shared::leaf", "right::shared::leaf"]);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(leaf_rows(&path), new_rows);
    drop(indexer);
    drop(db);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    let db = Arc::new(db);
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &config);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(leaf_rows(&path), new_rows);
    assert_public_identity_rows(&db, &path, &["left::shared::leaf", "right::shared::leaf"]);
    std::fs::write(&source_path, SOURCE.replace("right", "renamed_right")).unwrap();
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        1
    );
    assert_public_identity_rows(
        &db,
        &path,
        &["left::shared::leaf", "renamed_right::shared::leaf"],
    );
    std::fs::remove_file(&source_path).unwrap();
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_removed,
        1
    );
    assert!(leaf_rows(&path).is_empty());
}

#[test]
fn fresh_owned_indexer_preserves_rejected_targets_and_exact_caller_env_uids() {
    let root = tempfile::tempdir().unwrap();
    let text = "int helper(int x) { return x; }\nnamespace a { namespace tail { const char *leaf() { helper(1); return getenv(\"FIRST\"); } } } namespace b { namespace tail { const char *leaf() { helper(2); return getenv(\"SECOND\"); } } }\nnamespace north { int unique(int x) { return x; } }\nstruct Other { int unique(int); };\nint wrong(int x) { return south::unique(x); }\nint member(Other obj) { return obj.unique(1); }\nint ambiguous() { return a::tail::leaf() != nullptr; }\n";
    std::fs::write(root.path().join(FILE), text).unwrap();
    let path = root.path().join(".codecortex/index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    let db = Arc::new(db);
    let config = IndexingConfig {
        max_concurrent_parse: Some(1),
        dispatch_synthesis: false,
        ..Default::default()
    };
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &config);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        1
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    for caller in ["wrong", "member", "ambiguous"] {
        let result: (Option<String>, Option<String>, Option<String>) = conn.query_row(
            "SELECT target_symbol_id,callee_symbol_uid,target_file_path FROM call_edges WHERE caller_symbol=?1",
            [caller], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap();
        assert_eq!(
            result,
            (None, None, None),
            "{caller}: indexer must not rebind a rejected namespace target"
        );
    }
    let mut stmt = conn.prepare("SELECT s.qname FROM call_edges c JOIN symbols s ON s.symbol_uid=c.caller_symbol_uid WHERE c.callee_symbol='helper' ORDER BY s.qname").unwrap();
    let qnames: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(qnames, ["a::tail::leaf", "b::tail::leaf"]);
    for (key, expected) in [("FIRST", "a::tail::leaf"), ("SECOND", "b::tail::leaf")] {
        let qname: String = conn.query_row("SELECT s.qname FROM data_flow_edges d JOIN symbols s ON s.symbol_uid=d.source_symbol_uid WHERE d.env_key=?1", [key], |r| r.get(0)).unwrap();
        assert_eq!(qname, expected);
    }
}

fn assert_cross_file_negative(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let edge: (Option<String>, Option<String>, String) = conn.query_row(
        "SELECT target_symbol_id,callee_symbol_uid,resolution_strategy FROM call_edges WHERE file_path='b.cpp' AND caller_symbol='wrong'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(
        edge.0, None,
        "cross-file qualifier mismatch must not acquire a target id"
    );
    assert_eq!(
        edge.1, None,
        "cross-file qualifier mismatch must not acquire A UID"
    );
    assert_eq!(edge.2, "cpp_namespace_owner_unproven");
    let reference: (Option<String>, Option<String>, String) = conn.query_row(
        "SELECT target_symbol_id,target_symbol_uid,resolution_strategy FROM symbol_refs WHERE file_path='b.cpp' AND symbol_name='unique'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(
        reference,
        (None, None, "cpp_namespace_owner_unproven".into())
    );
    let payload: String = conn
        .query_row(
            "SELECT payload FROM resolution_manifests WHERE file_path='b.cpp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let manifest: cc_model::resolution::ResolutionManifest =
        serde_json::from_str(&payload).unwrap();
    assert!(manifest.records.iter().filter(|r| r.query == "unique").all(|r| matches!(&r.outcome, cc_model::resolution::ResolutionOutcome::Unresolved { reason } if reason == "cpp_namespace_owner_unproven")));
    assert_eq!(
        manifest
            .records
            .iter()
            .filter(|r| r.query == "unique")
            .count(),
        2
    );
}

#[test]
fn fresh_owned_cross_file_negative_evidence_survives_dirty_reload_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.cpp");
    let b = root.path().join("b.cpp");
    std::fs::write(&a, "namespace north { int unique(int x) { return x; } }\n").unwrap();
    let caller = "int wrong(int x) { return south::unique(x); }\n";
    std::fs::write(&b, caller).unwrap();
    let path = root.path().join(".codecortex/index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    let db = Arc::new(db);
    let config = IndexingConfig {
        max_concurrent_parse: Some(1),
        dispatch_synthesis: false,
        ..Default::default()
    };
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &config);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        2
    );
    assert_cross_file_negative(&path);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_cross_file_negative(&path);
    // Change a dependency's symbol inventory while b.cpp remains byte-identical.
    std::fs::write(
        &a,
        "namespace north { int unique(int x) { return x + 1; } int unique(int x, int y) { return x + y; } }\n",
    )
    .unwrap();
    let changed = indexer.build_index(root.path(), false).unwrap();
    println!(
        "dirty report: {}",
        serde_json::to_string(&changed.dirty_plan).unwrap()
    );
    assert_eq!(changed.files_parsed, 1);
    assert!(
        changed.dirty_plan.selected_dependents >= 1,
        "the unchanged caller must actually be dirty-reloaded"
    );
    assert_eq!(std::fs::read_to_string(&b).unwrap(), caller);
    assert_cross_file_negative(&path);
    drop(indexer);
    drop(db);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    let db = Arc::new(db);
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &config);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_cross_file_negative(&path);
}

#[test]
fn fresh_owned_unaffected_c_cpp_global_and_method_targets_keep_resolution() {
    let root = tempfile::tempdir().unwrap();
    for (file, text) in [
        ("a.c", "int c_helper(int x) { return x; }"),
        ("b.c", "int c_use(int x) { return c_helper(x); }"),
        ("global.cpp", "int cpp_helper(int x) { return x; }"),
        ("use.cpp", "int cpp_use(int x) { return cpp_helper(x); }"),
        ("method.cpp", "struct Box { int method(int x) { return x; } }; int method_use(Box box) { return box.method(1); }"),
        ("a.py", "def py_helper(x):\n    return x\n"),
        ("b.py", "def py_use(x):\n    return py_helper(x)\n"),
    ] {
        std::fs::write(root.path().join(file), text).unwrap();
    }
    let path = root.path().join(".codecortex/index.sqlite3");
    let (db, _) = IndexDb::open(&path).unwrap();
    let db = Arc::new(db);
    let config = IndexingConfig {
        max_concurrent_parse: Some(1),
        dispatch_synthesis: false,
        ..Default::default()
    };
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &config);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        7
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    for caller in ["c_use", "cpp_use", "method_use", "py_use"] {
        let (uid, strategy): (Option<String>, String) = conn.query_row("SELECT callee_symbol_uid,resolution_strategy FROM call_edges WHERE caller_symbol=?1", [caller], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(uid.is_some(), "legacy target {caller} should still resolve");
        assert_ne!(strategy, "cpp_namespace_owner_unproven");
    }
}
