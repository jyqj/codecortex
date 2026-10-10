//! Disk-backed comparison of the complete existing canonical oracle surface.
//!
//! SQLite orders serialized canonical rows, including duplicates. Both sorted
//! streams retain the original canonical Value equality before their digests
//! are reported (in particular, -0.0 and +0.0 compare equal). The
//! disposable index bounds sorting memory without introducing a second fact
//! projection, sampling rows, or increasing the legacy in-memory row budget.
use super::{columns, db_error, select_columns, snapshot, RowProjection, TABLES};
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

fn serialized_row(
    row: &rusqlite::Row<'_>,
    cols: &[String],
    projection: &mut RowProjection<'_>,
    limits: StreamingLimits,
) -> Result<String> {
    check_row_size(row, cols, limits.max_row_bytes)?;
    let serialized = projection.serialize_row(row)?;
    if serialized.len() > limits.max_row_bytes {
        return Err(invalid(
            "streaming oracle serialized row byte budget exceeded",
        ));
    }
    Ok(serialized)
}

fn account_bytes(bytes: &mut u64, serialized: &str, limits: StreamingLimits) -> Result<()> {
    *bytes = bytes
        .checked_add(serialized.len() as u64)
        .ok_or_else(|| invalid("streaming oracle byte count overflow"))?;
    if *bytes > limits.max_canonical_bytes {
        return Err(invalid("streaming oracle canonical byte budget exceeded"));
    }
    Ok(())
}

const INSERT_TIERS: [usize; 3] = [64, 8, 1];
const PENDING_BYTES: usize = 64 * 1024;

/// Bounded insertion batching only: preserve the original canonical bytes and
/// side-local ordinal order. The SQLite primary tree still does all sorting.
struct CanonicalInsert<'a> {
    statements: [rusqlite::Statement<'a>; 3],
    side: u8,
    pending: Vec<(String, i64)>,
    pending_bytes: usize,
    #[cfg(test)]
    executed_tiers: Vec<usize>,
}

impl<'a> CanonicalInsert<'a> {
    fn new(transaction: &'a rusqlite::Transaction<'_>, side: u8) -> Result<Self> {
        let prepare = |rows: usize| {
            let values = vec!["(?, ?, ?)"; rows].join(", ");
            transaction
                .prepare(&format!(
                    "INSERT INTO canonical_rows(side, value, ordinal) VALUES {values}"
                ))
                .map_err(db_error)
        };
        Ok(Self {
            statements: [
                prepare(64)?,
                prepare(8)?,
                transaction
                    .prepare("INSERT INTO canonical_rows(side, value, ordinal) VALUES (?1, ?2, ?3)")
                    .map_err(db_error)?,
            ],
            side,
            pending: Vec::with_capacity(INSERT_TIERS[0]),
            pending_bytes: 0,
            #[cfg(test)]
            executed_tiers: Vec::new(),
        })
    }

    fn execute(
        statement: &mut rusqlite::Statement<'_>,
        side: u8,
        rows: &[(String, i64)],
    ) -> Result<()> {
        for (index, (value, ordinal)) in rows.iter().enumerate() {
            let base = index * 3;
            statement
                .raw_bind_parameter(base + 1, side)
                .map_err(db_error)?;
            statement
                .raw_bind_parameter(base + 2, value)
                .map_err(db_error)?;
            statement
                .raw_bind_parameter(base + 3, ordinal)
                .map_err(db_error)?;
        }
        statement.raw_execute().map_err(db_error)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let mut rest = self.pending.as_slice();
        for (tier, statement) in INSERT_TIERS.into_iter().zip(&mut self.statements) {
            while rest.len() >= tier {
                let (rows, tail) = rest.split_at(tier);
                Self::execute(statement, self.side, rows)?;
                #[cfg(test)]
                self.executed_tiers.push(tier);
                rest = tail;
            }
        }
        self.pending.clear();
        self.pending_bytes = 0;
        Ok(())
    }

    fn push(&mut self, value: String, ordinal: i64) -> Result<()> {
        // A legal row larger than the batch allowance retains the original
        // single-row path. It is never rejected by this new buffer bound.
        if value.len() > PENDING_BYTES {
            self.flush()?;
            self.statements[2]
                .execute((self.side, value, ordinal))
                .map_err(db_error)?;
            #[cfg(test)]
            self.executed_tiers.push(1);
            return Ok(());
        }
        if self.pending.len() == INSERT_TIERS[0] || self.pending_bytes + value.len() > PENDING_BYTES
        {
            self.flush()?;
        }
        self.pending_bytes += value.len();
        self.pending.push((value, ordinal));
        Ok(())
    }
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
    let mut projection = RowProjection::new(&cols);
    let mut statement = source
        .prepare(&format!(
            "SELECT {} FROM \"{table}\"",
            select_columns(&cols)
        ))
        .map_err(db_error)?;
    let mut cursor = statement.query([]).map_err(db_error)?;
    let transaction = scratch.transaction().map_err(db_error)?;
    let mut insert = CanonicalInsert::new(&transaction, side)?;
    let mut count = 0;
    while let Some(row) = cursor.next().map_err(db_error)? {
        if count >= limits.max_rows_per_table {
            return Err(invalid(format!(
                "streaming oracle row budget exceeded: {table} side {side}"
            )));
        }
        let serialized = serialized_row(row, &cols, &mut projection, limits)?;
        account_bytes(canonical_bytes, &serialized, limits)?;
        let ordinal = i64::try_from(count)
            .map_err(|_| invalid("streaming oracle ordinal exceeds SQLite integer range"))?;
        insert.push(serialized, ordinal)?;
        count += 1;
    }
    insert.flush()?;
    drop(insert);
    transaction.commit().map_err(db_error)?;
    Ok(count)
}

// Match rusqlite 0.40.1 Row::get::<_, String>(0), but keep successful text
// borrowed from the current SQLite row. In particular, malformed UTF8 has its
// original Utf8Error variant, and non-text values retain their column name/type.
fn row_text<'row>(row: &'row rusqlite::Row<'_>) -> rusqlite::Result<&'row str> {
    let value = row.get_ref(0)?;
    match value {
        ValueRef::Text(bytes) => {
            std::str::from_utf8(bytes).map_err(|error| rusqlite::Error::Utf8Error(0, error))
        }
        _ => Err(rusqlite::Error::InvalidColumnType(
            0,
            row.as_ref().column_name(0)?.to_owned(),
            value.data_type(),
        )),
    }
}

fn next_row<'row>(rows: &'row mut rusqlite::Rows<'_>) -> Result<Option<&'row str>> {
    rows.next()
        .map_err(db_error)?
        .map(|row| row_text(row).map_err(db_error))
        .transpose()
}

fn next_pair<'a, 'b>(
    arows: &'a mut rusqlite::Rows<'_>,
    brows: &'b mut rusqlite::Rows<'_>,
) -> Result<(Option<&'a str>, Option<&'b str>)> {
    // A's step AND conversion precede B's step, including on either failure.
    // Each borrow ends before its own cursor advances on the next iteration.
    let av = next_row(arows)?;
    let bv = next_row(brows)?;
    Ok((av, bv))
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
            let (av, bv) = next_pair(&mut arows, &mut brows)?;
            if av.is_none() && bv.is_none() {
                break;
            }
            if !equal_rows(av, bv)? {
                different_rows += 1;
                if examples.len() < 3 {
                    examples.push(json!({
                        "sorted_row": position,
                        "incremental": example(av)?,
                        "full": example(bv)?,
                    }));
                }
            }
            append_digest(&mut ah, av, position);
            append_digest(&mut bh, bv, position);
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
    use crate::benchmark::oracle::project_row;

    // Original single-row implementation, retained independently of batching.
    fn reference_spool(
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

    fn source_rows(values: &[rusqlite::types::Value]) -> Connection {
        let source = Connection::open_in_memory().unwrap();
        source
            .execute_batch("CREATE TABLE symbols(name TEXT, value);")
            .unwrap();
        for value in values {
            source
                .execute("INSERT INTO symbols VALUES('same',?1)", [value])
                .unwrap();
        }
        source
    }

    fn ordered_rows(scratch: &Connection, side: u8) -> Vec<String> {
        scratch
            .prepare(SORTED_ROWS_SQL)
            .unwrap()
            .query_map([side], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn batched_spool_matches_original_at_tier_and_byte_boundaries() {
        use rusqlite::types::Value as SqlValue;
        let seed = [
            SqlValue::Null,
            SqlValue::Integer(1),
            SqlValue::Real(1.0),
            SqlValue::Real(-0.0),
            SqlValue::Real(0.0),
            SqlValue::Text("é/中文\0\n\"".into()),
            SqlValue::Blob(vec![0, 255, 128]),
            SqlValue::Text("duplicate".into()),
            SqlValue::Text("duplicate".into()),
        ];
        for count in [0, 1, 7, 8, 9, 63, 64, 65, 127, 128, 129] {
            let values: Vec<_> = (0..count).map(|n| seed[n % seed.len()].clone()).collect();
            assert_spool_matches_reference(&values);
        }
        // The first two serialized rows cross the byte bound without reaching
        // 64 rows; the third is legal under the original row limit but cannot
        // fit in a batch. A trailing short row still has to be flushed.
        assert_spool_matches_reference(&[
            SqlValue::Text("x".repeat(PENDING_BYTES / 2)),
            SqlValue::Text("y".repeat(PENDING_BYTES / 2)),
            SqlValue::Text("z".repeat(PENDING_BYTES + 1)),
            SqlValue::Integer(17),
        ]);
    }

    fn assert_spool_matches_reference(values: &[rusqlite::types::Value]) {
        let directory = tempfile::tempdir().unwrap();
        let limits = StreamingLimits::default();
        let mut batch = open_scratch(&directory.path().join("batch.sqlite3"), limits).unwrap();
        let mut reference =
            open_scratch(&directory.path().join("reference.sqlite3"), limits).unwrap();
        let mut actual_bytes = 0;
        let mut expected_bytes = 0;
        for side in 0..2 {
            let mut input = values.to_vec();
            if side == 1 {
                input.reverse();
            }
            let source = source_rows(&input);
            assert_eq!(
                spool(
                    &source,
                    &mut batch,
                    "symbols",
                    side,
                    limits,
                    &mut actual_bytes
                )
                .unwrap(),
                reference_spool(
                    &source,
                    &mut reference,
                    "symbols",
                    side,
                    limits,
                    &mut expected_bytes
                )
                .unwrap()
            );
            assert_eq!(actual_bytes, expected_bytes);
            let actual = ordered_rows(&batch, side);
            let expected = ordered_rows(&reference, side);
            assert_eq!(actual, expected);
            assert_eq!(
                manifest::digest(&serde_json::to_vec(&actual).unwrap()),
                manifest::digest(&serde_json::to_vec(&expected).unwrap())
            );
            let ordinals: Vec<i64> = batch
                .prepare("SELECT ordinal FROM canonical_rows WHERE side=?1 ORDER BY ordinal")
                .unwrap()
                .query_map([side], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(
                ordinals,
                (0..i64::try_from(input.len()).unwrap()).collect::<Vec<_>>()
            );
        }
        assert_eq!(ordered_rows(&batch, 0), ordered_rows(&batch, 1));
    }

    #[test]
    fn batch_inserts_reduce_actual_statements_with_bounded_pending_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let mut scratch = open_scratch(
            &directory.path().join("scratch.sqlite3"),
            StreamingLimits::default(),
        )
        .unwrap();
        let tx = scratch.transaction().unwrap();
        let mut insert = CanonicalInsert::new(&tx, 0).unwrap();
        for ordinal in 0..73 {
            insert.push(ordinal.to_string(), ordinal).unwrap();
            assert!(insert.pending.len() <= 64);
            assert!(insert.pending_bytes <= PENDING_BYTES);
        }
        insert.flush().unwrap();
        assert_eq!(insert.executed_tiers, [64, 8, 1]);
        assert_eq!(insert.pending_bytes, 0);
        assert!(insert.pending.is_empty());
        drop(insert);
        let mut insert = CanonicalInsert::new(&tx, 1).unwrap();
        insert
            .push(format!("\"{}\"", "x".repeat(PENDING_BYTES - 2)), 0)
            .unwrap();
        assert_eq!(insert.pending_bytes, PENDING_BYTES);
        insert.push("1".into(), 1).unwrap();
        assert_eq!(insert.pending_bytes, 1);
        insert
            .push(format!("\"{}\"", "y".repeat(PENDING_BYTES - 1)), 2)
            .unwrap();
        assert_eq!(insert.pending_bytes, 0);
        insert.push("2".into(), 3).unwrap();
        insert.flush().unwrap();
        assert_eq!(insert.executed_tiers, [1, 1, 1, 1]);
        drop(insert);
        tx.commit().unwrap();
        assert_eq!(ordered_rows(&scratch, 0).len(), 73);
        assert_eq!(ordered_rows(&scratch, 1).len(), 4);
    }

    #[test]
    fn batched_spool_budget_errors_match_original_and_never_commit_pending_rows() {
        use rusqlite::types::Value as SqlValue;
        let mut cases = Vec::new();
        let short = vec![SqlValue::Integer(1); 67];
        cases.push((
            short.clone(),
            StreamingLimits {
                max_rows_per_table: 65,
                ..StreamingLimits::default()
            },
            "row budget",
        ));
        let serialized_bytes = serde_json::to_string(&json!({"name":"same", "value":1}))
            .unwrap()
            .len() as u64;
        cases.push((
            short.clone(),
            StreamingLimits {
                max_canonical_bytes: serialized_bytes * 65,
                ..StreamingLimits::default()
            },
            "canonical byte budget",
        ));
        let mut long_tail = short[..65].to_vec();
        long_tail.push(SqlValue::Text("x".repeat(1024)));
        cases.push((
            long_tail,
            StreamingLimits {
                max_row_bytes: 128,
                ..StreamingLimits::default()
            },
            "input row byte budget",
        ));
        // Escaping expands a valid input row beyond its serialized byte cap.
        let mut escaped_tail = short[..65].to_vec();
        escaped_tail.push(SqlValue::Text("\n".repeat(70)));
        cases.push((
            escaped_tail,
            StreamingLimits {
                max_row_bytes: 128,
                ..StreamingLimits::default()
            },
            "serialized row byte budget",
        ));
        for (case, (values, limits, error_kind)) in cases.into_iter().enumerate() {
            let directory = tempfile::tempdir().unwrap();
            let source = source_rows(&values);
            let mut batch = open_scratch(&directory.path().join("batch.sqlite3"), limits).unwrap();
            let mut reference =
                open_scratch(&directory.path().join("reference.sqlite3"), limits).unwrap();
            let mut actual_bytes = 0;
            let mut expected_bytes = 0;
            let actual =
                spool(&source, &mut batch, "symbols", 0, limits, &mut actual_bytes).unwrap_err();
            let expected = reference_spool(
                &source,
                &mut reference,
                "symbols",
                0,
                limits,
                &mut expected_bytes,
            )
            .unwrap_err();
            assert_eq!(actual.to_string(), expected.to_string(), "case {case}");
            assert!(actual.to_string().contains(error_kind));
            assert_eq!(actual_bytes, expected_bytes);
            assert!(batch.is_autocommit() && reference.is_autocommit());
            // These bounded fixtures fit in the existing cache. On actual
            // disk/page failures the journal-OFF scratch is discarded, never
            // reused or interpreted as a successfully compared partial table.
            assert!(ordered_rows(&batch, 0).is_empty());
            assert!(ordered_rows(&reference, 0).is_empty());
        }
    }

    #[test]
    fn batched_spool_scratch_exhaustion_remains_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let values = vec![rusqlite::types::Value::Text("x".repeat(4096)); 100];
        let source = source_rows(&values);
        let limits = StreamingLimits {
            max_scratch_bytes: 64 * 1024,
            ..StreamingLimits::default()
        };
        let mut batch = open_scratch(&directory.path().join("batch.sqlite3"), limits).unwrap();
        let mut reference =
            open_scratch(&directory.path().join("reference.sqlite3"), limits).unwrap();
        let actual = spool(&source, &mut batch, "symbols", 0, limits, &mut 0).unwrap_err();
        let expected =
            reference_spool(&source, &mut reference, "symbols", 0, limits, &mut 0).unwrap_err();
        assert_eq!(actual.to_string(), expected.to_string());
        assert!(actual.to_string().contains("full"));
        assert!(batch.is_autocommit() && reference.is_autocommit());
        drop(batch);
        drop(reference);
        directory.close().unwrap();
    }

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

    #[test]
    fn borrowed_cursor_text_matches_owned_conversion_and_row_storage() {
        let connection = Connection::open_in_memory().unwrap();
        for sql in [
            "SELECT 'plain' AS canonical",
            "SELECT 'é/中文' || char(0) || 'tail' AS canonical",
            "SELECT '' AS canonical",
            "SELECT CAST(x'80' AS TEXT) AS canonical",
            "SELECT NULL AS canonical",
            "SELECT 7 AS canonical",
            "SELECT -0.0 AS canonical",
            "SELECT x'6162' AS canonical",
        ] {
            let mut statement = connection.prepare(sql).unwrap();
            let mut rows = statement.query([]).unwrap();
            let row = rows.next().unwrap().unwrap();
            let expected = row.get::<_, String>(0);
            let actual = row_text(row);
            match (expected, actual) {
                (Ok(expected), Ok(actual)) => {
                    assert_eq!(actual, expected);
                    let ValueRef::Text(bytes) = row.get_ref(0).unwrap() else {
                        panic!("successful String conversion requires SQLite text");
                    };
                    assert_eq!(actual.as_ptr(), bytes.as_ptr());
                    assert_eq!(actual.len(), bytes.len());
                }
                (Err(expected), Err(actual)) => {
                    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
                    assert_eq!(actual.to_string(), expected.to_string());
                    assert_eq!(db_error(actual).to_string(), db_error(expected).to_string());
                }
                _ => panic!("borrowed and owned text conversion disagree for {sql}"),
            }
        }
    }

    #[test]
    fn borrowed_cursor_pair_preserves_both_rows_duplicates_and_asymmetric_eof() {
        let directory = tempfile::tempdir().unwrap();
        let scratch = open_scratch(
            &directory.path().join("canonical.sqlite3"),
            StreamingLimits::default(),
        )
        .unwrap();
        let common = [
            r#"{"n":-0.0}"#.to_string(),
            r#"{"n":0.0}"#.to_string(),
            r#"{"s":"duplicate"}"#.to_string(),
            r#"{"s":"duplicate"}"#.to_string(),
            r#"{"s":"é/中文\u0000tail"}"#.to_string(),
            serde_json::to_string(&json!({"s": "x".repeat(20 * 1024)})).unwrap(),
        ];
        for side in 0..2 {
            for (ordinal, value) in common.iter().enumerate() {
                scratch
                    .execute(
                        "INSERT INTO canonical_rows(side,value,ordinal) VALUES (?1,?2,?3)",
                        (side, value, i64::try_from(ordinal).unwrap()),
                    )
                    .unwrap();
            }
        }
        scratch
            .execute(
                "INSERT INTO canonical_rows(side,value,ordinal) VALUES (1,?1,99)",
                [r#"{"zz":"B-only"}"#],
            )
            .unwrap();
        let expected_a = ordered_rows(&scratch, 0);
        let expected_b = ordered_rows(&scratch, 1);
        let empty = Vec::new();
        for (a_side, b_side, a_expected, b_expected) in [
            (0, 1, &expected_a, &expected_b),
            (1, 0, &expected_b, &expected_a),
            (2, 1, &empty, &expected_b),
        ] {
            let mut owned_examples = Vec::new();
            let mut actual_a = blake3::Hasher::new();
            let mut actual_b = blake3::Hasher::new();
            actual_a.update(b"[");
            actual_b.update(b"[");
            {
                let mut astmt = scratch.prepare(SORTED_ROWS_SQL).unwrap();
                let mut bstmt = scratch.prepare(SORTED_ROWS_SQL).unwrap();
                let mut arows = astmt.query([a_side]).unwrap();
                let mut brows = bstmt.query([b_side]).unwrap();
                let mut position = 0;
                loop {
                    let (av, bv) = next_pair(&mut arows, &mut brows).unwrap();
                    // A remains valid after B steps, and neither borrow escapes
                    // the iteration. The owned baseline retains duplicate rows.
                    assert_eq!(av, a_expected.get(position).map(String::as_str));
                    assert_eq!(bv, b_expected.get(position).map(String::as_str));
                    if av.is_none() && bv.is_none() {
                        break;
                    }
                    if !equal_rows(av, bv).unwrap() && owned_examples.len() < 3 {
                        owned_examples.push((example(av).unwrap(), example(bv).unwrap()));
                    }
                    append_digest(&mut actual_a, av, position as u64);
                    append_digest(&mut actual_b, bv, position as u64);
                    position += 1;
                }
                assert_eq!(position, a_expected.len().max(b_expected.len()));
                assert_eq!(next_pair(&mut arows, &mut brows).unwrap(), (None, None));
            }
            actual_a.update(b"]");
            actual_b.update(b"]");
            assert_eq!(
                actual_a.finalize().to_hex().as_str(),
                manifest::digest(format!("[{}]", a_expected.join(",")).as_bytes())
            );
            assert_eq!(
                actual_b.finalize().to_hex().as_str(),
                manifest::digest(format!("[{}]", b_expected.join(",")).as_bytes())
            );
            // These bounded examples own their values after both statements drop.
            let expected_examples: Vec<_> = (0..a_expected.len().max(b_expected.len()))
                .filter(|&index| {
                    !equal_rows(
                        a_expected.get(index).map(String::as_str),
                        b_expected.get(index).map(String::as_str),
                    )
                    .unwrap()
                })
                .take(3)
                .map(|index| {
                    (
                        example(a_expected.get(index).map(String::as_str)).unwrap(),
                        example(b_expected.get(index).map(String::as_str)).unwrap(),
                    )
                })
                .collect();
            assert_eq!(owned_examples, expected_examples);
        }
    }

    #[test]
    fn borrowed_cursor_pair_keeps_original_step_and_conversion_error_order() {
        fn owned_next(rows: &mut rusqlite::Rows<'_>) -> Result<Option<String>> {
            rows.next()
                .map_err(db_error)?
                .map(|row| row.get::<_, String>(0).map_err(db_error))
                .transpose()
        }
        let connection = Connection::open_in_memory().unwrap();
        let a_values = "SELECT 'a-first' AS value UNION ALL SELECT 'a-second'";
        let b_values = "SELECT 'b-first' AS value UNION ALL SELECT 'b-second'";
        let conversion_error = "SELECT CAST(x'80' AS TEXT) AS value";
        let step_error = "SELECT abs(-9223372036854775808) AS value";
        let empty = "SELECT 'unused' AS value WHERE 0";
        for (a_sql, b_sql, a_fails) in [
            (conversion_error, b_values, true),
            (step_error, b_values, true),
            (a_values, conversion_error, false),
            (a_values, step_error, false),
            (empty, conversion_error, false),
            (empty, step_error, false),
        ] {
            let mut original_a = connection.prepare(a_sql).unwrap();
            let mut original_b = connection.prepare(b_sql).unwrap();
            let mut original_arows = original_a.query([]).unwrap();
            let mut original_brows = original_b.query([]).unwrap();
            let expected = (|| -> Result<()> {
                owned_next(&mut original_arows)?;
                owned_next(&mut original_brows)?;
                Ok(())
            })()
            .unwrap_err();
            let mut astmt = connection.prepare(a_sql).unwrap();
            let mut bstmt = connection.prepare(b_sql).unwrap();
            let mut arows = astmt.query([]).unwrap();
            let mut brows = bstmt.query([]).unwrap();
            let actual = next_pair(&mut arows, &mut brows).unwrap_err();
            assert_eq!(actual.to_string(), expected.to_string());
            if a_fails {
                // Neither A's sqlite3_step failure nor its UTF8 conversion
                // failure may advance B before returning the original error.
                assert_eq!(next_row(&mut brows).unwrap(), Some("b-first"));
            } else if a_sql == empty {
                assert_eq!(next_row(&mut arows).unwrap(), None);
            } else {
                // B fails only after A has advanced and converted exactly once.
                assert_eq!(next_row(&mut arows).unwrap(), Some("a-second"));
            }
        }
    }
}
