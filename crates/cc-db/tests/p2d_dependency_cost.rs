//! Statement work, row bounds and replacement growth; not total I/O or end-to-end performance.
use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{resolution::*, ImportRecord, Language, ParseOutcome};
use std::collections::BTreeSet;
fn unit(path: String, hot: bool) -> FileWriteUnit {
    let mut outcome = ParseOutcome {
        resolution: ResolutionManifest::new(),
        ..Default::default()
    };
    if hot {
        outcome
            .resolution
            .dependency(DependencyKind::NameBucket, "ping");
        outcome.imports.push(ImportRecord {
            context: Default::default(),
            file_path: path.clone(),
            import_string: "api".into(),
            resolved_path: Some("api.py".into()),
            imported_name: Some("ping".into()),
            alias: None,
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        });
    }
    FileWriteUnit {
        rel_path: path,
        language: Language::Python,
        content_hash: "hash".into(),
        mtime: 1.0,
        size: 1,
        outcome,
    }
}
#[test]
fn p2d_dependency_queries_bound_rows_across_unrelated_scale_and_completed_prefixes() {
    let mut samples = Vec::new();
    for n in [100, 1000, 5000] {
        let d = tempfile::tempdir().unwrap();
        let db = IndexDb::open_with_read_pool_size(&d.path().join("index.db"), 1)
            .unwrap()
            .0;
        let mut units: Vec<_> = (0..n)
            .map(|i| unit(format!("cold/f{i:05}.py"), false))
            .collect();
        units.extend((0..600).map(|i| unit(format!("hot/f{i:05}.py"), true)));
        db.writes().replace_files_batch(&units).unwrap();
        let events = BTreeSet::from([ResolutionDependency::new(
            DependencyKind::NameBucket,
            "ping",
        )]);
        for prefix in [0, 598] {
            let excluded: Vec<_> = (0..prefix).map(|i| format!("hot/f{i:05}.py")).collect();
            let (names, nwork) = db
                .reads()
                .resolution_dependents_with_work(&events, 1, &excluded)
                .unwrap();
            let (surface, swork) = db
                .reads()
                .surface_dependents_bounded(&["api.py".into()], 1, &excluded)
                .unwrap();
            assert_eq!(names, surface);
            assert_eq!(names.len(), 2);
            assert_eq!(names[0], format!("hot/f{prefix:05}.py"));
            assert!(nwork.rows <= 2 + prefix);
            assert!(swork.rows <= 3 * (2 + prefix));
            samples.push(serde_json::json!({"unrelated":n,"completed":prefix,"resolution":nwork,"surface":swork}));
        }
        for generation in 0..32 {
            let mut changed = unit("hot/f00000.py".into(), true);
            changed.outcome.resolution = ResolutionManifest::new();
            changed
                .outcome
                .resolution
                .dependency(DependencyKind::NameBucket, format!("next_{generation}"));
            db.writes().replace_files_batch(&[changed]).unwrap();
            let rows = db
                .reads()
                .query_json("SELECT COUNT(*) AS n FROM resolution_dependencies", &[])
                .unwrap();
            assert_eq!(rows[0]["n"], 600);
        }
        db.writes()
            .remove_files_batch(
                &(0..600)
                    .map(|i| format!("hot/f{i:05}.py"))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(
            db.reads()
                .query_json("SELECT COUNT(*) AS n FROM resolution_dependencies", &[])
                .unwrap()[0]["n"],
            0
        );
    }
    // Adding 4900 unrelated files must not change VM work into a full table scan.
    for prefix in [0, 598] {
        let rows: Vec<_> = samples
            .iter()
            .filter(|r| r["completed"] == prefix)
            .collect();
        for kind in ["resolution", "surface"] {
            let low = rows[0][kind]["vm_steps"].as_u64().unwrap();
            let high = rows[2][kind]["vm_steps"].as_u64().unwrap();
            assert!(
                high <= low * 2 + 100,
                "{kind} unrelated scaling {low}->{high}"
            );
        }
    }
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("p2d-dependency-work.json"),
            serde_json::to_vec_pretty(&samples).unwrap(),
        )
        .unwrap();
    }
}
