//! Persistent incremental session vs independent full snapshots after each edit.
//! Maintenance parity here does not establish parser semantic completeness.
use std::collections::BTreeMap;
use std::path::Path;
use cc_db::index_db::IndexDb;
use cc_eval::runner::CodeIndexBackend;
use serde_json::{json,Value};
use tempfile::TempDir;

fn materialize(root:&Path,files:&BTreeMap<String,String>) {
    for (path,text) in files {
        let target=root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target,text).unwrap();
    }
}
fn snapshot(root:&Path) -> Value {
    let db=IndexDb::open(&root.join(".codecortex/index.sqlite3")).unwrap().0;
    let queries=[
        ("symbols","SELECT file_path, name, kind, qname, signature, symbol_uid, start_line, end_line FROM symbols ORDER BY file_path, start_line, name, symbol_uid"),
        ("imports","SELECT file_path, import_string, resolved_path, imported_name, alias, is_reexport FROM imports ORDER BY file_path, import_string, imported_name, alias"),
        ("calls","SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, target_file_path, resolution_kind, resolution_strategy FROM call_edges ORDER BY file_path, line, caller_symbol, callee_symbol, callee_symbol_uid"),
        ("refs","SELECT file_path, line, column_no, symbol_name, target_symbol_uid, target_file_path, resolution_kind, resolution_strategy FROM symbol_refs ORDER BY file_path, line, column_no, symbol_name"),
        ("chunks","SELECT chunk_id, file_path, start_line, end_line, symbol_name FROM chunks ORDER BY file_path, start_line, chunk_id"),
    ];
    let mut result=serde_json::Map::new();
    for (name,sql) in queries { result.insert(name.into(),json!(db.reads().query_json(sql,&[]).unwrap())); }
    Value::Object(result)
}
fn exercise(api_path:&str, caller_path:&str, original:&str, caller:&str, changed:&str) {
    let live=TempDir::new().unwrap();
    let mut files=BTreeMap::from([(api_path.to_owned(),original.to_owned()),(caller_path.to_owned(),caller.to_owned())]);
    materialize(live.path(),&files);
    let backend=CodeIndexBackend::new(live.path()).unwrap();
    let initial=snapshot(live.path());
    assert!(initial["calls"].as_array().unwrap().iter().any(|edge|
        edge["file_path"]==caller_path && edge["callee_symbol_uid"].as_str().is_some()),
        "fixture must have an initially resolved cross-file call: {initial:#}");
    for replacement in [Some(changed),None] {
        let report=if let Some(text)=replacement {
            files.insert(api_path.into(),text.into());
            std::fs::write(live.path().join(api_path),text).unwrap();
            backend.build_index_report_scoped(&[api_path.into()]).unwrap()
        } else {
            files.remove(api_path);
            std::fs::remove_file(live.path().join(api_path)).unwrap();
            backend.call_tool("index",&json!({"full":false,"removed_paths":[api_path]})).unwrap()
        };
        assert_eq!(report["dirty_propagation"],"normal","{report:#}");
        let incremental=snapshot(live.path());
        let fresh=TempDir::new().unwrap(); materialize(fresh.path(),&files);
        let full=CodeIndexBackend::new_unindexed(fresh.path()).unwrap();
        full.build_index_report(true).unwrap();
        let rebuilt=snapshot(fresh.path());
        assert_eq!(incremental,rebuilt,"incremental differs after editing {api_path}: {replacement:?}");
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
    exercise("api.py","caller.py",
      "def compute(value):\n    return value + 1\n",
      "from api import compute\n\ndef caller():\n    return compute(2)\n",
      "def compute(value, scale=3):\n    return value * scale\n");
}
#[test]
fn rust_unknown_export_surface_is_not_treated_as_unchanged() {
    exercise("api.rs","lib.rs",
      "pub fn compute(value: i32) -> i32 { value + 1 }\n",
      "mod api;\nuse crate::api::compute;\npub fn caller() -> i32 { compute(2) }\n",
      "pub fn compute(value: i64) -> i64 { value * 3 }\n");
}
