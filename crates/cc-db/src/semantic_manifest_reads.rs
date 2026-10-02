//! Read-only access to the published semantic manifest (P6-010).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (constraint row P6-010: "过滤先于 exact top-k、bounded batch、稳定 ties、
//! 空间隔离；删除/异空间向量不可返回") and
//! `artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
//! P6-010 ("候选来源：semantic_manifest × artifact cache，逐 bounded batch
//! 装载"). This module is the manifest half of that candidate source: a
//! deterministic, keyset-paginated scan over one published vector space.
//!
//! Scope discipline:
//!
//! - **Write paths untouched.** Every outbox/manifest write belongs to
//!   [`crate::semantic_outbox`] (P6-006/P6-007); this module issues SELECTs
//!   only. "删除文档不可返回" is structural: a deleted document's manifest row
//!   is gone (same-transaction revoke or FK `ON DELETE CASCADE`), so a deleted
//!   document can never enter the candidate stream.
//! - **Space isolation at the load layer.** The scan filters
//!   `WHERE space_id = ?`, so rows of a different
//!   [`VectorSpace`](cc_model) digest never become candidates — the brief's
//!   "不同空间拒混在装载层而非打分层保证".
//! - **Deterministic order.** Keyset pagination on the `doc_key` primary key
//!   (`doc_key > after ORDER BY doc_key ASC LIMIT batch`): stable across
//!   calls, no OFFSET re-scan, and the natural tie-break domain of the exact
//!   backend ("分数相等按 doc_key 字典序").
//! - **Language is a defensive join.** `semantic_manifest.file_path` is
//!   redundant from `document_manifest`; the language lives on `files`. Under
//!   enforced foreign keys a manifest row always has its file chain, so the
//!   LEFT JOIN is `Some` in practice; `None` means the file row is already
//!   gone and the caller must treat the language as "matches no language
//!   filter" (conservative exclusion), never as "matches all".
//!
//! Read model only: no lock is taken beyond the caller's read connection, and
//! no clock is read (the scan has no temporal column in its surface).

use cc_model::{CcError, CcResult};
use rusqlite::Connection;

/// One candidate row of the published visible set (semantic_manifest).
///
/// Field set is exactly what the filtered exact backend needs to (a) apply the
/// `HardScope` file/language intersection and (b) address the artifact cache
/// via the `artifact_ref` addressing tuple. `doc_version` is carried for
/// downstream candidate provenance (C10) and fencing diagnostics; the scan
/// itself performs no fencing — publication fencing is P6-011's CAS domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticManifestRow {
    pub doc_key: String,
    pub doc_version: String,
    pub file_path: String,
    /// Digest of the actual embedded input bytes (`InputDigest`, P6-003).
    pub input_digest: String,
    /// `VectorSpace::digest()` hex of the published space.
    pub space_id: String,
    /// Cache content address `cas.v1:<namespace>:<space_id>:<input_digest>:
    /// <spec_digest>:<checksum>` (P6-008).
    pub artifact_ref: String,
    /// `files.language` text; `None` only when the file row is absent (see
    /// module docs: matches no language filter, never all of them).
    pub language: Option<String>,
}

/// The P6-011 publication-fence triple of one document's current
/// `semantic_manifest` row (P7-011 hydrate fence read side). `encoding_key`
/// is the embedded-input digest handle the publish CAS copied from the
/// document manifest — the field a consuming hit's `DocumentRef` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticPublicationRow {
    pub doc_version: String,
    pub encoding_key: String,
    pub space_id: String,
}

/// Read-only handle over one connection for semantic-manifest candidate scans.
pub struct SemanticManifestReads<'a> {
    conn: &'a Connection,
}

impl<'a> SemanticManifestReads<'a> {
    /// Bind to a caller-owned read connection (read pool or transaction).
    pub fn on(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// The one current publication row of a document (P7-011 hydrate fence
    /// read side; additive read-only). `semantic_manifest` carries a `doc_key`
    /// PRIMARY KEY — exactly one row per document, whatever space holds it —
    /// so the caller can check the P6-011 publish fences against it: the row
    /// must exist, live in the *active* space, and carry the `doc_version` /
    /// `encoding_key` the hit claims. `None` = the document has no published
    /// vector row at all (never published, or the publication was revoked).
    ///
    /// The projection is the fence triple, not [`SemanticManifestRow`]: the
    /// candidate row shape is the P6-010 scan surface, while the fence
    /// compares `doc_version` / `encoding_key` (the publish CAS's fence-4
    /// copy of `document_manifest.encoding_key`) plus `space_id`.
    pub fn published_row(&self, doc_key: &str) -> CcResult<Option<SemanticPublicationRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT doc_version, encoding_key, space_id \
                 FROM semantic_manifest WHERE doc_key = ?1",
            )
            .map_err(crate::sql_util::db_err)?;
        stmt.query_row([doc_key], |row| {
            Ok(SemanticPublicationRow {
                doc_version: row.get(0)?,
                encoding_key: row.get(1)?,
                space_id: row.get(2)?,
            })
        })
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(crate::sql_util::db_err)
    }

    /// Deterministic keyset batch of one space's published rows.
    ///
    /// Returns rows with `doc_key > after_doc_key` in ascending `doc_key`
    /// order, at most `batch_rows` of them (empty slice when the space is
    /// exhausted). `batch_rows == 0` is a caller bug and is rejected instead
    /// of silently looping forever.
    pub fn scan_space(
        &self,
        space_id: &str,
        after_doc_key: &str,
        batch_rows: usize,
    ) -> CcResult<Vec<SemanticManifestRow>> {
        if batch_rows == 0 {
            return Err(CcError::InvalidParams(
                "semantic manifest scan requires batch_rows >= 1".into(),
            ));
        }
        let mut stmt = self
            .conn
            .prepare_cached(
            "SELECT m.doc_key, m.doc_version, m.file_path, m.input_digest, \
                    m.space_id, m.artifact_ref, f.language \
             FROM semantic_manifest AS m \
             LEFT JOIN files AS f ON f.file_path = m.file_path \
             WHERE m.space_id = ?1 AND m.doc_key > ?2 \
             ORDER BY m.doc_key ASC \
             LIMIT ?3",
        )
        .map_err(crate::sql_util::db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![space_id, after_doc_key, batch_rows as i64],
                |row| {
                    Ok(SemanticManifestRow {
                        doc_key: row.get(0)?,
                        doc_version: row.get(1)?,
                        file_path: row.get(2)?,
                        input_digest: row.get(3)?,
                        space_id: row.get(4)?,
                        artifact_ref: row.get(5)?,
                        language: row.get(6)?,
                    })
                },
            )
            .map_err(crate::sql_util::db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::sql_util::db_err)?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v22_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(
            crate::index_migrate::migrate_index_db(&conn).unwrap(),
            crate::index_migrate::SchemaStatus::Initialized
        );
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn
    }

    /// Insert the full FK chain (files → chunks → document_manifest →
    /// semantic_manifest) for one candidate row.
    fn seed(
        conn: &Connection,
        doc_key: &str,
        file_path: &str,
        language: &str,
        space_id: &str,
    ) {
        conn.execute_batch(&format!(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
             VALUES('{file_path}','{language}','hash',1.0,1,'2026-01-01');
             INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
             VALUES('c-{doc_key}','{file_path}','{language}',0,1,2,'body');
             INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
               reference_json,record_json) \
             VALUES('{doc_key}','v1','{file_path}','c-{doc_key}',NULL,'{{}}','{{}}');
             INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
               space_id,artifact_ref,published_at,published_incarnation) \
             VALUES('{doc_key}','v1','{file_path}','enc','in-{doc_key}','{space_id}','art','2026-01-01','inc');"
        ))
        .unwrap();
    }

    #[test]
    fn scan_is_keyset_paginated_and_ordered() {
        let conn = v22_conn();
        for key in ["d3", "d1", "d2"] {
            seed(&conn, key, &format!("{key}.rs"), "rust", "sp");
        }
        let reads = SemanticManifestReads::on(&conn);

        let first = reads.scan_space("sp", "", 2).unwrap();
        assert_eq!(
            first.iter().map(|r| r.doc_key.as_str()).collect::<Vec<_>>(),
            ["d1", "d2"]
        );

        let second = reads
            .scan_space("sp", first.last().unwrap().doc_key.as_str(), 2)
            .unwrap();
        assert_eq!(
            second.iter().map(|r| r.doc_key.as_str()).collect::<Vec<_>>(),
            ["d3"]
        );

        assert!(reads.scan_space("sp", "d3", 2).unwrap().is_empty());
    }

    #[test]
    fn scan_is_space_isolated_at_the_load_layer() {
        let conn = v22_conn();
        seed(&conn, "d1", "a.rs", "rust", "sp-a");
        seed(&conn, "d2", "b.rs", "rust", "sp-b");
        let rows = SemanticManifestReads::on(&conn)
            .scan_space("sp-a", "", 10)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].doc_key, "d1");
        assert_eq!(rows[0].space_id, "sp-a");
    }

    #[test]
    fn scan_joins_language_and_carries_provenance_columns() {
        let conn = v22_conn();
        seed(&conn, "d1", "a.rs", "rust", "sp");
        let row = &SemanticManifestReads::on(&conn).scan_space("sp", "", 10).unwrap()[0];
        assert_eq!(row.language.as_deref(), Some("rust"));
        assert_eq!(row.doc_version, "v1");
        assert_eq!(row.input_digest, "in-d1");
        assert_eq!(row.artifact_ref, "art");
        assert_eq!(row.file_path, "a.rs");
    }

    #[test]
    fn deleted_document_cascades_out_of_the_candidate_stream() {
        // ADR P6-010: "删除/异空间向量不可返回" — structural, not scoring-time:
        // the FK CASCADE removes the manifest row, so a deleted document has
        // no candidate row at all.
        let conn = v22_conn();
        seed(&conn, "d1", "a.rs", "rust", "sp");
        conn.execute("DELETE FROM document_manifest WHERE doc_key='d1'", [])
            .unwrap();
        assert!(SemanticManifestReads::on(&conn)
            .scan_space("sp", "", 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn published_row_reads_the_single_current_publication_whatever_space() {
        let conn = v22_conn();
        seed(&conn, "d1", "a.rs", "rust", "sp-a");
        // Re-home the publication to another space (simulating a space
        // switch): doc_key stays the single key, the row follows the space.
        conn.execute(
            "UPDATE semantic_manifest SET space_id='sp-b' WHERE doc_key='d1'",
            [],
        )
        .unwrap();
        let row = SemanticManifestReads::on(&conn)
            .published_row("d1")
            .unwrap()
            .expect("publication row exists in the owning space");
        assert_eq!(row.space_id, "sp-b");
        assert_eq!(row.doc_version, "v1");
        assert_eq!(row.encoding_key, "enc");

        // Revoked publication (row deleted) → None, never an error.
        conn.execute("DELETE FROM semantic_manifest WHERE doc_key='d1'", [])
            .unwrap();
        assert!(SemanticManifestReads::on(&conn)
            .published_row("d1")
            .unwrap()
            .is_none());
        // Never-published document → None.
        assert!(SemanticManifestReads::on(&conn)
            .published_row("d2")
            .unwrap()
            .is_none());
    }

    #[test]
    fn empty_space_scans_empty_and_zero_batch_is_rejected() {
        let conn = v22_conn();
        assert!(SemanticManifestReads::on(&conn)
            .scan_space("sp", "", 10)
            .unwrap()
            .is_empty());
        let err = SemanticManifestReads::on(&conn)
            .scan_space("sp", "", 0)
            .unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
    }
}
