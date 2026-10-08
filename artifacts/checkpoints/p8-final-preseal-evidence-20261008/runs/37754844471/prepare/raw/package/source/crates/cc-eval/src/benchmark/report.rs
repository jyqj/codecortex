use super::{
    gate::{self, Gate},
    invalid,
    manifest::{digest, InputManifest},
    metrics::{self, Scores},
    schema::*,
    statistics, Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write, path::Path};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub suite: Suite,
    pub input: InputManifest,
    pub adapter: String,
    pub adapter_version: String,
    pub engine: serde_json::Value,
    pub query_snapshot_digest: String,
    pub normalized_digest: String,
    pub measurement_profile: String,
    pub source_verification: String,
    pub infrastructure_failure: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseScore {
    pub id: String,
    pub category: String,
    pub family: String,
    pub repetitions: usize,
    pub top1: f64,
    pub ndcg10: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub queries: usize,
    pub measured_rows: usize,
    pub mean_top1: f64,
    pub mean_ndcg10: f64,
    pub cases: Vec<CaseScore>,
    pub category_means: BTreeMap<String, (f64, f64)>,
    pub family_ndcg_ci: Option<statistics::Interval>,
    pub unverified_hits: usize,
    pub invalid_hits: usize,
}
pub fn json(path: &Path, value: &impl Serialize) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
pub fn jsonl<T: Serialize>(path: &Path, values: &[T]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for v in values {
        serde_json::to_writer(&mut f, v)?;
        f.write_all(b"\n")?;
    }
    Ok(())
}
pub fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    std::fs::read_to_string(path)?
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}
/// On explicit cancellation keep the partial raw/normalized prefix and a nonzero gate.
pub fn record_cancelled(out: &Path) -> Result<()> {
    if !out.exists() {
        return Ok(());
    }
    let gate = Gate {
        status: "cancelled".into(),
        exit_code: 3,
        reasons: vec!["cancelled_by_user; partial artifacts retained".into()],
    };
    json(&out.join("gate.json"), &gate)?;
    let path = out.join("manifest.json");
    if path.exists() {
        let mut m: RunManifest = super::manifest::json_file(&path)?;
        m.infrastructure_failure = Some("cancelled_by_user".into());
        let normalized = out.join("normalized.jsonl");
        if normalized.exists() {
            m.normalized_digest = super::manifest::file_digest(&normalized)?;
        }
        json(&path, &m)?;
    }
    for e in std::fs::read_dir(out)? {
        let e = e?;
        if e.file_type()?.is_dir()
            && (e.file_name().to_string_lossy().starts_with("suite-")
                || e.path().join("manifest.json").is_file())
        {
            record_cancelled(&e.path())?;
        }
    }
    Ok(())
}
pub fn summarize(
    rows: &[Row],
    queries: &[Query],
    profile: ScoreProfile,
    seed: u64,
) -> Result<(Summary, Vec<Scores>)> {
    let map: BTreeMap<&str, &Query> = queries.iter().map(|q| (q.id.as_str(), q)).collect();
    let mut scores = Vec::new();
    for r in rows {
        let q = map
            .get(r.case_id.as_str())
            .ok_or_else(|| invalid("result for unknown query"))?;
        scores.push(metrics::score(r, q, profile)?);
    }
    let mut cases = Vec::new();
    let mut categories: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    let mut families: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for q in queries.iter().filter(|q| !q.no_answer) {
        let samples: Vec<&Scores> = rows
            .iter()
            .zip(&scores)
            .filter(|(r, _)| r.case_id == q.id)
            .map(|(_, s)| s)
            .collect();
        let n = samples.len();
        let a = if n == 0 {
            0.0
        } else {
            samples.iter().map(|s| s.top1).sum::<f64>() / n as f64
        };
        let b = if n == 0 {
            0.0
        } else {
            samples.iter().map(|s| s.ndcg10).sum::<f64>() / n as f64
        };
        cases.push(CaseScore {
            id: q.id.clone(),
            category: q.category.clone(),
            family: q.query_family.clone(),
            repetitions: n,
            top1: a,
            ndcg10: b,
        });
        categories
            .entry(q.category.clone())
            .or_default()
            .push((a, b));
        families.entry(q.query_family.clone()).or_default().push(b);
    }
    let category_means = categories
        .into_iter()
        .map(|(k, v)| {
            let n = v.len() as f64;
            (
                k,
                (
                    v.iter().map(|x| x.0).sum::<f64>() / n,
                    v.iter().map(|x| x.1).sum::<f64>() / n,
                ),
            )
        })
        .collect();
    let family_values: Vec<f64> = families
        .values()
        .map(|v| v.iter().sum::<f64>() / v.len() as f64)
        .collect();
    let n = cases.len() as f64;
    Ok((
        Summary {
            queries: queries.len(),
            measured_rows: rows.len(),
            mean_top1: if n == 0.0 {
                0.0
            } else {
                cases.iter().map(|s| s.top1).sum::<f64>() / n
            },
            mean_ndcg10: if n == 0.0 {
                0.0
            } else {
                cases.iter().map(|s| s.ndcg10).sum::<f64>() / n
            },
            cases,
            category_means,
            family_ndcg_ci: statistics::bootstrap(&family_values, seed),
            unverified_hits: rows
                .iter()
                .flat_map(|r| &r.hits)
                .filter(|h| h.evidence_valid.is_none())
                .count(),
            invalid_hits: rows
                .iter()
                .flat_map(|r| &r.hits)
                .filter(|h| h.evidence_valid == Some(false))
                .count(),
        },
        scores,
    ))
}
/// Slice existing per-question means, never treat repeated requests as independent
/// questions. Annotation-only query language is distinct from source language.
pub fn query_slices(cases: &[CaseScore], queries: &[Query]) -> serde_json::Value {
    let mut groups: BTreeMap<(String, String, String), (usize, f64, f64)> = BTreeMap::new();
    for c in cases {
        let Some(q) = queries.iter().find(|q| q.id == c.id && !q.no_answer) else {
            continue;
        };
        let label = |key: &str| {
            q.annotations
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unspecified")
                .to_owned()
        };
        let entry = groups
            .entry((
                q.category.clone(),
                label("query_language"),
                label("lexical_anchor"),
            ))
            .or_default();
        entry.0 += 1;
        entry.1 += c.top1;
        entry.2 += c.ndcg10;
    }
    let slices: Vec<_> = groups.into_iter().map(|((category, language, anchor),(n,top1,ndcg))|
        serde_json::json!({"category":category,"query_language":language,"lexical_anchor":anchor,"questions":n,"mean_top1":top1/n as f64,"mean_ndcg10":ndcg/n as f64})).collect();
    serde_json::json!({"schema_version":1,"aggregation":"per_question_mean; no_answer kept separately in scores/gate; translations share query_family and are not independent families","no_answer_questions":queries.iter().filter(|q|q.no_answer).count(),"slices":slices})
}

pub fn finish(out: &Path, manifest: &RunManifest, queries: &[Query], rows: &[Row]) -> Result<Gate> {
    let (summary, scores) = summarize(rows, queries, manifest.suite.scoring, manifest.suite.seed)?;
    let gate = gate::evaluate(
        rows,
        &scores,
        queries.len() * manifest.suite.repetitions,
        manifest.infrastructure_failure.as_deref(),
    );
    json(&out.join("metrics.json"), &summary)?;
    json(
        &out.join("query-slices.json"),
        &query_slices(&summary.cases, queries),
    )?;
    jsonl(&out.join("scores.jsonl"), &scores)?;
    let costs: Vec<_> = rows.iter().map(|r| -> Result<_> {
        super::validation::relative_path(&r.raw_path)?;
        let raw: serde_json::Value = super::manifest::json_file(&out.join(&r.raw_path))?;
        Ok(serde_json::json!({"case_id":r.case_id,"repetition":r.repetition,"status":r.status,"elapsed_us":r.elapsed_us,"work":super::sampler::retrieval_work(&raw)}))
    }).collect::<Result<_>>()?;
    jsonl(&out.join("costs.jsonl"), &costs)?;
    let latencies: Vec<u64> = rows
        .iter()
        .filter(|r| matches!(r.status, ResultStatus::Success | ResultStatus::NoMatch))
        .map(|r| r.elapsed_us)
        .collect();
    let latency = statistics::distribution(&latencies);
    json(&out.join("latency-summary.json"), &latency)?;
    // Preserve the legacy successful-request distribution and Row schema. The
    // additive artifact includes every status and explicitly refuses to infer a
    // cache hit or a reopened process from warmup, repetition or profile names.
    let samples: Vec<_> = rows
        .iter()
        .map(|row| statistics::LatencySample {
            evidence: statistics::LatencyEvidence::default(),
            status: row.status,
            elapsed_us: Some(row.elapsed_us),
        })
        .collect();
    let layers = statistics::latency_layers(&samples, queries.len() * manifest.suite.repetitions);
    json(&out.join("latency-strata.json"), &layers)?;
    // The original run format does not lock resources in its manifest. Include
    // the observed source digest, without upgrading it to locked release proof.
    // Older replay fixtures can legitimately have no resource artifact at all.
    let resource_path = out.join("resources.jsonl");
    let (resource_samples, resource_digest) = match std::fs::read(&resource_path) {
        Ok(bytes) => {
            let text =
                std::str::from_utf8(&bytes).map_err(|_| invalid("resource snapshot encoding"))?;
            let samples = text
                .lines()
                .filter(|line| !line.is_empty())
                .map(serde_json::from_str)
                .collect::<std::result::Result<Vec<super::sampler::Resources>, _>>()?;
            (samples, Some(digest(&bytes)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), None),
        Err(e) => return Err(e.into()),
    };
    let ledger = serde_json::json!({
        "schema_version": 1,
        "source": "resources.jsonl; stage snapshots only; source digest observed, not bound by legacy manifest",
        "resource_source_digest": resource_digest,
        "memory": super::sampler::memory_ledger(&resource_samples),
        "disk": super::sampler::disk_ledger(&[], false)?,
        "cost": super::sampler::cost_ledger(&[])?,
        "io_read_bytes": null,
        "io_write_bytes": null,
        "unavailable_reason": "legacy runner records no disjoint disk layout, I/O counters or model billing receipts; originating retrieval work in costs.jsonl is not current-request billing",
    });
    json(&out.join("resource-ledger.json"), &ledger)?;
    json(&out.join("gate.json"), &gate)?;
    let mut md=format!("# CodeCortex benchmark baseline\n\nSuite: `{}`\nAdapter: `{}`\nScoring: `{:?}`\nMeasurement profile: `{}`\n\nStatus: **{}** (exit {})\n\nQueries: {}; measured rows: {}.\nMean Top-1: {:.6}; mean nDCG@10: {:.6}.\n\nThese are baseline observations, not a release quality certificate.\nLatency: {:?}; samples are retained, not best-of.\nSource evidence: {} invalid, {} unverified hits.\n\n| Case | Top-1 | nDCG@10 | Repetitions |\n|---|---:|---:|---:|\n",manifest.suite.name,manifest.adapter,manifest.suite.scoring,manifest.measurement_profile,gate.status,gate.exit_code,summary.queries,summary.measured_rows,summary.mean_top1,summary.mean_ndcg10,latency,summary.invalid_hits,summary.unverified_hits);
    for c in &summary.cases {
        md.push_str(&format!(
            "| {} | {:.6} | {:.6} | {} |\n",
            c.id.replace('|', "/"),
            c.top1,
            c.ndcg10,
            c.repetitions
        ));
    }
    md.push_str("\n## Measurement coverage\n\n`latency-summary.json` retains its legacy success/no-match-only semantics. `latency-strata.json` retains every measured attempt, including deadline-censored timeouts and failures, and reports missing samples against the planned denominator. This runner records no per-request lifecycle/result-cache receipt, so these queries remain `unknown`; repetition and warmup do not prove cache hits. No OS cold-cache or performance-gate pass is inferred.\n\n`resource-ledger.json` reports observed per-role RSS snapshots and missing observations. Native/ps runner alternatives, server PID/tree values and peaks from different stages are never summed. Disk layout, I/O and model billing remain unavailable until a profile supplies direct observations.\n");
    md.push_str("\n## Failures\n");
    for r in &gate.reasons {
        md.push_str(&format!("\n- {}", r.replace('\n', " ")));
    }
    std::fs::write(out.join("report.md"), md)?;
    Ok(gate)
}
pub fn replay(out: &Path) -> Result<Gate> {
    let m: RunManifest = super::manifest::json_file(&out.join("manifest.json"))?;
    if m.schema_version != 1 || m.adapter_version != ADAPTER_VERSION {
        return Err(invalid("unsupported run version"));
    }
    if digest(&std::fs::read(out.join("queries.jsonl"))?) != m.query_snapshot_digest {
        return Err(invalid("query snapshot drift"));
    }
    let q: Vec<Query> = read_jsonl(&out.join("queries.jsonl"))?;
    super::validation::queries(&q, m.suite.scoring)?;
    if digest(&std::fs::read(out.join("normalized.jsonl"))?) != m.normalized_digest {
        return Err(invalid("normalized results drift"));
    }
    let rows: Vec<Row> = read_jsonl(&out.join("normalized.jsonl"))?;
    let mut seen = std::collections::BTreeSet::new();
    for r in &rows {
        if r.repetition >= m.suite.repetitions || !seen.insert((&r.case_id, r.repetition)) {
            return Err(invalid("duplicate/out-of-range result repetition"));
        }
        super::validation::relative_path(&r.raw_path)?;
        if digest(&std::fs::read(out.join(&r.raw_path))?) != r.raw_digest {
            return Err(invalid("raw response drift"));
        }
    }
    finish(out, &m, &q, &rows)
}
