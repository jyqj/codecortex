//! Comparisons reject mismatched inputs. Quality units are query families, not repetitions.
use super::{
    gate::Gate,
    invalid, manifest,
    report::{RunManifest, Summary},
    statistics, Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema_version: u32,
    pub max_ndcg_regression: f64,
    pub max_top1_regression: f64,
    pub max_latency_ratio: f64,
    pub minimum_latency_samples: usize,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || ![
                self.max_ndcg_regression,
                self.max_top1_regression,
                self.max_latency_ratio,
            ]
            .iter()
            .all(|x| x.is_finite())
            || !(0.0..=1.0).contains(&self.max_ndcg_regression)
            || !(0.0..=1.0).contains(&self.max_top1_regression)
            || self.max_latency_ratio < 1.0
            || self.minimum_latency_samples < 30
        {
            return Err(invalid("invalid comparison policy"));
        }
        Ok(())
    }
}
pub fn compare(base: &Path, candidate: &Path, policy: &Policy) -> Result<serde_json::Value> {
    policy.validate()?;
    // Recompute from hash-checked artifacts instead of trusting cached metrics.
    super::report::replay(base)?;
    super::report::replay(candidate)?;
    let a: RunManifest = manifest::json_file(&base.join("manifest.json"))?;
    let b: RunManifest = manifest::json_file(&candidate.join("manifest.json"))?;
    if a.input.source_digest != b.input.source_digest
        || a.input.query_digest != b.input.query_digest
        || a.input.config_digest != b.input.config_digest
        || a.suite.scoring != b.suite.scoring
        || a.suite.top_k != b.suite.top_k
        || a.suite.timeout_ms != b.suite.timeout_ms
        || a.suite.warmup != b.suite.warmup
        || a.suite.seed != b.suite.seed
        || a.suite.repetitions != b.suite.repetitions
        || a.measurement_profile != b.measurement_profile
        || a.adapter != b.adapter
        || a.adapter_version != b.adapter_version
        || a.engine["os"] != b.engine["os"]
        || a.engine["arch"] != b.engine["arch"]
        || a.engine["cpu_parallelism"] != b.engine["cpu_parallelism"]
        || a.engine["sdkroot"] != b.engine["sdkroot"]
        || a.engine["hardware"] != b.engine["hardware"]
    {
        return Err(invalid("non-comparable source/corpus/config/scoring/budget/environment; use a separately declared experiment"));
    }
    let sa: Summary = manifest::json_file(&base.join("metrics.json"))?;
    let sb: Summary = manifest::json_file(&candidate.join("metrics.json"))?;
    let old: BTreeMap<&str, _> = sa.cases.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut by_family: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for c in &sb.cases {
        let p = old
            .get(c.id.as_str())
            .ok_or_else(|| invalid("case identities changed"))?;
        by_family
            .entry(c.family.clone())
            .or_default()
            .push(c.ndcg10 - p.ndcg10);
    }
    let values: Vec<f64> = by_family
        .values()
        .map(|v| v.iter().sum::<f64>() / v.len() as f64)
        .collect();
    let ci = statistics::bootstrap(&values, b.suite.seed);
    let top1_delta = sb.mean_top1 - sa.mean_top1;
    let mut reasons = Vec::new();
    let mut inconclusive = a.engine["hardware"].is_null() || b.engine["hardware"].is_null();
    let ga: Gate = manifest::json_file(&base.join("gate.json"))?;
    let gb: Gate = manifest::json_file(&candidate.join("gate.json"))?;
    if ga.exit_code != 0 || gb.exit_code != 0 {
        reasons.push("baseline or candidate has failed integrity/availability gate".to_string());
    }
    if top1_delta < -policy.max_top1_regression {
        reasons.push("Top-1 regression".into());
    }
    match &ci {
        Some(i) if i.low >= -policy.max_ndcg_regression => {}
        Some(i) if i.high < -policy.max_ndcg_regression => reasons.push("nDCG regression".into()),
        _ => inconclusive = true,
    }
    let ra: Vec<super::schema::Row> = super::report::read_jsonl(&base.join("normalized.jsonl"))?;
    let rb: Vec<super::schema::Row> =
        super::report::read_jsonl(&candidate.join("normalized.jsonl"))?;
    let da = statistics::distribution(&ra.iter().map(|r| r.elapsed_us).collect::<Vec<_>>());
    let db = statistics::distribution(&rb.iter().map(|r| r.elapsed_us).collect::<Vec<_>>());
    let latency_ratio = da
        .p95_us
        .zip(db.p95_us)
        .filter(|(x, _)| *x > 0)
        .map(|(x, y)| y as f64 / x as f64);
    if da.samples < policy.minimum_latency_samples || db.samples < policy.minimum_latency_samples {
        inconclusive = true;
    } else if latency_ratio.is_some_and(|r| r > policy.max_latency_ratio) {
        reasons.push("p95 regression".into());
    }
    let status = if !reasons.is_empty() {
        "failed"
    } else if inconclusive {
        "inconclusive"
    } else {
        "passed"
    };
    Ok(
        serde_json::json!({"status":status,"exit_code":if status=="passed"{0}else{1},"top1_delta":top1_delta,"family_ndcg_delta_ci":ci,"p95_ratio":latency_ratio,"baseline_samples":da.samples,"candidate_samples":db.samples,"reasons":reasons,"policy":policy}),
    )
}
