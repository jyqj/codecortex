//! The large-data oracle must preserve the old canonical projection exactly.
//! These databases are synthetic control inputs, not retrieval-quality scores.
use cc_eval::benchmark::{manifest, oracle};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".codecortex")).unwrap();
    let db = open(root.path());
    for table in oracle::tables() {
        db.execute_batch(&format!(
            "CREATE TABLE \"{table}\"(name TEXT, value, mtime INTEGER, indexed_at INTEGER, id INTEGER)"
        ))
        .unwrap();
    }
    root
}

fn open(root: &Path) -> Connection {
    Connection::open(root.join(".codecortex/index.sqlite3")).unwrap()
}

fn table<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["tables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["table"] == name)
        .unwrap()
}

fn compare(a: &Path, b: &Path) -> Value {
    oracle::compare_streaming(a, b, oracle::StreamingLimits::default()).unwrap()
}

#[test]
fn agrees_with_legacy_for_all_types_duplicates_and_physical_columns() {
    let a = fixture();
    let b = fixture();
    for (root, reverse) in [(a.path(), false), (b.path(), true)] {
        let db = open(root);
        let mut values = vec![
            rusqlite::types::Value::Null,
            rusqlite::types::Value::Integer(1),
            rusqlite::types::Value::Real(1.0),
            rusqlite::types::Value::Real(-0.0),
            rusqlite::types::Value::Text("é/中文\0\n\"".into()),
            rusqlite::types::Value::Blob(vec![0, 255, 128]),
            rusqlite::types::Value::Text("duplicate".into()),
            rusqlite::types::Value::Text("duplicate".into()),
        ];
        if reverse {
            values.reverse();
        }
        for name in oracle::tables() {
            for value in &values {
                db.execute(
                    &format!("INSERT INTO \"{name}\" VALUES (?1,?2,?3,?4,?5)"),
                    params!["same", value, 0, 0, 0],
                )
                .unwrap();
            }
        }
        if reverse {
            db.execute("UPDATE files SET mtime=99,indexed_at=99", [])
                .unwrap();
            db.execute("UPDATE imports SET id=99", []).unwrap();
        }
    }
    let legacy_a = oracle::canonical(a.path()).unwrap();
    let legacy_b = oracle::canonical(b.path()).unwrap();
    assert_eq!(legacy_a, legacy_b);
    let report = compare(a.path(), b.path());
    let same_source = compare(a.path(), a.path());
    let expected_bytes: u64 = legacy_a
        .values()
        .chain(legacy_b.values())
        .flatten()
        .map(|row| serde_json::to_string(row).unwrap().len() as u64)
        .sum();
    assert_eq!(report["canonical_bytes"], expected_bytes);
    assert_eq!(same_source["canonical_bytes"], expected_bytes);
    let capacity = oracle::compare_streaming_scale_capacity_v1(a.path(), b.path()).unwrap();
    assert_eq!(capacity["tables"], report["tables"]);
    assert_eq!(capacity["equal"], report["equal"]);
    assert_eq!(capacity["canonical_bytes"], report["canonical_bytes"]);
    assert_eq!(capacity["capacity_profile"], "scale_capacity_v1");
    assert_eq!(
        capacity["limits"]["max_canonical_bytes"],
        16u64 * 1024 * 1024 * 1024
    );
    for limit in ["max_scratch_bytes", "max_rows_per_table", "max_row_bytes"] {
        assert_eq!(capacity["limits"][limit], report["limits"][limit]);
    }
    assert_eq!(capacity["sorting_cache_kib"], 2048);
    assert!(report["capacity_profile"].is_null());
    assert_eq!(report["equal"], true);
    for name in oracle::tables() {
        let row = table(&report, name);
        let witnessed = table(&same_source, name);
        assert_eq!(row["equal_input_order_witness"], false);
        assert_eq!(witnessed["equal_input_order_witness"], true);
        assert_eq!(witnessed["incremental_rows"], row["incremental_rows"]);
        assert_eq!(witnessed["full_rows"], row["incremental_rows"]);
        assert_eq!(witnessed["incremental_digest"], row["incremental_digest"]);
        assert_eq!(witnessed["full_digest"], row["incremental_digest"]);
        assert_eq!(row["incremental_rows"], legacy_a[*name].len());
        assert_eq!(row["full_rows"], legacy_b[*name].len());
        assert_eq!(
            row["incremental_digest"],
            manifest::digest(&serde_json::to_vec(&legacy_a[*name]).unwrap())
        );
        assert_eq!(
            row["full_digest"],
            manifest::digest(&serde_json::to_vec(&legacy_b[*name]).unwrap())
        );
    }
}

#[test]
fn cached_canonical_order_matches_original_serialized_key_bytes() {
    let root = fixture();
    let mut db = open(root.path());
    let tx = db.transaction().unwrap();
    let mut expected = Vec::new();
    {
        let mut insert = tx
            .prepare("INSERT INTO symbols(name,value,mtime,indexed_at,id) VALUES (?1,?2,0,0,0)")
            .unwrap();
        // Deliberately scrambled insertion order, repeated rows, escaped and
        // non-ASCII keys, long common payload prefixes, and signed zeros.
        // Construct the expected projection independently of project_row.
        for n in 0..2048 {
            let name = format!("é/中文\\\"{:04}", (n * 73) % 509);
            let value = format!("{}:{:04}", "same\\\"prefix\n".repeat(32), n % 193);
            insert.execute(params![name, value]).unwrap();
            expected.push(json!({
                "name": name, "value": value, "mtime": 0, "indexed_at": 0, "id": 0,
            }));
        }
        for zero in [-0.0f64, 0.0f64, -0.0f64, 0.0f64] {
            insert.execute(params!["zero", zero]).unwrap();
            expected.push(json!({
                "name": "zero", "value": zero, "mtime": 0, "indexed_at": 0, "id": 0,
            }));
        }
    }
    tx.commit().unwrap();
    // Keep the former comparator here as an independent compatibility
    // control. Compare serialized bytes as well as Values so signed-zero
    // encodings and exact ordering cannot disappear behind Value equality.
    expected.sort_by_key(|row| serde_json::to_string(row).unwrap());
    let actual = oracle::canonical(root.path()).unwrap();
    assert_eq!(actual["symbols"], expected);
    assert_eq!(
        serde_json::to_vec(&actual["symbols"]).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
}

#[test]
fn preserves_legacy_signed_zero_equality_without_rewriting_order_or_digests() {
    let a = fixture();
    let b = fixture();
    open(a.path())
        .execute(
            "INSERT INTO symbols(name,value) VALUES ('zero',?1)",
            [-0.0f64],
        )
        .unwrap();
    open(b.path())
        .execute(
            "INSERT INTO symbols(name,value) VALUES ('zero',?1)",
            [0.0f64],
        )
        .unwrap();
    let ca = oracle::canonical(a.path()).unwrap();
    let cb = oracle::canonical(b.path()).unwrap();
    assert_eq!(ca, cb);
    let ad = manifest::digest(&serde_json::to_vec(&ca["symbols"]).unwrap());
    let bd = manifest::digest(&serde_json::to_vec(&cb["symbols"]).unwrap());
    assert_ne!(
        ad, bd,
        "control must retain the two original signed-zero encodings"
    );
    let report = compare(a.path(), b.path());
    assert_eq!(report["equal"], true);
    assert_eq!(table(&report, "symbols")["equal_input_order_witness"], false);
    assert_eq!(table(&report, "symbols")["incremental_digest"], ad);
    assert_eq!(table(&report, "symbols")["full_digest"], bd);
}

#[test]
fn detects_content_multiplicity_and_missing_rows_without_hash_only_equality() {
    let a = fixture();
    let b = fixture();
    for root in [a.path(), b.path()] {
        let db = open(root);
        db.execute_batch(
            "INSERT INTO symbols(name,value) VALUES('target',1),('target',1);
                          INSERT INTO call_edges(name,value) VALUES('api','original');",
        )
        .unwrap();
    }
    let db = open(b.path());
    db.execute(
        "DELETE FROM symbols WHERE rowid=(SELECT MIN(rowid) FROM symbols)",
        [],
    )
    .unwrap();
    db.execute("UPDATE call_edges SET value='different'", [])
        .unwrap();
    let report = compare(a.path(), b.path());
    assert_eq!(report["equal"], false);
    assert_eq!(report["different_tables"], json!(["symbols", "call_edges"]));
    assert_eq!(table(&report, "symbols")["incremental_rows"], 2);
    assert_eq!(table(&report, "symbols")["full_rows"], 1);
    for name in ["symbols", "call_edges"] {
        assert_eq!(table(&report, name)["different_row_count"], 1);
    }
}

#[test]
fn every_existing_table_remains_part_of_the_comparison() {
    let a = fixture();
    let b = fixture();
    let db = open(b.path());
    for name in oracle::tables() {
        db.execute(
            &format!("INSERT INTO \"{name}\"(name,value) VALUES ('fact',1)"),
            [],
        )
        .unwrap();
    }
    let report = compare(a.path(), b.path());
    assert_eq!(report["different_tables"], json!(oracle::tables()));
    assert!(report["tables"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["equal"] == false));
}

#[test]
fn compares_more_than_the_legacy_row_budget_and_detects_the_last_row_change() {
    let a = fixture();
    let b = fixture();
    for root in [a.path(), b.path()] {
        let mut db = open(root);
        let tx = db.transaction().unwrap();
        let mut insert = tx
            .prepare("INSERT INTO symbols(name,value) VALUES (?1,?2)")
            .unwrap();
        for i in 0..100_002 {
            insert
                .execute(params![format!("symbol-{i:06}"), i])
                .unwrap();
        }
        drop(insert);
        tx.commit().unwrap();
    }
    assert!(oracle::canonical(a.path())
        .unwrap_err()
        .to_string()
        .contains("row budget"));
    let report = compare(a.path(), b.path());
    assert_eq!(report["equal"], true);
    assert_eq!(table(&report, "symbols")["incremental_rows"], 100_002);
    assert_eq!(table(&report, "symbols")["equal_input_order_witness"], true);
    open(b.path())
        .execute("UPDATE symbols SET value=-1 WHERE name='symbol-100001'", [])
        .unwrap();
    let report = compare(a.path(), b.path());
    assert_eq!(report["equal"], false);
    assert_eq!(table(&report, "symbols")["different_row_count"], 1);
    assert_eq!(table(&report, "symbols")["equal_input_order_witness"], false);
}

#[test]
fn equal_sequence_witness_still_checks_the_second_input_budget_and_first_error() {
    let a = fixture();
    let b = fixture();
    for root in [a.path(), b.path()] {
        open(root)
            .execute("INSERT INTO symbols(name,value) VALUES ('same',7)", [])
            .unwrap();
    }
    let canonical = oracle::canonical(a.path()).unwrap();
    let first_side_bytes = serde_json::to_string(&canonical["symbols"][0])
        .unwrap()
        .len() as u64;
    let limits = oracle::StreamingLimits {
        max_canonical_bytes: first_side_bytes,
        ..oracle::StreamingLimits::default()
    };
    assert!(oracle::compare_streaming(a.path(), b.path(), limits)
        .unwrap_err()
        .to_string()
        .contains("canonical byte budget"));

    open(b.path())
        .execute("INSERT INTO symbols(name,value) VALUES ('extra',8)", [])
        .unwrap();
    let limits = oracle::StreamingLimits {
        max_rows_per_table: 1,
        ..oracle::StreamingLimits::default()
    };
    assert!(oracle::compare_streaming(a.path(), b.path(), limits)
        .unwrap_err()
        .to_string()
        .contains("row budget exceeded: symbols side 1"));

    // A's original spool must fail before B's different, oversized row is
    // inspected. An attempted witness must not change this error precedence.
    open(a.path())
        .execute_batch("INSERT INTO symbols(name,value) VALUES ('bad',CAST(x'80' AS TEXT));")
        .unwrap();
    open(b.path())
        .execute("UPDATE symbols SET value=?1", ["x".repeat(256)])
        .unwrap();
    let limits = oracle::StreamingLimits {
        max_row_bytes: 128,
        ..oracle::StreamingLimits::default()
    };
    assert!(oracle::compare_streaming(a.path(), b.path(), limits)
        .unwrap_err()
        .to_string()
        .contains("UTF8"));
}

#[test]
fn views_and_virtual_tables_keep_the_original_two_spool_comparison() {
    for virtual_table in [false, true] {
        let a = fixture();
        let b = fixture();
        for root in [a.path(), b.path()] {
            let db = open(root);
            db.execute("DROP TABLE symbols", []).unwrap();
            if virtual_table {
                db.execute_batch(
                    "CREATE VIRTUAL TABLE symbols USING fts5(name,value);
                     INSERT INTO symbols(name,value) VALUES ('same','fact');",
                )
                .unwrap();
            } else {
                db.execute_batch(
                    "CREATE VIEW symbols AS SELECT 'same' AS name, 'fact' AS value;",
                )
                .unwrap();
            }
        }
        let ca = oracle::canonical(a.path()).unwrap();
        let cb = oracle::canonical(b.path()).unwrap();
        assert_eq!(ca, cb);
        let report = compare(a.path(), b.path());
        let row = table(&report, "symbols");
        assert_eq!(report["equal"], true);
        assert_eq!(row["equal_input_order_witness"], false);
        assert_eq!(
            row["full_digest"],
            manifest::digest(&serde_json::to_vec(&cb["symbols"]).unwrap())
        );
    }
}

#[test]
fn row_byte_and_disk_budgets_fail_instead_of_certifying_a_prefix() {
    let a = fixture();
    let b = fixture();
    for root in [a.path(), b.path()] {
        let db = open(root);
        for i in 0..100 {
            db.execute(
                "INSERT INTO symbols(name,value) VALUES (?1,?2)",
                params![i.to_string(), "x".repeat(1000)],
            )
            .unwrap();
        }
    }
    let base = oracle::StreamingLimits::default();
    for limits in [
        oracle::StreamingLimits {
            max_rows_per_table: 99,
            ..base
        },
        oracle::StreamingLimits {
            max_canonical_bytes: 100,
            ..base
        },
        oracle::StreamingLimits {
            max_row_bytes: 100,
            ..base
        },
        oracle::StreamingLimits {
            max_scratch_bytes: 64 * 1024,
            ..base
        },
    ] {
        assert!(oracle::compare_streaming(a.path(), b.path(), limits).is_err());
    }
    // A failed comparison neither consumes nor changes its authoritative inputs.
    assert_eq!(compare(a.path(), b.path())["equal"], true);
}

#[test]
fn malformed_schema_utf8_and_foreign_keys_cannot_be_equal() {
    let a = fixture();
    let b = fixture();
    let db = open(b.path());
    db.execute_batch("INSERT INTO symbols(name,value) VALUES ('bad',CAST(x'80' AS TEXT));")
        .unwrap();
    assert!(
        oracle::compare_streaming(a.path(), b.path(), oracle::StreamingLimits::default())
            .unwrap_err()
            .to_string()
            .contains("UTF8")
    );
    db.execute_batch(
        "DELETE FROM symbols;
        CREATE TABLE parent(key INTEGER PRIMARY KEY);
        CREATE TABLE child(key INTEGER REFERENCES parent(key));
        PRAGMA foreign_keys=OFF;
        INSERT INTO child VALUES (1);",
    )
    .unwrap();
    assert!(
        oracle::compare_streaming(a.path(), b.path(), oracle::StreamingLimits::default())
            .unwrap_err()
            .to_string()
            .contains("foreign_key_check")
    );
    db.execute_batch("DELETE FROM child; DROP TABLE symbols;")
        .unwrap();
    assert!(
        oracle::compare_streaming(a.path(), b.path(), oracle::StreamingLimits::default())
            .unwrap_err()
            .to_string()
            .contains("table missing")
    );
}
