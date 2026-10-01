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
    #[serde(default)]
    pub source_support_anchors: Vec<String>,
    #[serde(default)]
    pub source_support_scope: String,
    #[serde(default)]
    pub intent_facet_anchors: BTreeMap<String, String>,
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
    select_with_query(ranked, intent, limit, "")
}

/// A literal, distinctive program-name mention can identify a source file
/// through its imports/bootstrap, while common route/API words rank unrelated
/// comments above its actual implementations. Reserve bounded existing source
/// evidence from that file, never invent a framework or change scores/scope.
pub fn select_with_query(
    ranked: &[SearchHit],
    intent: Intent,
    limit: usize,
    query: &str,
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
    let mut source_support = BTreeSet::new();
    static PROGRAM_TOKEN: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap());
    let proper_terms: BTreeSet<(String, bool)> = PROGRAM_TOKEN
        .find_iter(query)
        .enumerate()
        .map(|(index, item)| (item.as_str(), index == 0))
        .filter(|(word, _)| {
            word.len() >= 3 && (word.chars().any(|c| c.is_ascii_uppercase()) || word.contains('_'))
        })
        .take(12)
        .map(|(word, first)| {
            (
                word.to_lowercase(),
                first && word.chars().skip(1).all(|c| c.is_ascii_lowercase()),
            )
        })
        .collect();
    let mut distinctive_files = BTreeSet::new();
    let mut title_fallback_files = BTreeSet::new();
    for (term, title_fallback) in proper_terms {
        let matching: BTreeSet<&str> = ranked
            .iter()
            .take(limit.saturating_mul(2))
            .enumerate()
            .filter(|(index, hit)| {
                proofs[*index].validate(&hit.text)
                    && cc_db::fts::tokenize_codeish(&hit.text).contains(&term)
            })
            .map(|(_, hit)| hit.file_path.as_str())
            .collect();
        if matching.len() == 1 {
            let file = (*matching.iter().next().unwrap()).to_owned();
            if title_fallback {
                title_fallback_files.insert(file);
            } else {
                distinctive_files.insert(file);
            }
        }
    }
    if distinctive_files.is_empty() {
        distinctive_files = title_fallback_files;
    }
    for (index, hit) in ranked.iter().take(limit.saturating_mul(2)).enumerate() {
        if source_support.len() >= limit.saturating_sub(1).min(2) {
            break;
        }
        if distinctive_files.contains(&hit.file_path)
            && hit
                .symbol_kind
                .is_some_and(|kind| matches!(kind.as_str(), "function" | "method"))
            && proofs[index].owner.is_some()
            && proofs[index].validate(&hit.text)
            && proofs[index].signature.is_some_and(|signature| {
                signature.start < signature.end
                    && proofs[index].owner.is_some_and(|owner| {
                        owner.start <= signature.start && signature.end <= owner.end
                    })
                    && proofs[index].span.start <= signature.start
                    && signature.end <= proofs[index].span.end
            })
            && cc_db::fts::tokenize_codeish(&cc_db::fts::expand_query_text(&hit.text))
                .iter()
                .any(|word| cc_db::fts::tokenize_codeish(query).contains(word))
        {
            source_support.insert(index);
        }
    }
    // At least half the result slots are rank anchors. Locate has no file
    // diversity rule: many useful pieces from one source stay admissible.
    let anchors = if desired.is_empty() {
        limit.saturating_sub(source_support.len()).max(1)
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
    for &index in &source_support {
        if chosen.len() < limit {
            admit(index, ranked, &proofs, &mut chosen, &mut selected_union)?;
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
    let mut intent_facet_anchors = BTreeMap::new();
    for want in desired {
        if let Some(index) = chosen.iter().find(|&&index| facet(&ranked[index]) == *want) {
            intent_facet_anchors.insert((*want).to_string(), ranked[*index].chunk_id.clone());
        }
    }
    let mut facets = BTreeMap::new();
    let selected: Vec<_> = chosen
        .iter()
        .map(|&i| {
            *facets.entry(facet(&ranked[i]).to_string()).or_insert(0) += 1;
            let mut hit = ranked[i].clone();
            // Caller/provider metadata is never an authority for packing priority.
            hit.metadata
                .as_object_mut()
                .unwrap()
                .remove("coverage_priority");
            hit.metadata
                .as_object_mut()
                .unwrap()
                .remove("evidence_priority");
            if intent_facet_anchors.values().any(|id| *id == hit.chunk_id) {
                hit.metadata["evidence_priority"] =
                    serde_json::json!(cc_model::context::EvidencePriority::IntentFacet);
            } else if source_support.contains(&i) {
                hit.metadata["evidence_priority"] = serde_json::json!(
                    cc_model::context::EvidencePriority::DistinctiveSourceSupport
                );
            }
            hit
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
            source_support_anchors: source_support
                .iter()
                .filter(|index| chosen.contains(index))
                .map(|&index| ranked[index].chunk_id.clone())
                .collect(),
            source_support_scope: "literal_program_cue_in_validated_twice_topk_window_only; not_global_uniqueness_or_exact_identity".into(),
            intent_facet_anchors,
        },
    ))
}
