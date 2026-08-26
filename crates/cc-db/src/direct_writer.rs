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
    /// 2. Creates tables (and virtual tables) from `schema_sql`, deferring indexes
    /// 3. Calls `write_fn` inside a single transaction for bulk INSERT
    /// 4. Creates indexes after all data is written
    /// 5. Restores normal PRAGMAs (WAL mode, synchronous=NORMAL)
    /// 6. Runs `PRAGMA integrity_check` to validate
    pub fn write_db(
        path: &Path,
        schema_sql: &str,
        write_fn: impl FnOnce(&rusqlite::Transaction) -> CcResult<()>,
    ) -> CcResult<()> {
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

        let table_sql = extract_table_statements(schema_sql);
        conn.execute_batch(&table_sql)
            .map_err(stage_err("schema tables"))?;

        {
            let tx = conn
                .unchecked_transaction()
                .map_err(stage_err("transaction"))?;
            write_fn(&tx)?;
            tx.commit().map_err(stage_err("commit"))?;
        }

        let index_sql = extract_index_statements(schema_sql);
        if !index_sql.trim().is_empty() && index_sql.trim() != ";" {
            conn.execute_batch(&index_sql)
                .map_err(stage_err("indexes"))?;
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

fn extract_table_statements(sql: &str) -> String {
    let mut result = String::new();
    for stmt in split_sql_statements(sql) {
        let kind = classify_statement(stmt);
        if matches!(
            kind,
            StmtKind::CreateTable | StmtKind::CreateVirtualTable | StmtKind::Insert
        ) {
            result.push_str(stmt);
            result.push_str(";\n");
        }
    }
    result
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

fn split_sql_statements(sql: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\'' {
                        i += 1;
                        if i < len && bytes[i] == b'\'' {
                            i += 1;
                        } else {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b'-' if i + 1 < len && bytes[i + 1] == b'-' => {
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b';' => {
                let stmt = &sql[start..i];
                let trimmed = stmt.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed);
                }
                start = i + 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
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

    #[test]
    fn extract_table_stmts() {
        let sql = "CREATE TABLE t1 (id INTEGER PRIMARY KEY);\n\
                   CREATE INDEX idx_t1 ON t1(id);\n\
                   CREATE VIRTUAL TABLE t1_fts USING fts5(text);\n\
                   INSERT INTO t1 VALUES(1);";
        let result = extract_table_statements(sql);
        assert!(result.contains("CREATE TABLE t1"));
        assert!(result.contains("CREATE VIRTUAL TABLE t1_fts"));
        assert!(result.contains("INSERT INTO t1"));
        assert!(!result.contains("CREATE INDEX"));
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
