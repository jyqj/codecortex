//! Current document manifests. Writes are internal parts of the existing file transaction.
use crate::{
    index_db::{FileWriteUnit, IndexDb},
    sql_util::db_err,
};
use cc_model::{
    identity::{DocumentRecord, DocumentRef},
    CcError, CcResult,
};
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;
const MAX_FILE_DOCUMENTS: usize = 250_000;
/// Stored parser language, not guessed from a graph node's display path.
pub fn indexed_source_language(db: &IndexDb, path: &str) -> CcResult<Option<String>> {
    if !cc_model::repo_path::is_canonical_file(path) {
        return Err(CcError::InvalidParams("invalid source path".into()));
    }
    let conn = db.read_conn()?;
    conn.query_row(
        "SELECT language FROM files WHERE file_path=?",
        [path],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_err)
}
pub fn indexed_source_digest(db: &IndexDb, path: &str) -> CcResult<Option<String>> {
    let conn = db.read_conn()?;
    conn.query_row(
        "SELECT content_hash FROM files WHERE file_path=?",
        [path],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_err)
}
pub fn references(db: &IndexDb, path: &str) -> CcResult<Vec<DocumentRef>> {
    if !cc_model::repo_path::is_canonical_file(path) {
        return Err(CcError::InvalidParams("invalid document path".into()));
    }
    let conn = db.read_conn()?;
    let mut stmt=conn.prepare_cached("SELECT reference_json,doc_key,doc_version,encoding_key FROM document_manifest WHERE file_path=? ORDER BY doc_key LIMIT 250001").map_err(db_err)?;
    let rows = stmt
        .query_map([path], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(db_err)?;
    let mut result = Vec::new();
    for row in rows {
        let (raw, key, version, encoding) = row.map_err(db_err)?;
        let reference: DocumentRef = serde_json::from_str(&raw)?;
        if reference.doc_key != key
            || reference.doc_version != version
            || reference.encoding_key != encoding
        {
            return Err(CcError::Database("document reference mismatch".into()));
        }
        result.push(reference);
    }
    if result.len() > MAX_FILE_DOCUMENTS {
        return Err(CcError::Database("document read budget exceeded".into()));
    }
    Ok(result)
}
/// Index-local comparison only. This is not an asynchronous/vector publish API.
pub fn is_current(db: &IndexDb, reference: &DocumentRef) -> CcResult<bool> {
    let conn = db.read_conn()?;
    let current: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT doc_version,encoding_key FROM document_manifest WHERE doc_key=?",
            [&reference.doc_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_err)?;
    Ok(current.is_some_and(|(v, e)| v == reference.doc_version && e == reference.encoding_key))
}
pub(crate) fn insert_on(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
    let Some(batch) = &file.outcome.documents else {
        return Ok(());
    };
    let chunks: BTreeMap<_, _> = file
        .outcome
        .chunks
        .iter()
        .filter(|c| c.source.is_some())
        .map(|c| (c.chunk_id.as_str(), c))
        .collect();
    if batch.records.len() != chunks.len() || batch.records.len() > MAX_FILE_DOCUMENTS {
        return Err(CcError::InvalidParams(
            "document/chunk coverage mismatch".into(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut insert=conn.prepare_cached("INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,?2,?3,?4,?5,?6,?7)").map_err(db_err)?;
    for record in &batch.records {
        let chunk = chunks
            .get(record.chunk_id.as_str())
            .ok_or_else(|| CcError::InvalidParams("document has no matching chunk".into()))?;
        record.validate(&chunk.text)?;
        if !seen.insert(&record.chunk_id)
            || record.file_path != file.rel_path
            || record.file_path != chunk.file_path
            || chunk.source.as_ref() != Some(&record.source)
            || file.outcome.chunk_policy.as_deref() != Some(record.policy.as_str())
            || file.content_hash != record.source.source.content_digest
            || file.outcome.document_spec.as_ref()
                != Some(&cc_model::identity::hash(&record.encoding_spec)?)
        {
            return Err(CcError::InvalidParams(
                "document file provenance mismatch".into(),
            ));
        }
        insert
            .execute(rusqlite::params![
                record.reference.doc_key,
                record.reference.doc_version,
                record.file_path,
                record.chunk_id,
                record.reference.encoding_key,
                serde_json::to_string(&record.reference)?,
                serde_json::to_string(record)?
            ])
            .map_err(db_err)?;
    }
    Ok(())
}
/// Revalidate full persisted manifests for an already-ranked window, including
/// warm-cache hits. This reads no compressed source blobs and delegates to the
/// same record validator as ordinary hydration.
pub fn verify_source_records(db: &IndexDb, hits: &[cc_model::SearchHit]) -> CcResult<()> {
    if hits.len() > 4096 {
        return Err(CcError::InvalidParams(
            "final manifest window exceeds budget".into(),
        ));
    }
    let by_id: BTreeMap<_, _> = hits
        .iter()
        .map(|hit| (hit.chunk_id.as_str(), hit))
        .collect();
    if by_id.len() != hits.len() {
        return Err(CcError::Database("duplicate final document locator".into()));
    }
    if hits.is_empty() {
        return Ok(());
    }
    let ids: Vec<_> = by_id.keys().copied().collect();
    let conn = db.read_conn()?;
    let mut found = 0;
    for batch in ids.chunks(crate::sql_util::IN_BATCH_SIZE) {
        let sql = format!(
            "SELECT chunk_id,record_json FROM document_manifest WHERE chunk_id IN ({})",
            crate::sql_util::sql_in_placeholders(batch.len())
        );
        let mut statement = conn.prepare_cached(&sql).map_err(db_err)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(batch), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        for row in rows {
            let (id, raw) = row.map_err(db_err)?;
            let hit = by_id
                .get(id.as_str())
                .ok_or_else(|| CcError::Database("unrequested document record".into()))?;
            let source: cc_model::source::ChunkSource =
                serde_json::from_value(hit.metadata["source_evidence"].clone())?;
            let expected: DocumentRef = serde_json::from_value(hit.metadata["document"].clone())?;
            let actual = decode(&raw, &hit.text, &id, &hit.file_path, Some(&source))?;
            if actual != expected {
                return Err(CcError::Database(
                    "final manifest reference mismatch".into(),
                ));
            }
            found += 1;
        }
    }
    if found != hits.len() {
        return Err(CcError::Database("final manifest record missing".into()));
    }
    Ok(())
}

/// Validate the stored record when hydrating its exact chunk; do not silently
/// downgrade a malformed record into an unversioned legacy result.
pub(crate) fn decode(
    raw: &str,
    text: &str,
    chunk_id: &str,
    path: &str,
    source: Option<&cc_model::source::ChunkSource>,
) -> CcResult<DocumentRef> {
    let record: DocumentRecord = serde_json::from_str(raw)?;
    record.validate(text)?;
    if record.chunk_id != chunk_id || record.file_path != path || source != Some(&record.source) {
        return Err(CcError::Database(
            "document hydrated against different chunk".into(),
        ));
    }
    Ok(record.reference)
}
