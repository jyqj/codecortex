//! White-box diagnostic oracle plus independent public-protocol probes.
//! This is not reported as the black-box retrieval benchmark.
use super::{
    invalid,
    manifest::{self, LoadedSuite},
    mutations::MutationPlan,
    normalizer, report, Result,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
const TABLES: &[&str] = &[
    "document_manifest",
    "resolution_frontier",
    "semantic_edges",
    "dispatch_sites",
    "resolution_manifests",
    "resolution_dependencies",
    "public_surfaces",
    "files",
    "symbols",
    "imports",
    "symbol_refs",
    "call_edges",
    "chunks",
    "test_edges",
    "routes",
];
mod streaming;
pub use streaming::{compare_streaming, compare_streaming_scale_capacity_v1, StreamingLimits};

fn db_error(e: rusqlite::Error) -> super::BenchError {
    super::BenchError::Protocol(format!("oracle DB: {e}"))
}
/// The exact comparison surface; no target identity or strategy projection.
pub fn tables() -> &'static [&'static str] {
    TABLES
}
/// Schema introspection also validates zero-row truth assertions.
pub fn table_columns(root: &Path, table: &str) -> Result<std::collections::BTreeSet<String>> {
    if !TABLES.contains(&table) {
        return Err(invalid("unknown oracle table"));
    }
    let conn = Connection::open_with_flags(
        root.join(".codecortex/index.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(db_error)?;
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(db_error)?;
    let columns = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(db_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_error)?;
    Ok(columns)
}
fn snapshot(root: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(
        root.join(".codecortex/index.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(db_error)?;
    // Keep schema, integrity checks and every table in the same read snapshot.
    c.execute_batch("BEGIN DEFERRED TRANSACTION")
        .map_err(db_error)?;
    let integrity: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(db_error)?;
    if integrity != "ok" {
        return Err(invalid("oracle integrity_check failed"));
    }
    let fk: i64 = c
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(db_error)?;
    if fk != 0 {
        return Err(invalid("oracle foreign_key_check failed"));
    }
    Ok(c)
}

fn columns(c: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = c
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(db_error)?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(db_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_error)?;
    // One projection for both diagnostic implementations. No target identity,
    // strategy, semantic payload, multiplicity or table is discarded.
    let cols: Vec<String> = cols
        .into_iter()
        .filter(|col| {
            !(table == "files" && matches!(col.as_str(), "mtime" | "indexed_at")
                || table == "imports" && col == "id")
        })
        .collect();
    if cols.is_empty() {
        return Err(invalid(format!("oracle table missing: {table}")));
    }
    Ok(cols)
}

fn select_columns(cols: &[String]) -> String {
    cols.iter()
        .map(|n| format!("\"{}\"", n.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",")
}

fn project_row(row: &rusqlite::Row<'_>, cols: &[String]) -> Result<Value> {
    let mut values = serde_json::Map::new();
    for (i, col) in cols.iter().enumerate() {
        let v = match row.get_ref(i).map_err(db_error)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(v) => json!(v),
            ValueRef::Real(v) => json!(v),
            ValueRef::Text(v) => {
                json!(std::str::from_utf8(v).map_err(|_| invalid("oracle UTF8"))?)
            }
            ValueRef::Blob(v) => json!({"blob_digest":manifest::digest(v)}),
        };
        values.insert(col.clone(), v);
    }
    Ok(Value::Object(values))
}

pub fn canonical(root: &Path) -> Result<BTreeMap<String, Vec<Value>>> {
    let c = snapshot(root)?;
    let mut result = BTreeMap::new();
    for table in TABLES {
        let cols = columns(&c, table)?;
        let select = select_columns(&cols);
        let mut stmt = c
            .prepare(&format!("SELECT {select} FROM \"{table}\""))
            .map_err(db_error)?;
        let mut cursor = stmt.query([]).map_err(db_error)?;
        let mut rows = Vec::new();
        while let Some(row) = cursor.next().map_err(db_error)? {
            if rows.len() > 100000 {
                return Err(invalid("P0 diagnostic oracle row budget exceeded"));
            }
            rows.push(project_row(row, &cols)?);
        }
        // The serialized row is the original ordering key, not an equality
        // digest. Cache it once per row instead of serializing both operands
        // on every comparison (especially expensive for manifest payloads).
        // The keys are released after this table is sorted; projection,
        // duplicate multiplicity and the subsequent Value comparison stay
        // unchanged, including the original signed-zero ordering.
        rows.sort_by_cached_key(|v| serde_json::to_string(v).unwrap_or_default());
        result.insert((*table).to_string(), rows);
    }
    Ok(result)
}
pub fn run(loaded: &LoadedSuite, plan: &MutationPlan, out: &Path) -> Result<Value> {
    if plan.schema_version != 1 || plan.steps.is_empty() {
        return Err(invalid("mutation plan version/steps"));
    }
    let a = manifest::materialize(loaded)?;
    let b = manifest::materialize(loaded)?;
    let aa = crate::runner::CodeIndexBackend::new(a.path()).map_err(|e| invalid(e.to_string()))?;
    let bb = crate::runner::CodeIndexBackend::new(b.path()).map_err(|e| invalid(e.to_string()))?;
    std::fs::create_dir_all(out)?;
    let mut checkpoints = Vec::new();
    for (i, op) in plan.steps.iter().enumerate() {
        op.apply(a.path())?;
        op.apply(b.path())?;
        let inc = aa
            .build_index_report(false)
            .map_err(|e| invalid(e.to_string()))?;
        let full = bb
            .build_index_report(true)
            .map_err(|e| invalid(e.to_string()))?;
        let ca = canonical(a.path())?;
        let cb = canonical(b.path())?;
        let different: Vec<&str> = TABLES
            .iter()
            .copied()
            .filter(|t| ca.get(*t) != cb.get(*t))
            .collect();
        let mut probes = Vec::new();
        for p in &plan.probes {
            if p.query.is_empty() || p.top_k == 0 || p.top_k > 200 {
                return Err(invalid("invalid oracle probe"));
            }
            let mut args = json!({"query":p.query,"top_k":p.top_k,"mode":"hybrid"});
            if let Some(prefix) = &p.path_prefix {
                args["path_prefix"] = json!(prefix);
            }
            let ra = aa
                .call_tool("search", &args)
                .map_err(|e| invalid(e.to_string()))?;
            let rb = bb
                .call_tool("search", &args)
                .map_err(|e| invalid(e.to_string()))?;
            let paths = |v: &Value| {
                normalizer::mcp(v).map(|(h, _)| {
                    h.into_iter()
                        .map(|h| (h.path, h.start_line, h.end_line))
                        .collect::<Vec<_>>()
                })
            };
            probes.push(json!({"query":p.query,"same_locations":paths(&ra)?==paths(&rb)?,"incremental":ra,"full":rb}));
        }
        report::json(&out.join(format!("checkpoint-{i}-incremental.json")), &ca)?;
        report::json(&out.join(format!("checkpoint-{i}-full.json")), &cb)?;
        checkpoints.push(json!({"step":i,"mutation":op,"equal":different.is_empty(),"different_tables":different,"incremental_report":inc,"full_report":full,"probes":probes}));
    }
    let equal = checkpoints.iter().all(|v| {
        v["equal"] == true
            && v["probes"]
                .as_array()
                .is_some_and(|a| a.iter().all(|p| p["same_locations"] == true))
    });
    let result = json!({"schema_version":1,"evidence_layer":"in_process_MCP_plus_read_only_SQL_diagnostic","tables":TABLES,"excluded_columns":{"files":["mtime","indexed_at"],"imports":["id"]},"equal":equal,"exit_code":if equal{0}else{1},"checkpoints":checkpoints});
    report::json(&out.join("oracle.json"), &result)?;
    Ok(result)
}
