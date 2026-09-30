use std::collections::HashSet;

use cc_model::config::SearchConfig;
use cc_model::retrieval::{LaneCoverage, LaneStatus};
use cc_model::CcResult;

use super::{LaneContext, LaneRun, RetrievalLane, LANE_EXACT_SYMBOL};

pub(crate) struct ExactSymbolLane;

impl RetrievalLane for ExactSymbolLane {
    fn lane_id(&self) -> &'static str {
        LANE_EXACT_SYMBOL
    }

    fn weight(&self, config: &SearchConfig) -> f64 {
        config.exact_symbol_weight
    }

    fn is_enabled(&self, context: &LaneContext<'_>) -> bool {
        context.config.exact_symbol_weight > 0.0
            && !context.plan.is_empty_scope()
            && context.plan.exact_symbol_query().is_some()
    }

    fn annotates_hits(&self) -> bool {
        true
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        self.run_detailed(context).map(|run| run.hits)
    }

    fn run_detailed(&self, context: &LaneContext<'_>) -> CcResult<LaneRun> {
        let Some(query) = context.plan.exact_symbol_query() else {
            return Ok(LaneRun::complete(Vec::new()));
        };
        let limit = context
            .config
            .exact_symbol_top_k
            .max(context.plan.limits().top_k);
        let mut hits = context.db.retrieval().exact_symbol_chunk_hits(
            query,
            &context.plan.chunk_scope(),
            limit.saturating_add(1),
        )?;
        let lower_bound = hits.len();
        let truncated = hits.len() > limit;
        hits.truncate(limit);
        let exact_ids: HashSet<String> = hits.iter().map(|(id, _)| id.clone()).collect();
        Ok(LaneRun {
            status: if truncated {
                LaneStatus::Partial
            } else {
                LaneStatus::Complete
            },
            coverage: if truncated {
                LaneCoverage::partial(None, lower_bound)
            } else {
                LaneCoverage::complete(None, hits.len())
            },
            truncation_reason: truncated.then(|| "candidate_limit".into()),
            exact_ids,
            hits,
            grep: None,
            lexical_work: Default::default(),
        })
    }
}
