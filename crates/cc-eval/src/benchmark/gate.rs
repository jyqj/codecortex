use super::{metrics::Scores, schema::*};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub status: String,
    pub exit_code: i32,
    pub reasons: Vec<String>,
}
pub fn evaluate(
    rows: &[Row],
    scores: &[Scores],
    expected: usize,
    infrastructure: Option<&str>,
) -> Gate {
    if infrastructure == Some("cancelled_by_user")
        || rows.iter().any(|r| r.status == ResultStatus::Cancelled)
    {
        return Gate {
            status: "cancelled".into(),
            exit_code: 3,
            reasons: vec!["partial artifacts retained; no complete measurement claim".into()],
        };
    }
    let mut reasons = Vec::new();
    let mut code = 0;
    if let Some(e) = infrastructure {
        reasons.push(e.into());
        code = 2;
    }
    if expected == 0 {
        reasons.push("no expected measurements".into());
        code = 2;
    }
    if rows.len() != expected || scores.len() != rows.len() {
        reasons.push("missing measurements".into());
        code = 2;
    }
    for (i, r) in rows.iter().enumerate() {
        if r.status == ResultStatus::ProtocolError {
            reasons.push(format!("{} protocol failure", r.case_id));
            code = 2;
        } else if !matches!(r.status, ResultStatus::Success | ResultStatus::NoMatch) {
            reasons.push(format!("{} {:?}", r.case_id, r.status));
            if code == 0 {
                code = 1;
            }
        }
        if r.hits.iter().any(|h| h.evidence_valid == Some(false)) {
            reasons.push(format!("{} invalid source evidence", r.case_id));
            if code == 0 {
                code = 1;
            }
        }
        if scores.get(i).and_then(|s| s.no_answer_correct) == Some(false) {
            reasons.push(format!("{} no-answer failure", r.case_id));
            if code == 0 {
                code = 1;
            }
        }
    }
    Gate {
        status: if code == 0 {
            "baseline_recorded_not_quality_certified"
        } else if code == 2 {
            "invalid_measurement"
        } else {
            "gate_failed"
        }
        .into(),
        exit_code: code,
        reasons,
    }
}
