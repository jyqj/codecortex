//! Capability availability is not query coverage or a dense-readiness claim.
//! Read existing metadata only; never probe a provider or start indexing here.
use crate::service_factory::QueryServices;
use cc_db::index_db::IndexDb;
use cc_model::{config::ProjectConfig, query::RetrievalStrategy, CcError, CcResult};
use serde_json::{json, Value};
use std::path::Path;

pub const CAPABILITY_SPEC: &str = "retrieval-capabilities-v1";

pub(crate) fn snapshot(
    project: Option<&Path>,
    db: Option<&IndexDb>,
    config: Option<&ProjectConfig>,
    services: &QueryServices,
) -> Value {
    let strategy = config.map(|c| c.query.strategy).unwrap_or_default();
    let attached = services.semantic().is_some();
    let mut result = json!({
        "has_project":project.is_some(),"has_index":false,
        "indexed_files":null,"indexed_symbols":null,
        "capabilities":{"search":false,"graph":false,"impact":false},
        "retrieval":{
            "spec":CAPABILITY_SPEC,
            "index_state":if project.is_none(){"no_project"}else{"closed"},
            "local_state":"unavailable",
            "semantic_state":if attached{"port_attached_unverified"}else{"not_configured"},
            "dense_state":"disabled","dense_reason":"provider_and_vector_publication_not_implemented",
            "generation":null,"resolution_freshness":null,
            "query_coverage":{"state":"not_measured","scope":"per_query_not_global"},
            "default_strategy":strategy,
            "default_effective_strategy":if strategy==RetrievalStrategy::Semantic && !attached {None}else if strategy==RetrievalStrategy::Local || !attached {Some(RetrievalStrategy::Local)}else{Some(strategy)},
            "default_query_state":if strategy==RetrievalStrategy::Semantic && !attached{"semantic_unavailable"}else{"requires_index"},
            "diagnostic_sources":{"build":"BuildExplain on index response","graph":"GraphExplain on query response","lanes":"evidence_summary.retrieval.lanes or lane_receipts","source":"evidence_summary.source_freshness","selection":"evidence_summary.selection","packing":"evidence_summary.packing"},
            "query_pins":services.query_pins(),"pin_scope":"owned_query_views_including_inflight_clones_not_request_count",
            "execution":services.pool.stats(),"execution_scope":"process_shared_not_project_local"
        }
    });
    let (Some(project), Some(db)) = (project, db) else {
        return result;
    };
    let observed = (|| -> CcResult<_> {
        for _ in 0..3 {
            let before = db.reads().read_generation()?;
            let stats = db.reads().stats(project)?;
            let freshness = db.reads().resolution_freshness()?;
            if db.reads().read_generation()? == before {
                return Ok((before, stats, freshness));
            }
        }
        Err(CcError::RetrievalChanged { attempts: 3 })
    })();
    match observed {
        Ok((generation, stats, freshness)) => {
            result["has_index"] = json!(true);
            result["indexed_files"] = json!(stats.indexed_files);
            result["indexed_symbols"] = json!(stats.indexed_symbols);
            result["capabilities"] = json!({"search":true,"graph":true,"impact":true});
            result["retrieval"]["index_state"] = json!(if stats.indexed_files == 0 {
                "empty"
            } else {
                "available"
            });
            result["retrieval"]["local_state"] = json!(if freshness.complete {
                "available"
            } else {
                "resolution_pending"
            });
            result["retrieval"]["generation"] = json!(generation);
            result["retrieval"]["resolution_freshness"] = json!(freshness);
            if strategy != RetrievalStrategy::Semantic || attached {
                result["retrieval"]["default_query_state"] =
                    json!("available_with_per_query_checks");
            }
        }
        Err(error) => {
            result["retrieval"]["index_state"] = json!("error");
            result["retrieval"]["default_query_state"] = json!("unavailable");
            result["retrieval"]["error"] = json!({"message":error.to_string().chars().take(512).collect::<String>(),"retryable":matches!(error,CcError::RetrievalChanged{..})});
        }
    }
    if let Some(error) = config.and_then(|c| c.query.validate().err()) {
        result["retrieval"]["default_query_state"] = json!("invalid_config");
        result["retrieval"]["local_state"] = json!("invalid_config");
        result["retrieval"]["config_error"] = json!(error.to_string());
    }
    result
}
