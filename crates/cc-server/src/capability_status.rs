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
    #[test]
    fn query_network_authorization_is_separate_and_requires_live_encoder() {
        let dir = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&dir.path().join("query-policy.sqlite3"))
            .unwrap()
            .0;
        let services = QueryServices::default();
        let mut config = ProjectConfig::default();
        let view = |config: &ProjectConfig| {
            snapshot(Some(dir.path()), Some(&db), Some(config), &services)["retrieval"]
                ["query_encoding"]
                .clone()
        };
        assert_eq!(view(&config)["configured_opt_in"], false);
        config.semantic.enabled = true;
        config.semantic.network_opt_in = true;
        assert_eq!(view(&config)["network_authorized"], false);
        assert_eq!(view(&config)["reason"], "query_network_opt_in_required");
        config.semantic.allow_query_network = true;
        assert_eq!(view(&config)["configured_opt_in"], true);
        assert_eq!(view(&config)["network_authorized"], false);
        assert_eq!(
            view(&config)["reason"],
            if cfg!(feature = "semantic-http") {
                "query_encoder_not_attached"
            } else {
                "semantic_http_feature_required"
            }
        );
        #[cfg(feature = "semantic")]
        {
            let retired = std::sync::Arc::new(cc_db::semantic_publish::LifecycleFence::default());
            let live = std::sync::Arc::new(cc_db::semantic_publish::LifecycleFence::default());
            services.set_query_encoding_lifecycle(Some(retired.clone()));
            services.set_query_encoding_lifecycle(Some(live.clone()));
            retired.close();
            assert_eq!(
                view(&config)["network_authorized"],
                cfg!(feature = "semantic-http")
            );
            live.close();
            assert_eq!(view(&config)["network_authorized"], false);
        }
    }
    #[cfg(feature = "semantic")]
    #[test]
    fn old_active_space_cannot_report_configured_new_model_ready() {
        let dir = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&dir.path().join("space-status.db"))
            .unwrap()
            .0;
        let old = cc_semantic::types::VectorSpace::new("synthetic/old", 2)
            .unwrap()
            .digest()
            .unwrap();
        db.register_semantic_space(old.as_str(), "{}").unwrap();
        db.switch_semantic_active_space(old.as_str(), "old-config")
            .unwrap();
        let mut result = json!({"retrieval":{"semantic_state":"ready","dense_state":"ready","dense_published":3}});
        apply_configured_space_state(
            &mut result,
            &db,
            &crate::service_factory::SemanticWiredInfo {
                model_id: "synthetic/new".into(),
                dimensions: 2,
            },
        );
        assert_eq!(result["retrieval"]["semantic_state"], "backfilling");
        assert_eq!(
            result["retrieval"]["dense_reason"],
            "semantic_configured_space_pending"
        );
        assert_eq!(result["retrieval"]["dense_published"], 0);
    }
    #[cfg(feature = "semantic")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_worker_publication_between_status_reads_never_mixes_epochs() {
        use cc_semantic::{
            ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
            providers::fake::{FakeProvider, FakeProviderConfig},
        };
        use std::sync::{Arc, Condvar, Mutex};
        use std::time::{Duration, Instant};
        struct Held {
            inner: FakeProvider,
            entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
            release: Arc<(Mutex<bool>, Condvar)>,
        }
        impl EmbeddingProvider for Held {
            fn space(&self) -> &cc_semantic::types::VectorSpace {
                self.inner.space()
            }
            fn embed_documents(
                &self,
                inputs: &[DocumentInput],
            ) -> Result<Vec<Vec<f32>>, ProviderError> {
                if let Some(tx) = self.entered.lock().unwrap().take() {
                    tx.send(()).unwrap();
                }
                let (lock, wake) = &*self.release;
                let mut released = lock.lock().unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !*released {
                    assert!(Instant::now() < deadline);
                    released = wake
                        .wait_timeout(released, Duration::from_millis(5))
                        .unwrap()
                        .0;
                }
                self.inner.embed_documents(inputs)
            }
            fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
                self.inner.embed_queries(inputs)
            }
        }
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0 .0.lock().unwrap() = true;
                self.0 .1.notify_all();
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut config = ProjectConfig::default();
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/status-interleave".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        std::fs::write(
            dir.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("one.rs"), "pub fn one() -> u32 { 947 }\n").unwrap();
        let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
        index.build_index(false).unwrap();
        let handle = index.query_handle().unwrap();
        let subsystem = Arc::new(
            crate::semantic_wiring::assemble_with(
                &dir.path().to_string_lossy(),
                &config,
                handle.db.clone(),
                |key| {
                    (key == cc_semantic::cache::CACHE_ROOT_ENV)
                        .then(|| dir.path().join("cache").to_string_lossy().into_owned())
                },
                false,
            )
            .unwrap()
            .unwrap(),
        );
        crate::semantic_wiring::attach(&handle.services, &subsystem);
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let guard = Release(release.clone());
        let worker = crate::semantic_runtime::SemanticRuntime::new(
            handle.db.clone(),
            subsystem.clone(),
            handle.services.clone(),
            Arc::new(Held {
                inner: FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone())),
                entered: Mutex::new(Some(entered)),
                release: release.clone(),
            }),
        )
        .unwrap();
        assert!(worker.schedule());
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap();
        let before = handle.db.reads().read_generation().unwrap();
        assert_eq!(
            handle
                .db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .published,
            0
        );
        let mut reads = 0;
        let status = snapshot_observing(
            Some(dir.path()),
            Some(&handle.db),
            Some(&config),
            &handle.services,
            || {
                reads += 1;
                if reads != 1 {
                    return;
                }
                *release.0.lock().unwrap() = true;
                release.1.notify_all();
                let deadline = Instant::now() + Duration::from_secs(5);
                while {
                    let coverage = handle.db.reads().semantic_coverage().unwrap().coverage;
                    coverage.published == 0 || coverage.uncovered != 0
                } {
                    assert!(
                        Instant::now() < deadline,
                        "real worker must commit within original bound: coverage={:?}, worker={:?}",
                        handle.db.reads().semantic_coverage().unwrap(),
                        handle
                            .services
                            .semantic_worker()
                            .and_then(|s| s.failure_reason())
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            },
        );
        drop(guard);
        let after = handle.db.reads().read_generation().unwrap();
        assert_eq!(after.incarnation, before.incarnation);
        assert_eq!(after.index_epoch, before.index_epoch);
        assert_eq!(after.evidence_epoch, before.evidence_epoch);
        assert!(after.semantic_epoch > before.semantic_epoch);
        assert_eq!(status["retrieval"]["dense_state"], "ready");
        assert!(status["retrieval"]["dense_published"].as_u64().unwrap() > 0);
        assert_eq!(
            status["retrieval"]["dense_published"],
            status["retrieval"]["dense_desired"]
        );
        assert_eq!(
            status["retrieval"]["generation"],
            json!(after),
            "ready must carry the actual publication epoch, never the earlier root epoch"
        );
        assert_eq!(
            reads, 2,
            "one crossed snapshot is discarded; the next consistent snapshot wins"
        );
        eprintln!(
            "STATUS_INTERLEAVE {}",
            json!({"before":before,"after":after,"returned":status["retrieval"]["generation"],"root_attempts":reads,"dense_state":status["retrieval"]["dense_state"]})
        );
        worker.close();
        index.close();
    }
    #[cfg(feature = "semantic")]
    #[test]
    fn status_churn_has_three_attempt_ceiling_and_no_ready_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("one.rs");
        std::fs::write(&file, "pub fn one() -> u32 { 731 }\n").unwrap();
        let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
        index.build_index(false).unwrap();
        let db = index.index_db().unwrap().clone();
        let services = QueryServices::default();
        let mut attempts = 0;
        let status = snapshot_observing(Some(dir.path()), Some(&db), None, &services, || {
            attempts += 1;
            std::fs::write(
                &file,
                format!("pub fn one() -> u32 {{ {} }}\n", 947 + attempts),
            )
            .unwrap();
            index.build_index(false).unwrap();
        });
        assert_eq!(attempts, 3);
        assert_eq!(status["retrieval"]["index_state"], "error");
        assert_eq!(status["retrieval"]["default_query_state"], "unavailable");
        assert_eq!(status["retrieval"]["error"]["retryable"], true);
        assert!(status["retrieval"]["generation"].is_null());
        assert_eq!(status["retrieval"]["dense_state"], "disabled");
    }
}
