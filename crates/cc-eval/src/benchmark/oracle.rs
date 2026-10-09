//! White-box diagnostic oracle plus independent public-protocol probes.
//! This is not reported as the black-box retrieval benchmark.
use super::{
    invalid,
    manifest::{self, LoadedSuite},
    mutations::MutationPlan,
    normalizer, report, Result,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde::{ser::SerializeMap, Serialize, Serializer};
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

/// One immutable SELECT layout, reused only within one table and input side.
/// Keys are allocated lazily, after the streaming caller checks the first row's
/// size. Streaming text is borrowed only for the current row. The original
/// owned projection remains available to the diagnostic Value API.
struct RowProjection<'a> {
    cols: &'a [String],
    values: serde_json::Map<String, Value>,
    serialization_order: Option<Vec<usize>>,
}

enum ProjectedCell<'a> {
    Scalar(Value),
    Text(&'a str),
    BlobDigest(String),
}

impl Serialize for ProjectedCell<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Scalar(value) => value.serialize(serializer),
            Self::Text(value) => serializer.serialize_str(value),
            Self::BlobDigest(digest) => {
                let mut object = serializer.serialize_map(Some(1))?;
                object.serialize_entry("blob_digest", digest)?;
                object.end()
            }
        }
    }
}

struct BorrowedProjectedRow<'a, 'row> {
    cols: &'a [String],
    order: &'a [usize],
    cells: &'a [ProjectedCell<'row>],
}

impl Serialize for BorrowedProjectedRow<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(Some(self.order.len()))?;
        for &index in self.order {
            object.serialize_entry(&self.cols[index], &self.cells[index])?;
        }
        object.end()
    }
}

impl<'a> RowProjection<'a> {
    fn new(cols: &'a [String]) -> Self {
        Self {
            cols,
            values: serde_json::Map::new(),
            serialization_order: None,
        }
    }

    fn project(&mut self, row: &rusqlite::Row<'_>) -> Result<()> {
        let result: Result<()> = (|| {
            for (i, col) in self.cols.iter().enumerate() {
                let value = match row.get_ref(i).map_err(db_error)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => json!(v),
                    ValueRef::Real(v) => json!(v),
                    ValueRef::Text(v) => {
                        json!(std::str::from_utf8(v).map_err(|_| invalid("oracle UTF8"))?)
                    }
                    ValueRef::Blob(v) => json!({"blob_digest":manifest::digest(v)}),
                };
                if let Some(slot) = self.values.get_mut(col) {
                    *slot = value;
                } else {
                    self.values.insert(col.clone(), value);
                }
            }
            Ok(())
        })();
        if result.is_err() {
            self.clear_values();
        }
        result
    }

    fn clear_values(&mut self) {
        for value in self.values.values_mut() {
            *value = Value::Null;
        }
    }

    fn serialize_row(&mut self, row: &rusqlite::Row<'_>) -> Result<String> {
        let serialized: Result<String> = (|| {
            // Read and validate every cell in SELECT order, even a duplicate
            // key later overwritten by another column. This preserves the
            // owned projection's DB/UTF8 error precedence.
            let mut cells = Vec::with_capacity(self.cols.len());
            for index in 0..self.cols.len() {
                cells.push(match row.get_ref(index).map_err(db_error)? {
                    ValueRef::Null => ProjectedCell::Scalar(Value::Null),
                    ValueRef::Integer(value) => ProjectedCell::Scalar(json!(value)),
                    ValueRef::Real(value) => ProjectedCell::Scalar(json!(value)),
                    ValueRef::Text(value) => ProjectedCell::Text(
                        std::str::from_utf8(value).map_err(|_| invalid("oracle UTF8"))?,
                    ),
                    ValueRef::Blob(value) => ProjectedCell::BlobDigest(manifest::digest(value)),
                });
            }
            if self.serialization_order.is_none() {
                // Derive order from the actual serde Map implementation,
                // including feature unification. Duplicate keys retain their
                // original position but use their last SELECT column's value.
                let mut last_indices = BTreeMap::new();
                for (index, col) in self.cols.iter().enumerate() {
                    self.values.entry(col.clone()).or_insert(Value::Null);
                    last_indices.insert(col.as_str(), index);
                }
                self.serialization_order = Some(
                    self.values
                        .keys()
                        .map(|col| last_indices[col.as_str()])
                        .collect(),
                );
            }
            let borrowed = BorrowedProjectedRow {
                cols: self.cols,
                order: self
                    .serialization_order
                    .as_deref()
                    .expect("layout initialized"),
                cells: &cells,
            };
            serde_json::to_string(&borrowed).map_err(Into::into)
        })();
        self.clear_values();
        serialized
    }
}

fn project_row(row: &rusqlite::Row<'_>, cols: &[String]) -> Result<Value> {
    let mut projection = RowProjection::new(cols);
    projection.project(row)?;
    Ok(Value::Object(projection.values))
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

#[cfg(test)]
mod projection_tests {
    use super::*;
    use rusqlite::types::Value as SqlValue;

    #[test]
    fn reused_projection_keeps_exact_sql_value_bytes_and_only_its_layout() {
        let conn = Connection::open_in_memory().unwrap();
        let cols = vec!["z".into(), "a".into(), "odd\"列".into()];
        let mut projection = RowProjection::new(&cols);
        let mut statement = conn.prepare("SELECT ?1, ?2, ?3").unwrap();
        let cases = [
            (
                vec![
                    SqlValue::Blob(Vec::new()),
                    SqlValue::Integer(i64::MAX),
                    SqlValue::Text("é/中文\0\n\"".into()),
                ],
                r#"{"z":{"blob_digest":"af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"},"a":9223372036854775807,"odd\"列":"é/中文\u0000\n\""}"#,
            ),
            (
                vec![
                    SqlValue::Null,
                    SqlValue::Integer(i64::MIN),
                    SqlValue::Real(-0.0),
                ],
                r#"{"z":null,"a":-9223372036854775808,"odd\"列":-0.0}"#,
            ),
            (
                vec![
                    SqlValue::Text("replacement".into()),
                    SqlValue::Null,
                    SqlValue::Real(0.0),
                ],
                r#"{"z":"replacement","a":null,"odd\"列":0.0}"#,
            ),
        ];
        let mut key_buffers = None;
        for (values, expected) in &cases {
            let mut rows = statement.query(rusqlite::params_from_iter(values)).unwrap();
            let row = rows.next().unwrap().unwrap();
            let expected: Value = serde_json::from_str(expected).unwrap();
            let expected = serde_json::to_string(&expected).unwrap();
            assert_eq!(
                serde_json::to_string(&project_row(row, &cols).unwrap()).unwrap(),
                expected
            );
            assert_eq!(projection.serialize_row(row).unwrap(), expected);
            // Serializing the same row again retains duplicates byte for byte.
            assert_eq!(projection.serialize_row(row).unwrap(), expected);
            assert!(projection.values.values().all(Value::is_null));
            let actual_keys: Vec<_> = projection.values.keys().map(|key| key.as_ptr()).collect();
            if let Some(previous) = &key_buffers {
                assert_eq!(&actual_keys, previous);
            } else {
                key_buffers = Some(actual_keys);
            }
        }

        // Each table/side gets its own immutable column layout.
        let other_cols = vec!["only".into()];
        let mut other = RowProjection::new(&other_cols);
        let actual = conn
            .query_row("SELECT 5", [], |row| Ok(other.serialize_row(row).unwrap()))
            .unwrap();
        assert_eq!(actual, r#"{"only":5}"#);
        assert_eq!(other.values.len(), 1);
        assert_eq!(projection.values.len(), 3);
    }

    #[test]
    fn projection_error_clears_partial_and_prior_values_before_recovery() {
        let conn = Connection::open_in_memory().unwrap();
        let cols = vec!["left".into(), "right".into()];
        let mut projection = RowProjection::new(&cols);
        {
            let mut statement = conn.prepare("SELECT 'old-left', 'old-right'").unwrap();
            let mut rows = statement.query([]).unwrap();
            projection.project(rows.next().unwrap().unwrap()).unwrap();
        }
        assert_eq!(projection.values["right"], "old-right");
        {
            let mut statement = conn
                .prepare("SELECT 'written-before-error', CAST(x'80' AS TEXT)")
                .unwrap();
            let mut rows = statement.query([]).unwrap();
            let row = rows.next().unwrap().unwrap();
            let original_error = project_row(row, &cols).unwrap_err().to_string();
            let reused_error = projection.serialize_row(row).unwrap_err().to_string();
            assert_eq!(reused_error, original_error);
            assert!(reused_error.contains("oracle UTF8"));
        }
        assert_eq!(projection.values.len(), 2);
        assert!(projection.values.values().all(Value::is_null));
        let actual = conn
            .query_row("SELECT NULL, 7", [], |row| {
                Ok(projection.serialize_row(row).unwrap())
            })
            .unwrap();
        assert_eq!(actual, r#"{"left":null,"right":7}"#);
        assert!(projection.values.values().all(Value::is_null));
    }

    #[test]
    fn borrowed_projection_matches_owned_bytes_for_reordered_and_duplicate_columns() {
        use rusqlite::types::Value as SqlValue;
        let conn = Connection::open_in_memory().unwrap();
        let layouts = [
            vec!["z", "a", "odd\"列", "z", ""],
            vec!["", "z", "a", "odd\"列", "z"],
            vec!["same", "same", "same", "same", "same"],
        ];
        let cases = [
            vec![
                SqlValue::Text("overwritten\0\n\"".into()),
                SqlValue::Integer(i64::MIN),
                SqlValue::Blob(vec![0, 255, 128]),
                SqlValue::Real(-0.0),
                SqlValue::Text("é/中文\t\\".into()),
            ],
            vec![
                SqlValue::Null,
                SqlValue::Real(f64::INFINITY),
                SqlValue::Blob(Vec::new()),
                SqlValue::Real(f64::NEG_INFINITY),
                SqlValue::Integer(i64::MAX),
            ],
        ];
        let mut statement = conn.prepare("SELECT ?1, ?2, ?3, ?4, ?5").unwrap();
        for layout in layouts {
            let cols: Vec<String> = layout.into_iter().map(String::from).collect();
            let mut projection = RowProjection::new(&cols);
            for values in &cases {
                let mut rows = statement.query(rusqlite::params_from_iter(values)).unwrap();
                let row = rows.next().unwrap().unwrap();
                let expected = serde_json::to_string(&project_row(row, &cols).unwrap()).unwrap();
                assert_eq!(projection.serialize_row(row).unwrap(), expected);
                assert_eq!(projection.serialize_row(row).unwrap(), expected);
                assert!(projection.values.values().all(Value::is_null));
            }
        }
    }

    #[test]
    fn borrowed_projection_uses_original_value_real_encoding() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -0.0,
            0.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::MAX,
            1.2345678901234567,
        ] {
            let original = json!(value);
            let borrowed = ProjectedCell::Scalar(json!(value));
            assert_eq!(
                serde_json::to_string(&borrowed).unwrap(),
                serde_json::to_string(&original).unwrap()
            );
        }
    }

    #[test]
    fn borrowed_projection_preserves_select_error_order_and_recovers() {
        let conn = Connection::open_in_memory().unwrap();
        let cols = vec!["z".into(), "a".into(), "z".into()];
        let mut projection = RowProjection::new(&cols);
        for sql in [
            "SELECT CAST(x'80' AS TEXT), 'ok', 'overwrites-invalid-text'",
            "SELECT 1, CAST(x'80' AS TEXT)",
            "SELECT 1, 2",
        ] {
            let mut statement = conn.prepare(sql).unwrap();
            let mut rows = statement.query([]).unwrap();
            let row = rows.next().unwrap().unwrap();
            let original_error = project_row(row, &cols).unwrap_err().to_string();
            assert_eq!(
                projection.serialize_row(row).unwrap_err().to_string(),
                original_error
            );
            assert!(projection.values.values().all(Value::is_null));
            let actual = conn
                .query_row("SELECT 'old', NULL, 'last'", [], |row| {
                    let expected =
                        serde_json::to_string(&project_row(row, &cols).unwrap()).unwrap();
                    let actual = projection.serialize_row(row).unwrap();
                    assert_eq!(actual, expected);
                    Ok(actual)
                })
                .unwrap();
            let expected: Value = json!({"z": "last", "a": null});
            assert_eq!(actual, serde_json::to_string(&expected).unwrap());
        }
    }

    #[test]
    fn borrowed_projection_preserves_escaped_byte_lengths_at_row_boundaries() {
        let conn = Connection::open_in_memory().unwrap();
        let cols = vec!["payload".into()];
        let mut projection = RowProjection::new(&cols);
        let mut statement = conn.prepare("SELECT ?1").unwrap();
        for limit in [128, 65_536, 1_048_576] {
            for offset in [-1_isize, 0, 1] {
                // The compact JSON wrapper {"payload":""} is fourteen bytes.
                let text = "x".repeat((limit as isize + offset - 14) as usize);
                let mut rows = statement.query([&text]).unwrap();
                let row = rows.next().unwrap().unwrap();
                let expected = serde_json::to_string(&project_row(row, &cols).unwrap()).unwrap();
                let actual = projection.serialize_row(row).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(actual.len(), (limit as isize + offset) as usize);
                assert_eq!(actual.len() > limit, offset > 0);
            }
        }
        let escaped = "\0\n\t\"\\é".repeat(256);
        let mut rows = statement.query([&escaped]).unwrap();
        let row = rows.next().unwrap().unwrap();
        let expected = serde_json::to_string(&project_row(row, &cols).unwrap()).unwrap();
        let actual = projection.serialize_row(row).unwrap();
        assert_eq!(actual.as_bytes(), expected.as_bytes());
        assert!(actual.len() > escaped.len());
    }

    // Exact pre-change streaming baseline: keep its Map across rows instead
    // of substituting the slower, per-row project_row diagnostic API.
    fn original_serialize_row(
        projection: &mut RowProjection<'_>,
        row: &rusqlite::Row<'_>,
    ) -> Result<String> {
        projection.project(row)?;
        let serialized: Result<String> =
            serde_json::to_string(&projection.values).map_err(Into::into);
        projection.clear_values();
        serialized
    }

    #[test]
    #[ignore = "explicit finite release cost observation; no timing acceptance threshold"]
    fn borrowed_serialization_release_cost_probe() {
        use rusqlite::types::Value as SqlValue;
        use std::time::Instant;

        fn collect(
            statement: &mut rusqlite::Statement<'_>,
            projection: &mut RowProjection<'_>,
            borrowed: bool,
        ) -> (Vec<String>, u128) {
            let started = Instant::now();
            let mut rows = statement.query([]).unwrap();
            let mut output = Vec::new();
            while let Some(row) = rows.next().unwrap() {
                output.push(if borrowed {
                    projection.serialize_row(row).unwrap()
                } else {
                    original_serialize_row(projection, row).unwrap()
                });
            }
            (output, started.elapsed().as_nanos())
        }

        let cases = [
            (
                "multiple_text",
                vec![
                    SqlValue::Text("é/中文\n\"\\".repeat(512)),
                    SqlValue::Text("manifest-payload".repeat(256)),
                    SqlValue::Text("qualified::name".repeat(128)),
                    SqlValue::Text("path/to/source.rs".repeat(128)),
                    SqlValue::Integer(17),
                    SqlValue::Null,
                ],
            ),
            (
                "short_fields",
                vec![
                    SqlValue::Text("a".into()),
                    SqlValue::Text("bc".into()),
                    SqlValue::Integer(i64::MIN),
                    SqlValue::Integer(i64::MAX),
                    SqlValue::Real(-0.0),
                    SqlValue::Real(1.25),
                ],
            ),
            (
                "blob",
                vec![
                    SqlValue::Blob([0, 255, 128, 1].repeat(1024)),
                    SqlValue::Blob(Vec::new()),
                    SqlValue::Text("blob-name".into()),
                    SqlValue::Integer(4),
                    SqlValue::Null,
                    SqlValue::Real(0.0),
                ],
            ),
            ("empty_text", vec![SqlValue::Text(String::new()); 6]),
            ("null", vec![SqlValue::Null; 6]),
        ];
        let cols: Vec<String> = ["z", "a", "payload", "path", "ordinal", "empty"]
            .into_iter()
            .map(String::from)
            .collect();
        for (case, values) in cases {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE sample(c0,c1,c2,c3,c4,c5)")
                .unwrap();
            for _ in 0..256 {
                conn.execute(
                    "INSERT INTO sample VALUES(?1,?2,?3,?4,?5,?6)",
                    rusqlite::params_from_iter(&values),
                )
                .unwrap();
            }
            let mut statement = conn
                .prepare("SELECT c0,c1,c2,c3,c4,c5 FROM sample ORDER BY rowid")
                .unwrap();
            let mut original = RowProjection::new(&cols);
            let mut borrowed = RowProjection::new(&cols);
            let (warm_original, _) = collect(&mut statement, &mut original, false);
            let (warm_borrowed, _) = collect(&mut statement, &mut borrowed, true);
            assert_eq!(warm_original, warm_borrowed);
            drop((warm_original, warm_borrowed));
            for round in 0..20 {
                let ((expected, original_ns), (actual, borrowed_ns)) = if round % 2 == 0 {
                    (
                        collect(&mut statement, &mut original, false),
                        collect(&mut statement, &mut borrowed, true),
                    )
                } else {
                    let actual = collect(&mut statement, &mut borrowed, true);
                    let expected = collect(&mut statement, &mut original, false);
                    (expected, actual)
                };
                assert_eq!(actual, expected);
                assert_eq!(actual.len(), 256);
                let original_digest = manifest::digest(&serde_json::to_vec(&expected).unwrap());
                let borrowed_digest = manifest::digest(&serde_json::to_vec(&actual).unwrap());
                assert_eq!(original_digest, borrowed_digest);
                println!(
                    "{}",
                    json!({
                        "schema": "p8-oracle-serialization-cost-v1",
                        "case": case,
                        "round": round,
                        "first": if round % 2 == 0 { "original" } else { "borrowed" },
                        "rows": actual.len(),
                        "canonical_bytes": actual.iter().map(String::len).sum::<usize>(),
                        "original_ns": original_ns.to_string(),
                        "borrowed_ns": borrowed_ns.to_string(),
                        "original_digest": original_digest,
                        "borrowed_digest": borrowed_digest,
                        "timing_scope": "same SQLite SELECT traversal plus row serialization and output retention",
                        "timing_threshold": null,
                        "original_streaming_source_blob": "4c7fdb879e039af164b3b10ea1491d538c69ac68"
                    })
                );
            }
        }
    }
}
