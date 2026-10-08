//! Versioned linear-gain scores, independent of adapters and production retrieval.
use super::{invalid, schema::*, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scores {
    pub top1: f64,
    pub ndcg10: f64,
    pub recall5: Option<f64>,
    pub recall10: Option<f64>,
    pub mrr10: Option<f64>,
    pub span_precision: Option<f64>,
    pub span_recall: Option<f64>,
    pub no_answer_correct: Option<bool>,
}
pub fn path_matches(actual: &str, pattern: &str) -> Result<bool> {
    super::fnmatch::matches(actual, pattern)
}
fn dcg(gains: &[u8]) -> f64 {
    gains
        .iter()
        .enumerate()
        .map(|(i, g)| *g as f64 / ((i + 2) as f64).log2())
        .sum()
}
fn ndcg(gains: &[u8], grades: &[u8]) -> f64 {
    let mut ideal = grades.to_vec();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    ideal.truncate(10);
    let d = dcg(&ideal);
    if d == 0.0 {
        0.0
    } else {
        dcg(gains) / d
    }
}
pub fn compatibility(paths: &[String], expected: &[String]) -> Result<Scores> {
    if expected.is_empty() {
        return Err(invalid("empty expected_files"));
    }
    let mut seen = BTreeSet::new();
    let mut used = BTreeSet::new();
    let mut gains = Vec::new();
    let mut top1 = 0.0;
    for (rank, p) in paths
        .iter()
        .filter(|p| seen.insert(p.as_str()))
        .take(10)
        .enumerate()
    {
        let mut matched = None;
        for (i, e) in expected.iter().enumerate() {
            if !used.contains(&i) && path_matches(p, e)? {
                matched = Some(i);
                break;
            }
        }
        if let Some(i) = matched {
            used.insert(i);
            gains.push(if i == 0 { 2 } else { 1 });
            if rank == 0 {
                top1 = 1.0;
            }
        } else {
            gains.push(0);
        }
    }
    let grades: Vec<u8> = (0..expected.len())
        .map(|i| if i == 0 { 2 } else { 1 })
        .collect();
    Ok(Scores {
        top1,
        ndcg10: ndcg(&gains, &grades),
        ..Scores::default()
    })
}
fn matches_answer(h: &Hit, a: &Alternative) -> bool {
    if h.path != a.path || h.evidence_valid == Some(false) {
        return false;
    }
    if let Some(s) = &a.symbol {
        if h.symbol_name.as_ref() != Some(&s.name) {
            return false;
        }
        if s.qname
            .as_ref()
            .is_some_and(|v| h.qname.as_ref() != Some(v))
        {
            return false;
        }
        if s.kind.as_ref().is_some_and(|v| h.kind.as_ref() != Some(v)) {
            return false;
        }
    }
    if let Some(g) = &a.span {
        let Some(s) = &h.span else {
            return false;
        };
        if s.start.max(g.start) >= s.end.min(g.end) {
            return false;
        }
    }
    true
}
pub fn native(hits: &[Hit], q: &Query) -> Scores {
    if q.no_answer {
        return Scores {
            no_answer_correct: Some(hits.is_empty()),
            ..Scores::default()
        };
    }
    let primary = |h: &Hit| {
        q.answers
            .iter()
            .any(|g| g.primary && g.alternatives.iter().any(|a| matches_answer(h, a)))
    };
    let top1 = hits.first().is_some_and(primary) as u8 as f64;
    let mut used = BTreeSet::new();
    let mut gains = Vec::new();
    let mut at5 = 0;
    let mut at10 = 0;
    let mut mrr = 0.0;
    for (i, h) in hits.iter().take(10).enumerate() {
        if mrr == 0.0 && primary(h) {
            mrr = 1.0 / (i + 1) as f64;
        }
        let best = q
            .answers
            .iter()
            .enumerate()
            .filter(|(j, g)| {
                !used.contains(j) && g.alternatives.iter().any(|a| matches_answer(h, a))
            })
            .max_by_key(|(j, g)| (g.grade, std::cmp::Reverse(*j)));
        if let Some((j, g)) = best {
            used.insert(j);
            gains.push(g.grade);
            at10 += 1;
            if i < 5 {
                at5 += 1;
            }
        } else {
            gains.push(0);
        }
    }
    let (span_precision, span_recall) = super::span_metrics::coverage(hits, q);
    Scores {
        top1,
        ndcg10: ndcg(
            &gains,
            &q.answers.iter().map(|g| g.grade).collect::<Vec<_>>(),
        ),
        recall5: Some(at5 as f64 / q.answers.len() as f64),
        recall10: Some(at10 as f64 / q.answers.len() as f64),
        mrr10: Some(mrr),
        span_precision,
        span_recall,
        no_answer_correct: None,
    }
}
pub fn score(row: &Row, q: &Query, profile: ScoreProfile) -> Result<Scores> {
    // A partial empty scan is not evidence that the answer does not exist.
    if q.no_answer && row.status == ResultStatus::Partial {
        return Ok(Scores {
            no_answer_correct: Some(false),
            ..Scores::default()
        });
    }
    if !matches!(
        row.status,
        ResultStatus::Success | ResultStatus::NoMatch | ResultStatus::Partial
    ) {
        return Ok(Scores {
            no_answer_correct: q.no_answer.then_some(false),
            ..Scores::default()
        });
    }
    match profile {
        ScoreProfile::OceCompat => compatibility(
            &row.hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
            &q.expected_files,
        ),
        ScoreProfile::Native => Ok(native(&row.hits, q)),
    }
}
