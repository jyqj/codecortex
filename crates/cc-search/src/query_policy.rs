//! One declarative policy for strategy, intent obligations and child budgets.
use cc_model::{
    query::{QueryConfig, RetrievalStrategy},
    search::SearchRequest,
    CcError, CcResult, Intent,
};
use serde::Serialize;

pub fn hard_scope(request: &SearchRequest) -> CcResult<cc_model::retrieval::HardScope> {
    let dsl = crate::dsl::parse_search_dsl(&request.query);
    crate::scope::normalize_request(&mut request.clone(), &dsl)
}

pub const POLICY_VERSION: &str = "query-policy-local-semantic-canonical-path-domain-v4";
pub const PATH_SOURCE_DOMAIN: &str =
    "canonical_scoped_existing_path_docs; else_bounded_tokens; not_whole_file_coverage";
pub const GRAPH_SOURCE_MAPPING: &str =
    "uid_byte_declaration_document; complete_mapping_is_not_whole_symbol_body_coverage";
#[derive(Debug, Clone, Serialize)]
pub struct LaneObligation {
    pub lane_id: &'static str,
    pub role: &'static str,
    pub timeout_ms: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct QueryPolicy {
    pub version: &'static str,
    pub graph_source_mapping: &'static str,
    pub path_source_domain: &'static str,
    pub requested: RetrievalStrategy,
    pub effective: RetrievalStrategy,
    pub intent: Intent,
    pub deadline_ms: u64,
    pub lane_timeout_ms: u64,
    pub semantic_timeout_ms: u64,
    pub semantic_top_k: usize,
    pub semantic_state: &'static str,
    pub obligations: Vec<LaneObligation>,
    pub completeness: &'static str,
    pub no_answer: &'static str,
}
impl QueryPolicy {
    pub fn resolve(
        config: &QueryConfig,
        request: &SearchRequest,
        semantic_configured: bool,
    ) -> CcResult<Self> {
        config.validate()?;
        let requested = request.retrieval_strategy.unwrap_or(config.strategy);
        if requested == RetrievalStrategy::Semantic && !semantic_configured {
            return Err(CcError::SemanticUnavailable);
        }
        let effective = if requested == RetrievalStrategy::Local || !semantic_configured {
            RetrievalStrategy::Local
        } else {
            requested
        };
        let intent = request.intent.unwrap_or_default();
        let mut obligations = Vec::new();
        for lane_id in ["exact_symbol", "path", "lexical", "grep", "graph"] {
            let role = match (intent, lane_id) {
                (Intent::Locate, "exact_symbol" | "path") => "identity",
                (Intent::Trace | Intent::Refactor | Intent::Fix | Intent::Test, "graph") => {
                    "structural"
                }
                (_, "lexical" | "grep") => "source",
                _ => "supporting",
            };
            obligations.push(LaneObligation {
                lane_id,
                role,
                timeout_ms: config.lane_timeout_ms.min(config.deadline_ms),
            });
        }
        if effective != RetrievalStrategy::Local {
            obligations.push(LaneObligation {
                lane_id: "semantic",
                role: "optional_recall",
                timeout_ms: config.semantic_timeout_ms.min(config.deadline_ms),
            });
        }
        Ok(Self {
            version: POLICY_VERSION,
            graph_source_mapping: GRAPH_SOURCE_MAPPING,
            path_source_domain: PATH_SOURCE_DOMAIN,
            requested,
            effective,
            intent,
            deadline_ms: config.deadline_ms,
            lane_timeout_ms: config.lane_timeout_ms.min(config.deadline_ms),
            semantic_timeout_ms: config.semantic_timeout_ms.min(config.deadline_ms),
            semantic_top_k: config.semantic_top_k,
            semantic_state: if requested == RetrievalStrategy::Local {
                "disabled"
            } else if !semantic_configured {
                "not_configured"
            } else {
                "configured"
            },
            obligations,
            completeness:
                "all_executed_lane_limits_remain_visible; obligation_role_does_not_erase_partial",
            no_answer: "empty_partial_is_not_absence; no_semantic_absence_claim_from_weak_rank",
        })
    }
    pub fn fingerprint(&self) -> String {
        cc_model::identity::bytes_hash(&serde_json::to_vec(self).expect("finite query policy"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_without_port_is_local_and_explicit_semantic_is_unavailable() {
        let cfg = QueryConfig::default();
        let mut request = SearchRequest {
            retrieval_strategy: Some(RetrievalStrategy::Auto),
            ..Default::default()
        };
        let auto = QueryPolicy::resolve(&cfg, &request, false).unwrap();
        assert_eq!(auto.effective, RetrievalStrategy::Local);
        assert_eq!(auto.semantic_state, "not_configured");
        request.retrieval_strategy = Some(RetrievalStrategy::Semantic);
        assert!(matches!(
            QueryPolicy::resolve(&cfg, &request, false),
            Err(CcError::SemanticUnavailable)
        ));
        request.retrieval_strategy = Some(RetrievalStrategy::Local);
        assert_eq!(
            QueryPolicy::resolve(&cfg, &request, true)
                .unwrap()
                .semantic_state,
            "disabled"
        );
    }
    #[test]
    fn intent_and_budgets_are_fingerprinted_without_hiding_partial() {
        let cfg = QueryConfig::default();
        let request = SearchRequest::default();
        let a = QueryPolicy::resolve(&cfg, &request, false).unwrap();
        let b = QueryPolicy::resolve(
            &cfg,
            &SearchRequest {
                intent: Some(Intent::Trace),
                ..request
            },
            false,
        )
        .unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert!(b.completeness.contains("does_not_erase_partial"));
    }
}
