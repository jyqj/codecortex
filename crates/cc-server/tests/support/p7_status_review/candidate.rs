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
    snapshot_observing(project, db, config, services, || {})
}

// The observer is an internal seam for deterministic publication interleavings.
fn snapshot_observing(
    project: Option<&Path>,
    db: Option<&IndexDb>,
    config: Option<&ProjectConfig>,
    services: &QueryServices,
    mut observed_root: impl FnMut(),
) -> Value {
    let strategy = config.map(|c| c.query.strategy).unwrap_or_default();
    let attached = services.semantic().is_some();
    let wired = services.semantic_wired();
    let query_opt_in = config.is_some_and(|c| c.semantic.allow_query_network);
    let query_reason = if project.is_none() {
        Some("no_project")
    } else if db.is_none() {
        Some("closed")
    } else if !config.is_some_and(|c| c.semantic.enabled) {
        Some("semantic_disabled")
    } else if !config.is_some_and(|c| c.semantic.network_opt_in) {
        Some("network_opt_in_required")
    } else if !query_opt_in {
        Some("query_network_opt_in_required")
    } else if !cfg!(feature = "semantic-http") {
        Some("semantic_http_feature_required")
    } else if !services.query_encoding_active() {
        Some("query_encoder_not_attached")
    } else {
        None
    };
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
            "query_encoding":{"configured_opt_in":query_opt_in,"network_authorized":query_reason.is_none(),"reason":query_reason,"request_scope":"nonlocal_nonempty_only"},
            "default_strategy":strategy,
            "default_effective_strategy":if strategy==RetrievalStrategy::Semantic && !attached {None}else if strategy==RetrievalStrategy::Local || !attached {Some(RetrievalStrategy::Local)}else{Some(strategy)},
            "default_query_state":if strategy==RetrievalStrategy::Semantic && !attached{"semantic_unavailable"}else{"requires_index"},
            "diagnostic_sources":{"build":"BuildExplain on index response","graph":"GraphExplain on query response","lanes":"evidence_summary.retrieval.lanes or lane_receipts","semantic":"evidence_summary.retrieval.semantic (dense lane receipt projection)","source":"evidence_summary.source_freshness","selection":"evidence_summary.selection","packing":"evidence_summary.packing"},
            "query_pins":services.query_pins(),"pin_scope":"owned_query_views_including_inflight_clones_not_request_count",
            "execution":services.pool.stats(),"execution_scope":"process_shared_not_project_local"
        }
    });
    let (Some(project), Some(db)) = (project, db) else {
        // No live index: an attached+wired port cannot be verified against
        // anything — keep the conservative `port_attached_unverified`
        // wording, and stop claiming "publication not implemented" (the
        // wiring exists; the index is what is absent).
        if attached && wired.is_some() {
            result["retrieval"]["dense_reason"] = json!(null);
        }
        apply_semantic_degradation(&mut result, services, attached);
        return result;
    };
    // P7-014 real state machine: an attached port that came from a
    // successful composition-root wiring reports the true lifecycle states
    // (`backfilling` while the active space has live outbox work, `ready`
    // once the dense publication covers the eligible set). A bare attached
    // port (host-side injection without wiring) keeps the honest
    // `port_attached_unverified` wording; an unwired port keeps
    // `not_configured` (V18 口径零漂移). Read-only DB reads, best-effort:
    // a failed read keeps the conservative unverified wording.
    let observed = (|| -> CcResult<_> {
        for _ in 0..3 {
            let before = db.reads().read_generation()?;
            let stats = db.reads().stats(project)?;
            let freshness = db.reads().resolution_freshness()?;
            if db.reads().read_generation()? != before {
                continue;
            }
            observed_root();
            let mut semantic = result.clone();
            if attached && wired.is_some() {
                apply_semantic_wired(&mut semantic, db);
                #[cfg(feature = "semantic")]
                if let Some(wired_info) = wired.as_ref() {
                    apply_configured_space_state(&mut semantic, db, wired_info);
                }
                if let Some(reason) = services
                    .semantic_worker()
                    .and_then(|state| state.failure_reason())
                {
                    semantic["retrieval"]["semantic_state"] = json!("failed");
                    semantic["retrieval"]["semantic_worker_reason"] = json!(reason);
                }
            }
            // Coverage may perform its own retry, so its snapshot and all
            // semantic/configured-space reads must remain inside this outer
            // fence. Never combine an earlier root epoch with later readiness.
            if db.reads().read_generation()? == before {
                return Ok((before, stats, freshness, semantic));
            }
        }
        Err(CcError::RetrievalChanged { attempts: 3 })
    })();
    match observed {
        Ok((generation, stats, freshness, semantic)) => {
            result = semantic;
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
    apply_semantic_degradation(&mut result, services, attached);
    result
}

#[cfg(feature = "semantic")]
fn apply_configured_space_state(
    result: &mut Value,
    db: &IndexDb,
    wired: &crate::service_factory::SemanticWiredInfo,
) {
    let expected = cc_semantic::types::VectorSpace::new(&wired.model_id, wired.dimensions)
        .and_then(|space| space.digest());
    if let (Ok(expected), Ok(Some(active))) = (expected, db.semantic_active_space()) {
        if active != expected.as_str() {
            result["retrieval"]["semantic_state"] = json!("backfilling");
            result["retrieval"]["dense_state"] = json!("partial");
            result["retrieval"]["dense_reason"] = json!("semantic_configured_space_pending");
            result["retrieval"]["dense_published"] = json!(0);
        }
    }
}

/// The wired-port state projection (P7-014). `semantic_state`:
/// `backfilling` while the active space has live (`pending`/`claimed`)
/// outbox work, else `ready`; live counts land in additive
/// `semantic_pending`/`semantic_failed` fields. `dense_state`: `ready` when
/// the active-space publication covers the eligible document set,
/// `partial` otherwise, with `dense_published`/`dense_desired` counts and a
/// stable `dense_reason` naming the gap. Never a ready impersonation: the
/// unwired and bare-attached wordings are byte-identical to pre-P7-014.
fn apply_semantic_wired(result: &mut Value, db: &IndexDb) {
    let pending = db.reads().semantic_outbox_pending();
    let coverage = db.reads().semantic_coverage();
    let (Ok(pending), Ok(snapshot)) = (pending, coverage) else {
        return; // conservative: keep `port_attached_unverified` / `disabled`
    };
    let coverage = snapshot.coverage;
    if coverage.reason == Some(cc_db::semantic_coverage::ZeroEligibleReason::SemanticNotConfigured)
    {
        result["retrieval"]["dense_reason"] = json!("semantic_no_active_space");
        return;
    }
    result["retrieval"]["semantic_state"] = json!(if pending > 0 {
        "backfilling"
    } else if coverage.failed > 0 {
        "failed"
    } else if coverage.uncovered > 0 {
        "backfilling"
    } else {
        "ready"
    });
    result["retrieval"]["semantic_pending"] = json!(pending);
    result["retrieval"]["semantic_failed"] = json!(coverage.failed);
    if coverage.eligible > 0 && coverage.uncovered == 0 {
        result["retrieval"]["dense_state"] = json!("ready");
        result["retrieval"]["dense_reason"] = json!(null);
    } else {
        result["retrieval"]["dense_state"] = json!("partial");
        result["retrieval"]["dense_reason"] = json!(if coverage.uncovered > 0 {
            "semantic_coverage_uncovered"
        } else {
            "semantic_no_eligible_documents"
        });
    }
    result["retrieval"]["dense_published"] = json!(coverage.published);
    result["retrieval"]["dense_desired"] = json!(coverage.eligible);
}

/// P6-018 degraded 透出 (P6-012 deviation-1 hand-over): when the optional
/// semantic subsystem is attached AND its degradation ledger reports
/// `degraded` (a corrupt cache artifact was detected, or the re-embed
/// budget ran out), `semantic_state` becomes `"degraded"` and the stable
/// reasons are listed under `degraded_reason` — taking precedence over the
/// P7-014 wired `ready`/`backfilling` states (a degraded cache is never
/// reported ready). Plain cache misses never degrade (cold-cache is
/// normal); an unattached port keeps `"not_configured"`; an attached port
/// without wiring evidence keeps `"port_attached_unverified"`. Read-only:
/// no cache path is probed here.
fn apply_semantic_degradation(result: &mut Value, services: &QueryServices, attached: bool) {
    if !attached {
        return;
    }
    let Some(degradation) = services.semantic_degradation() else {
        return;
    };
    if !degradation.degraded {
        return;
    }
    result["retrieval"]["semantic_state"] = json!("degraded");
    result["retrieval"]["degraded_reason"] = json!(degradation.degraded_reasons);
}

#[path = "checks.rs"]
mod independent;
