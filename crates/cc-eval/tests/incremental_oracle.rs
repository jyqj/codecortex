//! Persistent incremental session vs independent full snapshots after each edit.
//! Maintenance parity here does not establish parser semantic completeness.
use cc_db::index_db::IndexDb;
use cc_eval::runner::CodeIndexBackend;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

fn materialize(root: &Path, files: &BTreeMap<String, String>) {
    for (path, text) in files {
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }
}
fn snapshot(root: &Path) -> Value {
    let db = IndexDb::open(&root.join(".codecortex/index.sqlite3"))
        .unwrap()
        .0;
    let queries=[
        ("lookups","SELECT file_path,kind,lookup_key FROM lookup_dependencies ORDER BY file_path,kind,lookup_key"),
        ("symbols","SELECT file_path, name, kind, qname, signature, symbol_uid, start_line, end_line FROM symbols ORDER BY file_path, start_line, name, symbol_uid"),
        ("imports","SELECT file_path, import_string, resolved_path, imported_name, alias, is_reexport FROM imports ORDER BY file_path, import_string, imported_name, alias"),
        ("calls","SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, target_file_path, resolution_kind, resolution_strategy FROM call_edges ORDER BY file_path, line, caller_symbol, callee_symbol, callee_symbol_uid"),
        ("refs","SELECT file_path, line, column_no, symbol_name, target_symbol_uid, target_file_path, resolution_kind, resolution_strategy FROM symbol_refs ORDER BY file_path, line, column_no, symbol_name"),
        ("chunks","SELECT chunk_id, file_path, start_line, end_line, symbol_name FROM chunks ORDER BY file_path, start_line, chunk_id"),
    ];
    let mut result = serde_json::Map::new();
    for (name, sql) in queries {
        result.insert(name.into(), json!(db.reads().query_json(sql, &[]).unwrap()));
    }
    Value::Object(result)
}
fn exercise(api_path: &str, caller_path: &str, original: &str, caller: &str, changed: &str) {
    let live = TempDir::new().unwrap();
    let mut files = BTreeMap::from([
        (api_path.to_owned(), original.to_owned()),
        (caller_path.to_owned(), caller.to_owned()),
    ]);
    materialize(live.path(), &files);
    let backend = CodeIndexBackend::new(live.path()).unwrap();
    let initial = snapshot(live.path());
    assert!(
        initial["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["file_path"] == caller_path
                && edge["callee_symbol_uid"].as_str().is_some()),
        "fixture must have an initially resolved cross-file call: {initial:#}"
    );
    for replacement in [Some(changed), None] {
        let report = if let Some(text) = replacement {
            files.insert(api_path.into(), text.into());
            std::fs::write(live.path().join(api_path), text).unwrap();
            backend
                .build_index_report_scoped(&[api_path.into()])
                .unwrap()
        } else {
            files.remove(api_path);
            std::fs::remove_file(live.path().join(api_path)).unwrap();
            backend
                .call_tool("index", &json!({"full":false,"removed_paths":[api_path]}))
                .unwrap()
        };
        assert_eq!(report["dirty_propagation"], "normal", "{report:#}");
        let incremental = snapshot(live.path());
        let fresh = TempDir::new().unwrap();
        materialize(fresh.path(), &files);
        let full = CodeIndexBackend::new_unindexed(fresh.path()).unwrap();
        full.build_index_report(true).unwrap();
        let rebuilt = snapshot(fresh.path());
        for section in ["symbols", "imports", "calls", "refs", "chunks"] {
            let actual = incremental[section].as_array().unwrap();
            let expected = rebuilt[section].as_array().unwrap();
            assert_eq!(
                actual.len(),
                expected.len(),
                "{section} cardinality after {api_path}: {replacement:?}"
            );
            for (row, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(a, b, "{section}[{row}] after {api_path}: {replacement:?}");
            }
        }
    }
}
#[test]
fn typescript_signature_and_deletion_match_full_rebuild() {
    exercise("api.ts","caller.ts",
      "export function compute(value: number): number { return value + 1; }\n",
      "import { compute } from './api';\nexport function caller() { return compute(2); }\n",
      "export function compute(value: number, scale: number = 3): number { return value * scale; }\n");
}
#[test]
fn python_unknown_export_surface_is_not_treated_as_unchanged() {
    exercise(
        "api.py",
        "caller.py",
        "def compute(value):\n    return value + 1\n",
        "from api import compute\n\ndef caller():\n    return compute(2)\n",
        "def compute(value, scale=3):\n    return value * scale\n",
    );
}
#[test]
fn rust_unknown_export_surface_is_not_treated_as_unchanged() {
    exercise(
        "api.rs",
        "lib.rs",
        "pub fn compute(value: i32) -> i32 { value + 1 }\n",
        "mod api;\nuse crate::api::compute;\npub fn caller() -> i32 { compute(2) }\n",
        "pub fn compute(value: i64) -> i64 { value * 3 }\n",
    );
}

fn assert_full_parity(root: &Path, files: &BTreeMap<String, String>) {
    let fresh = TempDir::new().unwrap();
    materialize(fresh.path(), files);
    let full = CodeIndexBackend::new_unindexed(fresh.path()).unwrap();
    full.build_index_report(true).unwrap();
    let a = snapshot(root);
    let b = snapshot(fresh.path());
    for section in ["symbols", "calls", "refs", "imports", "lookups", "chunks"] {
        assert_eq!(a[section], b[section], "{section}");
    }
}
#[test]
fn negative_names_and_competing_candidates_are_incrementally_revisited() {
    let live = TempDir::new().unwrap();
    let mut files = BTreeMap::from([(
        "caller.ts".into(),
        "export function caller() { return newCapability(2); }\n".into(),
    )]);
    materialize(live.path(), &files);
    let backend = CodeIndexBackend::new(live.path()).unwrap();
    assert!(snapshot(live.path())["calls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["callee_symbol_uid"].is_null()));
    for (path, text) in [
        (
            "api.ts",
            Some("export function newCapability(x: number) { return x + 1; }\n"),
        ),
        (
            "other.ts",
            Some("export function newCapability(x: number) { return x - 1; }\n"),
        ),
        ("api.ts", None),
        ("other.ts", None),
    ] {
        if let Some(text) = text {
            files.insert(path.into(), text.into());
            materialize(live.path(), &files);
            backend.build_index_report_scoped(&[path.into()]).unwrap();
        } else {
            files.remove(path);
            std::fs::remove_file(live.path().join(path)).unwrap();
            backend
                .call_tool("index", &json!({"full":false,"removed_paths":[path]}))
                .unwrap();
        }
        if path == "api.ts" && text.is_some() {
            assert!(
                snapshot(live.path())["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["callee_symbol_uid"].is_string()),
                "newly available function must be bound"
            );
        }
        assert_full_parity(live.path(), &files);
    }
}
#[test]
fn newly_available_python_module_refreshes_negative_import_lookup() {
    let live = TempDir::new().unwrap();
    let mut files = BTreeMap::from([(
        "caller.py".into(),
        "from missing import compute\ndef caller():\n    return compute(2)\n".into(),
    )]);
    materialize(live.path(), &files);
    let backend = CodeIndexBackend::new(live.path()).unwrap();
    files.insert(
        "missing.py".into(),
        "def compute(x):\n    return x + 1\n".into(),
    );
    materialize(live.path(), &files);
    backend
        .build_index_report_scoped(&["missing.py".into()])
        .unwrap();
    assert!(snapshot(live.path())["calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["callee_symbol_uid"].is_string()));
    assert_full_parity(live.path(), &files);
}
#[test]
fn no_op_does_not_erase_incomplete_resolution_freshness() {
    let live = TempDir::new().unwrap();
    std::fs::write(
        live.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":0}}"#,
    )
    .unwrap();
    let mut files = BTreeMap::from([(
        "caller.ts".into(),
        "export function caller() { return newCapability(2); }\n".into(),
    )]);
    materialize(live.path(), &files);
    let backend = CodeIndexBackend::new_unindexed(live.path()).unwrap();
    backend.build_index_report(true).unwrap();
    let freshness = || {
        let db = IndexDb::open(&live.path().join(".codecortex/index.sqlite3"))
            .unwrap()
            .0;
        db.reads()
            .get_metadata(cc_db::RESOLUTION_FRESHNESS_KEY)
            .unwrap()
            .unwrap()
    };
    assert_eq!(freshness(), "complete");
    files.insert(
        "api.ts".into(),
        "export function newCapability(x: number) { return x; }\n".into(),
    );
    materialize(live.path(), &files);
    backend
        .build_index_report_scoped(&["api.ts".into()])
        .unwrap();
    assert_eq!(freshness(), "incomplete");
    let report = backend.build_index_report(false).unwrap();
    assert_eq!(freshness(), "incomplete");
    assert_ne!(report["dirty_propagation"], "normal");
    backend.build_index_report(true).unwrap();
    assert_eq!(freshness(), "complete");
    assert_full_parity(live.path(), &files);
}

#[test]
fn reexports_aliases_and_unresolved_imports_are_module_authoritative() {
    let live = TempDir::new().unwrap();
    let mut files=BTreeMap::from([
        ("a.ts".into(),"export function first() {return 1;}\nexport function second() {return 2;}\nexport default function Default() {return 3;}\n".into()),
        ("barrel.ts".into(),"export { first as forwarded } from './a';\nexport * from './a';\n".into()),
        ("caller.ts".into(),"import Def, { first as local, second } from './a';\nimport { forwarded } from './barrel';\nimport { missing } from './absent';\nexport function run() { return [Def(), local(), second(), forwarded(), missing()]; }\n".into()),
        ("decoy.ts".into(),"export function missing() {return 99;}\n".into()),
    ]);
    materialize(live.path(), &files);
    let backend = CodeIndexBackend::new(live.path()).unwrap();
    let s = snapshot(live.path());
    for edge in s["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["file_path"] == "caller.ts")
    {
        if edge["callee_symbol"] == "missing" {
            assert!(edge["callee_symbol_uid"].is_null(), "{edge:#}");
        } else {
            assert_eq!(edge["target_file_path"], "a.ts", "{edge:#}");
        }
    }
    files.insert(
        "barrel.ts".into(),
        "export { second as forwarded } from './a';\nexport * from './a';\n".into(),
    );
    materialize(live.path(), &files);
    backend
        .build_index_report_scoped(&["barrel.ts".into()])
        .unwrap();
    assert_full_parity(live.path(), &files);
}
