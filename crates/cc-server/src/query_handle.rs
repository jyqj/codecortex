//! Arc-owned query view. No CodeIndex guard or read-pool connection crosses await.
use crate::{
    handlers::SharedCodeIndex,
    service_factory::{query_pool, QueryServices},
};
use cc_db::index_db::IndexDb;
use cc_model::{
    config::RepoSizeTier,
    context::ContextEnvelope,
    query::{QueryConfig, QueryControl, RetrievalStrategy},
    search::SearchRequest,
    semantic::SemanticRequest,
    CcResult, Intent,
};
use cc_search::{query_policy::QueryPolicy, SearchEngine};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone)]
pub struct QueryHandle {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) engine: Arc<SearchEngine>,
    pub(crate) project: PathBuf,
    pub(crate) tier: RepoSizeTier,
    pub(crate) config: QueryConfig,
    pub(crate) services: Arc<QueryServices>,
    pub(crate) _pin: Arc<crate::service_factory::QueryPin>,
    // Keep a routed runtime alive across LRU eviction; no lock is retained.
    pub(crate) _runtime: Option<SharedCodeIndex>,
}
impl QueryHandle {
    /// Only clone owned inputs while holding the lock. All SQL happens later.
    pub fn capture(runtime: &SharedCodeIndex) -> CcResult<Self> {
        let mut handle = crate::handlers::lock_index(runtime)?.query_handle()?;
        handle._runtime = Some(runtime.clone());
        Ok(handle)
    }
    pub fn project_path(&self) -> &std::path::Path {
        &self.project
    }
    pub fn config(&self) -> &QueryConfig {
        &self.config
    }
    pub fn db_instance_id(&self) -> u64 {
        self.db.admin().instance_id()
    }
    pub fn execution_stats(&self) -> cc_search::execution::ExecutionStats {
        self.services.pool.stats()
    }
    pub(crate) async fn resolution_freshness(
        &self,
        control: QueryControl,
    ) -> CcResult<cc_model::freshness::ResolutionFreshness> {
        let db = self.db.clone();
        self.services
            .pool
            .run_cpu(control, move || db.reads().resolution_freshness())
            .await
    }
    pub async fn search_async(
        &self,
        query: String,
        top_k: usize,
        intent: Option<Intent>,
        mut overrides: SearchRequest,
    ) -> CcResult<ContextEnvelope> {
        self.config.validate()?;
        self.engine.check_live()?;
        let control = match &overrides.control {
            Some(c) => c.limit_total(Duration::from_millis(self.config.deadline_ms)),
            None => QueryControl::new(Duration::from_millis(self.config.deadline_ms))?,
        };
        let mut cancel = control.cancel_on_drop();
        overrides.control = Some(control.clone());
        overrides.query = query.clone();
        overrides.intent = intent.or_else(|| Some(crate::engine::detect_intent(&query)));
        let port = self.services.semantic();
        let policy = QueryPolicy::resolve(&self.config, &overrides, port.is_some())?;
        if policy.effective != RetrievalStrategy::Local {
            let db = self.db.clone();
            let generation = self
                .services
                .pool
                .run_cpu(control.clone(), move || db.reads().read_generation())
                .await?;
            let request = SemanticRequest {
                query: query.clone(),
                scope: cc_search::query_policy::hard_scope(&overrides)?,
                limit: policy.semantic_top_k,
                policy_fingerprint: policy.fingerprint(),
                generation,
            };
            let child = control.child(Duration::from_millis(policy.semantic_timeout_ms));
            let response = if request.scope.is_empty() {
                cc_model::semantic::SemanticResponse {
                    generation: request.generation,
                    outcome: cc_model::retrieval::LaneOutcome::disabled("semantic", 1.0),
                }
            } else {
                cc_search::semantic_adapter::recall(
                    &self.services.pool,
                    port.as_deref().expect("resolved semantic port"),
                    request,
                    child,
                )
                .await?
            };
            overrides.semantic = Some(Arc::new(response));
        } else {
            overrides.semantic = None;
        }
        let handle = self.clone();
        let result = self
            .services
            .pool
            .run_cpu(control.clone(), move || {
                handle.search_in_context_with(&query, top_k, intent, overrides)
            })
            .await?;
        control.check()?;
        self.engine.check_live()?;
        cancel.disarm();
        Ok(result)
    }
    pub fn task_symbols(
        &self,
        task: &str,
        max_symbols: Option<usize>,
        expand_depth: Option<usize>,
        intent: Option<&str>,
    ) -> CcResult<serde_json::Value> {
        match self.task_symbols_direct(task, max_symbols, expand_depth, intent)? {
            Some(value) => Ok(value),
            None => Ok(serde_json::to_value(self.search_in_context(
                task,
                10,
                intent.and_then(|s| s.parse().ok()),
            )?)?),
        }
    }
}
/// Admission and the total clock precede lock acquisition, so a waiting caller
/// cannot accumulate unbounded spawn_blocking tasks outside the executor.
pub async fn capture_for_request(
    runtime: SharedCodeIndex,
    supplied: Option<QueryControl>,
) -> CcResult<(QueryHandle, QueryControl)> {
    let control = match supplied {
        Some(c) => c,
        None => QueryControl::new(Duration::from_secs(600))?,
    };
    let handle = query_pool()
        .run_cpu(control.clone(), move || QueryHandle::capture(&runtime))
        .await?;
    handle.config.validate()?;
    let control = control.limit_total(Duration::from_millis(handle.config.deadline_ms));
    control.check()?;
    Ok((handle, control))
}
