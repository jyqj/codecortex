//! Final source validation of ranked hits. Retrieval scores are immutable;
//! untrusted locators never become source facts without current DB + disk proof.
use crate::evidence::SourceVerifier;
use cc_db::index_db::IndexDb;
use cc_model::{
    generation::ReadGeneration,
    identity::DocumentRef,
    query::QueryControl,
    retrieval::HardScope,
    search::SearchHit,
    source::{ChunkSource, SourceIdentity, SourceSnapshot},
    CcError, CcResult, ContextNode, Language,
};
use std::{collections::BTreeMap, path::Path};

pub const HYDRATOR_SPEC: &str = "current-document-scope-source-generation-v1";

/// Additional handler metadata must still refer to the accepted read view.
pub fn validate_envelope_generation(db: &IndexDb, value: &serde_json::Value) -> CcResult<()> {
    if value
        .pointer("/machine_pack/kind")
        .and_then(serde_json::Value::as_str)
        != Some("code_index_context")
    {
        return Ok(());
    }
    let expected: ReadGeneration = serde_json::from_value(
        value
            .pointer("/evidence_summary/source_freshness/generation")
            .cloned()
            .ok_or_else(|| CcError::Database("context missing accepted generation".into()))?,
    )?;
    if db.reads().read_generation()? != expected {
        return Err(CcError::RetrievalChanged { attempts: 1 });
    }
    Ok(())
}

struct VerifiedLayout {
    identity: SourceIdentity,
    line_starts: Vec<usize>,
}
pub struct EvidenceHydrator<'a> {
    db: &'a IndexDb,
    verifier: SourceVerifier<'a>,
    scope: HardScope,
    generation: ReadGeneration,
    control: QueryControl,
    verified_identities: BTreeMap<String, VerifiedLayout>,
    verified_hits: usize,
}
impl<'a> EvidenceHydrator<'a> {
    pub fn new(
        db: &'a IndexDb,
        root: &'a Path,
        scope: HardScope,
        generation: ReadGeneration,
        control: QueryControl,
    ) -> CcResult<Self> {
        let result = Self {
            db,
            verifier: SourceVerifier::new(db, root),
            scope,
            generation,
            control,
            verified_identities: BTreeMap::new(),
            verified_hits: 0,
        };
        result.finish()?;
        Ok(result)
    }
    /// Bounded ranked window, checked before selection or budget compaction.
    /// Stable invalid identity is an error, not an empty/no-match result.
    pub fn hydrate(&mut self, hits: &[SearchHit]) -> CcResult<Vec<SearchHit>> {
        self.finish()?;
        if hits.len() > 4096 {
            return Err(CcError::Search(
                "final evidence candidate budget exceeded".into(),
            ));
        }
        cc_db::document_store::verify_source_records(self.db, hits)?;
        let ids: Vec<_> = hits.iter().map(|h| h.chunk_id.as_str()).collect();
        let rows = self.db.retrieval().chunk_candidate_rows_by_ids(&ids)?;
        let rows: BTreeMap<_, _> = rows.into_iter().map(|r| (r.chunk_id.clone(), r)).collect();
        let mut result = Vec::new();
        for hit in hits {
            self.control.check()?;
            let row = rows.get(&hit.chunk_id).ok_or_else(|| {
                CcError::Database("final evidence has no current document".into())
            })?;
            let reference: DocumentRef =
                serde_json::from_value(hit.metadata.get("document").cloned().ok_or_else(
                    || CcError::Database("final evidence missing document identity".into()),
                )?)?;
            let proof: ChunkSource =
                serde_json::from_value(hit.metadata.get("source_evidence").cloned().ok_or_else(
                    || CcError::Database("final evidence missing source proof".into()),
                )?)?;
            if row.file_path != hit.file_path
                || Language::from_name(&row.language) != hit.language
                || !self.scope.passes(&row.file_path, hit.language)
                || row.document != reference
                || row.source_evidence != proof
                || !proof.validate(&hit.text)
            {
                return Err(CcError::Database(
                    "final evidence identity/scope/source mismatch".into(),
                ));
            }
            if !self.verifier.path_current(&hit.file_path)? {
                continue;
            }
            // Recompute the snapshot identity once per verified file. Display
            // coordinates are checked against the same original byte sequence.
            let text = self
                .verifier
                .verified_text(&hit.file_path)
                .ok_or_else(|| CcError::Search("verified source body unavailable".into()))?;
            if !self.verified_identities.contains_key(&hit.file_path) {
                let mut line_starts = vec![0];
                line_starts.extend(
                    text.bytes()
                        .enumerate()
                        .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
                );
                self.verified_identities.insert(
                    hit.file_path.clone(),
                    VerifiedLayout {
                        identity: SourceSnapshot::new(text.as_bytes()).identity().clone(),
                        line_starts,
                    },
                );
            }
            let layout = &self.verified_identities[&hit.file_path];
            if layout.identity != proof.source
                || text.get(proof.span.start..proof.span.end) != Some(hit.text.as_str())
            {
                return Err(CcError::Database(
                    "final evidence snapshot identity mismatch".into(),
                ));
            }
            // Reuse the per-file line index rather than rescanning a whole file
            // for every hit. CRLF and UTF-8 coordinates remain byte based.
            let first = layout
                .line_starts
                .partition_point(|start| *start <= proof.span.start);
            let last = layout
                .line_starts
                .partition_point(|start| *start < proof.span.end);
            if usize::try_from(hit.start_line).ok() != Some(first)
                || usize::try_from(hit.end_line).ok() != Some(last)
            {
                return Err(CcError::Database(
                    "final evidence display coordinates mismatch".into(),
                ));
            }
            let mut verified = hit.clone();
            verified.metadata["source_freshness"] = serde_json::json!({"status":"current_verified","disk_checked":true,"hydrator":HYDRATOR_SPEC});
            self.verified_hits += 1;
            result.push(verified);
        }
        self.finish()?;
        Ok(result)
    }
    /// Graph nodes contain relation descriptions, not original source excerpts.
    /// Their file provenance is still constrained and disk freshness checked.
    pub fn graph_current(&mut self, node: &ContextNode) -> CcResult<bool> {
        self.control.check()?;
        if node.backing_file_path.is_some()
            || node.backing_source.is_some()
            || node.span_kind.is_some()
            || node.source_start_line.is_some()
            || node.source_end_line.is_some()
        {
            return Err(CcError::Database(
                "graph description cannot claim an unverified source slice".into(),
            ));
        }
        let Some(path) = node.file_path.as_deref() else {
            return Ok(node.backing_file_path.is_none() && node.span_kind.is_none());
        };
        let language = cc_db::document_store::indexed_source_language(self.db, path)?;
        let Some(language) = language else {
            return self.verifier.path_current(path).map(|_| false);
        };
        if !self.scope.passes(path, Language::from_name(&language)) {
            return Err(CcError::Database(
                "graph evidence is outside final hard scope".into(),
            ));
        }
        self.verifier.path_current(path)
    }
    pub fn finish(&self) -> CcResult<()> {
        self.control.check()?;
        if self.db.reads().read_generation()? != self.generation {
            return Err(CcError::RetrievalChanged { attempts: 1 });
        }
        Ok(())
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        let mut result = self.verifier.diagnostics();
        result["hydrator"] = serde_json::json!(HYDRATOR_SPEC);
        result["verified_hits"] = serde_json::json!(self.verified_hits);
        result["generation"] = serde_json::json!(self.generation);
        result["scope"] = serde_json::json!("bounded per-file disk verification and optimistic full read generation; not an atomic filesystem snapshot");
        result
    }
}
