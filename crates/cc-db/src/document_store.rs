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
/// Owned worker input, fenced against the currently indexed document version.
/// The read connection is released before the caller can contact a provider.
pub fn semantic_worker_input(
    db: &IndexDb,
    doc_key: &str,
    doc_version: &str,
    input_digest: &str,
) -> CcResult<Option<String>> {
    let raw: Option<(String, String)> = {
        let conn = db.read_conn()?;
        conn.query_row(
            "SELECT record_json,encoding_key FROM document_manifest WHERE doc_key=?1 AND doc_version=?2 AND encoding_key IS NOT NULL",
            rusqlite::params![doc_key, doc_version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional().map_err(db_err)?
    };
    let Some((raw, encoding)) = raw else {
        return Ok(None);
    };
    Ok(worker_record_input(&raw, doc_key, doc_version, &encoding)?
        .filter(|input| input.input_hash == input_digest)
        .map(|input| input.text))
}

fn worker_record_input(
    raw: &str,
    doc_key: &str,
    doc_version: &str,
    encoding: &str,
) -> CcResult<Option<cc_model::retrieval::EmbeddingInput>> {
    let record: DocumentRecord = serde_json::from_str(raw)?;
    if record.reference.doc_key != doc_key
        || record.reference.doc_version != doc_version
        || record.reference.encoding_key.as_deref() != Some(encoding)
    {
        return Err(CcError::Database(
            "worker document identity mismatch".into(),
        ));
    }
    let Some(input) = record.input.as_ref() else {
        return Ok(None);
    };
    let original = input
        .text
        .get(input.source_range.start..input.source_range.end)
        .ok_or_else(|| CcError::Database("worker document source range invalid".into()))?;
    record.validate(original)?;
    Ok(record.input)
}

/// A bounded keyset page. Cursor follows the physical rows, never a filtered
/// subset; malformed eligible records fail instead of hiding later documents.
pub struct SemanticWorkerPage {
    pub desired: Vec<crate::semantic_outbox::OutboxUpsert>,
    pub next_cursor: Option<String>,
}
pub fn semantic_worker_desired_page(
    db: &IndexDb,
    after: &str,
    limit: usize,
) -> CcResult<SemanticWorkerPage> {
    if !(1..=256).contains(&limit) {
        return Err(CcError::InvalidParams(
            "worker page limit must be 1..=256".into(),
        ));
    }
    let rows: Vec<(String, String, String, String)> = {
        let conn = db.read_conn()?;
        let mut stmt = conn.prepare_cached("SELECT doc_key,doc_version,encoding_key,record_json FROM document_manifest WHERE encoding_key IS NOT NULL AND doc_key>?1 ORDER BY doc_key LIMIT ?2").map_err(db_err)?;
        let result = stmt
            .query_map(rusqlite::params![after, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(db_err)?
            .collect::<Result<_, _>>()
            .map_err(db_err)?;
        result
    };
    let next_cursor = (rows.len() == limit).then(|| rows.last().unwrap().0.clone());
    let mut desired = Vec::with_capacity(rows.len());
    for (doc_key, doc_version, encoding, raw) in rows {
        let input = worker_record_input(&raw, &doc_key, &doc_version, &encoding)?
            .ok_or_else(|| CcError::Database("eligible worker document has no input".into()))?;
        desired.push(crate::semantic_outbox::OutboxUpsert {
            doc_key,
            doc_version,
            input_digest: input.input_hash,
        });
    }
    Ok(SemanticWorkerPage {
        desired,
        next_cursor,
    })
}

pub fn semantic_worker_publication_current(
    db: &IndexDb,
    task: &crate::semantic_outbox::OutboxUpsert,
    space: &str,
) -> CcResult<bool> {
    let conn = db.read_conn()?;
    conn.query_row("SELECT EXISTS(SELECT 1 FROM semantic_manifest WHERE doc_key=?1 AND doc_version=?2 AND input_digest=?3 AND space_id=?4)",
        rusqlite::params![task.doc_key, task.doc_version, task.input_digest, space], |r| r.get(0)).map_err(db_err)
}

/// Atomically enqueue only missing current versions. Reopening must preserve
/// live leases/backoff and terminal failures, never reset their attempt budget.
/// The version/input check shares the transaction with the frozen queue planner
/// so an old scan cannot supersede a newer index build's desired task.
pub fn enqueue_semantic_worker_missing(
    db: &IndexDb,
    desired: &[crate::semantic_outbox::OutboxUpsert],
) -> CcResult<crate::semantic_outbox::OutboxWriteStats> {
    if desired.len() > 256 {
        return Err(CcError::InvalidParams(
            "worker enqueue page exceeds bound".into(),
        ));
    }
    let mut conn = db.write_conn.lock().map_err(db_err)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db_err)?;
    let Some(space) = crate::semantic_outbox::active_space_on(&tx)? else {
        return Ok(Default::default());
    };
    let mut missing = Vec::new();
    for task in desired {
        let eligible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM document_manifest d WHERE d.doc_key=?1 AND d.doc_version=?2 AND d.encoding_key IS NOT NULL AND json_extract(d.record_json,'$.input.input_hash')=?3) AND NOT EXISTS(SELECT 1 FROM semantic_outbox t WHERE t.doc_key=?1 AND t.doc_version=?2 AND t.input_digest=?3 AND t.space_id=?4 AND t.state IN ('pending','claimed','failed'))",
            rusqlite::params![task.doc_key, task.doc_version, task.input_digest, space], |r| r.get(0)).map_err(db_err)?;
        if eligible {
            missing.push(task.clone());
        }
    }
    let stats = crate::semantic_outbox::supersede_and_enqueue_on(
        &tx,
        &crate::semantic_outbox::OutboxPlan {
            upserts: &missing,
            removals: &[],
            now_unix: crate::semantic_outbox::now_unix(),
        },
    )?;
    if stats.changed_semantic_state() {
        IndexDb::bump_semantic_epoch_on(&tx)?;
    }
    tx.commit().map_err(db_err)?;
    Ok(stats)
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
    let pooled = db.read_conn()?;
    let snapshot = pooled.unchecked_transaction().map_err(db_err)?;
    let conn = &*snapshot;
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
            let qname = crate::symbol_identity_store::load_on(
                conn,
                &id,
                &hit.file_path,
                Some(&actual),
                Some(&source),
                hit.symbol_name.as_deref(),
                hit.symbol_kind.map(|k| k.as_str()),
            )?;
            if hit.metadata.get("qname").and_then(|v| v.as_str()) != qname.as_deref()
                || hit.metadata.get("qname").is_some_and(|v| !v.is_string())
            {
                return Err(CcError::Database("final qname association mismatch".into()));
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
