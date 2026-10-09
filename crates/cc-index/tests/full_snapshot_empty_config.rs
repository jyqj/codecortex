//! Full rebuilds with no heuristic config tokens must still replace old links
//! and persist the gate metadata used by subsequent incremental builds.
use std::{path::Path, sync::Arc};

use cc_db::index_db::IndexDb;
use cc_index::Indexer;
use cc_model::config::IndexingConfig;

fn put(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn config_refs(db: &IndexDb) -> Vec<serde_json::Value> {
    db.reads()
        .query_json(
            "SELECT file_path, target_file_path, ref_kind FROM symbol_refs \
             WHERE ref_kind LIKE 'config_%' ORDER BY file_path, line, ref_kind",
            &[],
        )
        .unwrap()
}

fn assert_empty_cache(db: &IndexDb) {
    assert_eq!(
        db.reads().get_metadata("config_raw_tokens").unwrap(),
        Some("[]".to_string())
    );
    assert_eq!(
        db.reads().get_metadata("last_config_sig_algo").unwrap(),
        Some("2".to_string())
    );
    db.reads()
        .get_metadata("last_config_sig")
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let recorded_at = db.reads().get_metadata("last_indexed_at").unwrap().unwrap();
    chrono::DateTime::parse_from_rfc3339(&recorded_at).unwrap();
}

#[test]
fn empty_config_full_rebuild_clears_old_links_and_can_relink_later() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    put(&root, "src/lib.py", "def lib_handler():\n    return 1\n");
    put(&root, "settings.ini", "script = src/lib.py\n");
    let db = Arc::new(IndexDb::open(&temp.path().join("index.sqlite3")).unwrap().0);
    let indexer = Indexer::new(db.clone(), &root, &IndexingConfig::default());

    indexer.build_index(&root, true).unwrap();
    let linked = config_refs(&db);
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0]["target_file_path"], "src/lib.py");

    put(&root, "settings.ini", "enabled = true\nlimit = 7\n");
    indexer.build_index(&root, true).unwrap();
    assert!(config_refs(&db).is_empty());
    assert_empty_cache(&db);

    put(
        &root,
        "src/lib.py",
        "def lib_handler():\n    return 2\n\ndef extra():\n    return 3\n",
    );
    indexer.build_index(&root, false).unwrap();
    assert!(config_refs(&db).is_empty());
    assert_empty_cache(&db);

    put(&root, "settings.ini", "script = src/lib.py\n");
    indexer.build_index(&root, false).unwrap();
    assert_eq!(config_refs(&db), linked);
}

#[test]
fn typed_module_config_still_resolves_when_heuristic_tokens_are_empty() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    put(
        &root,
        "src/api.ts",
        "export function target() { return 1; }\n",
    );
    put(
        &root,
        "src/use.ts",
        "import { target } from '@api';\nexport function use() { return target(); }\n",
    );
    put(
        &root,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler","baseUrl":".","paths":{"@api":["src/api.ts"]}}}"#,
    );
    put(
        &root,
        "settings.yaml",
        "service: svc_1\nenabled: true\nlimit: 7\n",
    );
    let db = Arc::new(IndexDb::open(&temp.path().join("index.sqlite3")).unwrap().0);
    let indexer = Indexer::new(db.clone(), &root, &IndexingConfig::default());

    indexer.build_index(&root, true).unwrap();
    assert_empty_cache(&db);
    assert!(config_refs(&db).is_empty());
    let calls = db
        .reads()
        .query_json(
            "SELECT target_file_path FROM call_edges \
             WHERE file_path = 'src/use.ts' AND callee_symbol = 'target'",
            &[],
        )
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["target_file_path"], "src/api.ts");
}
