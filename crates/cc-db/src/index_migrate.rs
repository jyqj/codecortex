//! Index database schema management (rebuild-on-mismatch strategy).

use cc_model::CcResult;
use rusqlite::Connection;

/// Bump this whenever the schema changes. Any stored version that differs
/// from this value triggers a full database rebuild (delete + recreate).
pub const CURRENT_SCHEMA_VERSION: u32 = 12;

pub(crate) const FULL_SCHEMA_SQL: &str = include_str!("sql/index_v1.sql");

/// Check the stored schema version and apply the full schema if needed.
///
/// Returns `Ok(true)` if the database was freshly initialized (version was 0),
/// returns `Ok(false)` if the version already matches.
/// Returns `Err(SchemaMismatch)` if the stored version is non-zero but differs
/// from `CURRENT_SCHEMA_VERSION` — the caller should delete the db file and retry.
pub fn migrate_index_db(conn: &Connection) -> CcResult<SchemaStatus> {
    let stored = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(|e| cc_model::CcError::Database(e.to_string()))?;

    if stored == CURRENT_SCHEMA_VERSION {
        return Ok(SchemaStatus::UpToDate);
    }

    if stored != 0 {
        // Non-zero but mismatched — caller must delete and recreate.
        tracing::warn!(
            stored_version = stored,
            expected_version = CURRENT_SCHEMA_VERSION,
            "index schema version mismatch, rebuild required"
        );
        return Ok(SchemaStatus::Mismatch { stored });
    }

    // Fresh database (version 0): apply full schema.
    tracing::info!(
        version = CURRENT_SCHEMA_VERSION,
        "initializing index schema"
    );
    conn.execute_batch(FULL_SCHEMA_SQL)
        .map_err(|e| cc_model::CcError::Database(format!("schema init failed: {}", e)))?;
    conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|e| cc_model::CcError::Database(e.to_string()))?;

    Ok(SchemaStatus::Initialized)
}

/// Result of schema version check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaStatus {
    /// Schema is already at the expected version.
    UpToDate,
    /// Fresh database, schema was just created.
    Initialized,
    /// Stored version differs from expected — database file must be deleted.
    Mismatch { stored: u32 },
}

/// Get the current schema version.
pub fn index_schema_version(conn: &Connection) -> u32 {
    conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .unwrap_or(0)
}
