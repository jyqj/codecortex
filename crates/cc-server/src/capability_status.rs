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
    let wired = services.semantic_wired();
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
    // P7-014 real state machine: an attached port that came from a
    // successful composition-root wiring reports the true lifecycle states
    // (`backfilling` while the active space has live outbox work, `ready`
    // once the dense publication covers the eligible set). A bare attached
    // port (host-side injection without wiring) keeps the honest
    // `port_attached_unverified` wording; an unwired port keeps
    // `not_configured` (V18 口径零漂移). Read-only DB reads, best-effort:
    // a failed read keeps the conservative unverified wording.
    if attached && wired.is_some() {
        apply_semantic_wired(&mut result, db);
    }
    apply_semantic_degradation(&mut result, services, attached);
    result
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_factory::{QueryServices, SemanticDegradation};

    fn degraded(reasons: &[&str]) -> SemanticDegradation {
        SemanticDegradation {
            degraded: true,
            degraded_reasons: reasons.iter().map(|s| s.to_string()).collect(),
            corrupt_events: 1,
            quarantined_objects: 1,
            reembeds_used: 1,
            reembed_budget: Some(1),
            budget_exhausted: true,
        }
    }

    fn retrieval(json: &Value) -> &Value {
        &json["retrieval"]
    }

    #[test]
    fn stale_wiring_metadata_cannot_claim_an_unattached_port_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&dir.path().join("index.sqlite3")).unwrap().0;
        let services = QueryServices::default();
        services.set_semantic_wired(Some(crate::service_factory::SemanticWiredInfo {
            model_id: "fake/model".into(),
            dimensions: 2,
        }));
        let result = snapshot(Some(dir.path()), Some(&db), None, &services);
        assert_eq!(result["retrieval"]["semantic_state"], "not_configured");
        assert_eq!(result["retrieval"]["dense_state"], "disabled");
    }

    #[test]
    fn wired_status_requires_an_active_space_and_reports_publication_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.sqlite3");
        let db = IndexDb::open(&path).unwrap().0;
        let conn = rusqlite::Connection::open(&path).unwrap();
        let base = || {
            json!({"retrieval": {
                "semantic_state": "port_attached_unverified", "dense_state": "disabled"
            }})
        };
        let mut result = base();
        apply_semantic_wired(&mut result, &db);
        assert_eq!(
            result["retrieval"]["semantic_state"],
            "port_attached_unverified"
        );
        assert_eq!(result["retrieval"]["dense_state"], "disabled");
        assert_eq!(
            result["retrieval"]["dense_reason"],
            "semantic_no_active_space"
        );

        conn.execute_batch(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('active','{}','active');
             INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at)
               VALUES('src/a.rs','rust','hash',1.0,1,'2026-01-01');
             INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text)
               VALUES('chunk','src/a.rs','rust',0,1,1,'fn a() {}');
             INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json)
               VALUES('doc','v1','src/a.rs','chunk','encoding','{}','{}');"
        ).unwrap();
        let mut result = base();
        apply_semantic_wired(&mut result, &db);
        assert_eq!(result["retrieval"]["semantic_state"], "backfilling");
        assert_eq!(result["retrieval"]["dense_state"], "partial");
        assert_eq!(result["retrieval"]["dense_desired"], 1);
        assert_eq!(result["retrieval"]["dense_published"], 0);
        conn.execute_batch(
            "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at)
               VALUES('doc','v1','input','active','embed','pending',0,0,0);",
        )
        .unwrap();
        let mut result = base();
        apply_semantic_wired(&mut result, &db);
        assert_eq!(result["retrieval"]["semantic_state"], "backfilling");
        assert_eq!(result["retrieval"]["semantic_pending"], 1);
        conn.execute_batch("UPDATE semantic_outbox SET state='failed';")
            .unwrap();
        let mut result = base();
        apply_semantic_wired(&mut result, &db);
        assert_eq!(result["retrieval"]["semantic_state"], "failed");
        assert_eq!(result["retrieval"]["semantic_failed"], 1);
        assert_eq!(result["retrieval"]["dense_state"], "partial");
        conn.execute_batch("UPDATE semantic_outbox SET state='done';")
            .unwrap();

        conn.execute_batch(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,space_id,input_digest,artifact_ref,published_at,published_incarnation)
               VALUES('doc','v1','src/a.rs','encoding','active','input','artifact','2026-01-01','incarnation');"
        ).unwrap();
        let mut result = base();
        apply_semantic_wired(&mut result, &db);
        assert_eq!(result["retrieval"]["semantic_state"], "ready");
        assert_eq!(result["retrieval"]["dense_state"], "ready");
        assert!(result["retrieval"]["dense_reason"].is_null());
        assert_eq!(result["retrieval"]["dense_published"], 1);
    }

    #[test]
    fn attached_port_with_degradation_reports_degraded_state_and_reasons() {
        let services = QueryServices::default();
        services.set_semantic_degradation(Some(degraded(&[
            "corrupt cache artifacts detected: 1 event(s), 1 object(s) quarantined for diagnosis",
        ])));
        // `attached` mirrors services.semantic().is_some(); the port itself
        // is a cc-semantic type, so the attached=true branch is exercised
        // directly through the same helper snapshot() uses.
        let mut result = json!({
            "retrieval": {"semantic_state": "port_attached_unverified"}
        });
        apply_semantic_degradation(&mut result, &services, true);
        assert_eq!(retrieval(&result)["semantic_state"], "degraded");
        let reasons = retrieval(&result)["degraded_reason"].as_array().unwrap();
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0]
            .as_str()
            .unwrap()
            .contains("corrupt cache artifacts"));
    }

    #[test]
    fn unattached_healthy_or_unreported_states_are_never_degraded() {
        // Unattached: stays not_configured even if a stale snapshot exists.
        let services = QueryServices::default();
        services.set_semantic_degradation(Some(degraded(&["stale"])));
        let mut result = json!({"retrieval": {"semantic_state": "not_configured"}});
        apply_semantic_degradation(&mut result, &services, false);
        assert_eq!(retrieval(&result)["semantic_state"], "not_configured");
        assert!(retrieval(&result).get("degraded_reason").is_none());

        // Attached but healthy (no snapshot / degraded=false): unchanged.
        let healthy = QueryServices::default();
        healthy.set_semantic_degradation(Some(SemanticDegradation {
            degraded: false,
            ..SemanticDegradation::default()
        }));
        let mut result = json!({"retrieval": {"semantic_state": "port_attached_unverified"}});
        apply_semantic_degradation(&mut result, &healthy, true);
        assert_eq!(
            retrieval(&result)["semantic_state"],
            "port_attached_unverified"
        );
        let mut result = json!({"retrieval": {"semantic_state": "port_attached_unverified"}});
        apply_semantic_degradation(&mut result, &QueryServices::default(), true);
        assert_eq!(
            retrieval(&result)["semantic_state"],
            "port_attached_unverified"
        );
    }
}
