//! Index database schema management (rebuild-on-mismatch strategy with
//! incremental migration support).

use cc_model::CcResult;
use rusqlite::Connection;

/// Bump this whenever the schema changes. Any stored version that differs
/// from this value triggers a full database rebuild (delete + recreate),
/// unless a complete migration chain exists in [`MIGRATIONS`].
pub const CURRENT_SCHEMA_VERSION: u32 = 14;

pub(crate) const FULL_SCHEMA_SQL: &str = include_str!("sql/index_v1.sql");

/// A single incremental migration step from one schema version to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationStep {
    pub from_version: u32,
    pub to_version: u32,
    pub sql: &'static str,
    pub description: &'static str,
}

/// Registered migration steps. When the stored schema version is behind
/// `CURRENT_SCHEMA_VERSION`, the system tries to build a complete chain
/// from these steps before falling back to a full rebuild.
///
/// Add new entries here when bumping `CURRENT_SCHEMA_VERSION`:
/// ```ignore
/// MigrationStep {
///     from_version: 13,
///     to_version: 14,
///     sql: "ALTER TABLE test_edges ADD COLUMN parser_tier TEXT NOT NULL DEFAULT 'generic';",
///     description: "Add parser_tier to test_edges",
/// },
/// ```
pub static MIGRATIONS: &[MigrationStep] = &[
    MigrationStep {
        from_version: 13,
        to_version: 14,
        sql: "CREATE TABLE IF NOT EXISTS runtime_evidence (\
              evidence_id TEXT PRIMARY KEY, http_edge_id TEXT, route_id TEXT, \
              service_name TEXT NOT NULL, method TEXT, path TEXT NOT NULL, \
              status_code TEXT, observed_count INTEGER NOT NULL DEFAULT 1, \
              p95_ms REAL, first_seen TEXT NOT NULL, last_seen TEXT NOT NULL, \
              trace_source TEXT NOT NULL DEFAULT 'otlp');\
              CREATE INDEX IF NOT EXISTS idx_runtime_evidence_path ON runtime_evidence(path);\
              CREATE INDEX IF NOT EXISTS idx_runtime_evidence_edge ON runtime_evidence(http_edge_id);\
              CREATE TABLE IF NOT EXISTS adr (\
              adr_id TEXT PRIMARY KEY, title TEXT NOT NULL, \
              status TEXT NOT NULL DEFAULT 'accepted', \
              context TEXT NOT NULL DEFAULT '', decision TEXT NOT NULL DEFAULT '', \
              created_at TEXT NOT NULL, updated_at TEXT NOT NULL);\
              CREATE INDEX IF NOT EXISTS idx_adr_status ON adr(status);",
        description: "Add runtime_evidence and adr tables",
    },
];

/// Find a complete, ordered migration chain from `from` to `to`.
///
/// Returns `None` if no complete chain can be assembled from [`MIGRATIONS`].
fn find_migration_chain(from: u32, to: u32) -> Option<Vec<&'static MigrationStep>> {
    if from >= to {
        return None;
    }

    let mut relevant: Vec<&MigrationStep> = MIGRATIONS
        .iter()
        .filter(|s| s.from_version >= from && s.to_version <= to)
        .collect();

    relevant.sort_by_key(|s| s.from_version);

    // Verify the chain is complete and contiguous.
    if relevant.is_empty() {
        return None;
    }
    if relevant.first().unwrap().from_version != from {
        return None;
    }
    if relevant.last().unwrap().to_version != to {
        return None;
    }
    for window in relevant.windows(2) {
        if window[0].to_version != window[1].from_version {
            return None;
        }
    }

    Some(relevant)
}

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
        // Try incremental migration before falling back to full rebuild.
        if stored < CURRENT_SCHEMA_VERSION {
            if let Some(chain) = find_migration_chain(stored, CURRENT_SCHEMA_VERSION) {
                tracing::info!(
                    from = stored,
                    to = CURRENT_SCHEMA_VERSION,
                    steps = chain.len(),
                    "applying incremental schema migration"
                );

                // Run all migration steps inside a single transaction.
                conn.execute_batch("BEGIN;")
                    .map_err(|e| cc_model::CcError::Database(format!("migration begin failed: {e}")))?;

                for step in &chain {
                    tracing::debug!(
                        from = step.from_version,
                        to = step.to_version,
                        description = step.description,
                        "executing migration step"
                    );
                    if let Err(e) = conn.execute_batch(step.sql) {
                        // Roll back on failure.
                        let _ = conn.execute_batch("ROLLBACK;");
                        return Err(cc_model::CcError::Database(format!(
                            "migration step {} -> {} failed: {e}",
                            step.from_version, step.to_version
                        )));
                    }
                }

                conn.execute_batch("COMMIT;")
                    .map_err(|e| cc_model::CcError::Database(format!("migration commit failed: {e}")))?;

                conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                    .map_err(|e| cc_model::CcError::Database(e.to_string()))?;

                return Ok(SchemaStatus::Migrated {
                    from: stored,
                    to: CURRENT_SCHEMA_VERSION,
                });
            }
        }

        // Non-zero but mismatched and no migration chain — caller must delete and recreate.
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
    /// Schema was incrementally migrated from one version to another.
    Migrated { from: u32, to: u32 },
    /// Stored version differs from expected — database file must be deleted.
    Mismatch { stored: u32 },
}

/// Get the current schema version.
pub fn index_schema_version(conn: &Connection) -> u32 {
    conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_version_without_migration_chain_returns_mismatch() {
        // A version far behind CURRENT_SCHEMA_VERSION with no migration chain
        // should return Mismatch.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(FULL_SCHEMA_SQL).unwrap();
        // Set user_version to something well behind — no chain from 10 to current.
        conn.pragma_update(None, "user_version", 10u32).unwrap();

        let status = migrate_index_db(&conn).unwrap();
        assert_eq!(status, SchemaStatus::Mismatch { stored: 10 });
    }

    #[test]
    fn migration_13_to_14_succeeds() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(FULL_SCHEMA_SQL).unwrap();
        conn.pragma_update(None, "user_version", 13u32).unwrap();

        let status = migrate_index_db(&conn).unwrap();
        assert_eq!(
            status,
            SchemaStatus::Migrated { from: 13, to: 14 }
        );
    }

    #[test]
    fn find_chain_complete() {
        // Build a synthetic chain 10 -> 11 -> 12.
        let steps: &[MigrationStep] = &[
            MigrationStep {
                from_version: 10,
                to_version: 11,
                sql: "",
                description: "step 1",
            },
            MigrationStep {
                from_version: 11,
                to_version: 12,
                sql: "",
                description: "step 2",
            },
        ];

        let chain = find_chain_in(steps, 10, 12);
        assert!(chain.is_some());
        let chain = chain.unwrap();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].from_version, 10);
        assert_eq!(chain[1].to_version, 12);
    }

    #[test]
    fn find_chain_incomplete() {
        // Missing the 11 -> 12 step.
        let steps: &[MigrationStep] = &[MigrationStep {
            from_version: 10,
            to_version: 11,
            sql: "",
            description: "step 1",
        }];

        let chain = find_chain_in(steps, 10, 12);
        assert!(chain.is_none());
    }

    #[test]
    fn find_chain_gap_in_middle() {
        // Steps 10->11 and 12->13, missing 11->12.
        let steps: &[MigrationStep] = &[
            MigrationStep {
                from_version: 10,
                to_version: 11,
                sql: "",
                description: "step 1",
            },
            MigrationStep {
                from_version: 12,
                to_version: 13,
                sql: "",
                description: "step 3",
            },
        ];

        let chain = find_chain_in(steps, 10, 13);
        assert!(chain.is_none());
    }

    #[test]
    fn schema_status_migrated_equality() {
        let a = SchemaStatus::Migrated { from: 10, to: 13 };
        let b = SchemaStatus::Migrated { from: 10, to: 13 };
        let c = SchemaStatus::Migrated { from: 11, to: 13 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    /// Helper that mirrors `find_migration_chain` but operates on an
    /// arbitrary slice instead of the static `MIGRATIONS`.
    fn find_chain_in(steps: &[MigrationStep], from: u32, to: u32) -> Option<Vec<&MigrationStep>> {
        if from >= to {
            return None;
        }

        let mut relevant: Vec<&MigrationStep> = steps
            .iter()
            .filter(|s| s.from_version >= from && s.to_version <= to)
            .collect();

        relevant.sort_by_key(|s| s.from_version);

        if relevant.is_empty() {
            return None;
        }
        if relevant.first().unwrap().from_version != from {
            return None;
        }
        if relevant.last().unwrap().to_version != to {
            return None;
        }
        for window in relevant.windows(2) {
            if window[0].to_version != window[1].from_version {
                return None;
            }
        }

        Some(relevant)
    }
}
