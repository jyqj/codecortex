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
