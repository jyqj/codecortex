//! Caller-driven bounded semantic work after indexing; no resident timer.
use crate::{
    semantic_wiring::{self, SemanticSubsystem},
    service_factory::QueryServices,
};
use cc_db::index_db::IndexDb;
use cc_model::CcResult;
use cc_semantic::ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};

/// Apply the query absolute deadline and cancellation to an injected HTTP
/// transport before provider assembly. Wrapping performs no network I/O.
pub fn query_transport_with_budget(
    transport: Arc<dyn cc_semantic::providers::openai_compatible::EmbeddingHttpTransport>,
    control: cc_model::query::QueryControl,
    cancellation: tokio_util::sync::CancellationToken,
) -> Arc<dyn cc_semantic::providers::openai_compatible::EmbeddingHttpTransport> {
    crate::semantic_query_encoding::query_deadline_transport(transport, control, cancellation)
}

pub struct SemanticRuntime {
    db: Arc<IndexDb>,
    subsystem: Arc<SemanticSubsystem>,
    services: Arc<QueryServices>,
    status: Arc<crate::service_factory::SemanticWorkerStatus>,
    provider: ProviderSource,
    cancellation: tokio_util::sync::CancellationToken,
    closed: AtomicBool,
    lifecycle: Arc<cc_db::semantic_publish::LifecycleFence>,
    query_encoding: Mutex<Option<Arc<crate::semantic_query_encoding::QueryEncodingContext>>>,
    running: AtomicBool,
    requested: AtomicBool,
    backfill_cursor: Mutex<Option<String>>,
    configured_transition: Mutex<Option<String>>,
}
type ProviderFactory = dyn Fn(tokio_util::sync::CancellationToken) -> CcResult<Arc<dyn EmbeddingProvider>>
    + Send
    + Sync;
enum ProviderSource {
    Injected(Arc<dyn EmbeddingProvider>),
    Factory(Arc<ProviderFactory>),
}
struct Running(Arc<SemanticRuntime>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}
impl SemanticRuntime {
    pub fn new(
        db: Arc<IndexDb>,
        subsystem: Arc<SemanticSubsystem>,
        services: Arc<QueryServices>,
        provider: Arc<dyn EmbeddingProvider>,
    ) -> CcResult<Arc<Self>> {
        if provider.space() != &subsystem.space {
            return Err(cc_model::CcError::InvalidParams(
                "worker provider space mismatch".into(),
            ));
        }
        Self::from_source(db, subsystem, services, ProviderSource::Injected(provider))
    }

    fn from_source(
        db: Arc<IndexDb>,
        subsystem: Arc<SemanticSubsystem>,
        services: Arc<QueryServices>,
        provider: ProviderSource,
    ) -> CcResult<Arc<Self>> {
        let status = Arc::new(crate::service_factory::SemanticWorkerStatus::default());
        services.set_semantic_worker(Some(status.clone()));
        Ok(Arc::new(Self {
            db,
            subsystem,
            services,
            status,
            provider,
            cancellation: tokio_util::sync::CancellationToken::new(),
            closed: AtomicBool::new(false),
            lifecycle: Arc::new(cc_db::semantic_publish::LifecycleFence::default()),
            query_encoding: Mutex::new(None),
            running: AtomicBool::new(false),
            requested: AtomicBool::new(false),
            // Reopening an active index must also reconcile missing desired rows.
            backfill_cursor: Mutex::new(Some(String::new())),
            configured_transition: Mutex::new(None),
        }))
    }
    /// Lazy factory seam: the factory runs once per finite blocking job, and
    /// its provider is dropped on that same blocking thread before exit.
    pub fn new_with_factory(
        db: Arc<IndexDb>,
        subsystem: Arc<SemanticSubsystem>,
        services: Arc<QueryServices>,
        factory: impl Fn(tokio_util::sync::CancellationToken) -> CcResult<Arc<dyn EmbeddingProvider>>
            + Send
            + Sync
            + 'static,
    ) -> CcResult<Arc<Self>> {
        Self::from_source(
            db,
            subsystem,
            services,
            ProviderSource::Factory(Arc::new(factory)),
        )
    }
    fn resolve_provider(&self) -> CcResult<Arc<dyn EmbeddingProvider>> {
        let provider = match &self.provider {
            ProviderSource::Injected(provider) => provider.clone(),
            ProviderSource::Factory(factory) => factory(self.cancellation.clone())?,
        };
        if provider.space() != &self.subsystem.space {
            return Err(cc_model::CcError::InvalidParams(
                "worker provider space mismatch".into(),
            ));
        }
        Ok(provider)
    }
    /// Explicit operator configuration/provider installation authority.
    /// Stores a revision only; SQL and provider work remain caller-driven.
    pub(crate) fn authorize_configured_space_transition(&self) {
        *self
            .configured_transition
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(format!(
            "configured-doc-spec:{}",
            self.subsystem.doc_spec.as_str()
        ));
    }
    pub(crate) async fn encode_query(
        self: &Arc<Self>,
        query: String,
        control: cc_model::query::QueryControl,
    ) -> CcResult<()> {
        let context = self
            .query_encoding
            .lock()
            .map_err(|_| cc_model::CcError::Other("query encoder unavailable".into()))?
            .clone();
        if let Some(context) = context {
            crate::semantic_query_encoding::ensure_query_vector(
                context,
                query,
                control,
                self.services.pin(),
            )
            .await?;
        }
        Ok(())
    }
    pub fn close(&self) {
        self.lifecycle.close();
        self.closed.store(true, Ordering::Release);
        self.cancellation.cancel();
    }
    /// Returns false when closed, busy, or outside Tokio. Global capacity waits
    /// asynchronously; coalesced work retains one project pin, without DB locks.
    /// Capacity and the project pin stay owned until blocking work really exits.
    pub fn schedule(self: &Arc<Self>) -> bool {
        static CAPACITY: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return false;
        };
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        self.requested.store(true, Ordering::Release);
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let running = Running(self.clone());
        let capacity = CAPACITY
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone();
        let pin = self.services.pin();
        handle.spawn(async move {
            let permit = tokio::select! {
                _ = running.0.cancellation.cancelled() => return,
                permit = capacity.acquire_owned() => match permit { Ok(permit) => permit, Err(_) => return },
            };
            tokio::task::spawn_blocking(move || {
            running.0.requested.store(false, Ordering::Release);
            // A job is finite: at most 64 rounds / 30s, checked between port
            // calls. Transport/retry deadlines bound each synchronous call.
            let started = std::time::Instant::now();
            let provider = if running.0.closed.load(Ordering::Acquire) {
                None
            } else {
                match running.0.resolve_provider() {
                    Ok(provider) => Some(provider),
                    Err(_) => {
                        running.0.status.assembly_failed();
                        tracing::warn!("semantic provider assembly refused; queued work retained");
                        None
                    }
                }
            };
            let mut continue_work = false;
            for _ in 0..64 {
                let Some(provider) = provider.as_ref() else {
                    break;
                };
                running.0.requested.store(false, Ordering::Release);
                match running.0.run_round_with(provider.as_ref()) {
                    Ok(more) => {
                        continue_work = more;
                        running.0.status.clear();
                        if !more && !running.0.requested.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    Err(error) => {
                        continue_work = false;
                        running.0.status.round_failed();
                        tracing::warn!(%error, "semantic worker round failed");
                        break;
                    }
                }
                if started.elapsed() >= std::time::Duration::from_secs(30) {
                    break;
                }
            }
            // reqwest blocking client destruction must stay outside async execution.
            drop(provider);
            let worker = running.0.clone();
            drop(permit);
            drop(running);
            // Do not lose a build request arriving during the final round.
            let requested = worker.requested.swap(false, Ordering::AcqRel);
            if continue_work || requested { worker.schedule(); }
            drop(pin);
            });
        });
        true
    }
    #[cfg(test)]
    fn run_round(&self) -> CcResult<bool> {
        if self.closed.load(Ordering::Acquire) {
            return Ok(false);
        }
        self.run_round_with(self.resolve_provider()?.as_ref())
    }
    fn run_round_with(&self, provider: &dyn EmbeddingProvider) -> CcResult<bool> {
        if self.closed.load(Ordering::Acquire) {
            return Ok(false);
        }
        let incarnation = self.db.reads().read_generation()?.incarnation;
        if matches!(
            self.db.semantic_incarnation_freshness(incarnation)?,
            cc_db::semantic_rebuild::IncarnationFreshness::Stale { .. }
        ) {
            return Ok(false);
        }
        // Only the explicit composition-root install/configuration event
        // grants transition authority; ordinary index rounds cannot switch.
        let space_id = self.subsystem.space.digest()?;
        let revision = self
            .configured_transition
            .lock()
            .map_err(|_| cc_model::CcError::Database("configured transition poisoned".into()))?
            .clone();
        if let Some(revision) = revision {
            let spec = cc_semantic::spec::DocumentEncodingSpec::new(
                self.subsystem.space.clone(),
                None,
                self.subsystem.query_spec.max_tokens(),
                cc_model::chunk_policy::TOKEN_ESTIMATOR,
            )?;
            if !self.db.prepare_semantic_configured_space(
                space_id.as_str(),
                &cc_semantic::space_switch::space_spec_json(&spec)?,
                &revision,
                incarnation,
                &self.lifecycle,
            )? {
                return Ok(false);
            }
            *self.configured_transition.lock().map_err(|_| {
                cc_model::CcError::Database("configured transition poisoned".into())
            })? = None;
            *self.backfill_cursor.lock().map_err(|_| {
                cc_model::CcError::Database("worker backfill cursor poisoned".into())
            })? = Some(String::new());
        }
        match self.db.semantic_active_space()? {
            Some(active) if active != space_id.as_str() => {
                return Err(cc_model::CcError::InvalidParams(
                    "worker active space differs from configured provider".into(),
                ));
            }
            None => {
                let spec = cc_semantic::spec::DocumentEncodingSpec::new(
                    self.subsystem.space.clone(),
                    None,
                    self.subsystem.query_spec.max_tokens(),
                    cc_model::chunk_policy::TOKEN_ESTIMATOR,
                )?;
                if self.db.semantic_space_state(space_id.as_str())?.is_none() {
                    cc_semantic::space_switch::register_backfill_space(&self.db, &spec)?;
                }
                cc_semantic::space_switch::activate_space(
                    &self.db,
                    &self.subsystem.space,
                    "post-index-bootstrap",
                )?;
                *self.backfill_cursor.lock().map_err(|_| {
                    cc_model::CcError::Database("worker backfill cursor poisoned".into())
                })? = Some(String::new());
            }
            _ => {}
        }
        {
            let mut cursor = self.backfill_cursor.lock().map_err(|_| {
                cc_model::CcError::Database("worker backfill cursor poisoned".into())
            })?;
            if let Some(after) = cursor.as_ref() {
                let page =
                    cc_db::document_store::semantic_worker_desired_page(&self.db, after, 64)?;
                let mut missing = Vec::new();
                for desired in page.desired {
                    let Some(text) = cc_db::document_store::semantic_worker_input(
                        &self.db,
                        &desired.doc_key,
                        &desired.doc_version,
                        &desired.input_digest,
                    )?
                    else {
                        continue;
                    };
                    let input = DocumentInput::from_bytes(text.as_bytes())?.input_digest;
                    let published = cc_db::document_store::semantic_worker_publication_current(
                        &self.db,
                        &desired,
                        space_id.as_str(),
                    )?;
                    let cached = matches!(
                        self.subsystem.cache.get(
                            &self.subsystem.space,
                            &input,
                            &self.subsystem.doc_spec
                        )?,
                        cc_semantic::cache::CacheRead::Hit(_)
                    );
                    if !published || !cached {
                        missing.push(desired);
                    }
                }
                cc_db::document_store::enqueue_semantic_worker_missing(&self.db, &missing)?;
                *cursor = page.next_cursor;
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let provider = FencedProvider(self, provider);
        let outcome = semantic_wiring::drain_worker_batch(
            &self.db,
            &self.subsystem,
            &self.services,
            &provider,
            &|task| {
                if self.closed.load(Ordering::Acquire) {
                    return Ok(None);
                }
                cc_db::document_store::semantic_worker_input(
                    &self.db,
                    &task.doc_key,
                    &task.doc_version,
                    &task.input_digest,
                )?
                .map(|text| DocumentInput::from_bytes(text.as_bytes()))
                .transpose()
            },
            semantic_wiring::WorkerDrainOptions {
                owner: "post-index",
                max_batch: 16,
                now_unix: now,
                lifecycle: Some(&self.lifecycle),
            },
        )?;
        let backfilling = self
            .backfill_cursor
            .lock()
            .map_err(|_| cc_model::CcError::Database("worker backfill cursor poisoned".into()))?
            .is_some();
        Ok(!self.closed.load(Ordering::Acquire) && (backfilling || outcome.batch.claimed == 16))
    }
}
struct FencedProvider<'a>(&'a SemanticRuntime, &'a dyn EmbeddingProvider);
impl EmbeddingProvider for FencedProvider<'_> {
    fn space(&self) -> &cc_semantic::types::VectorSpace {
        self.1.space()
    }
    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(ProviderError::Cancelled);
        }
        // Reopening/reconciling never repays for a verified durable artifact.
        // Cache IO finishes before the fallback provider call.
        let mut cached = Vec::with_capacity(batch.len());
        for input in batch {
            match self.0.subsystem.cache.get(
                &self.0.subsystem.space,
                &input.input_digest,
                &self.0.subsystem.doc_spec,
            ) {
                Ok(cc_semantic::cache::CacheRead::Hit(hit)) => cached.push(hit.data),
                _ => {
                    cached.clear();
                    break;
                }
            }
        }
        let result = if !batch.is_empty() && cached.len() == batch.len() {
            Ok(cached)
        } else {
            self.1.embed_documents(batch)
        };
        if self.0.closed.load(Ordering::Acquire) {
            return Err(ProviderError::Cancelled);
        }
        result
    }
    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(ProviderError::Cancelled);
        }
        let result = self.1.embed_queries(batch);
        if self.0.closed.load(Ordering::Acquire) {
            return Err(ProviderError::Cancelled);
        }
        result
    }
}

/// Feature-gated production installation. No credential, HTTP client, cache
/// write, or request is created at project setup. Assembly/call/drop occur
/// only inside the finite blocking job after a successful indexing trigger.
#[cfg(feature = "semantic-http")]
pub fn from_config(
    db: Arc<IndexDb>,
    subsystem: Arc<SemanticSubsystem>,
    services: Arc<QueryServices>,
    config: &cc_model::config::SemanticProviderConfig,
) -> CcResult<Option<Arc<SemanticRuntime>>> {
    use cc_semantic::admission::{CostBudget, ReceiptLedger};
    use cc_semantic::providers::openai_compatible::{RetryPolicy, RetryingProvider};
    if !config.enabled || !config.network_opt_in {
        return Ok(None);
    }
    let retry = RetryPolicy::from_provider_config(config)?;
    let query_config = config.clone();
    let config = config.clone();
    let namespace = subsystem.namespace.clone();
    let receipts = Arc::new(ReceiptLedger::new(4096)?);
    let query_receipts = receipts.clone();
    let runtime =
        SemanticRuntime::new_with_factory(db, subsystem, services, move |cancellation| {
            let transport_config = config.clone();
            let transport_cancellation = cancellation.clone();
            let provider =
                crate::semantic_provider_factory::build_embedding_provider(&config, || {
                    crate::semantic_http_transport::build_http_transport_with_cancellation(
                        &transport_config,
                        transport_cancellation,
                    )
                })?
                .ok_or_else(|| {
                    cc_model::CcError::Config("semantic worker provider disabled".into())
                })?;
            let gate =
                (config.max_concurrent > 0).then(crate::service_factory::semantic_provider_gate);
            let admitted = Arc::new(AdmittedProvider {
                inner: provider,
                gate: gate.clone(),
                namespace: namespace.clone(),
                wait: std::time::Duration::from_millis(config.acquire_timeout_ms),
                cancellation,
                control: None,
            });
            Ok(Arc::new(
                RetryingProvider::new(
                    admitted,
                    retry.clone(),
                    crate::service_factory::semantic_circuit_breaker(),
                    gate,
                )
                .with_receipts(
                    receipts.clone(),
                    CostBudget::new(config.retry_max_cost_units),
                ),
            ) as Arc<dyn EmbeddingProvider>)
        })?;
    if query_config.allow_query_network {
        use crate::semantic_query_encoding::QueryEncodingContext;
        // Hard partition: foreground cannot consume either background slot.
        // Both classes acquire the same FIFO provider gate for each attempt.
        static FOREGROUND: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
        let capacity = FOREGROUND
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone();
        let namespace = runtime.subsystem.namespace.clone();
        let spec = runtime.subsystem.query_spec.clone();
        let max_tokens = spec.max_tokens() as usize;
        let input_budget = cc_semantic::admission::InputBudget::validated(
            1,
            max_tokens
                .checked_mul(4)
                .ok_or_else(|| cc_model::CcError::Config("query input bound overflow".into()))?,
            max_tokens,
        )?;
        let context = Arc::new(QueryEncodingContext {
            factory: Arc::new(move |control, cancellation| {
                control.check()?;
                let raw_config = query_config.clone();
                let transport_token = cancellation.clone();
                let transport_control = control.clone();
                let provider = crate::semantic_provider_factory::build_embedding_provider(
                    &query_config,
                    || {
                        let raw =
                            crate::semantic_http_transport::build_http_transport_with_cancellation(
                                &raw_config,
                                transport_token.clone(),
                            )?;
                        Ok(query_transport_with_budget(
                            raw,
                            transport_control,
                            transport_token,
                        ))
                    },
                )?
                .ok_or_else(|| cc_model::CcError::Config("query provider disabled".into()))?;
                let gate = (query_config.max_concurrent > 0)
                    .then(crate::service_factory::semantic_provider_gate);
                let admitted = Arc::new(AdmittedProvider {
                    inner: provider,
                    gate: gate.clone(),
                    namespace: namespace.clone(),
                    wait: std::time::Duration::from_millis(query_config.acquire_timeout_ms),
                    cancellation,
                    control: Some(control.clone()),
                });
                // One attempt: no retry/backoff can escape the absolute child budget.
                let mut policy = RetryPolicy::from_provider_config(&query_config)?;
                policy.max_attempts = 1;
                policy.total_deadline = policy.total_deadline.min(control.remaining());
                Ok(Arc::new(
                    RetryingProvider::new(
                        admitted,
                        policy,
                        crate::service_factory::semantic_circuit_breaker(),
                        gate,
                    )
                    .with_receipts(
                        query_receipts.clone(),
                        CostBudget::new(query_config.retry_max_cost_units),
                    ),
                ) as Arc<dyn EmbeddingProvider>)
            }),
            cache: runtime.subsystem.query_cache.clone(),
            namespace: runtime.subsystem.namespace.clone(),
            spec,
            input_budget,
            lifecycle: runtime.lifecycle.clone(),
            cancellation: runtime.cancellation.clone(),
            capacity,
        });
        *runtime
            .query_encoding
            .lock()
            .map_err(|_| cc_model::CcError::Other("query encoder unavailable".into()))? =
            Some(context);
        runtime
            .subsystem
            .recall
            .install_query_encoder(Arc::downgrade(&runtime));
        runtime
            .services
            .set_query_encoding_lifecycle(Some(runtime.lifecycle.clone()));
    } else {
        runtime.services.set_query_encoding_lifecycle(None);
    }
    Ok(Some(runtime))
}

#[cfg(feature = "semantic-http")]
struct AdmittedProvider {
    inner: Arc<dyn EmbeddingProvider>,
    gate: Option<Arc<cc_semantic::admission::ProviderGate>>,
    namespace: String,
    wait: std::time::Duration,
    cancellation: tokio_util::sync::CancellationToken,
    control: Option<cc_model::query::QueryControl>,
}
#[cfg(feature = "semantic-http")]
impl AdmittedProvider {
    fn check_control(&self) -> Result<(), ProviderError> {
        if let Some(control) = &self.control {
            control.check().map_err(|error| {
                if matches!(error, cc_model::CcError::QueryTimedOut) {
                    ProviderError::Timeout
                } else {
                    ProviderError::Cancelled
                }
            })?;
        }
        Ok(())
    }
    fn call(
        &self,
        call: impl FnOnce() -> Result<Vec<Vec<f32>>, ProviderError>,
    ) -> Result<Vec<Vec<f32>>, ProviderError> {
        if self.cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        self.check_control()?;
        let wait = self
            .control
            .as_ref()
            .map_or(self.wait, |control| self.wait.min(control.remaining()));
        let _permit = self
            .gate
            .as_ref()
            .map(|gate| {
                gate.try_acquire_permit(&self.namespace, wait)
                    .map_err(|_| ProviderError::Timeout)
            })
            .transpose()?;
        if self.cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        self.check_control()?;
        let result = call();
        self.check_control()?;
        if self.cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        result
    }
}
#[cfg(feature = "semantic-http")]
impl EmbeddingProvider for AdmittedProvider {
    fn space(&self) -> &cc_semantic::types::VectorSpace {
        self.inner.space()
    }
    fn embed_documents(&self, inputs: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.call(|| self.inner.embed_documents(inputs))
    }
    fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.call(|| self.inner.embed_queries(inputs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_model::config::ProjectConfig;
    use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};

    fn test_gate() -> &'static tokio::sync::Mutex<()> {
        static GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        GATE.get_or_init(|| tokio::sync::Mutex::new(()))
    }
    fn runtime() -> (tempfile::TempDir, Arc<SemanticRuntime>, Arc<FakeProvider>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(IndexDb::open(&dir.path().join("index.db")).unwrap().0);
        let mut config = ProjectConfig::default();
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/runtime".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        let services = Arc::new(QueryServices::default());
        let subsystem = Arc::new(
            semantic_wiring::try_init(
                &services,
                &dir.path().to_string_lossy(),
                &config,
                db.clone(),
            )
            .unwrap()
            .unwrap(),
        );
        let provider = Arc::new(FakeProvider::new(FakeProviderConfig::new(
            subsystem.space.clone(),
        )));
        let runtime = SemanticRuntime::new(db, subsystem, services, provider.clone()).unwrap();
        (dir, runtime, provider)
    }

    #[test]
    fn closed_round_and_missing_executor_never_contact_provider() {
        let (_dir, runtime, provider) = runtime();
        assert!(!runtime.schedule());
        assert!(!runtime.running.load(Ordering::Acquire));
        runtime.close();
        runtime.run_round().unwrap();
        assert_eq!(provider.call_count(), 0);
        let input = DocumentInput::from_bytes(b"local worker input").unwrap();
        assert_eq!(
            FencedProvider(&runtime, provider.as_ref()).embed_documents(&[input]),
            Err(ProviderError::Cancelled)
        );
        assert_eq!(provider.call_count(), 0);
    }

    #[tokio::test]
    async fn busy_project_rejects_duplicate_and_closed_schedule_releases_pin() {
        let _serial = test_gate().lock().await;
        let (_dir, runtime, provider) = runtime();
        runtime.running.store(true, Ordering::Release);
        assert!(!runtime.schedule());
        runtime.running.store(false, Ordering::Release);
        runtime.close();
        assert!(!runtime.schedule());
        assert_eq!(runtime.services.query_pins(), 0);
        assert_eq!(provider.call_count(), 0);
    }
    async fn wait_for_idle(worker: &Arc<SemanticRuntime>) {
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while worker.running.load(Ordering::Acquire) || worker.services.query_pins() != 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("bounded worker must finish");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_index_worker_crosses_pages_and_reopen_reuses_artifacts() {
        let _serial = test_gate().lock().await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = ProjectConfig::default();
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/runtime".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        std::fs::write(
            dir.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        // Real manifest rows, large enough to expose a one-page / one-batch job.
        for n in 0..1100 {
            std::fs::write(
                dir.path().join(format!("source_{n}.rs")),
                format!("pub fn source_{n}() -> u32 {{ {n} }}\n"),
            )
            .unwrap();
        }
        let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
        let subsystem = index.semantic_subsystem().unwrap();
        let provider = Arc::new(FakeProvider::new(FakeProviderConfig::new(
            subsystem.space.clone(),
        )));
        index.install_semantic_provider(provider.clone()).unwrap();
        let worker = index.semantic_runtime().unwrap();
        let index = Arc::new(std::sync::RwLock::new(index));
        let build_index = index.clone();
        tokio::task::spawn_blocking(move || crate::handlers::core::build_index(build_index, true))
            .await
            .unwrap()
            .unwrap();
        wait_for_idle(&worker).await;
        let coverage = worker.db.reads().semantic_coverage().unwrap().coverage;
        assert!(
            coverage.eligible > 64,
            "fixture must cross backfill page boundary"
        );
        let errors = worker
            .db
            .read_conn()
            .unwrap()
            .query_row(
                "SELECT COALESCE(group_concat(DISTINCT last_error),'none') FROM semantic_outbox",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(
            coverage.uncovered,
            0,
            "coverage={coverage:?}; errors={errors}; worker={:?}",
            worker.status.failure_reason()
        );
        assert_eq!(coverage.failed, 0);
        assert_eq!(worker.db.reads().semantic_outbox_pending().unwrap(), 0);
        let calls = provider.call_count();
        assert!(calls > 1024, "fixture must cross finite job boundary");
        let reopened = tokio::task::spawn_blocking(move || {
            let mut rt = index.write().unwrap();
            rt.close();
            rt.reopen().unwrap();
            rt.install_semantic_provider(provider.clone()).unwrap();
            (rt.semantic_runtime().unwrap(), provider)
        })
        .await
        .unwrap();
        assert!(reopened.0.schedule());
        wait_for_idle(&reopened.0).await;
        assert_eq!(
            reopened.1.call_count(),
            calls,
            "verified cache reuse must not repay provider"
        );
        assert_eq!(
            reopened
                .0
                .db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .uncovered,
            0
        );
    }
    struct HeldProvider {
        inner: FakeProvider,
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }
    impl EmbeddingProvider for HeldProvider {
        fn space(&self) -> &cc_semantic::types::VectorSpace {
            self.inner.space()
        }
        fn embed_documents(
            &self,
            inputs: &[DocumentInput],
        ) -> Result<Vec<Vec<f32>>, ProviderError> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).unwrap();
            }
            let (released, wake) = &*self.release;
            let mut released = released.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
            self.inner.embed_documents(inputs)
        }
        fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.inner.embed_queries(inputs)
        }
    }
    struct ReleaseHeld(Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl Drop for ReleaseHeld {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = true;
            self.0 .1.notify_all();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn provider_wait_releases_index_and_db_locks_and_close_fences_publication() {
        let _serial = test_gate().lock().await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = ProjectConfig::default();
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/held-runtime".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        std::fs::write(
            dir.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("one.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
        let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
        let subsystem = index.semantic_subsystem().unwrap();
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let release_guard = ReleaseHeld(release.clone());
        index
            .install_semantic_provider(Arc::new(HeldProvider {
                inner: FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone())),
                entered: Mutex::new(Some(entered)),
                release,
            }))
            .unwrap();
        let worker = index.semantic_runtime().unwrap();
        let index = Arc::new(std::sync::RwLock::new(index));
        let build_index = index.clone();
        tokio::task::spawn_blocking(move || crate::handlers::core::build_index(build_index, true))
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap();
        // The only read connection and writer transaction remain usable while
        // the synthetic provider is physically waiting.
        let db = worker.db.clone();
        let read_and_write = tokio::task::spawn_blocking(move || {
            db.reads().read_generation()?;
            db.enqueue_semantic_rebuild_plan(&[])?;
            Ok::<_, cc_model::CcError>(())
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), read_and_write)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        {
            let mut index = index
                .try_write()
                .expect("provider wait must release CodeIndex lock");
            index.close();
            assert!(index.semantic_runtime().is_none());
            assert!(worker.services.semantic().is_none());
            assert_eq!(
                worker.services.query_pins(),
                1,
                "physical worker retains its pin after close"
            );
            assert!(!worker.schedule());
        }
        drop(release_guard);
        wait_for_idle(&worker).await;
        assert_eq!(
            worker
                .db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .published,
            0
        );
    }
    #[tokio::test]
    async fn lazy_factory_failure_is_visible_and_retired_state_cannot_override_replacement() {
        let _serial = test_gate().lock().await;
        let (dir, old, _provider) = runtime();
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = attempts.clone();
        let worker = SemanticRuntime::new_with_factory(
            old.db.clone(),
            old.subsystem.clone(),
            old.services.clone(),
            move |_| {
                calls.fetch_add(1, Ordering::AcqRel);
                Err(cc_model::CcError::Config(
                    "synthetic factory refusal".into(),
                ))
            },
        )
        .unwrap();
        assert_eq!(attempts.load(Ordering::Acquire), 0);
        assert!(worker.schedule());
        wait_for_idle(&worker).await;
        assert_eq!(attempts.load(Ordering::Acquire), 1);
        let state = crate::capability_status::snapshot(
            Some(dir.path()),
            Some(&worker.db),
            None,
            &worker.services,
        );
        assert_eq!(state["retrieval"]["semantic_state"], "failed");
        assert_eq!(
            state["retrieval"]["semantic_worker_reason"],
            "semantic_provider_assembly_failed"
        );
        // A late error on the retired worker's status Arc must stay retired.
        let replacement = Arc::new(crate::service_factory::SemanticWorkerStatus::default());
        worker.services.set_semantic_worker(Some(replacement));
        worker.status.round_failed();
        assert!(worker
            .services
            .semantic_worker()
            .unwrap()
            .failure_reason()
            .is_none());
        semantic_wiring::teardown(&worker.services);
        assert!(worker.services.semantic_worker().is_none());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reopen_preserves_live_retry_budget_and_terminal_failure() {
        let _serial = test_gate().lock().await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = ProjectConfig::default();
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/retry-runtime".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        std::fs::write(
            dir.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("one.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
        let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
        let mut provider_config =
            FakeProviderConfig::new(index.semantic_subsystem().unwrap().space.clone());
        provider_config.fail_after_n_calls = Some(0);
        let provider = Arc::new(FakeProvider::new(provider_config));
        index.install_semantic_provider(provider.clone()).unwrap();
        let worker = index.semantic_runtime().unwrap();
        let shared = Arc::new(std::sync::RwLock::new(index));
        let build_index = shared.clone();
        tokio::task::spawn_blocking(move || crate::handlers::core::build_index(build_index, true))
            .await
            .unwrap()
            .unwrap();
        wait_for_idle(&worker).await;
        for expected in 2..=3 {
            // Unit fixture advances availability; L3 stdio waits real backoff.
            crate::test_seed::seed_conn(&worker.db)
                .execute(
                    "UPDATE semantic_outbox SET available_at=0 WHERE state='pending'",
                    [],
                )
                .unwrap();
            let replacement = {
                let mut index = shared.write().unwrap();
                index.close();
                index.reopen().unwrap();
                index.install_semantic_provider(provider.clone()).unwrap();
                index.semantic_runtime().unwrap()
            };
            assert!(replacement.schedule());
            wait_for_idle(&replacement).await;
            assert_eq!(provider.call_count(), expected);
        }
        let connection = crate::test_seed::seed_conn(&worker.db);
        let counts: (i64, i64) = connection
            .query_row(
                "SELECT COUNT(*),MAX(attempt_count) FROM semantic_outbox",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            counts,
            (1, 3),
            "reopen must neither duplicate nor reset the current task"
        );
        assert_eq!(
            worker
                .db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .failed,
            1
        );
        let index = shared.read().unwrap();
        assert_eq!(
            index.capabilities_info()["retrieval"]["semantic_state"],
            "failed"
        );
    }
}
