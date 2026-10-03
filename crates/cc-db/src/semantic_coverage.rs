//! Semantic coverage and the epoch-consistent read discipline (P6-012).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-012: "分母明确（eligible/published/failed/stale）；Aux 重试不冲刷完整
//! 查询缓存；零 eligible 有原因") and
//! `artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
//! P6-012 ("cc-db 新增只读统计（单连接一致读）"). This module is read-only:
//! it issues SELECTs only and never touches a clock.
//!
//! Coverage口径 (the brief's denominator, verbatim intent):
//!
//! - **eligible** — `document_manifest` rows whose `encoding_key` is not
//!   NULL (documents with an embeddable input, P6-003) counted *against the
//!   single active space*. No active space means nothing is eligible: the
//!   honest answer is zero with [`ZeroEligibleReason::SemanticNotConfigured`],
//!   never an inflated count of documents no space has declared.
//! - **published** — `semantic_manifest` rows of the active space (the
//!   visible set). Rows of `backfilling`/`revoked` spaces never count: a
//!   different space's vectors are never returned (P6-017, "不同空间分数永不
//!   混排"), so they are not coverage either.
//! - **uncovered** — `eligible - published` (the差集口径: the visible set is
//!   a subset of the eligible set by construction — publish CAS fence 4
//!   refuses NULL-encoding rows and the FK keeps every published row's base
//!   document present). The uncovered list is the per-document projection of
//!   the same差集, so a dense-recall consumer can state its scope honestly
//!   ("published over N of M eligible documents").
//! - **failed** — active-space `semantic_outbox` rows in terminal `failed`.
//! - **stale** — active-space live (`pending`/`claimed`) tasks whose
//!   `(doc_key, doc_version)` no longer matches `document_manifest` (the
//!   version moved on, or the base row is gone). The write path supersedes
//!   eagerly (P6-006), so this is normally zero; the counter exists to make
//!   any lag observable instead of assumed.
//!
//! Zero eligible always has a reason ([`ZeroEligibleReason`]); an eligible
//! count above zero reports `reason: None` (nothing to explain).
//!
//! Epoch read-side discipline (the read twin of the P6-011 write-side CAS):
//! every semantic diagnostic read takes the strict `ReadGeneration` before
//! and after its counting SELECTs **on one pooled connection** and retries
//! when the generation moved — the same single-connection consistent-read
//! pattern the capability snapshot already uses
//! (`crates/cc-server/src/capability_status.rs` before/after compare). The
//! returned snapshot therefore pairs the counts with the exact generation
//! they were computed against. `semantic_epoch == None` means "semantic not
//! ready" and is never folded to 0 (ADR-0003: strict reads only); it simply
//! coexists with a zero `published` count and the not-configured reason.
//! Epoch scope (the batch-3 closure ruling, unifying P6-006 with Q4):
//! `semantic_epoch` advances with *semantic state* — outbox re-enqueue that
//! changed the desired set (the P6-006 nonzero-stat convention) plus
//! visible-set changes (P6-011 publish CAS / revocation) — while `Auxiliary`
//! commits (claim/renew/retry bookkeeping) never advance it. Eligibility
//! changes alone do not move it either, so consumers must cache by the
//! snapshot generation, not assume coverage is epoch-invariant.

use cc_model::generation::ReadGeneration;
use cc_model::{CcError, CcResult};
use rusqlite::Connection;

use crate::index_db::ReadOps;
use crate::sql_util::db_err;

/// Why the eligible denominator is zero (brief: "零 eligible 有原因").
///
/// Reported when `eligible == 0`; an `eligible > 0` snapshot carries
/// `reason: None`. Precedence: not-configured wins — without an active space
/// the other questions are not even meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroEligibleReason {
    /// No `semantic_spaces` row is `active` (semantic not wired/configured).
    /// All counts are honestly zero — documents may exist but no space has
    /// claimed them.
    SemanticNotConfigured,
    /// An active space exists but `document_manifest` is empty.
    NoDocuments,
    /// An active space exists and documents exist, but none of them carries
    /// an embeddable input (`encoding_key IS NULL` everywhere).
    EncodingUnsupported,
}

/// Coverage of the active semantic space over the embeddable document set.
///
/// All counters are single-connection consistent reads (see module docs);
/// `uncovered` is the差集 `eligible - published` (saturating — the subset
/// invariant makes the subtraction non-negative in any legal database).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticCoverage {
    /// Embeddable documents (`encoding_key IS NOT NULL`) against the active
    /// space; 0 with [`ZeroEligibleReason::SemanticNotConfigured`] when no
    /// space is active.
    pub eligible: u64,
    /// Visible-set rows (`semantic_manifest`) of the active space only.
    pub published: u64,
    /// `eligible - published`: embeddable documents not yet visible in the
    /// active space.
    pub uncovered: u64,
    /// Active-space outbox tasks in terminal `failed`.
    pub failed: u64,
    /// Active-space live (`pending`/`claimed`) tasks whose base document no
    /// longer matches their `doc_version`.
    pub stale: u64,
    /// The zero-eligibility cause when `eligible == 0`, else `None`.
    pub reason: Option<ZeroEligibleReason>,
}

/// One document of the uncovered list (the per-document差集 projection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticUncovered {
    pub doc_key: String,
    /// Redundant from `document_manifest` for scope statements and filters.
    pub file_path: String,
    /// The version that would be published next (the current base version).
    pub doc_version: String,
    /// Authoritative `files.language`, matching the published manifest scan.
    /// `None` means the file row is missing, not `Language::Unknown`.
    pub language: Option<String>,
}

/// A coverage reading paired with the strict generation it was taken under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCoverageSnapshot {
    pub coverage: SemanticCoverage,
    /// `ReadGeneration` observed identical immediately before and after the
    /// counting SELECTs on the same connection.
    pub generation: ReadGeneration,
}

/// Consistent-read retry budget, mirroring the capability snapshot's
/// before/after compare (`RetrievalChanged { attempts: 3 }`).
const CONSISTENT_READ_ATTEMPTS: u32 = 3;

/// Coverage counters for the single active space, on the caller's connection
/// (`*_on` cc-db pattern). No active space → the honest all-zero snapshot
/// with [`ZeroEligibleReason::SemanticNotConfigured`]; the document table is
/// not even consulted (nothing is eligible for a space that does not exist).
pub fn coverage_on(conn: &Connection) -> CcResult<SemanticCoverage> {
    let Some(space_id) = crate::semantic_outbox::active_space_on(conn)? else {
        return Ok(SemanticCoverage {
            eligible: 0,
            published: 0,
            uncovered: 0,
            failed: 0,
            stale: 0,
            reason: Some(ZeroEligibleReason::SemanticNotConfigured),
        });
    };
    let count = |sql: &str, space: Option<&str>| -> CcResult<u64> {
        let mut stmt = conn.prepare_cached(sql).map_err(db_err)?;
        let value: i64 = match space {
            Some(s) => stmt.query_row([s], |r| r.get(0)).map_err(db_err)?,
            None => stmt.query_row([], |r| r.get(0)).map_err(db_err)?,
        };
        Ok(value.max(0) as u64)
    };
    let eligible = count(
        "SELECT count(*) FROM document_manifest WHERE encoding_key IS NOT NULL",
        None,
    )?;
    let published = count(
        "SELECT count(*) FROM semantic_manifest WHERE space_id=?1",
        Some(&space_id),
    )?;
    let failed = count(
        "SELECT count(*) FROM semantic_outbox WHERE space_id=?1 AND state='failed'",
        Some(&space_id),
    )?;
    let stale = count(
        "SELECT count(*) FROM semantic_outbox t \
         WHERE t.space_id=?1 AND t.state IN ('pending','claimed') \
         AND NOT EXISTS (SELECT 1 FROM document_manifest d \
                         WHERE d.doc_key=t.doc_key AND d.doc_version=t.doc_version)",
        Some(&space_id),
    )?;
    let reason = if eligible == 0 {
        let document_rows = count("SELECT count(*) FROM document_manifest", None)?;
        Some(if document_rows == 0 {
            ZeroEligibleReason::NoDocuments
        } else {
            ZeroEligibleReason::EncodingUnsupported
        })
    } else {
        None
    };
    Ok(SemanticCoverage {
        eligible,
        published,
        uncovered: eligible.saturating_sub(published),
        failed,
        stale,
        reason,
    })
}

/// The uncovered list: embeddable documents with no active-space publication,
/// keyset-paginated on `doc_key` (same discipline as the P6-010 candidate
/// scan — deterministic order, no OFFSET re-scan). Rows published only in a
/// `backfilling`/`revoked` space stay uncovered: a foreign space's vector is
/// never returned, so it never covers anything. No active space → empty
/// (nothing can be uncovered against a space that does not exist).
/// `limit == 0` is a caller bug and is rejected instead of looping forever.
pub fn uncovered_on(
    conn: &Connection,
    after_doc_key: &str,
    limit: usize,
) -> CcResult<Vec<SemanticUncovered>> {
    if limit == 0 {
        return Err(CcError::InvalidParams(
            "semantic uncovered scan requires limit >= 1".into(),
        ));
    }
    let Some(space_id) = crate::semantic_outbox::active_space_on(conn)? else {
        return Ok(Vec::new());
    };
    let mut stmt = conn
        .prepare_cached(
            "SELECT d.doc_key, d.file_path, d.doc_version, f.language \
             FROM document_manifest AS d \
             LEFT JOIN files AS f ON f.file_path=d.file_path \
             WHERE d.encoding_key IS NOT NULL AND d.doc_key > ?2 \
             AND NOT EXISTS (SELECT 1 FROM semantic_manifest m \
                             WHERE m.doc_key=d.doc_key AND m.space_id=?1) \
             ORDER BY d.doc_key ASC LIMIT ?3",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map(
            rusqlite::params![space_id, after_doc_key, limit as i64],
            |row| {
                Ok(SemanticUncovered {
                    doc_key: row.get(0)?,
                    file_path: row.get(1)?,
                    doc_version: row.get(2)?,
                    language: row.get(3)?,
                })
            },
        )
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

impl ReadOps<'_> {
    /// One generation-stable coverage reading: the strict `ReadGeneration` is
    /// taken before and after [`coverage_on`] on the same pooled connection;
    /// a moved generation retries, and an exhausted budget fails with
    /// `RetrievalChanged` rather than returning counts of mixed generations
    /// (the read twin of the P6-011 publish fencing — strict reads only).
    pub fn semantic_coverage(&self) -> CcResult<SemanticCoverageSnapshot> {
        for _ in 0..CONSISTENT_READ_ATTEMPTS {
            let conn = self.0.read_conn()?;
            let before = crate::read_generation::read_on(&conn)?;
            let coverage = coverage_on(&conn)?;
            let after = crate::read_generation::read_on(&conn)?;
            if before == after {
                return Ok(SemanticCoverageSnapshot {
                    coverage,
                    generation: before,
                });
            }
        }
        Err(CcError::RetrievalChanged {
            attempts: CONSISTENT_READ_ATTEMPTS,
        })
    }

    /// One generation-stable page of [`uncovered_on`] (same before/after
    /// discipline as [`ReadOps::semantic_coverage`]).
    pub fn semantic_uncovered(
        &self,
        after_doc_key: &str,
        limit: usize,
    ) -> CcResult<Vec<SemanticUncovered>> {
        for _ in 0..CONSISTENT_READ_ATTEMPTS {
            let conn = self.0.read_conn()?;
            let before = crate::read_generation::read_on(&conn)?;
            let rows = uncovered_on(&conn, after_doc_key, limit)?;
            let after = crate::read_generation::read_on(&conn)?;
            if before == after {
                return Ok(rows);
            }
        }
        Err(CcError::RetrievalChanged {
            attempts: CONSISTENT_READ_ATTEMPTS,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v22_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        assert!(matches!(
            crate::index_migrate::migrate_index_db(&conn).unwrap(),
            crate::index_migrate::SchemaStatus::Initialized
                | crate::index_migrate::SchemaStatus::UpToDate
        ));
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn
    }

    /// FK chain (files → chunks → document_manifest) for one document.
    fn seed_document(conn: &Connection, doc_key: &str, encoding_key: Option<&str>) {
        conn.execute_batch(&format!(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
               VALUES('src/{doc_key}.rs','rust','hash',1.0,1,'2026-01-01');
             INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
               VALUES('c-{doc_key}','src/{doc_key}.rs','rust',0,1,2,'body');
             INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
               reference_json,record_json) \
               VALUES('{doc_key}','v1','src/{doc_key}.rs','c-{doc_key}',{},'{{}}','{{}}');",
            match encoding_key {
                Some(k) => format!("'{k}'"),
                None => "NULL".to_string(),
            }
        ))
        .unwrap();
    }

    fn activate(conn: &Connection, space_id: &str) {
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space_id],
        )
        .unwrap();
    }

    fn register_space(conn: &Connection, space_id: &str, state: &str) {
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}',?2)",
            rusqlite::params![space_id, state],
        )
        .unwrap();
    }

    /// A published visible-set row for `doc_key` in `space_id` (bypasses the
    /// CAS on purpose: coverage reads must not depend on how the row got
    /// there, and the write side is P6-011's tested domain).
    fn publish(conn: &Connection, doc_key: &str, space_id: &str) {
        conn.execute(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,\
             input_digest,space_id,artifact_ref,published_at,published_incarnation) \
             SELECT doc_key,doc_version,file_path,encoding_key,'in',?2,'art','2026-01-01','inc' \
             FROM document_manifest WHERE doc_key=?1 \
             ON CONFLICT(doc_key) DO UPDATE SET space_id=excluded.space_id",
            rusqlite::params![doc_key, space_id],
        )
        .unwrap();
    }

    #[test]
    fn unwired_semantic_reports_zero_coverage_without_inflating() {
        let conn = v22_conn();
        // Documents exist and would be embeddable — but no space has claimed
        // them, so the honest口径 is zeros, not an inflated denominator.
        seed_document(&conn, "d1", Some("enc"));
        seed_document(&conn, "d2", Some("enc"));

        let coverage = coverage_on(&conn).unwrap();
        assert_eq!(coverage.eligible, 0);
        assert_eq!(coverage.published, 0);
        assert_eq!(coverage.uncovered, 0);
        assert_eq!(coverage.failed, 0);
        assert_eq!(coverage.stale, 0);
        assert_eq!(
            coverage.reason,
            Some(ZeroEligibleReason::SemanticNotConfigured)
        );
        assert!(uncovered_on(&conn, "", 10).unwrap().is_empty());

        // A backfilling-only space is still not an active space.
        register_space(&conn, "sp-future", "backfilling");
        assert_eq!(
            coverage_on(&conn).unwrap().reason,
            Some(ZeroEligibleReason::SemanticNotConfigured)
        );
    }

    #[test]
    fn zero_eligible_is_explained_by_documents_and_encoding() {
        let conn = v22_conn();
        activate(&conn, "sp");
        // Active space, no documents at all.
        assert_eq!(
            coverage_on(&conn).unwrap().reason,
            Some(ZeroEligibleReason::NoDocuments)
        );
        // Documents exist but none is embeddable.
        seed_document(&conn, "d1", None);
        assert_eq!(
            coverage_on(&conn).unwrap().reason,
            Some(ZeroEligibleReason::EncodingUnsupported)
        );
        // One embeddable document: eligible > 0 ⇒ no reason to explain.
        seed_document(&conn, "d2", Some("enc"));
        let coverage = coverage_on(&conn).unwrap();
        assert_eq!(coverage.eligible, 1);
        assert_eq!(coverage.reason, None);
    }

    #[test]
    fn partial_coverage_reports_the_gap_and_the_uncovered_list() {
        let conn = v22_conn();
        activate(&conn, "sp");
        for key in ["d1", "d2", "d3"] {
            seed_document(&conn, key, Some("enc"));
        }
        publish(&conn, "d1", "sp");

        let coverage = coverage_on(&conn).unwrap();
        assert_eq!(
            coverage,
            SemanticCoverage {
                eligible: 3,
                published: 1,
                uncovered: 2,
                failed: 0,
                stale: 0,
                reason: None,
            }
        );

        // The list is the per-document projection of the same差集.
        let page = uncovered_on(&conn, "", 2).unwrap();
        assert_eq!(
            page.iter().map(|r| r.doc_key.as_str()).collect::<Vec<_>>(),
            ["d2", "d3"]
        );
        assert_eq!(page[0].file_path, "src/d2.rs");
        assert_eq!(page[0].doc_version, "v1");
        assert_eq!(page[0].language.as_deref(), Some("rust"));
        assert!(uncovered_on(&conn, "d3", 2).unwrap().is_empty());
        // Deterministic single-row window past the published doc.
        assert_eq!(uncovered_on(&conn, "d1", 1).unwrap()[0].doc_key, "d2");
    }

    #[test]
    fn uncovered_projects_authoritative_language_without_dropping_missing_files() {
        let conn = v22_conn();
        activate(&conn, "sp");
        for key in ["d1", "d2", "d3"] {
            seed_document(&conn, key, Some("enc"));
        }
        conn.execute(
            "UPDATE files SET language='python' WHERE file_path='src/d1.rs'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE files SET language='unknown' WHERE file_path='src/d2.rs'",
            [],
        )
        .unwrap();
        // Defensive LEFT JOIN semantics match the published scan. Normal
        // FK-enforced writes would cascade the document away with its file.
        conn.execute_batch(
            "PRAGMA foreign_keys=OFF; DELETE FROM files WHERE file_path='src/d3.rs';",
        )
        .unwrap();
        let rows = uncovered_on(&conn, "", 3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].language.as_deref(), Some("python"));
        assert_eq!(rows[1].language.as_deref(), Some("unknown"));
        assert_eq!(rows[2].language, None);
        assert_eq!(uncovered_on(&conn, "d2", 1).unwrap(), rows[2..]);
        assert_eq!(coverage_on(&conn).unwrap().uncovered, 3);
    }

    #[test]
    fn full_coverage_reports_no_gap_and_no_reason() {
        let conn = v22_conn();
        activate(&conn, "sp");
        for key in ["d1", "d2"] {
            seed_document(&conn, key, Some("enc"));
            publish(&conn, key, "sp");
        }
        let coverage = coverage_on(&conn).unwrap();
        assert_eq!(coverage.eligible, 2);
        assert_eq!(coverage.published, 2);
        assert_eq!(coverage.uncovered, 0);
        assert_eq!(coverage.reason, None);
        assert!(uncovered_on(&conn, "", 10).unwrap().is_empty());
    }

    #[test]
    fn foreign_space_rows_never_count_as_published() {
        let conn = v22_conn();
        activate(&conn, "sp-new");
        register_space(&conn, "sp-old", "revoked");
        for key in ["d1", "d2"] {
            seed_document(&conn, key, Some("enc"));
        }
        // Both documents were published in the old (now revoked) space; only
        // d1 has been re-published into the active space.
        publish(&conn, "d1", "sp-old");
        publish(&conn, "d2", "sp-old");
        publish(&conn, "d1", "sp-new");

        let coverage = coverage_on(&conn).unwrap();
        assert_eq!(coverage.eligible, 2);
        assert_eq!(coverage.published, 1, "only the active space counts");
        assert_eq!(coverage.uncovered, 1);
        // d2 is uncovered in the active space even though a revoked-space
        // vector exists — a foreign space covers nothing (P6-017 混排拒绝).
        let uncovered = uncovered_on(&conn, "", 10).unwrap();
        assert_eq!(
            uncovered
                .iter()
                .map(|r| r.doc_key.as_str())
                .collect::<Vec<_>>(),
            ["d2"]
        );
    }

    #[test]
    fn failed_counts_only_active_space_failures() {
        let conn = v22_conn();
        activate(&conn, "sp");
        register_space(&conn, "sp-old", "revoked");
        seed_document(&conn, "d1", Some("enc"));
        seed_document(&conn, "d2", Some("enc"));
        // d2's task is re-homed to the revoked space (simulating a task left
        // over from before the space switch): its failure must not count.
        for (key, space) in [("d1", "sp"), ("d2", "sp-old")] {
            crate::semantic_outbox::supersede_and_enqueue_on(
                &conn,
                &crate::semantic_outbox::OutboxPlan {
                    upserts: &[crate::semantic_outbox::OutboxUpsert {
                        doc_key: key.into(),
                        doc_version: "v1".into(),
                        input_digest: format!("in-{key}"),
                    }],
                    removals: &[],
                    now_unix: 900.0,
                },
            )
            .unwrap();
            if space != "sp" {
                conn.execute(
                    "UPDATE semantic_outbox SET space_id=?2 WHERE doc_key=?1",
                    rusqlite::params![key, space],
                )
                .unwrap();
            }
            let task = crate::semantic_outbox::claim_next_on(&conn, space, "w", 1000.0, 60.0)
                .unwrap()
                .unwrap();
            assert!(crate::semantic_outbox::retry_on(
                &conn,
                task.task_id,
                &task.token,
                "boom",
                1100.0,
                5.0,
                1,
            )
            .unwrap());
        }
        assert_eq!(coverage_on(&conn).unwrap().failed, 1);
    }

    #[test]
    fn stale_counts_live_tasks_lagging_the_base_manifest() {
        let conn = v22_conn();
        activate(&conn, "sp");
        seed_document(&conn, "d1", Some("enc"));
        seed_document(&conn, "d2", Some("enc"));
        seed_document(&conn, "d3", Some("enc"));
        seed_document(&conn, "d4", Some("enc"));
        for key in ["d1", "d2", "d3", "d4"] {
            crate::semantic_outbox::supersede_and_enqueue_on(
                &conn,
                &crate::semantic_outbox::OutboxPlan {
                    upserts: &[crate::semantic_outbox::OutboxUpsert {
                        doc_key: key.into(),
                        doc_version: "v1".into(),
                        input_digest: format!("in-{key}"),
                    }],
                    removals: &[],
                    now_unix: 900.0,
                },
            )
            .unwrap();
        }
        // d1: pending at the current version — live, not stale.
        // d2: base version moved on (simulated mid-window write) → stale.
        conn.execute(
            "UPDATE document_manifest SET doc_version='v2' WHERE doc_key='d2'",
            [],
        )
        .unwrap();
        // d3: base row gone (deletion between claim and publish) → stale.
        conn.execute("DELETE FROM document_manifest WHERE doc_key='d3'", [])
            .unwrap();
        // d4: claimed at the current version — live, not stale.
        let task = crate::semantic_outbox::claim_next_on(&conn, "sp", "w", 1000.0, 60.0)
            .unwrap()
            .unwrap();
        assert_eq!(task.doc_key, "d1", "claim is FIFO by task_id");

        let coverage = coverage_on(&conn).unwrap();
        assert_eq!(coverage.stale, 2, "d2 (version moved) + d3 (base gone)");
        assert_eq!(coverage.eligible, 3, "d3's base row is gone, not eligible");
        assert_eq!(coverage.published, 0);
        assert_eq!(coverage.uncovered, 3, "stale tasks are not coverage");
    }

    #[test]
    fn coverage_reads_are_pure_and_zero_limit_is_rejected() {
        let conn = v22_conn();
        activate(&conn, "sp");
        seed_document(&conn, "d1", Some("enc"));
        let before = crate::read_generation::read_on(&conn).unwrap();
        let _ = coverage_on(&conn).unwrap();
        let _ = uncovered_on(&conn, "", 5).unwrap();
        assert_eq!(
            before,
            crate::read_generation::read_on(&conn).unwrap(),
            "coverage reads never touch a clock"
        );
        assert!(matches!(
            uncovered_on(&conn, "", 0).unwrap_err(),
            CcError::InvalidParams(_)
        ));
    }
}
