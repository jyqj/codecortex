//! Index database schema management (rebuild-on-mismatch strategy).

use cc_model::CcResult;
use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

pub(crate) const FULL_SCHEMA_SQL: &str = include_str!("sql/index_v1.sql");

/// Check the stored schema version and apply the full schema if needed.
///
/// Returns `Ok(Initialized)` if the database was freshly created (version was 0).
/// Returns `Ok(UpToDate)` if the version already matches.
/// Returns `Ok(Mismatch)` if the stored version differs — the caller should
/// delete the db file and retry.
pub fn migrate_index_db(conn: &Connection) -> CcResult<SchemaStatus> {
    let stored = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(|e| cc_model::CcError::Database(e.to_string()))?;

    if stored == CURRENT_SCHEMA_VERSION {
        return Ok(SchemaStatus::UpToDate);
    }

    if stored != 0 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_initializes_schema() {
        let conn = Connection::open_in_memory().unwrap();
        let status = migrate_index_db(&conn).unwrap();
        assert_eq!(status, SchemaStatus::Initialized);

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn matching_version_returns_up_to_date() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(FULL_SCHEMA_SQL).unwrap();
        conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
            .unwrap();

        let status = migrate_index_db(&conn).unwrap();
        assert_eq!(status, SchemaStatus::UpToDate);
    }

    #[test]
    fn old_version_returns_mismatch() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(FULL_SCHEMA_SQL).unwrap();
        conn.pragma_update(None, "user_version", 99u32).unwrap();

        let status = migrate_index_db(&conn).unwrap();
        assert_eq!(status, SchemaStatus::Mismatch { stored: 99 });
    }
}
