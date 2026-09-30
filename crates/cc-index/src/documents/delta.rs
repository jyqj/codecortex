//! Deterministic current-manifest diff and projection. No provider, file IO or second parser.
use cc_model::{
    identity::{DocumentBatch, DocumentDelta, DocumentRecord, DocumentRef},
    source::SourceSnapshot,
    CcResult, ParseOutcome,
};
use std::collections::{BTreeMap, BTreeSet};
pub fn encoding_spec() -> CcResult<String> {
    Ok(serde_json::to_string(&(
        cc_model::identity::DOCUMENT_VERSION,
        super::render::FORMAT_VERSION,
        super::render::RenderOptions::default(),
        cc_model::chunk_policy::TOKEN_ESTIMATOR,
    ))?)
}
pub fn spec_fingerprint() -> &'static str {
    static SPEC: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        cc_model::identity::hash(&encoding_spec().expect("finite encoding spec"))
            .expect("finite spec")
    });
    &SPEC
}
pub fn compare(old: &[DocumentRef], records: &[DocumentRecord]) -> DocumentDelta {
    let previous: BTreeMap<_, _> = old.iter().map(|r| (&r.doc_key, r)).collect();
    let current: BTreeSet<_> = records.iter().map(|r| &r.reference.doc_key).collect();
    let inputs: BTreeSet<_> = old.iter().filter_map(|r| r.encoding_key.as_ref()).collect();
    let mut delta = DocumentDelta::default();
    for record in records {
        let reference = &record.reference;
        if previous
            .get(&reference.doc_key)
            .is_some_and(|r| r.doc_version == reference.doc_version)
        {
            delta.unchanged += 1;
        } else {
            delta.upsert.push(reference.clone());
        }
        delta.reusable_inputs += usize::from(
            reference
                .encoding_key
                .as_ref()
                .is_some_and(|h| inputs.contains(h)),
        );
        delta.render_failed += usize::from(record.input.is_none());
    }
    delta.removed = old
        .iter()
        .filter(|r| !current.contains(&r.doc_key))
        .cloned()
        .collect();
    delta.upsert.sort_by(|a, b| a.doc_key.cmp(&b.doc_key));
    delta.removed.sort_by(|a, b| a.doc_key.cmp(&b.doc_key));
    delta
}
pub fn prepare(
    source: &SourceSnapshot<'_>,
    outcome: &ParseOutcome,
    previous: &[DocumentRef],
) -> CcResult<DocumentBatch> {
    let spec = encoding_spec()?;
    let policy = outcome
        .chunk_policy
        .as_deref()
        .ok_or_else(|| cc_model::CcError::InvalidParams("document policy missing".into()))?;
    let mut occurrences: BTreeMap<&str, u32> = BTreeMap::new();
    let mut records = Vec::with_capacity(outcome.chunks.len());
    for chunk in &outcome.chunks {
        let Some(proof) = &chunk.source else { continue };
        let occurrence = occurrences.entry(&proof.slice_digest).or_default();
        let input = super::render::render(source, chunk, super::render::RenderOptions::default())
            .map_err(|e| e.to_string());
        // Source mismatch is not a render limitation and must fail the build.
        if proof.source != *source.identity() || source.slice(proof.span)? != chunk.text {
            return Err(cc_model::CcError::InvalidParams(
                "document source mismatch".into(),
            ));
        }
        records.push(DocumentRecord::new(
            chunk,
            policy,
            *occurrence,
            &spec,
            input,
        )?);
        *occurrence += 1;
    }
    let delta = compare(previous, &records);
    Ok(DocumentBatch { records, delta })
}
