//! Index database schema management (rebuild-on-mismatch strategy).

use crate::sql_util::db_err;
use cc_model::CcResult;
use rusqlite::Connection;

/// v7: scan/diff content hashes switched from SHA-256 to blake3 (hex width
/// unchanged). Stored hashes are not comparable across algorithms, so the
/// version bump routes pre-blake3 databases through the standard
/// rebuild-on-mismatch reset instead of letting every file re-hash as
/// "changed" on the first incremental build.
/// v8/v9 were unaccepted PublicSurface drafts. v10 persists the reviewed
/// Rust/JS v3 policies, including nested active attributes and computed keys.
/// Reset legacy/draft caches: unchanged files must not retain falsely-known
/// interfaces from an earlier extractor.
/// v11 adds durable resolution outcomes/dependencies and indexed Go package
/// membership. Old caches cannot certify absence of this required evidence.
/// v13: durable resolution invalidation frontier and AST-backed Python calls.
/// Legacy caches may contain invented parser_exact calls and cannot be reused.
/// v14 removes legacy JS/TS regex-created calls/refs and fixes call positions.
/// v15: immutable project inputs and module-resolution evidence; prior caches
/// must not retain pre-configuration import/name fallback results.
/// v16 persists import syntax/scope plus package and multilingual module evidence.
/// v17 distinguishes package/file/external/ambiguous/unknown module outcomes and Go inputs.
/// v18 removes cross-language legacy module guesses and strengthens captured input reads.
/// v19 preserves original chunk bytes and source-coordinate evidence.
/// v20 stamps each file with the chunk policy that actually committed.
/// v21: current document versions and prepared model inputs; old caches require rebuild.
pub const CURRENT_SCHEMA_VERSION: u32 = 21;

pub(crate) const FULL_SCHEMA_SQL: &str = include_str!("sql/index_v1.sql");

/// Check the stored schema version and apply the full schema if needed.
///
/// Returns `Ok(Initialized)` if the database was freshly created (version was 0).
/// Returns `Ok(UpToDate)` if the version already matches.
/// Returns `Ok(Mismatch)` if the stored version differs — the caller should
/// destructively reset the database and retry.
pub fn migrate_index_db(conn: &Connection) -> CcResult<SchemaStatus> {
    let stored = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(db_err)?;

    if stored == CURRENT_SCHEMA_VERSION {
        crate::read_generation::ensure(conn)?;
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
        .map_err(db_err)?;

    crate::read_generation::ensure(conn)?;
    Ok(SchemaStatus::Initialized)
}

/// Result of schema version check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaStatus {
    /// Schema is already at the expected version.
    UpToDate,
    /// Fresh database, schema was just created.
    Initialized,
    /// Stored version differs from expected — database must be rebuilt.
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
