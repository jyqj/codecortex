//! Source-bound identity authority. Every read uses the caller's SQLite snapshot.
use crate::{index_db::FileWriteUnit, sql_util::db_err};
use cc_model::{
    identity::DocumentRef,
    source::{BoundaryKind, ChunkSource},
    symbol_identity::ChunkSymbolIdentity,
    CcError, CcResult,
};
use rusqlite::{Connection, OptionalExtension};

fn invalid() -> CcError {
    CcError::Database("source-bound symbol identity mismatch".into())
}

fn symbol_matches_on(conn: &Connection, identity: &ChunkSymbolIdentity) -> CcResult<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM symbols WHERE symbol_id=?1 AND file_path=?2 AND symbol_uid=?3 AND name=?4 AND kind=?5 AND qname=?6 AND start_line=?7 AND start_col=?8 AND end_line=?9 AND end_col=?10)",
        rusqlite::params![identity.symbol_id,identity.file_path,identity.symbol_uid,identity.name,identity.kind.as_str(),identity.qname,identity.start_line,identity.start_col,identity.end_line,identity.end_col], |r| r.get(0)).map_err(db_err)
}

pub(crate) fn insert_on(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
    let mut seen = std::collections::BTreeSet::new();
    for identity in &file.outcome.symbol_identities {
        let structure = file.outcome.source_structure.as_ref().ok_or_else(invalid)?;
        let chunk = file
            .outcome
            .chunks
            .iter()
            .find(|c| c.chunk_id == identity.chunk_id)
            .ok_or_else(invalid)?;
        let proof = chunk.source.as_ref().ok_or_else(invalid)?;
        let document = file
            .outcome
            .documents
            .as_ref()
            .and_then(|d| d.records.iter().find(|d| d.chunk_id == identity.chunk_id))
            .ok_or_else(invalid)?;
        let candidates: Vec<_> = file
            .outcome
            .symbols
            .iter()
            .filter(|s| {
                (s.start_line, s.start_col, s.end_line, s.end_col)
                    == (
                        identity.start_line,
                        identity.start_col,
                        identity.end_line,
                        identity.end_col,
                    )
            })
            .collect();
        if !seen.insert(&identity.chunk_id)
            || !identity.valid_shape()
            || !structure.complete
            || structure.capability != "existing_tree_sitter_ast"
            || structure.source != identity.source
            || structure
                .boundaries
                .iter()
                .filter(|b| {
                    b.kind == BoundaryKind::Symbol
                        && b.span == identity.owner
                        && b.name.as_deref() == Some(&identity.name)
                        && b.symbol_kind == Some(identity.kind)
                        && b.signature == proof.signature
                        && b.owns_chunk(proof, &chunk.text)
                })
                .count()
                != 1
            || candidates.len() != 1
            || !identity.matches_symbol(candidates[0])
            || identity.file_path != file.rel_path
            || chunk.file_path != file.rel_path
            || file.content_hash != identity.source.content_digest
            || proof.source != identity.source
            || proof.owner != Some(identity.owner)
            || !proof.validate(&chunk.text)
            || document.source != *proof
            || document.reference != identity.document
            || chunk.symbol_name.as_deref() != Some(&identity.name)
            || chunk.symbol_kind != Some(identity.kind)
        {
            return Err(invalid());
        }
        // The actual SQL survivor is authoritative; parser candidates replaced
        // by existing write semantics cannot acquire an association.
        if !symbol_matches_on(conn, identity)? {
            continue;
        }
        conn.execute("INSERT INTO chunk_symbol_identity(chunk_id,file_path,doc_key,doc_version,symbol_id,format_version,record_json) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![identity.chunk_id,identity.file_path,identity.document.doc_key,identity.document.doc_version,identity.symbol_id,identity.format_version,serde_json::to_string(identity)?]).map_err(db_err)?;
        let qname = load_on(
            conn,
            &chunk.chunk_id,
            &chunk.file_path,
            Some(&identity.document),
            Some(proof),
            chunk.symbol_name.as_deref(),
            chunk.symbol_kind.map(|k| k.as_str()),
        )?;
        if qname.as_deref() != Some(&identity.qname) {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn load_on(
    conn: &Connection,
    chunk_id: &str,
    path: &str,
    document: Option<&DocumentRef>,
    proof: Option<&ChunkSource>,
    name: Option<&str>,
    kind: Option<&str>,
) -> CcResult<Option<String>> {
    let row: Option<(String,String,String,String,String,u32)> = conn.query_row(
        "SELECT record_json,file_path,doc_key,doc_version,symbol_id,format_version FROM chunk_symbol_identity WHERE chunk_id=?1", [chunk_id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(db_err)?;
    let Some((raw, stored_path, key, version, symbol_id, format)) = row else {
        return Ok(None);
    };
    let identity: ChunkSymbolIdentity = serde_json::from_str(&raw)?;
    let source = proof.ok_or_else(invalid)?;
    let reference = document.ok_or_else(invalid)?;
    if !identity.valid_shape()
        || identity.chunk_id != chunk_id
        || identity.file_path != path
        || stored_path != path
        || key != reference.doc_key
        || version != reference.doc_version
        || symbol_id != identity.symbol_id
        || format != identity.format_version
        || identity.document != *reference
        || identity.source != source.source
        || source.owner != Some(identity.owner)
        || name != Some(&identity.name)
        || kind != Some(identity.kind.as_str())
        || !symbol_matches_on(conn, &identity)?
    {
        return Err(invalid());
    }
    let current: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM files f JOIN chunks c ON c.file_path=f.file_path JOIN document_manifest d ON d.chunk_id=c.chunk_id WHERE c.chunk_id=?1 AND f.file_path=?2 AND f.content_hash=?3 AND d.doc_key=?4 AND d.doc_version=?5 AND c.symbol_name=?6 AND c.symbol_kind=?7)",
        rusqlite::params![chunk_id,path,identity.source.content_digest,reference.doc_key,reference.doc_version,identity.name,identity.kind.as_str()], |r| r.get(0)).map_err(db_err)?;
    if !current {
        return Err(invalid());
    }
    let raw_source: Option<String> = conn
        .query_row(
            "SELECT source_json FROM chunks WHERE chunk_id=?1",
            [chunk_id],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    let stored_source: ChunkSource =
        serde_json::from_str(raw_source.as_deref().ok_or_else(invalid)?)?;
    if stored_source != *source {
        return Err(invalid());
    }
    Ok(Some(identity.qname))
}
