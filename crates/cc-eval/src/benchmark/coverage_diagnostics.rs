//! Read-only post-run coverage observations. No query or gold input is accepted.
//! Scanner exclusions explain this candidate's admission policy, not language
//! impossibility or the sufficiency of an indexed file's retrieval evidence.
use super::{invalid, manifest::FileRecord, Result};
use cc_index::{IgnoreRules, Scanner};
use cc_model::config::ProjectConfig;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

pub fn snapshot(work: &Path, files: &[FileRecord], config: &Value) -> Result<Value> {
    let config: ProjectConfig = serde_json::from_value(config.clone())?;
    let database = work.join(".codecortex/index.sqlite3");
    for path in [work.join(".codecortex"), database.clone()] {
        if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(invalid("coverage database must not be a symlink"));
        }
    }
    let sql_error = |e: rusqlite::Error| invalid(format!("coverage snapshot: {e}"));
    let mut connection = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql_error)?;
    let transaction = connection.transaction().map_err(sql_error)?;
    let mut statement = transaction
        .prepare("SELECT file_path, language, parser_tier FROM files ORDER BY file_path")
        .map_err(sql_error)?;
    let observed = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(sql_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    let indexed: BTreeSet<_> = observed.iter().map(|row| row.0.as_str()).collect();
    let admitted: BTreeSet<_> = files.iter().map(|file| file.path.as_str()).collect();
    let unexpected: Vec<_> = indexed.difference(&admitted).copied().collect();
    let (scanned, walked) = Scanner::new(work, &config.indexing).scan_with_manifest();
    let scanned: BTreeSet<_> = scanned.into_iter().map(|file| file.rel_path).collect();
    let walked: BTreeSet<_> = walked.files.into_iter().map(|file| file.rel_path).collect();
    let ignored = IgnoreRules::load(work, &config.indexing);
    let mut reasons = std::collections::BTreeMap::<&str, usize>::new();
    let records: Vec<_> = files
        .iter()
        .map(|file| {
            let exists = indexed.contains(file.path.as_str());
            let reason = if exists {
                "indexed"
            } else if scanned.contains(&file.path) {
                // Actual admission takes precedence over inferred exclusions.
                "scanner_admitted_but_not_indexed_reason_unknown"
            } else if !config.indexing.include_hidden_files
                && file.path.split('/').any(|part| part.starts_with('.'))
            {
                "hidden_path_excluded_by_scanner"
            } else if ignored.is_ignored(&file.path) {
                "configured_ignore"
            } else if file.bytes > config.indexing.max_file_bytes {
                "above_product_file_limit"
            } else if walked.contains(&file.path) {
                "format_not_admitted_by_scanner"
            } else {
                "walk_did_not_observe_path_reason_unknown"
            };
            *reasons.entry(reason).or_default() += 1;
            json!({"path":file.path,"bytes":file.bytes,"input_digest":file.digest,
                "indexed":exists,"scanner_admitted":scanned.contains(&file.path),"reason":reason})
        })
        .collect();
    Ok(json!({
        "schema_version":1,
        "scope":"post-run read-only SQLite files table and the same compiled scanner/config; never a readiness override",
        "source":".codecortex/index.sqlite3",
        "input_files":files.len(),
        "indexed_files":observed.len(),
        "unexpected_indexed_paths":unexpected,
        "reason_counts":reasons,
        "records":records,
        "indexed_file_metadata":observed,
        "retrieval_capability_certified":false,
        "valid_measurement_eligible":false,
    }))
}
