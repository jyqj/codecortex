//! Prove declaration ownership while the exact parser snapshot is available.
use cc_model::{
    source::{BoundaryKind, SourceSnapshot},
    symbol_identity::{ChunkSymbolIdentity, SYMBOL_IDENTITY_FORMAT},
    CcResult, ParseOutcome,
};
use std::collections::BTreeMap;

pub fn prepare(
    source: &SourceSnapshot<'_>,
    outcome: &ParseOutcome,
) -> CcResult<Vec<ChunkSymbolIdentity>> {
    let Some(structure) = outcome
        .source_structure
        .as_ref()
        .filter(|s| s.complete && s.capability == "existing_tree_sitter_ast" && s.validate(source))
    else {
        return Ok(vec![]);
    };
    let Some(documents) = &outcome.documents else {
        return Ok(vec![]);
    };
    let documents: BTreeMap<_, _> = documents
        .records
        .iter()
        .map(|d| (d.chunk_id.as_str(), d))
        .collect();
    // Preserve all candidates at identical endpoints: do not fold ambiguities.
    let mut symbols = BTreeMap::<_, Vec<_>>::new();
    for s in &outcome.symbols {
        symbols
            .entry((s.start_line, s.start_col, s.end_line, s.end_col))
            .or_default()
            .push(s);
    }
    let mut owners = BTreeMap::<_, Vec<_>>::new();
    for boundary in &structure.boundaries {
        if boundary.kind == BoundaryKind::Symbol {
            owners
                .entry((boundary.span.start, boundary.span.end))
                .or_default()
                .push(boundary);
        }
    }
    let mut cpp_proofs = BTreeMap::<_, Vec<_>>::new();
    for proof in &outcome.cpp_qualified_owner_proofs {
        cpp_proofs
            .entry((
                proof.definition.start,
                proof.definition.end,
                proof.symbol_id.as_str(),
            ))
            .or_default()
            .push(proof);
    }
    let mut identities = Vec::new();
    for chunk in &outcome.chunks {
        let Some(proof) = &chunk.source else {
            continue;
        };
        if proof.source != *source.identity()
            || !proof.validate(&chunk.text)
            || source.slice(proof.span)? != chunk.text
        {
            continue;
        }
        let Some(owner) = proof.owner else {
            continue;
        };
        // UTF-8 validation includes both owner endpoints, not just the chunk slice.
        if source.slice(owner).is_err() {
            continue;
        }
        let Some(boundaries) = owners.get(&(owner.start, owner.end)) else {
            continue;
        };
        if boundaries.len() != 1 {
            continue;
        }
        let boundary = boundaries[0];
        if boundary.signature != proof.signature {
            continue;
        }
        // Only this declaration's body or its explicitly attached leading
        // documentation can inherit its identity. Same-name neighbors cannot.
        if !boundary.owns_chunk(proof, &chunk.text) {
            continue;
        }
        let (sl, sc) = source.point(owner.start)?;
        let (el, ec) = source.point(owner.end)?;
        let key = (sl as u32, sc as u32, el as u32, ec as u32);
        let Some(candidates) = symbols.get(&key) else {
            continue;
        };
        if candidates.len() != 1 {
            continue;
        }
        let symbol = candidates[0];
        if symbol.file_path != chunk.file_path
            || chunk.symbol_name.as_deref() != Some(&symbol.name)
            || chunk.symbol_kind != Some(symbol.kind)
            || boundary.name.as_deref() != Some(&symbol.name)
            || boundary.symbol_kind != Some(symbol.kind)
        {
            continue;
        }
        if symbol.cpp_qualified_owner.is_b1()
            || cpp_proofs.contains_key(&(owner.start, owner.end, symbol.symbol_id.as_str()))
        {
            // A durable eligibility flag is not declaration provenance. Only
            // the current parser snapshot's unique exact-span proof can admit
            // a B1 identity; dirty reload never manufactures this evidence.
            let Some(proofs) = cpp_proofs.get(&(owner.start, owner.end, symbol.symbol_id.as_str()))
            else {
                continue;
            };
            if proofs.len() != 1 || !proofs[0].matches(source, owner, symbol) {
                continue;
            }
        }
        let (Some(qname), Some(uid), Some(document)) = (
            &symbol.qname,
            &symbol.symbol_uid,
            documents.get(chunk.chunk_id.as_str()),
        ) else {
            continue;
        };
        if document.source != *proof || document.file_path != chunk.file_path {
            continue;
        }
        let identity = ChunkSymbolIdentity {
            format_version: SYMBOL_IDENTITY_FORMAT,
            chunk_id: chunk.chunk_id.clone(),
            file_path: chunk.file_path.clone(),
            document: document.reference.clone(),
            source: source.identity().clone(),
            owner,
            start_line: key.0,
            start_col: key.1,
            end_line: key.2,
            end_col: key.3,
            symbol_id: symbol.symbol_id.clone(),
            symbol_uid: uid.clone(),
            name: symbol.name.clone(),
            kind: symbol.kind,
            qname: qname.clone(),
        };
        if identity.valid_shape() {
            identities.push(identity);
        }
    }
    Ok(identities)
}
