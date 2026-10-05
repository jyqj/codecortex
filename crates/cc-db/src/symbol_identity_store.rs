//! Source-bound identity authority. Every read uses the caller's SQLite snapshot.
use crate::{index_db::FileWriteUnit, sql_util::db_err};
use cc_model::retrieval_cost::SqlWork;
use cc_model::{
    cpp_owner::{CppQualifiedOwnerProof, CppQualifiedOwnerState},
    id::StableId,
    identity::DocumentRef,
    source::{BoundaryKind, ChunkSource},
    symbol_identity::ChunkSymbolIdentity,
    CcError, CcResult, SymbolKind, SymbolRecord,
};
use rusqlite::{Connection, OptionalExtension};

pub(crate) struct Projection<'a> {
    pub chunk_id: &'a str,
    pub path: &'a str,
    pub document: Option<&'a DocumentRef>,
    pub proof: Option<&'a ChunkSource>,
    pub name: Option<&'a str>,
    pub kind: Option<&'a str>,
}

fn query_on<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    work: &mut Option<&mut SqlWork>,
    read: impl FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> CcResult<Option<T>> {
    let mut stmt = conn.prepare_cached(sql).map_err(db_err)?;
    crate::statement_work::reset(&stmt);
    let result = stmt.query_row(params, read).optional().map_err(db_err)?;
    if let Some(work) = work.as_deref_mut() {
        work.merge(crate::statement_work::finish(
            &stmt,
            usize::from(result.is_some()),
        ));
    }
    Ok(result)
}

fn invalid() -> CcError {
    CcError::Database("source-bound symbol identity mismatch".into())
}

/// Persistence can validate the parser's evidence against the already-validated
/// identity and symbol, but cannot independently reparse the absent full source.
fn b1_proof_matches(
    proof: &CppQualifiedOwnerProof,
    identity: &ChunkSymbolIdentity,
    symbol: &SymbolRecord,
) -> bool {
    proof.source == identity.source
        && proof.definition == identity.owner
        && proof.owner_declaration.start < proof.owner_declaration.end
        && proof.owner_declaration.end <= proof.definition.start
        && !proof.owner_path.is_empty()
        && proof.owner_path.len() <= 64
        && proof
            .owner_path
            .iter()
            .all(|part| !part.is_empty() && !part.contains("::"))
        && proof.state == symbol.cpp_qualified_owner
        && proof.symbol_id == identity.symbol_id
        && proof.qname == identity.qname
        && proof.qname == format!("{}::{}", proof.owner_path.join("::"), symbol.name)
        && proof.symbol_uid == identity.symbol_uid
        && b1_identity_shape(
            proof.state,
            identity,
            symbol.signature.as_deref(),
            symbol.container.as_deref(),
        )
}

fn b1_identity_shape(
    state: CppQualifiedOwnerState,
    identity: &ChunkSymbolIdentity,
    signature: Option<&str>,
    container: Option<&str>,
) -> bool {
    let expected_kind = match state {
        CppQualifiedOwnerState::ProvenNamespace => SymbolKind::Function,
        CppQualifiedOwnerState::ProvenType => SymbolKind::Method,
        _ => return false,
    };
    let Some((owner, leaf)) = identity.qname.rsplit_once("::") else {
        return false;
    };
    let parts: Vec<_> = owner.split("::").collect();
    identity.kind == expected_kind
        && container.is_none()
        && leaf == identity.name
        && parts.len() <= 64
        && parts.iter().all(|part| !part.is_empty())
        && identity.symbol_uid
            == StableId::symbol_uid(
                &identity.file_path,
                &identity.qname,
                expected_kind.as_str(),
                signature,
            )
}

fn symbol_matches_on(
    conn: &Connection,
    identity: &ChunkSymbolIdentity,
    expected_state: Option<CppQualifiedOwnerState>,
    work: &mut Option<&mut SqlWork>,
) -> CcResult<bool> {
    let row = query_on(conn,"SELECT cpp_qualified_owner,signature,container FROM symbols WHERE symbol_id=?1 AND file_path=?2 AND symbol_uid=?3 AND name=?4 AND kind=?5 AND qname=?6 AND start_line=?7 AND start_col=?8 AND end_line=?9 AND end_col=?10",
        rusqlite::params![identity.symbol_id,identity.file_path,identity.symbol_uid,identity.name,identity.kind.as_str(),identity.qname,identity.start_line,identity.start_col,identity.end_line,identity.end_col], work, |r| Ok((crate::sql_util::cpp_qualified_owner(r, 0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))?;
    let Some((state, signature, container)) = row else {
        return Ok(false);
    };
    if expected_state.is_some_and(|expected| expected != state)
        || (state.is_b1()
            && !b1_identity_shape(state, identity, signature.as_deref(), container.as_deref()))
    {
        return Err(invalid());
    }
    Ok(true)
}

pub(crate) fn insert_on(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
    if file.outcome.symbol_identities.is_empty() {
        return Ok(());
    }
    let structure = file.outcome.source_structure.as_ref().ok_or_else(invalid)?;
    let chunks: std::collections::BTreeMap<_, _> = file
        .outcome
        .chunks
        .iter()
        .map(|c| (c.chunk_id.as_str(), c))
        .collect();
    let documents: std::collections::BTreeMap<_, _> = file
        .outcome
        .documents
        .as_ref()
        .ok_or_else(invalid)?
        .records
        .iter()
        .map(|d| (d.chunk_id.as_str(), d))
        .collect();
    let mut symbols = std::collections::BTreeMap::<_, Vec<_>>::new();
    for symbol in &file.outcome.symbols {
        symbols
            .entry((
                symbol.start_line,
                symbol.start_col,
                symbol.end_line,
                symbol.end_col,
            ))
            .or_default()
            .push(symbol);
    }
    let mut owners = std::collections::BTreeMap::<_, Vec<_>>::new();
    for boundary in &structure.boundaries {
        if boundary.kind == BoundaryKind::Symbol {
            owners
                .entry((boundary.span.start, boundary.span.end))
                .or_default()
                .push(boundary);
        }
    }
    let mut b1_proofs = std::collections::BTreeMap::<_, Vec<_>>::new();
    for proof in &file.outcome.cpp_qualified_owner_proofs {
        b1_proofs
            .entry(proof.symbol_id.as_str())
            .or_default()
            .push(proof);
    }
    let mut seen = std::collections::BTreeSet::new();
    for identity in &file.outcome.symbol_identities {
        let chunk = chunks.get(identity.chunk_id.as_str()).ok_or_else(invalid)?;
        let proof = chunk.source.as_ref().ok_or_else(invalid)?;
        let document = documents
            .get(identity.chunk_id.as_str())
            .ok_or_else(invalid)?;
        let candidates = symbols
            .get(&(
                identity.start_line,
                identity.start_col,
                identity.end_line,
                identity.end_col,
            ))
            .ok_or_else(invalid)?;
        let boundaries = owners
            .get(&(identity.owner.start, identity.owner.end))
            .ok_or_else(invalid)?;
        if !seen.insert(&identity.chunk_id)
            || !identity.valid_shape()
            || !structure.complete
            || structure.capability != "existing_tree_sitter_ast"
            || structure.source != identity.source
            || boundaries.len() != 1
            || boundaries[0].name.as_deref() != Some(&identity.name)
            || boundaries[0].symbol_kind != Some(identity.kind)
            || boundaries[0].signature != proof.signature
            || !boundaries[0].owns_chunk(proof, &chunk.text)
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
        let symbol = candidates[0];
        let owner_proofs = b1_proofs.get(symbol.symbol_id.as_str());
        if symbol.cpp_qualified_owner.is_b1() {
            if file.language != cc_model::Language::Cpp
                || !owner_proofs.is_some_and(|proofs| {
                    proofs.len() == 1 && b1_proof_matches(proofs[0], identity, symbol)
                })
            {
                return Err(invalid());
            }
        } else if owner_proofs.is_some() {
            // A mismatched enum cannot strip the proof requirement from a B1 row.
            return Err(invalid());
        }
        // The actual SQL survivor is authoritative; parser candidates replaced
        // by existing write semantics cannot acquire an association.
        if !symbol_matches_on(conn, identity, Some(symbol.cpp_qualified_owner), &mut None)? {
            continue;
        }
        conn.execute("INSERT INTO chunk_symbol_identity(chunk_id,file_path,doc_key,doc_version,symbol_id,format_version,record_json) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![identity.chunk_id,identity.file_path,identity.document.doc_key,identity.document.doc_version,identity.symbol_id,identity.format_version,serde_json::to_string(identity)?]).map_err(db_err)?;
        let qname = load_on(
            conn,
            Projection {
                chunk_id: &chunk.chunk_id,
                path: &chunk.file_path,
                document: Some(&identity.document),
                proof: Some(proof),
                name: chunk.symbol_name.as_deref(),
                kind: chunk.symbol_kind.map(|k| k.as_str()),
            },
            None,
        )?;
        if qname.as_deref() != Some(&identity.qname) {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn load_on(
    conn: &Connection,
    projection: Projection<'_>,
    mut work: Option<&mut SqlWork>,
) -> CcResult<Option<String>> {
    let Projection {
        chunk_id,
        path,
        document,
        proof,
        name,
        kind,
    } = projection;
    let row: Option<(String,String,String,String,String,u32)> = query_on(conn,
        "SELECT record_json,file_path,doc_key,doc_version,symbol_id,format_version FROM chunk_symbol_identity WHERE chunk_id=?1", [chunk_id], &mut work,
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
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
        || !symbol_matches_on(conn, &identity, None, &mut work)?
    {
        return Err(invalid());
    }
    let current: bool = query_on(conn,"SELECT EXISTS(SELECT 1 FROM files f JOIN chunks c ON c.file_path=f.file_path JOIN document_manifest d ON d.chunk_id=c.chunk_id WHERE c.chunk_id=?1 AND f.file_path=?2 AND f.content_hash=?3 AND d.doc_key=?4 AND d.doc_version=?5 AND c.symbol_name=?6 AND c.symbol_kind=?7)",
        rusqlite::params![chunk_id,path,identity.source.content_digest,reference.doc_key,reference.doc_version,identity.name,identity.kind.as_str()], &mut work, |r| r.get(0))?.ok_or_else(invalid)?;
    if !current {
        return Err(invalid());
    }
    let raw_source: Option<String> = query_on(
        conn,
        "SELECT source_json FROM chunks WHERE chunk_id=?1",
        [chunk_id],
        &mut work,
        |r| r.get(0),
    )?
    .flatten();
    let stored_source: ChunkSource =
        serde_json::from_str(raw_source.as_deref().ok_or_else(invalid)?)?;
    if stored_source != *source {
        return Err(invalid());
    }
    Ok(Some(identity.qname))
}
