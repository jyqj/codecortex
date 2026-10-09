use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{
    package_surface::PackageKey,
    public_surface::{PublicSurface, SurfaceEntry, VisibilityDomain},
    resolution::*,
    Language, ParseOutcome,
};
use std::collections::BTreeSet;
fn unit(path: &str) -> FileWriteUnit {
    let mut outcome = ParseOutcome {
        resolution: ResolutionManifest::new(),
        ..Default::default()
    };
    outcome
        .resolution
        .dependency(DependencyKind::NameBucket, "missing");
    outcome.resolution.record(ResolutionRecord {
        site_kind: "call".into(),
        site_id: format!("call:{path}"),
        query: "missing".into(),
        outcome: ResolutionOutcome::Unresolved {
            reason: "no_candidate".into(),
        },
    });
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Python,
        content_hash: "hash".into(),
        mtime: 1.0,
        size: 1,
        outcome,
    }
}
#[test]
fn dependency_manifest_roundtrip_reverse_lookup_delete_and_rollback() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open_with_read_pool_size(&d.path().join("db"), 1)
        .unwrap()
        .0;
    let mut file = unit("use.py");
    file.outcome
        .resolution
        .dependency(DependencyKind::MissingPath, "api.py");
    let before = db.reads().generation().unwrap();
    db.writes().replace_files_batch(&[file]).unwrap();
    let after = db.reads().generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    let stored = db.reads().resolution_manifests(&["use.py".into()]).unwrap();
    assert!(stored["use.py"].complete);
    for event in [
        ResolutionDependency::new(DependencyKind::NameBucket, "missing"),
        ResolutionDependency::new(DependencyKind::MissingPath, "api.py"),
    ] {
        assert_eq!(
            db.reads()
                .resolution_dependents(&BTreeSet::from([event]), 10, &[])
                .unwrap(),
            vec!["use.py"]
        );
    }
    let mut bad = unit("bad.py");
    bad.outcome.resolution.version = 999;
    assert!(db
        .writes()
        .replace_files_batch(&[unit("ok.py"), bad])
        .is_err());
    assert_eq!(db.reads().generation().unwrap(), after);
    assert_eq!(db.reads().list_file_paths().unwrap(), vec!["use.py"]);
    assert_eq!(
        db.reads().resolution_manifests(&["use.py".into()]).unwrap(),
        stored
    );

    // Reuse the writer after a rollback which already wrote another path.
    // The replacement must remove old dependencies and bind its own payload.
    let mut replacement = unit("use.py");
    replacement.outcome.resolution = ResolutionManifest::new();
    replacement
        .outcome
        .resolution
        .dependency(DependencyKind::MissingPath, "replacement.py");
    db.writes().replace_files_batch(&[replacement]).unwrap();
    let replacement_generation = db.reads().generation().unwrap();
    assert_eq!(replacement_generation.index_epoch, after.index_epoch + 1);
    let replacement_stored = db.reads().resolution_manifests(&["use.py".into()]).unwrap();
    assert!(replacement_stored["use.py"].records.is_empty());
    let replacement_event =
        ResolutionDependency::new(DependencyKind::MissingPath, "replacement.py");
    assert_eq!(
        replacement_stored["use.py"].dependencies,
        BTreeSet::from([replacement_event.clone()])
    );
    assert_eq!(
        db.reads()
            .resolution_dependents(&BTreeSet::from([replacement_event]), 10, &[])
            .unwrap(),
        vec!["use.py"]
    );
    drop(db);
    let db = IndexDb::open_with_read_pool_size(&d.path().join("db"), 1)
        .unwrap()
        .0;
    assert_eq!(db.reads().generation().unwrap(), replacement_generation);
    assert_eq!(
        db.reads().resolution_manifests(&["use.py".into()]).unwrap(),
        replacement_stored
    );
    for event in [
        ResolutionDependency::new(DependencyKind::NameBucket, "missing"),
        ResolutionDependency::new(DependencyKind::MissingPath, "api.py"),
    ] {
        assert!(db
            .reads()
            .resolution_dependents(&BTreeSet::from([event]), 10, &[])
            .unwrap()
            .is_empty());
    }
    assert_eq!(
        db.reads()
            .resolution_dependents(
                &BTreeSet::from([ResolutionDependency::new(
                    DependencyKind::MissingPath,
                    "replacement.py"
                )]),
                10,
                &[]
            )
            .unwrap(),
        vec!["use.py"]
    );
    db.writes().remove_files_batch(&["use.py".into()]).unwrap();
    assert!(db
        .reads()
        .resolution_manifests(&["use.py".into()])
        .unwrap()
        .is_empty());
    assert!(db
        .reads()
        .resolution_dependents(
            &BTreeSet::from([ResolutionDependency::new(
                DependencyKind::NameBucket,
                "missing"
            )]),
            10,
            &[]
        )
        .unwrap()
        .is_empty());
}
#[test]
fn bounded_frontier_excludes_changed_files_before_counting_and_crosses_batches() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open_with_read_pool_size(&d.path().join("db"), 1)
        .unwrap()
        .0;
    let units: Vec<_> = (0..600).map(|i| unit(&format!("f{i:04}.py"))).collect();
    let paths: Vec<_> = units.iter().map(|u| u.rel_path.clone()).collect();
    db.writes().replace_files_batch(&units).unwrap();
    assert_eq!(db.reads().resolution_manifests(&paths).unwrap().len(), 600);
    let events = BTreeSet::from([ResolutionDependency::new(
        DependencyKind::NameBucket,
        "missing",
    )]);
    assert_eq!(
        db.reads()
            .resolution_dependents(&events, 1, &paths[..598])
            .unwrap(),
        paths[598..]
    );
    assert_eq!(
        db.reads()
            .resolution_dependents(&events, 0, &[])
            .unwrap()
            .len(),
        1
    );
    let mut events: BTreeSet<_> = (0..600)
        .map(|i| ResolutionDependency::new(DependencyKind::NameBucket, format!("no_{i}")))
        .collect();
    events.insert(ResolutionDependency::new(
        DependencyKind::NameBucket,
        "missing",
    ));
    assert_eq!(
        db.reads()
            .resolution_dependents(&events, 3, &paths[..10])
            .unwrap(),
        paths[10..14]
    );
}
#[test]
fn package_aggregate_tracks_files_and_test_visibility() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&d.path().join("db")).unwrap().0;
    let mut units = Vec::new();
    for path in ["pkg/a.go", "pkg/b.go", "pkg/api_test.go"] {
        let mut u = unit(path);
        u.language = Language::Go;
        let mut s = PublicSurface::new("go", path, "test-static-v1");
        s.entries.push(SurfaceEntry {
            qualified_name: "pkg".into(),
            exported_name: String::new(),
            kind: "go_package".into(),
            visibility: VisibilityDomain::Module,
            signature: vec![],
            conditions: vec![],
        });
        s.normalize();
        u.outcome.public_surface = s;
        units.push(u);
    }
    db.writes().replace_files_batch(&units).unwrap();
    let key = PackageKey {
        directory: "pkg".into(),
        name: "pkg".into(),
        test_files: false,
    };
    let a = db.reads().package_surface(&key).unwrap();
    assert_eq!(a.members.len(), 2);
    let mut test = key.clone();
    test.test_files = true;
    assert_eq!(db.reads().package_surface(&test).unwrap().members.len(), 3);
    db.writes()
        .remove_files_batch(&["pkg/b.go".into()])
        .unwrap();
    let b = db.reads().package_surface(&key).unwrap();
    assert_ne!(a.fingerprint, b.fingerprint);
    assert_eq!(b.members, vec!["pkg/a.go"]);
}
#[test]
fn corrupt_payload_is_rejected() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("db");
    let db = IndexDb::open(&path).unwrap().0;
    db.writes().replace_files_batch(&[unit("a.py")]).unwrap();
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute("UPDATE resolution_manifests SET digest='wrong'", [])
        .unwrap();
    assert!(db.reads().resolution_manifests(&["a.py".into()]).is_err());
}

fn snapshot_dependency_unit(path: &str, count: usize) -> FileWriteUnit {
    let mut file = unit(path);
    file.outcome.resolution.dependencies.clear();
    for n in (0..count).rev() {
        file.outcome
            .resolution
            .dependency(DependencyKind::NameBucket, format!("key-{n:05}"));
        // The manifest's original BTreeSet deduplication remains authoritative.
        file.outcome
            .resolution
            .dependency(DependencyKind::NameBucket, format!("key-{n:05}"));
    }
    let record = file.outcome.resolution.records[0].clone();
    file.outcome.resolution.records.push(record);
    file
}

fn snapshot_dependency_tables(db: &IndexDb) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    let conn = rusqlite::Connection::open_with_flags(
        db.admin().db_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    [
        "SELECT rowid,* FROM resolution_dependencies ORDER BY rowid",
        "SELECT * FROM resolution_manifests ORDER BY file_path",
        "SELECT * FROM public_surfaces ORDER BY file_path",
    ]
    .into_iter()
    .map(|sql| {
        let mut statement = conn.prepare(sql).unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get::<_, rusqlite::types::Value>(column))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    })
    .collect()
}

#[test]
fn snapshot_dependency_manifest_and_rows_match_original_full_writer() {
    let directory = tempfile::tempdir().unwrap();
    let old = IndexDb::open_with_read_pool_size(&directory.path().join("old.db"), 1)
        .unwrap()
        .0;
    let new = IndexDb::open_with_read_pool_size(&directory.path().join("new.db"), 1)
        .unwrap()
        .0;
    let files: Vec<_> = [0usize, 1, 7, 8, 9, 63, 64, 65, 73]
        .into_iter()
        .map(|count| snapshot_dependency_unit(&format!("n-{count}.py"), count))
        .collect();
    let old_before = old.reads().generation().unwrap();
    let new_before = new.reads().generation().unwrap();
    old.admin()
        .rebuild_with_temp_db(|conn| {
            for file in &files {
                IndexDb::insert_file_data(conn, file)?;
            }
            Ok(())
        })
        .unwrap();
    new.admin()
        .rebuild_with_temp_db(|conn| {
            cc_db::SnapshotWriteTxn::new(conn)
                .write_file_data_for_rebuild(&files, &Default::default())
        })
        .unwrap();
    assert_eq!(
        old.reads().generation().unwrap().index_epoch,
        old_before.index_epoch + 1
    );
    assert_eq!(
        new.reads().generation().unwrap().index_epoch,
        new_before.index_epoch + 1
    );
    assert_eq!(
        snapshot_dependency_tables(&old),
        snapshot_dependency_tables(&new)
    );
    let paths: Vec<_> = files.iter().map(|file| file.rel_path.clone()).collect();
    let original = old.reads().resolution_manifests(&paths).unwrap();
    let batched = new.reads().resolution_manifests(&paths).unwrap();
    assert_eq!(original, batched);
    assert!(batched.values().all(|manifest| manifest.records.len() == 1));

    // An ordinary live replacement after the snapshot keeps the unchanged
    // incremental writer, delete behavior and one generation bump.
    let before = new.reads().generation().unwrap();
    new.writes()
        .replace_files_batch(&[snapshot_dependency_unit("n-73.py", 8)])
        .unwrap();
    assert_eq!(
        new.reads().generation().unwrap().index_epoch,
        before.index_epoch + 1
    );
    assert_eq!(
        new.reads()
            .resolution_manifests(&["n-73.py".into()])
            .unwrap()["n-73.py"]
            .dependencies
            .len(),
        8
    );
}

#[test]
fn snapshot_dependency_failure_keeps_live_rows_and_generation_like_original() {
    let directory = tempfile::tempdir().unwrap();
    let mut errors = Vec::new();
    for snapshot in [false, true] {
        let path = directory.path().join(format!("failure-{snapshot}.db"));
        let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        db.writes()
            .replace_files_batch(&[unit("retained.py")])
            .unwrap();
        let before = db.reads().generation().unwrap();
        let retained = snapshot_dependency_tables(&db);
        let mut mode_errors = Vec::new();
        for invalid_manifest in [false, true] {
            let good = snapshot_dependency_unit("first.py", 73);
            let mut failed = snapshot_dependency_unit("failed.py", 73);
            if invalid_manifest {
                failed.outcome.resolution.version = 999;
            }
            let result = db.admin().rebuild_with_temp_db(|conn| {
                if !invalid_manifest {
                    conn.execute_batch(
                        "CREATE TRIGGER fail_dependency BEFORE INSERT ON resolution_dependencies
                         WHEN NEW.file_path='failed.py' AND NEW.key='key-00070'
                         BEGIN SELECT RAISE(ABORT,'injected dependency failure'); END;",
                    )
                    .map_err(|error| cc_model::CcError::Database(error.to_string()))?;
                }
                let files = [good, failed, snapshot_dependency_unit("never.py", 8)];
                if snapshot {
                    cc_db::SnapshotWriteTxn::new(conn)
                        .write_file_data_for_rebuild(&files, &Default::default())?;
                } else {
                    for file in &files {
                        IndexDb::insert_file_data(conn, file)?;
                    }
                }
                Ok(())
            });
            let error = result.unwrap_err().to_string();
            assert!(error.contains(if invalid_manifest {
                "unsupported version"
            } else {
                "injected dependency failure"
            }));
            mode_errors.push(error);
            assert_eq!(db.reads().generation().unwrap(), before);
            assert_eq!(db.reads().list_file_paths().unwrap(), vec!["retained.py"]);
            assert_eq!(snapshot_dependency_tables(&db), retained);
        }
        errors.push(mode_errors);
    }
    assert_eq!(errors[0], errors[1]);
}

#[test]
fn public_snapshot_dependency_failure_preserves_original_catch_and_commit_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let mut observations = Vec::new();
    for snapshot in [false, true] {
        let path = directory
            .path()
            .join(format!("public-prefix-{snapshot}.db"));
        let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE")
            .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_dependency BEFORE INSERT ON resolution_dependencies
             WHEN NEW.file_path='failed.py' AND NEW.key='key-00070'
             BEGIN SELECT RAISE(ABORT,'injected dependency failure'); END;",
        )
        .unwrap();
        let files = [
            snapshot_dependency_unit("first.py", 73),
            snapshot_dependency_unit("failed.py", 73),
            snapshot_dependency_unit("never.py", 8),
        ];
        let result = if snapshot {
            cc_db::SnapshotWriteTxn::new(&conn).write_file_data(&files, &Default::default())
        } else {
            files
                .iter()
                .try_for_each(|file| IndexDb::insert_file_data(&conn, file))
        };
        let error = result.unwrap_err().to_string();
        assert!(error.contains("injected dependency failure"));
        // This existing borrowed public seam leaves transaction ownership with
        // its caller. Catching the error must preserve the original row prefix.
        assert!(!conn.is_autocommit());
        conn.execute_batch("COMMIT").unwrap();
        assert!(conn.is_autocommit());
        drop(conn);

        // Read from a new connection, so these are committed rows rather than
        // an observation of uncommitted state inside the failed writer.
        let committed = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut statement = committed
            .prepare(
                "SELECT key FROM resolution_dependencies \
                 WHERE file_path='failed.py' ORDER BY key",
            )
            .unwrap();
        let keys = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let expected: Vec<_> = (0..70).map(|n| format!("key-{n:05}")).collect();
        assert_eq!(keys, expected);
        assert_eq!(
            committed
                .query_row(
                    "SELECT COUNT(*) FROM resolution_dependencies WHERE file_path='first.py'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            73
        );
        assert_eq!(
            committed
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE file_path='never.py'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        observations.push((error, snapshot_dependency_tables(&db)));
    }
    assert_eq!(observations[0], observations[1]);
}
