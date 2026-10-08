//! Disk-backed comparison of the complete existing canonical oracle surface.
//!
//! SQLite orders serialized canonical rows, including duplicates. Both sorted
//! streams retain the original canonical Value equality before their digests
//! are reported (in particular, -0.0 and +0.0 compare equal). The
//! disposable index bounds sorting memory without introducing a second fact
//! projection, sampling rows, or increasing the legacy in-memory row budget.
use super::{columns, db_error, project_row, select_columns, snapshot, TABLES};
use crate::benchmark::{invalid, manifest, Result};
use rusqlite::{types::ValueRef, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StreamingLimits {
    pub max_rows_per_table: u64,
    /// Aggregate serialized row bytes across both inputs and every table.
    pub max_canonical_bytes: u64,
    /// Hard SQLite page limit for the disposable canonical row store.
    pub max_scratch_bytes: u64,
    pub max_row_bytes: usize,
}

impl Default for StreamingLimits {
    fn default() -> Self {
        Self {
            max_rows_per_table: 5_000_000,
            max_canonical_bytes: 4 * 1024 * 1024 * 1024,
            max_scratch_bytes: 8 * 1024 * 1024 * 1024,
            max_row_bytes: 1024 * 1024,
        }
    }
}

impl StreamingLimits {
    fn validate(&self) -> Result<()> {
        if !(1..=10_000_000).contains(&self.max_rows_per_table)
            || !(1..=8 * 1024 * 1024 * 1024).contains(&self.max_canonical_bytes)
            || !(64 * 1024..=16 * 1024 * 1024 * 1024).contains(&self.max_scratch_bytes)
            || !(1..=4 * 1024 * 1024).contains(&self.max_row_bytes)
        {
            return Err(invalid("invalid streaming oracle resource limits"));
        }
        Ok(())
    }
}

fn check_row_size(row: &rusqlite::Row<'_>, cols: &[String], limit: usize) -> Result<()> {
    let mut bytes = 0usize;
    for (i, col) in cols.iter().enumerate() {
        let size = match row.get_ref(i).map_err(db_error)? {
            ValueRef::Text(v) | ValueRef::Blob(v) => v.len(),
            _ => 16,
        };
        bytes = bytes.saturating_add(col.len()).saturating_add(size);
        if bytes > limit {
            return Err(invalid("streaming oracle input row byte budget exceeded"));
        }
    }
    Ok(())
}

fn spool(
    source: &Connection,
    scratch: &mut Connection,
    table: &str,
    side: u8,
    limits: StreamingLimits,
    canonical_bytes: &mut u64,
) -> Result<u64> {
    let cols = columns(source, table)?;
    let mut statement = source
        .prepare(&format!(
            "SELECT {} FROM \"{table}\"",
            select_columns(&cols)
        ))
        .map_err(db_error)?;
    let mut cursor = statement.query([]).map_err(db_error)?;
    let transaction = scratch.transaction().map_err(db_error)?;
    let mut insert = transaction
        .prepare("INSERT INTO canonical_rows(side, value, ordinal) VALUES (?1, ?2, ?3)")
        .map_err(db_error)?;
    let mut count = 0;
    while let Some(row) = cursor.next().map_err(db_error)? {
        if count >= limits.max_rows_per_table {
            return Err(invalid(format!(
                "streaming oracle row budget exceeded: {table} side {side}"
            )));
        }
        check_row_size(row, &cols, limits.max_row_bytes)?;
        let serialized = serde_json::to_string(&project_row(row, &cols)?)?;
        if serialized.len() > limits.max_row_bytes {
            return Err(invalid(
                "streaming oracle serialized row byte budget exceeded",
            ));
        }
        *canonical_bytes = canonical_bytes
            .checked_add(serialized.len() as u64)
            .ok_or_else(|| invalid("streaming oracle byte count overflow"))?;
        if *canonical_bytes > limits.max_canonical_bytes {
            return Err(invalid("streaming oracle canonical byte budget exceeded"));
        }
        let ordinal = i64::try_from(count)
            .map_err(|_| invalid("streaming oracle ordinal exceeds SQLite integer range"))?;
        insert
            .execute((side, serialized, ordinal))
            .map_err(db_error)?;
        count += 1;
    }
    drop(insert);
    transaction.commit().map_err(db_error)?;
    Ok(count)
}

fn next_row(rows: &mut rusqlite::Rows<'_>) -> Result<Option<String>> {
    rows.next()
        .map_err(db_error)?
        .map(|row| row.get::<_, String>(0).map_err(db_error))
        .transpose()
}

fn equal_rows(a: Option<&str>, b: Option<&str>) -> Result<bool> {
    if a == b {
        return Ok(true);
    }
    match (a, b) {
        (Some(a), Some(b)) => {
            // The legacy oracle sorts serialized rows but compares Value.
            // Keep both operations: normalizing signed zero before sorting
            // would change ordering, while text-only equality changes truth.
            Ok(serde_json::from_str::<Value>(a)? == serde_json::from_str::<Value>(b)?)
        }
        _ => Ok(false),
    }
}

fn append_digest(hasher: &mut blake3::Hasher, row: Option<&str>, position: u64) {
    if let Some(row) = row {
        if position != 0 {
            hasher.update(b",");
        }
        hasher.update(row.as_bytes());
    }
}

fn example(row: Option<&str>) -> Result<Value> {
    match row {
        None => Ok(Value::Null),
        Some(row) if row.len() <= 16 * 1024 => Ok(serde_json::from_str(row)?),
        Some(row) => Ok(json!({
            "example_body_omitted": true,
            "canonical_bytes": row.len(),
            "canonical_digest": manifest::digest(row.as_bytes()),
        })),
    }
}

/// Compare all fifteen tables under independent, consistent read snapshots.
/// Scratch is private and removed on both success and failure. Limit exhaustion,
/// malformed databases and disk errors are errors, never equality or a sample.
pub fn compare_streaming(a: &Path, b: &Path, limits: StreamingLimits) -> Result<Value> {
    limits.validate()?;
    compare_with_validated_limits(a, b, limits, None)
}

/// Explicit capacity for the registered P8 scale_capacity_v1 workload. This
/// does not widen the default oracle or its general configurable limit range.
/// The cumulative byte allowance covers both sides of all fifteen tables;
/// the simultaneously occupied scratch, per-row and per-table bounds stay put.
pub fn compare_streaming_scale_capacity_v1(a: &Path, b: &Path) -> Result<Value> {
    compare_with_validated_limits(
        a,
        b,
        StreamingLimits {
            max_canonical_bytes: 16 * 1024 * 1024 * 1024,
            ..StreamingLimits::default()
        },
        Some("scale_capacity_v1"),
    )
}

const SORTED_ROWS_SQL: &str =
    "SELECT value FROM canonical_rows WHERE side=?1 ORDER BY value COLLATE BINARY";

fn open_scratch(path: &Path, limits: StreamingLimits) -> Result<Connection> {
    let scratch = Connection::open(path).map_err(db_error)?;
    // OFF is appropriate only for this disposable, non-authoritative spool.
    // The primary B-tree is the sole row store and also serves ordered reads;
    // no second copy of canonical payloads or external sorting file is needed.
    // The ordinal keeps every duplicate. Equal serialized keys have equal
    // values, so the tie order cannot change the original Value comparison.
    scratch
        .execute_batch(&format!(
            "PRAGMA page_size=4096;
             PRAGMA journal_mode=OFF;
             PRAGMA synchronous=OFF;
             PRAGMA temp_store=FILE;
             PRAGMA cache_size=-2048;
             PRAGMA mmap_size=0;
             PRAGMA max_page_count={};
             CREATE TABLE canonical_rows(
                 side INTEGER NOT NULL,
                 value TEXT COLLATE BINARY NOT NULL,
                 ordinal INTEGER NOT NULL,
                 PRIMARY KEY(side, value, ordinal)
             ) WITHOUT ROWID;",
            limits.max_scratch_bytes / 4096,
        ))
        .map_err(db_error)?;
    Ok(scratch)
}

fn compare_with_validated_limits(
    a: &Path,
    b: &Path,
    limits: StreamingLimits,
    capacity_profile: Option<&str>,
) -> Result<Value> {
    let a = snapshot(a)?;
    let b = snapshot(b)?;
    let directory = tempfile::tempdir()?;
    let scratch_path = directory.path().join("canonical.sqlite3");
    let mut scratch = open_scratch(&scratch_path, limits)?;
    let mut tables = Vec::new();
    let mut different = Vec::new();
    let mut canonical_bytes = 0u64;
    for table in TABLES {
        scratch
            .execute("DELETE FROM canonical_rows", [])
            .map_err(db_error)?;
        let ac = spool(&a, &mut scratch, table, 0, limits, &mut canonical_bytes)?;
        let bc = spool(&b, &mut scratch, table, 1, limits, &mut canonical_bytes)?;
        let mut astmt = scratch.prepare(SORTED_ROWS_SQL).map_err(db_error)?;
        let mut bstmt = scratch.prepare(SORTED_ROWS_SQL).map_err(db_error)?;
        let mut arows = astmt.query([0]).map_err(db_error)?;
        let mut brows = bstmt.query([1]).map_err(db_error)?;
        let mut ah = blake3::Hasher::new();
        let mut bh = blake3::Hasher::new();
        ah.update(b"[");
        bh.update(b"[");
        let mut position = 0u64;
        let mut different_rows = 0u64;
        let mut examples = Vec::new();
        loop {
            let av = next_row(&mut arows)?;
            let bv = next_row(&mut brows)?;
            if av.is_none() && bv.is_none() {
                break;
            }
            if !equal_rows(av.as_deref(), bv.as_deref())? {
                different_rows += 1;
                if examples.len() < 3 {
                    examples.push(json!({
                        "sorted_row": position,
                        "incremental": example(av.as_deref())?,
                        "full": example(bv.as_deref())?,
                    }));
                }
            }
            append_digest(&mut ah, av.as_deref(), position);
            append_digest(&mut bh, bv.as_deref(), position);
            position += 1;
        }
        ah.update(b"]");
        bh.update(b"]");
        if different_rows != 0 {
            different.push(*table);
        }
        tables.push(json!({
            "table": table,
            "equal": different_rows == 0,
            "incremental_rows": ac,
            "full_rows": bc,
            "incremental_digest": ah.finalize().to_hex().to_string(),
            "full_digest": bh.finalize().to_hex().to_string(),
            "different_row_count": different_rows,
            "difference_alignment": "sorted_row",
            "different_row_examples": examples,
        }));
    }
    Ok(json!({
        "status": if different.is_empty() { "equal" } else { "different" },
        "equal": different.is_empty(),
        "different_tables": different,
        "tables": tables,
        "comparison": "complete shared canonical projection and Value equality; duplicate-preserving serialized disk order; every row compared before hashing",
        "oracle_mode": "disk_backed_exact_v1",
        "storage_layout": "without_rowid_value_ordinal_v1",
        "capacity_profile": capacity_profile,
        "limits": limits,
        "canonical_bytes": canonical_bytes,
        "scratch_peak_bytes": std::fs::metadata(&scratch_path)?.len(),
        "sorting_cache_kib": 2048,
        "difference_examples_per_table": 3,
        "difference_example_body_limit_bytes": 16 * 1024,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sole_primary_tree_preserves_duplicate_order_without_external_sorting() {
        let directory = tempfile::tempdir().unwrap();
        let mut scratch = open_scratch(
            &directory.path().join("canonical.sqlite3"),
            StreamingLimits::default(),
        )
        .unwrap();
        let mut expected = vec![
            "{\"n\":-0.0}".to_string(),
            "{\"n\":0.0}".to_string(),
            "é/中文\\\"".to_string(),
            "é/中文\\\"".to_string(),
            "x".repeat(4000),
        ];
        for side in 0..2 {
            let tx = scratch.transaction().unwrap();
            for (ordinal, value) in expected.iter().rev().enumerate() {
                tx.execute(
                    "INSERT INTO canonical_rows(side,value,ordinal) VALUES (?1,?2,?3)",
                    (side, value, i64::try_from(ordinal).unwrap()),
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        expected.sort();
        for side in 0..2 {
            let query_plan: Vec<String> = scratch
                .prepare(&format!("EXPLAIN QUERY PLAN {SORTED_ROWS_SQL}"))
                .unwrap()
                .query_map([side], |row| row.get(3))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert!(query_plan.iter().any(|step| step.contains("PRIMARY KEY")));
            assert!(query_plan.iter().all(|step| !step.contains("TEMP B-TREE")));
            let actual: Vec<String> = scratch
                .prepare(SORTED_ROWS_SQL)
                .unwrap()
                .query_map([side], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(actual, expected);
        }
        assert_eq!(
            scratch
                .pragma_query_value(None, "cache_size", |r| r.get::<_, i64>(0))
                .unwrap(),
            -2048
        );
        assert_eq!(
            scratch
                .pragma_query_value(None, "mmap_size", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn named_capacity_never_widens_general_oracle_admission() {
        let original = StreamingLimits::default();
        assert_eq!(original.max_canonical_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(original.max_scratch_bytes, 8 * 1024 * 1024 * 1024);
        for bytes in [8 * 1024 * 1024 * 1024 + 1, 16 * 1024 * 1024 * 1024] {
            let limits = StreamingLimits {
                max_canonical_bytes: bytes,
                ..original
            };
            assert!(limits.validate().is_err());
            // Admission fails before either nonexistent input is touched.
            assert!(
                compare_streaming(Path::new("absent-a"), Path::new("absent-b"), limits)
                    .unwrap_err()
                    .to_string()
                    .contains("invalid streaming oracle resource limits")
            );
        }
    }
}
