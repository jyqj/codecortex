//! Mechanism-only costs and reproducible paired effects. Missing public billing
//! and cache attribution remain unknown, including in the local arm.
use super::{protocol, QueryObservation};
use crate::benchmark::{report::Summary, schema::Query, statistics, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn cost_rows(rows: &[QueryObservation], profile: &str) -> Vec<Value> {
    rows.iter().map(|row| json!({
        "profile":profile,"sequence":row.sequence,"input_digest":row.input_digest,
        "requested":row.requested,"effective":row.effective,"error":row.error,
        "originating_work":row.originating_cost,"validation_work":row.validation_work,
        "semantic_coverage":row.semantic_coverage,
        "provider_cost_units":null,"provider_cost_unknown":true,
        "cache_reuse":null,"cache_reuse_unknown":true,
        "attribution":"public output does not expose provider billing or current-query cache-reuse attribution; originating work is not additive current-request cost"
    })).collect()
}

pub fn cost_columns(rows: &[QueryObservation], profile: &str) -> Value {
    json!({"profile":profile,"requests_including_warmup":rows.len(),
        "originating_work_observations":rows.iter().filter(|row| !row.originating_cost.is_null()).count(),
        "originating_work_unknown":rows.iter().filter(|row| row.originating_cost.is_null()).count(),
        "validation_work_observations":rows.iter().filter(|row| !row.validation_work.is_null()).count(),
        "validation_work_unknown":rows.iter().filter(|row| row.validation_work.is_null()).count(),
        "provider_cost_units":null,"provider_cost_unknown":rows.len(),
        "cache_reuse":null,"cache_reuse_unknown":rows.len(),
        "scope":"observation counts only; no sum of originating-work receipts and no zero-filled monetary/cache estimate"})
}

fn means(families: &BTreeMap<String, Vec<f64>>) -> BTreeMap<String, f64> {
    families
        .iter()
        .map(|(family, values)| {
            (
                family.clone(),
                values.iter().sum::<f64>() / values.len() as f64,
            )
        })
        .collect()
}

/// Case means come from the existing scorer. Repeated requests are averaged
/// before paired differences; translations share a query-family sampling unit.
/// The exposed case/family deltas and seed make every interval reproducible.
pub fn paired_summary(
    baseline: &Summary,
    candidate: &Summary,
    queries: &[Query],
    repetitions: usize,
    seed: u64,
    profile: &str,
) -> Result<Value> {
    let mut left: BTreeMap<_, _> = baseline
        .cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    let mut right: BTreeMap<_, _> = candidate
        .cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    if repetitions == 0
        || left.len() != baseline.cases.len()
        || right.len() != candidate.cases.len()
    {
        return Err(protocol("duplicate cases or missing strategy repetitions"));
    }
    let mut cases = Vec::new();
    let mut families: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut strata: BTreeMap<(String, String), BTreeMap<String, Vec<f64>>> = BTreeMap::new();
    for query in queries.iter().filter(|query| !query.no_answer) {
        let a = left
            .remove(query.id.as_str())
            .ok_or_else(|| protocol("missing baseline case"))?;
        let b = right
            .remove(query.id.as_str())
            .ok_or_else(|| protocol("missing candidate case"))?;
        for case in [a, b] {
            if case.family != query.query_family
                || case.category != query.category
                || case.repetitions != repetitions
                || ![case.top1, case.ndcg10]
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            {
                return Err(protocol("paired case identity, population or score drift"));
            }
        }
        let delta = b.ndcg10 - a.ndcg10;
        families
            .entry(query.query_family.clone())
            .or_default()
            .push(delta);
        strata
            .entry((query.split.clone(), query.category.clone()))
            .or_default()
            .entry(query.query_family.clone())
            .or_default()
            .push(delta);
        cases.push(json!({"profile":profile,"id":query.id,"split":query.split,
            "category":query.category,"query_family":query.query_family,
            "repetitions_averaged":repetitions,
            "delta_top1":b.top1-a.top1,"delta_ndcg10":delta}));
    }
    if !left.is_empty() || !right.is_empty() || cases.is_empty() {
        return Err(protocol(
            "paired score population differs from the locked suite",
        ));
    }
    let family_means = means(&families);
    let values = family_means.values().copied().collect::<Vec<_>>();
    let strata = strata.into_iter().map(|((split, category), families)| {
        let family_means = means(&families);
        let values = family_means.values().copied().collect::<Vec<_>>();
        json!({"profile":profile,"split":split,"category":category,
            "family_means":family_means,"family_ndcg_delta_ci":statistics::bootstrap(&values, seed)})
    }).collect::<Vec<_>>();
    Ok(
        json!({"profile":profile,"effect_interpretation":"none; authored/fake mechanism observations only",
        "direction":"candidate minus local","method":"paired query means; family bootstrap; category/split strata reported separately",
        "seed":seed,"bootstrap_draws":2_000,"percentile_indices_zero_based":[49,1949],
        "no_answer_queries_excluded_from_effect_ci":queries.iter().filter(|query| query.no_answer).count(),
        "family_means":family_means,"family_ndcg_delta_ci":statistics::bootstrap(&values, seed),
        "cases":cases,"strata":strata,
        "quality_gate":"not_evaluated; fake/engineering effects have no semantic benefit interpretation"}),
    )
}
