//! Bounded grep stages. Every decoded chunk is charged once; the final scope never
//! becomes the soft candidate set. Diagnostics are local to this query, not globals.
use crate::lanes::{grep_prefilter_phrase, LaneContext, LaneRun};
use cc_model::{
    retrieval::{GrepDiagnostics, LaneCoverage, LaneStatus},
    CcResult,
};
use std::{collections::HashSet, sync::Arc};

pub(crate) fn retrieve(context: &LaneContext<'_>) -> CcResult<LaneRun> {
    let plan = context.plan;
    let cap = context.config.grep_scan_cap;
    let limit = plan.limits().grep;
    let regex = regex::RegexBuilder::new(&regex::escape(plan.grep_query()))
        .case_insensitive(true)
        .build()
        .map_err(|e| cc_model::CcError::Search(e.to_string()))?;
    let mut state = State {
        context,
        regex,
        seen: HashSet::new(),
        matches: Vec::new(),
        scanned: 0,
    };
    let mut diagnostic = GrepDiagnostics {
        status: "partial".into(),
        scan_cap: cap,
        ..Default::default()
    };
    // Reserve part of the budget for outside-soft recall. Tiny cap=1 deliberately
    // gives the explicit/ordered hint first opportunity; the result stays partial.
    if let Some(soft) = plan.soft_chunk_scope() {
        let soft_cap = cap.div_ceil(2);
        let report = state.stage(&soft, None, soft_cap, limit)?;
        diagnostic.soft_scanned = report.decoded;
        diagnostic
            .stages
            .push(cc_model::retrieval_cost::GrepStageWork {
                stage: "soft".into(),
                work: report.work,
            });
        diagnostic.skipped_duplicates += report.skipped;
    }
    let hard = plan.chunk_scope();
    if state.scanned < cap && state.matches.len() < limit {
        if let Some(phrase) = grep_prefilter_phrase(plan.grep_query()) {
            let reserved = (cap - state.scanned).div_ceil(2);
            let before = state.scanned;
            match state.stage(&hard, Some(&phrase), reserved, limit) {
                Ok(report) => {
                    diagnostic.skipped_duplicates += report.skipped;
                    diagnostic
                        .stages
                        .push(cc_model::retrieval_cost::GrepStageWork {
                            stage: "prefilter".into(),
                            work: report.work,
                        });
                }
                Err(_) => diagnostic.prefilter_failed = true,
            }
            // Even a partially failing prefilter cannot reset budget or forget ids.
            diagnostic.prefilter_scanned = state.scanned - before;
        }
    }
    let mut exhausted = false;
    if state.matches.len() < limit {
        // A zero remaining cap still permits a metadata-only end-of-cursor probe.
        let remaining = cap.saturating_sub(state.scanned);
        let report = state.stage(&hard, None, remaining, limit)?;
        diagnostic.fallback_scanned = report.decoded;
        diagnostic
            .stages
            .push(cc_model::retrieval_cost::GrepStageWork {
                stage: "fallback".into(),
                work: report.work,
            });
        diagnostic.skipped_duplicates += report.skipped;
        exhausted = report.exhausted;
    }
    diagnostic.scanned = state.scanned;
    if exhausted && !diagnostic.prefilter_failed {
        diagnostic.status = "complete".into();
    } else if state.matches.len() >= limit && !diagnostic.prefilter_failed {
        diagnostic.status = "limited".into();
        diagnostic.reason = Some("candidate_limit".into());
    } else {
        diagnostic.reason = Some(
            if diagnostic.prefilter_failed {
                "prefilter_error"
            } else {
                "scan_cap"
            }
            .into(),
        );
    }
    // Keep the original recency merge when unscoped; hard file lists retain their
    // stable probe order. Soft priority controls admission under a bounded budget.
    if !plan.has_file_scope() {
        state
            .matches
            .sort_by_key(|(rowid, _)| std::cmp::Reverse(*rowid));
    }
    let hits: Vec<(String, f64)> = state
        .matches
        .into_iter()
        .enumerate()
        .map(|(rank, (_, cid))| (cid, 1.0 / (rank + 1) as f64))
        .collect();
    let complete = diagnostic.status == "complete";
    let truncation_reason = diagnostic.reason.clone();
    let candidate_count = hits.len();
    Ok(LaneRun {
        hits,
        exact_ids: HashSet::new(),
        status: if complete {
            LaneStatus::Complete
        } else {
            LaneStatus::Partial
        },
        coverage: if complete {
            LaneCoverage::complete(Some(diagnostic.scanned), candidate_count)
        } else {
            LaneCoverage::partial(Some(diagnostic.scanned), candidate_count)
        },
        truncation_reason,
        grep: Some(diagnostic),
        lexical_work: Default::default(),
    })
}
struct State<'a, 'b> {
    context: &'a LaneContext<'b>,
    regex: regex::Regex,
    seen: HashSet<String>,
    matches: Vec<(i64, String)>,
    scanned: usize,
}
impl State<'_, '_> {
    fn stage(
        &mut self,
        scope: &cc_db::ChunkScope,
        phrase: Option<&str>,
        cap: usize,
        limit: usize,
    ) -> CcResult<cc_db::GrepScanReport> {
        self.context.plan.control().check()?;
        // Snapshot at the bounded stage boundary; maximum size is the decode cap.
        let already_seen = self.seen.clone();
        self.context
            .db
            .retrieval()
            .scan_grep_stage(scope, phrase, &already_seen, cap, |row| {
                if self.context.plan.control().check().is_err() {
                    return false;
                }
                self.scanned += 1;
                self.seen.insert(row.chunk_id.clone());
                let lang = crate::plan::parse_language_name(&row.language_name);
                if self.context.plan.passes_filters(&row.file_path, lang)
                    && self.regex.is_match(&row.text)
                {
                    let stored = self.context.plan.control().publish(|| {
                        if let Ok(mut cache) = self.context.chunk_text_cache.lock() {
                            cache.put(
                                (self.context.cache_epoch, row.chunk_id.clone()),
                                Arc::from(row.text.as_str()),
                            );
                        }
                    });
                    if stored.is_err() {
                        return false;
                    }
                    self.matches.push((row.rowid, row.chunk_id));
                }
                self.matches.len() < limit
            })
    }
}
