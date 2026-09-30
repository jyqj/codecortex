//! Release-only mechanism costs on disposable source trees. Raw observations,
//! not a tail-latency/100k/whole-system resource certification.
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::{path::Path, time::Instant};
fn put(root: &Path, path: &str, text: &str) {
    std::fs::write(root.join(path), text).unwrap();
}
fn rows(root: &Path) -> Value {
    let c = rusqlite::Connection::open_with_flags(
        root.join(".codecortex/index.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count = |table: &str| {
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
    };
    json!({"files":count("files"),"symbols":count("symbols"),"dependency_rows":count("resolution_dependencies"),"frontier_rows":count("resolution_frontier"),"db_bytes":std::fs::metadata(root.join(".codecortex/index.sqlite3")).unwrap().len()})
}
#[test]
#[ignore = "release mechanism cost; explicit execution required"]
// This test must compile in debug CI but only run with an explicit release profile.
#[allow(clippy::assertions_on_constants)]
fn p2d_release_incremental_growth_and_bounded_resume() {
    assert!(!cfg!(debug_assertions), "requires --release");
    let mut observations = Vec::new();
    for n in [1000, 5000] {
        let d = tempfile::tempdir().unwrap();
        put(
            d.path(),
            ".codecortex.json",
            r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":5,"db_read_pool_size":1}}"#,
        );
        for i in 0..n {
            put(
                d.path(),
                &format!("f{i:05}.py"),
                &format!("def cold{i}():\n    return {i}\n"),
            );
        }
        for i in 0..25 {
            put(
                d.path(),
                &format!("use{i:02}.py"),
                "def entry(x):\n    return ping(x)\n",
            );
        }
        put(d.path(), "api.py", "def ping(x):\n    return x\n");
        let mut index = CodeIndex::new(Some(d.path())).unwrap();
        let start = Instant::now();
        let full = index.build_index(true).unwrap();
        observations.push(json!({"n":n,"kind":"full","elapsed_us":start.elapsed().as_micros(),"report":full,"storage":rows(d.path())}));
        let baseline = rows(d.path());
        for iteration in 0..12 {
            let start = Instant::now();
            let noop = index.build_index(false).unwrap();
            assert_eq!(noop.files_parsed, 0);
            assert_eq!(noop.dirty_plan.selected_dependents, 0);
            assert_eq!(noop.dirty_plan.dependency_sql.statements, 0);
            observations.push(json!({"n":n,"kind":"noop","iteration":iteration,"elapsed_us":start.elapsed().as_micros(),"report":noop}));
            put(
                d.path(),
                "api.py",
                &format!("def ping(x):\n    return x+{iteration}\n"),
            );
            let start = Instant::now();
            let body = index.build_index(false).unwrap();
            assert_eq!(body.files_parsed, 1);
            assert_eq!(body.dirty_plan.selected_dependents, 0);
            observations.push(json!({"n":n,"kind":"body","iteration":iteration,"elapsed_us":start.elapsed().as_micros(),"report":body}));
            put(
                d.path(),
                "api.py",
                &format!("def ping(x,extra{iteration}=0):\n    return x\n"),
            );
            for tick in 0..8 {
                let start = Instant::now();
                let r = index.build_index(false).unwrap();
                assert!(r.dirty_plan.selected_dependents <= 5);
                let complete = r.resolution_freshness.complete;
                observations.push(json!({"n":n,"kind":"api_resume","iteration":iteration,"tick":tick,"elapsed_us":start.elapsed().as_micros(),"report":r,"storage":rows(d.path())}));
                if complete {
                    break;
                }
                assert!(tick < 7, "must finish finite fanout");
            }
            put(d.path(), "api.py", "def ping(x):\n    return x\n");
            for _ in 0..8 {
                if index
                    .build_index(false)
                    .unwrap()
                    .resolution_freshness
                    .complete
                {
                    break;
                }
            }
            let storage = rows(d.path());
            assert_eq!(storage["dependency_rows"], baseline["dependency_rows"]);
            assert_eq!(storage["frontier_rows"], 0);
        }
    }
    let out = std::env::var("CODECORTEX_BENCH_OBSERVATIONS").expect("observations path required");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        Path::new(&out).join("p2d-release-cost.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
}
