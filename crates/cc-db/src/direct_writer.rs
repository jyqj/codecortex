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

use rusqlite::Connection;

// Low-level SQLite constants retained for future direct page-writing path.
#[allow(dead_code)]
/// SQLite page size for direct writer (64KB, larger than default 4KB)
const PAGE_SIZE: u32 = 65536;

#[allow(dead_code)]
/// SQLite file format header size
const HEADER_SIZE: usize = 100;

#[allow(dead_code)]
/// B-tree leaf table page type
const BTREE_LEAF_TABLE: u8 = 0x0D;

#[allow(dead_code)]
/// B-tree interior table page type
const BTREE_INTERIOR_TABLE: u8 = 0x05;

/// A single database page being constructed (retained for future low-level use)
#[allow(dead_code)]
struct PageBuilder {
    /// Page data buffer (PAGE_SIZE bytes)
    data: Vec<u8>,
    /// Current cell count
    cell_count: u16,
    /// Offset where next cell content starts (grows downward from end).
    /// Stored as u32 to avoid overflow when PAGE_SIZE == 65536 (exceeds u16::MAX).
    /// SQLite encodes a content-offset of 65536 as the value 0 in the 2-byte header field.
    content_offset: u32,
    /// Cell pointer offsets (grows upward from header)
    cell_pointers: Vec<u16>,
}

#[allow(dead_code)]
impl PageBuilder {
    fn new(page_type: u8) -> Self {
        let mut data = vec![0u8; PAGE_SIZE as usize];
        data[0] = page_type;
        Self {
            data,
            cell_count: 0,
            content_offset: PAGE_SIZE,
            cell_pointers: Vec::new(),
        }
    }

    /// Available space for cell content
    fn available_space(&self) -> usize {
        let header_end = if self.cell_count == 0 {
            8
        } else {
            8 + self.cell_pointers.len() * 2
        };
        let co = self.content_offset as usize;
        if co > header_end + 2 {
            co - header_end - 2
        } else {
            0
        }
    }

    /// Add a cell to this page. Returns false if not enough space.
    fn add_cell(&mut self, cell_data: &[u8]) -> bool {
        if cell_data.len() > self.available_space() {
            return false;
        }
        self.content_offset -= cell_data.len() as u32;
        let co = self.content_offset as usize;
        self.data[co..co + cell_data.len()].copy_from_slice(cell_data);
        // Cell pointers are always within a single page so u16 is sufficient here.
        self.cell_pointers.push(self.content_offset as u16);
        self.cell_count += 1;
        true
    }

    /// Finalize page: write header fields and cell pointer array
    fn finalize(&mut self) {
        // Cell count at offset 3-4
        self.data[3] = (self.cell_count >> 8) as u8;
        self.data[4] = (self.cell_count & 0xFF) as u8;
        // First byte of cell content at offset 5-6.
        // SQLite spec: 65536 is encoded as 0 in this 2-byte field.
        let co_encoded = (self.content_offset & 0xFFFF) as u16;
        self.data[5] = (co_encoded >> 8) as u8;
        self.data[6] = (co_encoded & 0xFF) as u8;
        // Cell pointer array starts at offset 8
        for (i, &ptr) in self.cell_pointers.iter().enumerate() {
            let offset = 8 + i * 2;
            self.data[offset] = (ptr >> 8) as u8;
            self.data[offset + 1] = (ptr & 0xFF) as u8;
        }
    }
}

/// SQLite varint encoding (1-9 bytes, big-endian with MSB continuation)
#[allow(dead_code)]
fn encode_varint(value: u64) -> Vec<u8> {
    if value <= 0x7F {
        return vec![value as u8];
    }
    let mut bytes = Vec::with_capacity(9);
    let mut v = value;
    let mut buf = [0u8; 9];
    let mut i = 8;
    buf[i] = (v & 0x7F) as u8;
    v >>= 7;
    while v > 0 {
        i -= 1;
        buf[i] = 0x80 | (v & 0x7F) as u8;
        v >>= 7;
    }
    bytes.extend_from_slice(&buf[i..=8]);
    bytes
}

/// Determine the SQLite serial type for an integer value
#[allow(dead_code)]
fn int_serial_type(value: i64) -> u8 {
    if value == 0 {
        return 8;
    }
    if value == 1 {
        return 9;
    }
    if (-128..=127).contains(&value) {
        return 1;
    }
    if (-32768..=32767).contains(&value) {
        return 2;
    }
    if (-8388608..=8388607).contains(&value) {
        return 3;
    }
    if (-2147483648..=2147483647).contains(&value) {
        return 4;
    }
    6 // 8-byte integer
}

/// Serial type for a text value
#[allow(dead_code)]
fn text_serial_type(len: usize) -> u64 {
    (len as u64) * 2 + 13
}

/// Serial type for a blob value
#[allow(dead_code)]
fn blob_serial_type(len: usize) -> u64 {
    (len as u64) * 2 + 12
}

/// The direct writer handle.
///
/// Provides `write_db` — a high-speed path that creates a fresh SQLite file
/// with aggressive PRAGMAs, writes all data via a caller-supplied closure,
/// then validates the result with `PRAGMA integrity_check`.
pub struct DirectWriter {
    /// Retained for future low-level page-writing path.
    #[allow(dead_code)]
    pages: Vec<Vec<u8>>,
}

impl DirectWriter {
    pub fn new() -> Self {
        Self { pages: Vec::new() }
    }

    /// Write the 100-byte SQLite database header (retained for future low-level use)
    #[allow(dead_code)]
    fn write_header(&self, page1: &mut [u8], total_pages: u32) {
        // Magic string
        page1[0..16].copy_from_slice(b"SQLite format 3\0");
        // Page size (2 bytes, big-endian; 65536 stored as 1 since it's 2^16)
        page1[16] = 0x01;
        page1[17] = 0x00;
        // File format versions
        page1[18] = 1; // write version
        page1[19] = 1; // read version
                       // Reserved space
        page1[20] = 0;
        // Max embedded payload fraction
        page1[21] = 64;
        // Min embedded payload fraction
        page1[22] = 32;
        // Leaf payload fraction
        page1[23] = 32;
        // File change counter (4 bytes)
        let fcc = 1u32;
        page1[24..28].copy_from_slice(&fcc.to_be_bytes());
        // Database size in pages
        page1[28..32].copy_from_slice(&total_pages.to_be_bytes());
        // Schema cookie
        page1[40..44].copy_from_slice(&1u32.to_be_bytes());
        // Schema format number
        page1[44..48].copy_from_slice(&4u32.to_be_bytes());
        // Text encoding (1 = UTF-8)
        page1[56..60].copy_from_slice(&1u32.to_be_bytes());
        // Version-valid-for (same as fcc)
        page1[92..96].copy_from_slice(&fcc.to_be_bytes());
        // SQLite version number
        page1[96..100].copy_from_slice(&3046000u32.to_be_bytes());
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
        write_fn: impl FnOnce(&rusqlite::Transaction) -> Result<(), String>,
    ) -> Result<(), String> {
        // 1. Create new database file
        let conn = Connection::open(path).map_err(|e| format!("open: {}", e))?;

        // 2. Aggressive PRAGMAs for maximum write throughput
        conn.execute_batch(
            "PRAGMA page_size = 65536;
             PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             PRAGMA locking_mode = EXCLUSIVE;
             PRAGMA cache_size = -262144;
             PRAGMA temp_store = MEMORY;
             PRAGMA mmap_size = 0;",
        )
        .map_err(|e| format!("pragmas: {}", e))?;

        // 3. Create schema (tables + virtual tables, skip indexes)
        let table_sql = extract_table_statements(schema_sql);
        conn.execute_batch(&table_sql)
            .map_err(|e| format!("schema tables: {}", e))?;

        // 4. Bulk write inside a single transaction
        {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("transaction: {}", e))?;
            write_fn(&tx)?;
            tx.commit().map_err(|e| format!("commit: {}", e))?;
        }

        // 5. Create indexes after all data is written (much faster than during inserts)
        let index_sql = extract_index_statements(schema_sql);
        if !index_sql.trim().is_empty() && index_sql.trim() != ";" {
            conn.execute_batch(&index_sql)
                .map_err(|e| format!("indexes: {}", e))?;
        }

        // 6. Restore normal PRAGMAs for subsequent readers
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA locking_mode = NORMAL;",
        )
        .map_err(|e| format!("restore pragmas: {}", e))?;

        // 7. Integrity check
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| format!("integrity: {}", e))?;

        if integrity != "ok" {
            return Err(format!("integrity check failed: {}", integrity));
        }

        Ok(())
    }

    /// Verify an existing database using SQLite's integrity check
    pub fn verify(path: &Path) -> Result<bool, String> {
        let conn = Connection::open(path).map_err(|e| format!("open for verify: {}", e))?;
        let result: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| format!("integrity_check: {}", e))?;
        Ok(result == "ok")
    }
}

impl Default for DirectWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract CREATE TABLE, CREATE VIRTUAL TABLE, and INSERT statements from SQL.
/// Skips CREATE INDEX / CREATE UNIQUE INDEX (those are applied after bulk insert).
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

/// Extract CREATE INDEX and CREATE UNIQUE INDEX statements from SQL.
fn extract_index_statements(sql: &str) -> String {
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

#[derive(Debug, PartialEq)]
enum StmtKind {
    CreateTable,
    CreateVirtualTable,
    CreateIndex,
    CreateUniqueIndex,
    Insert,
    Other,
}

/// Classify a SQL statement, skipping leading comments and whitespace.
fn classify_statement(stmt: &str) -> StmtKind {
    // Strip leading comment lines and whitespace to find the actual SQL keyword
    let mut s = stmt;
    loop {
        s = s.trim_start();
        if s.starts_with("--") {
            // Skip to end of line
            if let Some(nl) = s.find('\n') {
                s = &s[nl + 1..];
            } else {
                return StmtKind::Other; // entire statement is a comment
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

/// Split SQL text by semicolons, respecting that semicolons inside string
/// literals or SQL comments should not be treated as statement delimiters.
///
/// This is a simple splitter that handles the common cases in our schema SQL.
/// It does NOT handle all edge cases (e.g. nested quotes, dollar-quoted strings).
fn split_sql_statements(sql: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'\'' => {
                // Skip string literal
                i += 1;
                while i < len {
                    if bytes[i] == b'\'' {
                        i += 1;
                        if i < len && bytes[i] == b'\'' {
                            // Escaped quote
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
                // Skip line comment
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

    // Handle trailing statement without semicolon
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
    fn varint_encoding() {
        assert_eq!(encode_varint(0), vec![0]);
        assert_eq!(encode_varint(1), vec![1]);
        assert_eq!(encode_varint(127), vec![127]);
        assert_eq!(encode_varint(128), vec![0x81, 0x00]);
        assert_eq!(encode_varint(300), vec![0x82, 0x2C]);
    }

    #[test]
    fn serial_types() {
        assert_eq!(int_serial_type(0), 8);
        assert_eq!(int_serial_type(1), 9);
        assert_eq!(int_serial_type(100), 1);
        assert_eq!(int_serial_type(1000), 2);
        assert_eq!(text_serial_type(5), 23);
        assert_eq!(blob_serial_type(10), 32);
    }

    #[test]
    fn page_builder_basic() {
        let mut page = PageBuilder::new(BTREE_LEAF_TABLE);
        assert!(page.available_space() > 60000);
        assert!(page.add_cell(&[1, 2, 3, 4]));
        assert_eq!(page.cell_count, 1);
        page.finalize();
        assert_eq!(page.data[0], BTREE_LEAF_TABLE);
        assert_eq!(page.data[4], 1); // cell count low byte
    }

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
    fn direct_writer_creates_valid_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite3");

        DirectWriter::write_db(
            &db_path,
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
             CREATE INDEX idx_test_name ON test(name);",
            |tx| {
                tx.execute("INSERT INTO test VALUES (1, 'hello')", [])
                    .map_err(|e| e.to_string())?;
                tx.execute("INSERT INTO test VALUES (2, 'world')", [])
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        )
        .unwrap();

        // Verify integrity
        assert!(DirectWriter::verify(&db_path).unwrap());

        // Verify data is readable
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
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO docs_fts VALUES ('Rust', 'A systems language')",
                [],
            )
            .map_err(|e| e.to_string())?;
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
            |_tx| Err("intentional error".to_string()),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("intentional error"));
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
                    .map_err(|e| e.to_string())?;
                for i in 0..10_000 {
                    stmt.execute(rusqlite::params![i, format!("item_{}", i)])
                        .map_err(|e| e.to_string())?;
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
        // Statements may include leading/trailing comments. The important
        // thing is that classify_statement() correctly identifies them.
        assert!(stmts[0].contains("CREATE TABLE"));
        assert!(stmts[1].contains("INSERT INTO t1 VALUES(1)"));
        assert!(stmts[2].contains("hello;world"));

        // Verify classify works through comments
        assert_eq!(classify_statement(stmts[0]), StmtKind::CreateTable);
        assert_eq!(classify_statement(stmts[1]), StmtKind::Insert);
        assert_eq!(classify_statement(stmts[2]), StmtKind::Insert);
    }
}
