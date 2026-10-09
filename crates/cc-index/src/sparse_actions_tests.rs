//! Local action-materialization diagnostic, separate from P8 scale admission.
use super::{FileAction, Indexer};
use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{config::IndexingConfig, Language, ParseOutcome};
use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

fn parsed(path: &str) -> FileWriteUnit {
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Python,
        content_hash: "action-cost-fixture".into(),
        mtime: 1.0,
        size: 1,
        outcome: ParseOutcome::default(),
    }
}

// The four consumers in dirty.rs, dependencies.rs, dirty reload and write.
// Map construction, promotions, filter/clone work and map disposal are timed;
// SQL, filesystem scanning, parsing, resolution and publication are not.
fn consumers(mut actions: HashMap<String, FileAction>, promoted: &[String]) -> [Vec<String>; 4] {
    let changed = actions
        .iter()
        .filter(|(_, a)| matches!(a, FileAction::Add | FileAction::Update))
        .map(|(p, _)| p.clone())
        .collect();
    let added = actions
        .iter()
        .filter(|(_, a)| matches!(a, FileAction::Add))
        .map(|(p, _)| p.clone())
        .collect();
    for path in promoted {
        actions.insert(path.clone(), FileAction::DirtyResolveOnly);
    }
    let reload = actions
        .iter()
        .filter(|(_, a)| matches!(a, FileAction::DirtyResolveOnly))
        .map(|(p, _)| p.clone())
        .collect();
    let write = actions
        .iter()
        .filter(|(_, a)| matches!(a, FileAction::DirtyResolveOnly))
        .map(|(p, _)| p.clone())
        .collect();
    black_box(&actions);
    [changed, added, reload, write]
}

#[test]
#[ignore = "paired local diagnostic; does not certify P8-005/P8-006"]
fn sparse_action_cost_observations() {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(IndexDb::open(&directory.path().join("index.db")).unwrap().0);
    let indexer = Indexer::new(db, directory.path(), &IndexingConfig::default());
    let existing = HashMap::new();
    let mut rows = Vec::new();
    for files in [1_000, 5_000, 10_000, 50_000, 100_000] {
        let paths: Vec<_> = (0..files)
            .map(|i| format!("module_{:05}/file_{i:06}.py", i / 8))
            .collect();
        let scanned: HashSet<String> = paths.iter().cloned().collect();
        for parsed_count in [0, 1, 200] {
            let units: Vec<_> = paths[..parsed_count].iter().map(|p| parsed(p)).collect();
            let promoted = &paths[parsed_count..parsed_count + 200];
            for repetition in 0..30 {
                // AB/BA pairs retain every observation and avoid a fixed
                // winner from always taking the second allocator/cache turn.
                let order = if repetition % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                };
                let mut elapsed = [0; 2];
                let mut outputs = Vec::new();
                for sparse in order {
                    let started = Instant::now();
                    let actions = if sparse {
                        indexer.build_write_actions(black_box(&units), black_box(&existing))
                    } else {
                        indexer.build_actions_map(
                            black_box(&units),
                            black_box(&existing),
                            black_box(&scanned),
                        )
                    };
                    let mut result = consumers(actions, black_box(promoted));
                    elapsed[usize::from(sparse)] = started.elapsed().as_nanos();
                    for paths in &mut result {
                        paths.sort();
                    }
                    outputs.push(result);
                }
                assert_eq!(outputs[0], outputs[1]);
                rows.push(serde_json::json!({
                    "files": files, "parsed_files": parsed_count, "promoted_files": 200,
                    "repetition": repetition, "sparse_first": order[0],
                    "dense_ns": elapsed[0], "sparse_ns": elapsed[1],
                    "dense_owned_actions": files, "sparse_owned_actions": parsed_count + 200
                }));
            }
        }
    }
    let report = serde_json::json!({
        "scope": "action map and four consumers only; no whole-build or P8 scale admission",
        "debug_assertions": cfg!(debug_assertions), "observations": rows
    });
    if let Ok(directory) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        use std::io::Write;
        std::fs::create_dir_all(&directory).unwrap();
        let mut out = std::fs::File::create_new(
            std::path::Path::new(&directory).join("sparse-action-cost.json"),
        )
        .unwrap();
        out.write_all(&serde_json::to_vec_pretty(&report).unwrap())
            .unwrap();
    } else {
        println!("{}", serde_json::to_string(&report).unwrap());
    }
}
