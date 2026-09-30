//! Preserve the highest-ranked anchor; reserve bounded evidence facets only
//! from the already-ranked, validated candidate window. Never rewrite scores.
use super::{
    overlap::{OverlapStats, SourceCoverage},
    SELECTION_SPEC,
};
use cc_model::{search::SearchHit, source::ChunkSource, CcResult, Intent};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct SelectionReport {
    pub spec: &'static str,
    pub intent: Intent,
    pub input_candidates: usize,
    pub selected: usize,
    pub requested_limit: usize,
    pub original_ranks: Vec<usize>,
    pub facets: BTreeMap<String, usize>,
    pub unmet_facets: Vec<String>,
    pub overlap: OverlapStats,
    pub selected_overlap: OverlapStats,
    pub source_covered_omissions: usize,
    pub limit_omitted: usize,
}
fn facet(hit: &SearchHit) -> &'static str {
    let p = hit.file_path.as_str();
    let name = p.rsplit('/').next().unwrap_or(p);
    if p.split('/')
        .any(|part| part == "test" || part == "tests" || part == "__tests__")
        || name.starts_with("test_")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("_test.go")
        || name.ends_with("_test.rs")
    {
        "test"
    } else if hit
        .symbol_kind
        .is_some_and(|kind| matches!(kind.as_str(), "interface" | "trait" | "type_alias"))
    {
        "interface"
    } else if hit.reasons.iter().any(|r| r == "doc-file") {
        "documentation"
    } else {
        "implementation"
    }
}
fn admit(
    index: usize,
    ranked: &[SearchHit],
    proofs: &[ChunkSource],
    chosen: &mut BTreeSet<usize>,
    selected: &mut SourceCoverage,
) -> CcResult<bool> {
    if chosen.contains(&index) {
        return Ok(false);
    }
    let proof = &proofs[index];
    if selected.new_bytes(
        &ranked[index].file_path,
        &proof.source.snapshot_id,
        proof.span,
    )? == 0
    {
        return Ok(false);
    }
    selected.add(
        &ranked[index].file_path,
        &proof.source.snapshot_id,
        proof.span,
    )?;
    chosen.insert(index);
    Ok(true)
}
pub fn select(
    ranked: &[SearchHit],
    intent: Intent,
    limit: usize,
) -> CcResult<(Vec<SearchHit>, SelectionReport)> {
    let mut union = SourceCoverage::default();
    let mut proofs = Vec::with_capacity(ranked.len());
    for hit in ranked {
        let proof: ChunkSource = serde_json::from_value(hit.metadata["source_evidence"].clone())?;
        union.add(&hit.file_path, &proof.source.snapshot_id, proof.span)?;
        proofs.push(proof);
    }
    let mut selected_union = SourceCoverage::default();
    let desired: &[&str] = match intent {
        Intent::Test | Intent::Fix | Intent::Refactor => &["implementation", "test", "interface"],
        Intent::Trace => &["implementation", "interface"],
        _ => &[],
    };
    let mut chosen = BTreeSet::new();
    // At least half the result slots are rank anchors. Locate has no file
    // diversity rule: many useful pieces from one source stay admissible.
    let anchors = if desired.is_empty() {
        limit
    } else {
        limit.div_ceil(2).max(1)
    };
    for index in 0..ranked.len() {
        if chosen.len() >= anchors.min(limit) {
            break;
        }
        admit(index, ranked, &proofs, &mut chosen, &mut selected_union)?;
    }
    for want in desired {
        if chosen.len() >= limit || chosen.iter().any(|&i| facet(&ranked[i]) == *want) {
            continue;
        }
        // Facet choices stay within twice the requested rank window. This
        // is a selection constraint, not a new relevance/confidence score.
        for index in 0..ranked.len().min(limit.saturating_mul(2)) {
            if facet(&ranked[index]) == *want
                && admit(index, ranked, &proofs, &mut chosen, &mut selected_union)?
            {
                break;
            }
        }
    }
    for index in 0..ranked.len() {
        if chosen.len() >= limit {
            break;
        }
        admit(index, ranked, &proofs, &mut chosen, &mut selected_union)?;
    }
    let mut source_covered_omissions = 0;
    for (index, proof) in proofs.iter().enumerate() {
        if !chosen.contains(&index)
            && selected_union.new_bytes(
                &ranked[index].file_path,
                &proof.source.snapshot_id,
                proof.span,
            )? == 0
        {
            source_covered_omissions += 1;
        }
    }
    let mut facets = BTreeMap::new();
    let selected: Vec<_> = chosen
        .iter()
        .map(|&i| {
            *facets.entry(facet(&ranked[i]).to_string()).or_insert(0) += 1;
            ranked[i].clone()
        })
        .collect();
    let unmet_facets = desired
        .iter()
        .filter(|s| !facets.contains_key(**s))
        .map(|s| (*s).into())
        .collect();
    Ok((
        selected,
        SelectionReport {
            spec: SELECTION_SPEC,
            intent,
            input_candidates: ranked.len(),
            selected: chosen.len(),
            requested_limit: limit,
            original_ranks: chosen.iter().map(|i| i + 1).collect(),
            facets,
            unmet_facets,
            overlap: union.stats,
            selected_overlap: selected_union.stats,
            source_covered_omissions,
            limit_omitted: ranked
                .len()
                .saturating_sub(chosen.len() + source_covered_omissions),
        },
    ))
}
