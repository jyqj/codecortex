//! High-speed SQLite writer for full index rebuilds.
//!
//! Uses a **hybrid strategy**: rusqlite for correctness + aggressive PRAGMAs
//! for speed (journal off, synchronous off, 64KB pages, exclusive locking).
//!
//! This avoids the risk of hand-writing B-tree pages while still being
//! significantly faster than the standard `rebuild_with_temp_db` path:
//! - `journal_mode = OFF` (no rollback journal overhead)
//! - `synchronous = OFF` (skip fsync)
//! - `page_size = 65536` (fewer I/O ops for large payloads)
//! - `locking_mode = EXCLUSIVE` (no lock acquire/release per statement)
//! - Indexes created *after* all data is inserted (bulk-load pattern)
//! - Single transaction for all INSERTs
//!
//! # Status
//! Production-ready. Enable via `use_direct_writer: true` in IndexingConfig.

use std::path::Path;

use cc_model::{CcError, CcResult};
use rusqlite::Connection;

/// Wrap a rusqlite error with a stage label, preserving the message shape the
/// String-error era produced (`"<stage>: <sqlite error>"`).
fn stage_err(stage: &str) -> impl Fn(rusqlite::Error) -> CcError + '_ {
    move |e| CcError::Database(format!("{stage}: {e}"))
}

pub struct DirectWriter;

impl DirectWriter {
    pub fn new() -> Self {
        Self
    }

    /// Create a high-speed SQLite database.
    ///
    /// 1. Opens a new database file with aggressive PRAGMAs
    /// 2. Executes the complete schema, then drops explicit indexes for bulk loading
    /// 3. Calls `write_fn` inside a single transaction for bulk INSERT
    /// 4. Creates indexes after all data is written
    /// 5. Restores normal PRAGMAs (WAL mode, synchronous=NORMAL)
    /// 6. Runs `PRAGMA integrity_check` to validate
    pub fn write_db(
        path: &Path,
        schema_sql: &str,
        write_fn: impl FnOnce(&rusqlite::Transaction) -> CcResult<()>,
    ) -> CcResult<()> {
        // SQLite's C API stops at NUL; do not silently discard a schema suffix.
        if schema_sql.as_bytes().contains(&0) {
            return Err(CcError::Database(
                "schema tables: embedded NUL in schema".into(),
            ));
        }
        let conn = Connection::open(path).map_err(stage_err("open"))?;
        // The per-file insert helpers rotate ~20 distinct prepare_cached
        // statements; the default capacity of 16 would re-prepare each one
        // on every file (see `IndexDb::open_and_ensure_schema`).
        conn.set_prepared_statement_cache_capacity(64);

        conn.execute_batch(
            "PRAGMA page_size = 65536;
             PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             PRAGMA locking_mode = EXCLUSIVE;
             PRAGMA cache_size = -262144;
             PRAGMA temp_store = MEMORY;
             PRAGMA mmap_size = 0;",
        )
        .map_err(stage_err("pragmas"))?;

        // SQLite executes the original schema in order: triggers, views and
        // seed statements must have exactly the same initialization semantics
        // as the normal temp-db path (including seed-time unique constraints).
        conn.execute_batch(schema_sql)
            .map_err(stage_err("schema tables"))?;

        // Read names and SQL from SQLite rather than parsing CREATE INDEX.
        // sql IS NOT NULL excludes automatic constraint indexes, which must
        // remain active. This also handles quoted names and partial indexes.
        let indexes: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'index' AND sql IS NOT NULL ORDER BY name")
                .map_err(stage_err("schema indexes"))?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(stage_err("schema indexes"))?;
            rows.collect::<rusqlite::Result<_>>()
                .map_err(stage_err("schema indexes"))?
        };
        for (name, _) in &indexes {
            let quoted = name.replace('"', "\"\"");
            conn.execute_batch(&format!("DROP INDEX \"{quoted}\";"))
                .map_err(stage_err("drop indexes"))?;
        }

        {
            let tx = conn
                .unchecked_transaction()
                .map_err(stage_err("transaction"))?;
            write_fn(&tx)?;
            tx.commit().map_err(stage_err("commit"))?;
        }

        for (_, sql) in &indexes {
            conn.execute_batch(sql).map_err(stage_err("indexes"))?;
        }

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA locking_mode = NORMAL;",
        )
        .map_err(stage_err("restore pragmas"))?;

        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(stage_err("integrity"))?;

        if integrity != "ok" {
            return Err(CcError::Database(format!(
                "integrity check failed: {}",
                integrity
            )));
        }

        Ok(())
    }

    pub fn verify(path: &Path) -> CcResult<bool> {
        let conn = Connection::open(path).map_err(stage_err("open for verify"))?;
        let result: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(stage_err("integrity_check"))?;
        Ok(result == "ok")
    }
}

impl Default for DirectWriter {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn extract_index_statements(sql: &str) -> String {
    let mut result = String::new();
    for stmt in split_sql_statements(sql) {
        let kind = classify_statement(stmt);
        if matches!(kind, StmtKind::CreateIndex | StmtKind::CreateUniqueIndex) {
            result.push_str(stmt);
            result.push_str(";\n");
        }
    }
    result
}

/// Select explicitly maintained indexes from the canonical schema, in caller order.
/// Missing or repeated names fail closed rather than silently changing maintenance.
pub(crate) fn selected_index_statements(sql: &str, names: &[&str]) -> Option<String> {
    let statements = split_sql_statements(sql);
    let mut result = String::new();
    for (position, name) in names.iter().enumerate() {
        if names[..position].contains(name) {
            return None;
        }
        let mut matching = statements.iter().copied().filter(|stmt| {
            matches!(
                classify_statement(stmt),
                StmtKind::CreateIndex | StmtKind::CreateUniqueIndex
            ) && index_name(stmt) == Some(*name)
        });
        let statement = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        result.push_str(statement);
        result.push_str(";\n");
    }
    Some(result)
}

/// Emit `DROP INDEX IF EXISTS <name>;` for every index defined in `sql`.
///
/// Derived from the same schema as [`extract_index_statements`] so the bulk-
/// rebuild drop/recreate pair shares one source of truth and can never drift
/// from the canonical index set in `index_v1.sql`.
pub(crate) fn drop_index_statements(sql: &str) -> String {
    let mut result = String::new();
    for stmt in split_sql_statements(sql) {
        let kind = classify_statement(stmt);
        if matches!(kind, StmtKind::CreateIndex | StmtKind::CreateUniqueIndex) {
            if let Some(name) = index_name(stmt) {
                result.push_str("DROP INDEX IF EXISTS ");
                result.push_str(name);
                result.push_str(";\n");
            }
        }
    }
    result
}

/// Extract the index name from `CREATE [UNIQUE] INDEX [IF NOT EXISTS] <name> ON ...`.
///
/// The name is the token immediately preceding `ON`.
fn index_name(stmt: &str) -> Option<&str> {
    let mut s = stmt.trim_start();
    while s.starts_with("--") {
        let nl = s.find('\n')?;
        s = s[nl + 1..].trim_start();
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    let on_pos = words.iter().position(|w| w.eq_ignore_ascii_case("ON"))?;
    let name_tok = words.get(on_pos.checked_sub(1)?)?;
    Some(name_tok.split('(').next().unwrap_or(name_tok))
}

#[derive(Debug, PartialEq)]
enum StmtKind {
    CreateTable,
    CreateVirtualTable,
    CreateIndex,
    CreateUniqueIndex,
    Insert,
    Other,
}

fn classify_statement(stmt: &str) -> StmtKind {
    let mut s = stmt;
    loop {
        s = s.trim_start();
        if s.starts_with("--") {
            if let Some(nl) = s.find('\n') {
                s = &s[nl + 1..];
            } else {
                return StmtKind::Other;
            }
        } else {
            break;
        }
    }
    let upper = s.to_uppercase();
    if upper.starts_with("CREATE VIRTUAL TABLE") {
        StmtKind::CreateVirtualTable
    } else if upper.starts_with("CREATE UNIQUE INDEX") {
        StmtKind::CreateUniqueIndex
    } else if upper.starts_with("CREATE INDEX") {
        StmtKind::CreateIndex
    } else if upper.starts_with("CREATE TABLE") {
        StmtKind::CreateTable
    } else if upper.starts_with("INSERT") {
        StmtKind::Insert
    } else {
        StmtKind::Other
    }
}

/// Helpers operate on a schema already validated by execute_batch. Preserve
/// the final tail (including unterminated input) for SQLite to diagnose when
/// executed; sqlite3_complete detects boundaries, not SQL syntax validity.
fn split_sql_statements(sql: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    for (i, byte) in sql.bytes().enumerate() {
        if byte != b';' {
            continue;
        }
        let candidate = &sql[start..=i];
        let Ok(c_sql) = std::ffi::CString::new(candidate) else {
            // Validated canonical schemas cannot contain NUL. Preserve the
            // tail rather than inventing a boundary for invalid helper input.
            break;
        };
        // SAFETY: CString provides a NUL-terminated UTF-8 input alive for the
        // call. SQLite handles quotes, comments and complete trigger bodies.
        if unsafe { rusqlite::ffi::sqlite3_complete(c_sql.as_ptr()) } == 1 {
            statements.push(candidate[..candidate.len() - 1].trim());
            start = i + 1;
        }
    }
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        statements.push(tail);
    }
    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPLEX_SCHEMA: &str = r#"
        /* schema ; comment */
        CREATE TABLE "source;table"(id INTEGER PRIMARY KEY, "value;name" TEXT UNIQUE);
        CREATE TABLE `audit;table`([event;name] TEXT);
        CREATE VIEW "view;name" AS SELECT "value;name" FROM "source;table";
        CREATE TRIGGER "trigger;name" AFTER INSERT ON "source;table" BEGIN
            INSERT INTO `audit;table` VALUES(CASE WHEN new.id > 0 THEN 'first;it''s' ELSE 'zero' END);
            -- body ; comment
            INSERT INTO `audit;table` VALUES('second;event'); /* body ; block */
        END;
        INSERT INTO "source;table" VALUES(1, 'seed;value');
        CREATE UNIQUE INDEX "unique;"" name" ON "source;table"("value;name");
        CREATE INDEX `index;name` ON `audit;table`([event;name]);
        INSERT INTO "source;table" VALUES(2, 'seed;two');
        -- trailing ; comment
    "#;

    #[test]
    fn selected_indexes_keep_order_and_reject_missing_or_duplicate_definitions() {
        let schema = "
            CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT);
            CREATE TRIGGER audit AFTER INSERT ON items BEGIN
                UPDATE items SET value='CREATE INDEX hidden ON items(id);' WHERE id=new.id;
            END;
            -- Preserve the partial predicate, including its quoted semicolon.
            CREATE INDEX second ON items(value) WHERE value='a;b';
            CREATE UNIQUE INDEX first ON items(id);
            CREATE INDEX unrelated ON items(value,id);
        ";
        let selected = selected_index_statements(schema, &["first", "second"]).unwrap();
        assert!(selected.find("INDEX first").unwrap() < selected.find("INDEX second").unwrap());
        assert!(selected.contains("WHERE value='a;b'"));
        assert!(!selected.contains("CREATE TRIGGER"));
        assert!(!selected.contains("INDEX unrelated"));
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT);")
            .unwrap();
        conn.execute_batch(&selected).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='index'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
        assert!(selected_index_statements(schema, &["missing"]).is_none());
        assert!(selected_index_statements(schema, &["hidden"]).is_none());
        assert!(selected_index_statements(schema, &["first", "first"]).is_none());
        let repeated = format!("{schema}\nCREATE INDEX first ON items(value);");
        assert!(selected_index_statements(&repeated, &["first"]).is_none());
    }

    #[test]
    fn sqlite_boundaries_preserve_triggers_quotes_comments_and_tail() {
        let statements = split_sql_statements(COMPLEX_SCHEMA);
        assert_eq!(statements.len(), 9); // eight SQL statements + trailing comment
        let conn = Connection::open_in_memory().unwrap();
        for statement in statements {
            conn.execute_batch(statement).unwrap();
        }
        let events: i64 = conn
            .query_row("SELECT count(*) FROM `audit;table`", [], |r| r.get(0))
            .unwrap();
        assert_eq!(events, 4);
        let tail = "CREATE TABLE tail(id); INSERT INTO tail VALUES(1)";
        assert_eq!(split_sql_statements(tail).len(), 2);
        let indexes = extract_index_statements(COMPLEX_SCHEMA);
        assert!(indexes.contains("unique;"));
        assert!(indexes.contains("index;name"));
        assert!(!indexes.contains("second;event"));
    }

    #[test]
    fn full_schema_preserves_seed_views_triggers_and_defers_explicit_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("complex.sqlite3");
        DirectWriter::write_db(&path, COMPLEX_SCHEMA, |tx| {
            let events: i64 = tx
                .query_row("SELECT count(*) FROM `audit;table`", [], |r| r.get(0))
                .map_err(stage_err("seed"))?;
            assert_eq!(events, 4);
            let explicit: i64 = tx
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type='index' AND sql IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
                .map_err(stage_err("indexes"))?;
            assert_eq!(explicit, 0);
            // Automatic UNIQUE constraint indexes remain active during loading.
            assert!(tx
                .execute_batch(r#"INSERT INTO "source;table" VALUES(3, 'seed;value');"#)
                .is_err());
            tx.execute_batch(r#"INSERT INTO "source;table" VALUES(3, 'loaded;value');"#)
                .map_err(stage_err("load"))
        })
        .unwrap();
        let actual = Connection::open(&path).unwrap();
        let expected = Connection::open_in_memory().unwrap();
        expected.execute_batch(COMPLEX_SCHEMA).unwrap();
        expected
            .execute_batch(r#"INSERT INTO "source;table" VALUES(3, 'loaded;value');"#)
            .unwrap();
        for conn in [&actual, &expected] {
            assert_eq!(
                conn.query_row("SELECT count(*) FROM `audit;table`", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                6
            );
            assert_eq!(
                conn.query_row(r#"SELECT count(*) FROM "view;name""#, [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                3
            );
        }
        let objects = |conn: &Connection| -> Vec<(String, String, Option<String>)> {
            conn.prepare("SELECT type,name,sql FROM sqlite_schema ORDER BY type,name")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(objects(&actual), objects(&expected));
    }

    #[test]
    fn schema_and_deferred_unique_errors_propagate_without_dropping_input() {
        use std::cell::Cell;
        let dir = tempfile::tempdir().unwrap();
        for (i, schema) in [
            "CREATE TABLE t(id); CREATE TRIGGER tr AFTER INSERT ON t BEGIN INSERT INTO t VALUES(2);",
            "CREATE TABLE t(id); INSERT INTO t VALUES('unterminated);",
            "CREATE TABLE t(id); this is invalid;",
            "CREATE TABLE t(id); CREATE VIEW v AS SELECT FROM t;",
            "CREATE TABLE t(id); /* unterminated",
            "CREATE TABLE t(id);\0CREATE TABLE discarded(id);",
            "CREATE TABLE t(id); CREATE UNIQUE INDEX u ON t(id); INSERT INTO t VALUES(1),(1);",
        ].iter().enumerate() {
            let called = Cell::new(false);
            let result = DirectWriter::write_db(&dir.path().join(format!("bad-{i}.db")), schema, |_| {
                called.set(true);
                Ok(())
            });
            // SQLite accepts an unterminated block comment as a comment tail.
            if i == 4 {
                assert!(result.is_ok());
                assert!(called.get());
            } else {
                assert!(result.unwrap_err().to_string().contains("schema tables"), "{schema}");
                assert!(!called.get());
            }
        }
        let path = dir.path().join("unique.db");
        let err = DirectWriter::write_db(
            &path,
            "CREATE TABLE t(id); CREATE UNIQUE INDEX u ON t(id);",
            |tx| {
                tx.execute_batch("INSERT INTO t VALUES(1),(1);")
                    .map_err(stage_err("load"))
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("indexes:"));
        let path = dir.path().join("tail.db");
        DirectWriter::write_db(&path, "CREATE TABLE t(id); INSERT INTO t VALUES(1)", |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(
            Connection::open(path)
                .unwrap()
                .query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn extract_index_stmts() {
        let sql = "CREATE TABLE t1 (id INTEGER PRIMARY KEY);\n\
                   CREATE INDEX idx_t1 ON t1(id);\n\
                   CREATE UNIQUE INDEX idx_t1_u ON t1(id);\n\
                   INSERT INTO t1 VALUES(1);";
        let result = extract_index_statements(sql);
        assert!(result.contains("CREATE INDEX"));
        assert!(result.contains("CREATE UNIQUE INDEX"));
        assert!(!result.contains("CREATE TABLE"));
        assert!(!result.contains("INSERT"));
    }

    #[test]
    fn drop_index_stmts_match_create_set() {
        let sql = "CREATE TABLE t1 (id INTEGER PRIMARY KEY);\n\
                   CREATE INDEX IF NOT EXISTS idx_a ON t1(id);\n\
                   CREATE UNIQUE INDEX idx_b ON t1(id, name);\n\
                   INSERT INTO t1 VALUES(1);";
        let drops = drop_index_statements(sql);
        // One DROP per index name; tables/inserts are ignored.
        assert!(drops.contains("DROP INDEX IF EXISTS idx_a;"));
        assert!(drops.contains("DROP INDEX IF EXISTS idx_b;"));
        assert!(!drops.contains("CREATE"));
        // Drop and create are derived from the same source, so the index sets match.
        let creates = extract_index_statements(sql);
        for name in ["idx_a", "idx_b"] {
            assert!(creates.contains(name), "create missing {name}");
            assert!(drops.contains(name), "drop missing {name}");
        }
    }

    #[test]
    fn direct_writer_creates_valid_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite3");

        DirectWriter::write_db(
            &db_path,
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
             CREATE INDEX idx_test_name ON test(name);",
            |tx| {
                tx.execute("INSERT INTO test VALUES (1, 'hello')", [])
                    .map_err(stage_err("insert"))?;
                tx.execute("INSERT INTO test VALUES (2, 'world')", [])
                    .map_err(stage_err("insert"))?;
                Ok(())
            },
        )
        .unwrap();

        assert!(DirectWriter::verify(&db_path).unwrap());

        let conn = Connection::open(&db_path).unwrap();
        let name: String = conn
            .query_row("SELECT name FROM test WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "hello");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn direct_writer_handles_fts_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("fts_test.sqlite3");

        let schema = "CREATE TABLE docs (id INTEGER PRIMARY KEY, title TEXT, body TEXT);
                      CREATE VIRTUAL TABLE docs_fts USING fts5(title, body);
                      CREATE INDEX idx_docs_title ON docs(title);";

        DirectWriter::write_db(&db_path, schema, |tx| {
            tx.execute(
                "INSERT INTO docs VALUES (1, 'Rust', 'A systems language')",
                [],
            )
            .map_err(stage_err("insert"))?;
            tx.execute(
                "INSERT INTO docs_fts VALUES ('Rust', 'A systems language')",
                [],
            )
            .map_err(stage_err("insert"))?;
            Ok(())
        })
        .unwrap();

        assert!(DirectWriter::verify(&db_path).unwrap());

        let conn = Connection::open(&db_path).unwrap();
        let title: String = conn
            .query_row(
                "SELECT title FROM docs_fts WHERE docs_fts MATCH 'systems'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "Rust");
    }

    #[test]
    fn direct_writer_write_fn_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("err_test.sqlite3");

        let result = DirectWriter::write_db(
            &db_path,
            "CREATE TABLE test (id INTEGER PRIMARY KEY);",
            |_tx| Err(CcError::Database("intentional error".to_string())),
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("intentional error"));
    }

    #[test]
    fn direct_writer_bulk_insert_perf() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("bulk_test.sqlite3");

        DirectWriter::write_db(
            &db_path,
            "CREATE TABLE items (id INTEGER PRIMARY KEY, data TEXT);
             CREATE INDEX idx_items_data ON items(data);",
            |tx| {
                let mut stmt = tx
                    .prepare_cached("INSERT INTO items VALUES (?1, ?2)")
                    .map_err(stage_err("prepare"))?;
                for i in 0..10_000 {
                    stmt.execute(rusqlite::params![i, format!("item_{}", i)])
                        .map_err(stage_err("insert"))?;
                }
                Ok(())
            },
        )
        .unwrap();

        assert!(DirectWriter::verify(&db_path).unwrap());

        let conn = Connection::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 10_000);
    }

    #[test]
    fn split_sql_handles_comments_and_strings() {
        let sql = "-- This is a comment\n\
                   CREATE TABLE t1 (id INTEGER); -- inline comment\n\
                   INSERT INTO t1 VALUES(1);\n\
                   INSERT INTO t1 VALUES('hello;world');";
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 3);
        assert!(stmts[0].contains("CREATE TABLE"));
        assert!(stmts[1].contains("INSERT INTO t1 VALUES(1)"));
        assert!(stmts[2].contains("hello;world"));

        assert_eq!(classify_statement(stmts[0]), StmtKind::CreateTable);
        assert_eq!(classify_statement(stmts[1]), StmtKind::Insert);
        assert_eq!(classify_statement(stmts[2]), StmtKind::Insert);
    }
}
