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
        for lane in crate::lanes::default_lane_registry() {
            let lane_id = lane.lane_id();
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

    #[test]
    fn obligations_follow_execution_order_with_intent_roles_and_budget_caps() {
        use RetrievalStrategy::{Auto, Local, Semantic};

        let execution_lanes = crate::lanes::default_lanes();
        let execution_ids: Vec<_> = execution_lanes.iter().map(|lane| lane.lane_id()).collect();
        // These roles are the published intent contract, in the existing
        // execution/fusion order guarded by the engine registry test.
        let intent_cases = [
            (
                Intent::Locate,
                ["identity", "identity", "source", "source", "supporting"],
            ),
            (
                Intent::Fix,
                ["supporting", "supporting", "source", "source", "structural"],
            ),
            (
                Intent::Refactor,
                ["supporting", "supporting", "source", "source", "structural"],
            ),
            (
                Intent::Trace,
                ["supporting", "supporting", "source", "source", "structural"],
            ),
            (
                Intent::Test,
                ["supporting", "supporting", "source", "source", "structural"],
            ),
            (
                Intent::Patch,
                ["supporting", "supporting", "source", "source", "supporting"],
            ),
            (
                Intent::Explain,
                ["supporting", "supporting", "source", "source", "supporting"],
            ),
            (
                Intent::Default,
                ["supporting", "supporting", "source", "source", "supporting"],
            ),
        ];
        // None exercises the configured strategy; explicit Local must suppress
        // a configured semantic port, while unconfigured Auto stays local.
        let strategy_cases = [
            (None, false, Local, "not_configured", false),
            (None, true, Auto, "configured", true),
            (Some(Local), false, Local, "disabled", false),
            (Some(Local), true, Local, "disabled", false),
            (Some(Auto), false, Local, "not_configured", false),
            (Some(Auto), true, Auto, "configured", true),
            (Some(Semantic), true, Semantic, "configured", true),
        ];
        for (lane_budget, semantic_budget, expected_local_cap, expected_semantic_cap) in
            [(250, 40, 100, 40), (40, 250, 40, 100)]
        {
            let config = QueryConfig {
                strategy: Auto,
                deadline_ms: 100,
                lane_timeout_ms: lane_budget,
                semantic_timeout_ms: semantic_budget,
                semantic_top_k: 7,
            };
            for (intent, expected_roles) in intent_cases {
                for (requested, configured, effective, semantic_state, has_semantic) in
                    strategy_cases
                {
                    let request = SearchRequest {
                        retrieval_strategy: requested,
                        intent: Some(intent),
                        ..Default::default()
                    };
                    let policy = QueryPolicy::resolve(&config, &request, configured).unwrap();
                    assert_eq!(policy.effective, effective);
                    assert_eq!(policy.semantic_state, semantic_state);
                    assert_eq!(policy.lane_timeout_ms, expected_local_cap);
                    assert_eq!(policy.semantic_timeout_ms, expected_semantic_cap);
                    assert_eq!(
                        policy.obligations.len(),
                        execution_ids.len() + usize::from(has_semantic)
                    );
                    let local = &policy.obligations[..execution_ids.len()];
                    assert_eq!(
                        local.iter().map(|lane| lane.lane_id).collect::<Vec<_>>(),
                        execution_ids
                    );
                    assert_eq!(
                        local.iter().map(|lane| lane.role).collect::<Vec<_>>(),
                        expected_roles
                    );
                    assert!(local
                        .iter()
                        .all(|lane| lane.timeout_ms == expected_local_cap));
                    if has_semantic {
                        let semantic = policy.obligations.last().unwrap();
                        assert_eq!(semantic.lane_id, "semantic");
                        assert_eq!(semantic.role, "optional_recall");
                        assert_eq!(semantic.timeout_ms, expected_semantic_cap);
                    }
                }
            }
        }
    }

    #[test]
    fn registry_cleanup_preserves_policy_wire_and_fingerprint() {
        // Frozen complete serialization from the pre-cleanup policy contract.
        // Do not build expected fields or IDs from the new registry: order,
        // names, roles and capped budgets are part of this fingerprint.
        const LOCAL: &str = concat!(
            r#"{"version":"query-policy-local-semantic-canonical-path-domain-v4","#,
            r#""graph_source_mapping":"uid_byte_declaration_document; complete_mapping_is_not_whole_symbol_body_coverage","#,
            r#""path_source_domain":"canonical_scoped_existing_path_docs; else_bounded_tokens; not_whole_file_coverage","#,
            r#""requested":"local","#,
            r#""effective":"local","#,
            r#""intent":"trace","#,
            r#""deadline_ms":100,"#,
            r#""lane_timeout_ms":100,"#,
            r#""semantic_timeout_ms":40,"#,
            r#""semantic_top_k":7,"#,
            r#""semantic_state":"disabled","#,
            r#""obligations":[{"lane_id":"exact_symbol","role":"supporting","timeout_ms":100},{"lane_id":"path","role":"supporting","timeout_ms":100},{"lane_id":"lexical","role":"source","timeout_ms":100},{"lane_id":"grep","role":"source","timeout_ms":100},{"lane_id":"graph","role":"structural","timeout_ms":100}],"#,
            r#""completeness":"all_executed_lane_limits_remain_visible; obligation_role_does_not_erase_partial","#,
            r#""no_answer":"empty_partial_is_not_absence; no_semantic_absence_claim_from_weak_rank"}"#,
        );
        const AUTO: &str = concat!(
            r#"{"version":"query-policy-local-semantic-canonical-path-domain-v4","#,
            r#""graph_source_mapping":"uid_byte_declaration_document; complete_mapping_is_not_whole_symbol_body_coverage","#,
            r#""path_source_domain":"canonical_scoped_existing_path_docs; else_bounded_tokens; not_whole_file_coverage","#,
            r#""requested":"auto","#,
            r#""effective":"auto","#,
            r#""intent":"trace","#,
            r#""deadline_ms":100,"#,
            r#""lane_timeout_ms":100,"#,
            r#""semantic_timeout_ms":40,"#,
            r#""semantic_top_k":7,"#,
            r#""semantic_state":"configured","#,
            r#""obligations":[{"lane_id":"exact_symbol","role":"supporting","timeout_ms":100},{"lane_id":"path","role":"supporting","timeout_ms":100},{"lane_id":"lexical","role":"source","timeout_ms":100},{"lane_id":"grep","role":"source","timeout_ms":100},{"lane_id":"graph","role":"structural","timeout_ms":100},{"lane_id":"semantic","role":"optional_recall","timeout_ms":40}],"#,
            r#""completeness":"all_executed_lane_limits_remain_visible; obligation_role_does_not_erase_partial","#,
            r#""no_answer":"empty_partial_is_not_absence; no_semantic_absence_claim_from_weak_rank"}"#,
        );
        let config = QueryConfig {
            strategy: RetrievalStrategy::Auto,
            deadline_ms: 100,
            lane_timeout_ms: 250,
            semantic_timeout_ms: 40,
            semantic_top_k: 7,
        };
        for (requested, expected) in [(Some(RetrievalStrategy::Local), LOCAL), (None, AUTO)] {
            let request = SearchRequest {
                retrieval_strategy: requested,
                intent: Some(Intent::Trace),
                ..Default::default()
            };
            let policy = QueryPolicy::resolve(&config, &request, true).unwrap();
            assert_eq!(serde_json::to_string(&policy).unwrap(), expected);
            assert_eq!(
                policy.fingerprint(),
                cc_model::identity::bytes_hash(expected.as_bytes())
            );
        }
    }
}
