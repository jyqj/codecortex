//! Opt-in query encoding composition service. No configuration or MCP surface.
//!
//! The composition root must gate installation on semantic-http, semantic
//! enabled, network opt-in AND a separate query-text opt-in. Local/empty-scope
//! requests must never call this service. Factory creation/call/drop all run
//! on the bounded blocking job, outside index/DB/CPU-pool locks. Factories must
//! use QueryCallContext::wrap_transport (core API) or query_deadline_transport
//! (exact root API) for every HTTP transport and must not log input/credentials. This service performs one encoding attempt (no retry).
//!
//! Foreground/background slots are reserved, not borrowed. Every background
//! job must hold acquire_background's permit until its blocking work exits;
//! otherwise class fairness is NOT established by this module alone. Both
//! classes must additionally use the same provider gate at the composition root.
//!
//! Integration (main owner): register this module, then build one
//! QueryEncodingContext from the existing subsystem cache/namespace/query spec.
//! Gate it with semantic.allow_query_network=false by default plus the existing
//! feature/enabled/network gates. Pass ensure_query_vector the captured view's
//! QueryPin and semantic child control, and reuse that control for exact scan.
//! The agreed Semaphore path is zero-queue/immediate QueryBusy; main must
//! provide dedicated foreground slots and reserve background progress. Factory
//! uses query_deadline_transport and the supplied child token; retry remains
//! disabled for this minimal path. Retire the shared LifecycleFence before
//! cancelling the project's token; token alone is transport signalling, not
//! the cache-publication authority.
//!
//! Cache publication order: LifecycleFence -> request QueryControl publication
//! mutex -> independent abandonment-control publication mutex -> cache mutex.
//! Cancellation takes only its own control mutex; retirement takes only the
//! lifecycle mutex. No provider/SQL/callback/await exists inside that chain.
//! Future drop cancels the abandonment control synchronously, so it linearizes
//! with the final cache put even when the transport cannot stop immediately.
use cc_db::semantic_publish::LifecycleFence;
use cc_model::{query::QueryControl, CcError, CcResult};
use cc_semantic::{
    admission::{InputBudget, OversizeReason},
    cache::{encode_queries, QueryCacheKey, QueryEncodeOutcome, QueryVectorCache},
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::openai_compatible::{
        EmbeddingHttpTransport, HttpRequest, HttpResponse, TransportError,
    },
    spec::QueryEncodingSpec,
    types::VectorSpace,
};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Hard partition: each class retains progress capacity even if the other
/// class has an unlimited stream of requests. The total running bound is the
/// sum of the two running limits; align it with the shared provider gate.
#[derive(Clone, Copy)]
pub struct NetworkLimits {
    pub query_running: usize,
    pub query_queued: usize,
    pub background_running: usize,
    pub background_queued: usize,
}
struct ClassCapacity {
    admission: Arc<Semaphore>,
    running: Arc<Semaphore>,
}
pub struct QueryNetworkCapacity {
    query: ClassCapacity,
    background: ClassCapacity,
}
/// Move into the blocking job. Dropping the waiting future must not drop
/// this permit while its provider call is still physically running.
pub struct NetworkPermit {
    _admission: Option<OwnedSemaphorePermit>,
    _running: OwnedSemaphorePermit,
}
impl ClassCapacity {
    fn new(running: usize, queued: usize) -> CcResult<Self> {
        let total = running
            .checked_add(queued)
            .filter(|&n| n <= Semaphore::MAX_PERMITS)
            .ok_or_else(|| {
                CcError::InvalidParams("query network capacity exceeds bounds".into())
            })?;
        if running == 0 {
            return Err(CcError::InvalidParams(
                "both network classes need running capacity".into(),
            ));
        }
        Ok(Self {
            admission: Arc::new(Semaphore::new(total)),
            running: Arc::new(Semaphore::new(running)),
        })
    }
    async fn acquire(&self, control: &QueryControl) -> CcResult<NetworkPermit> {
        control.check()?;
        let admission = self
            .admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| CcError::QueryBusy)?;
        let running = cc_search::execution::until(control, self.running.clone().acquire_owned())
            .await?
            .map_err(|_| CcError::QueryBusy)?;
        Ok(NetworkPermit {
            _admission: Some(admission),
            _running: running,
        })
    }
}
impl QueryNetworkCapacity {
    pub fn new(limits: NetworkLimits) -> CcResult<Arc<Self>> {
        limits
            .query_running
            .checked_add(limits.background_running)
            .filter(|&n| n <= Semaphore::MAX_PERMITS)
            .ok_or_else(|| {
                CcError::InvalidParams("total network capacity exceeds bounds".into())
            })?;
        Ok(Arc::new(Self {
            query: ClassCapacity::new(limits.query_running, limits.query_queued)?,
            background: ClassCapacity::new(limits.background_running, limits.background_queued)?,
        }))
    }
    pub async fn acquire_background(&self, control: &QueryControl) -> CcResult<NetworkPermit> {
        self.background.acquire(control).await
    }
}

/// Both the request's absolute control and an independent abandonment fence.
/// Dropping this service's future cancels only its job, not local fallback.
#[derive(Clone)]
pub struct QueryCallContext {
    control: QueryControl,
    abandoned: QueryControl,
    lifecycle: Arc<LifecycleFence>,
}
impl QueryCallContext {
    pub fn check(&self) -> CcResult<()> {
        self.control.check()?;
        self.abandoned.check()?;
        if !self.lifecycle.is_open() {
            return Err(CcError::QueryCancelled);
        }
        Ok(())
    }
    pub fn remaining(&self) -> std::time::Duration {
        self.control.remaining().min(self.abandoned.remaining())
    }
    /// Apply to the raw transport BEFORE installing it in the provider.
    /// Each request gets min(provider timeout, remaining absolute budget).
    /// Blocking cancellation may wait for that deadline; capacity stays held.
    pub fn wrap_transport(
        &self,
        inner: Arc<dyn EmbeddingHttpTransport>,
    ) -> Arc<dyn EmbeddingHttpTransport> {
        Arc::new(DeadlineTransport {
            inner,
            context: self.clone(),
            cancellation: None,
        })
    }
}
struct DeadlineTransport {
    inner: Arc<dyn EmbeddingHttpTransport>,
    context: QueryCallContext,
    cancellation: Option<tokio_util::sync::CancellationToken>,
}
/// Factory helper for the agreed (QueryControl, child CancellationToken)
/// signature. Wrap the raw transport before installing it in the provider;
/// this bounds even the existing adapter's default 30-second HttpRequest.
pub(crate) fn query_deadline_transport(
    inner: Arc<dyn EmbeddingHttpTransport>,
    control: QueryControl,
    cancellation: tokio_util::sync::CancellationToken,
) -> Arc<dyn EmbeddingHttpTransport> {
    Arc::new(DeadlineTransport {
        inner,
        context: QueryCallContext {
            control: control.clone(),
            abandoned: control,
            // Project retirement on this transport is represented by the
            // child token; actual cache publication uses the shared fence.
            lifecycle: Arc::new(LifecycleFence::default()),
        },
        cancellation: Some(cancellation),
    })
}
fn transport_control(error: CcError) -> TransportError {
    if matches!(error, CcError::QueryTimedOut) {
        TransportError::Timeout
    } else {
        TransportError::Cancelled
    }
}
impl EmbeddingHttpTransport for DeadlineTransport {
    fn post_json(&self, mut request: HttpRequest) -> Result<HttpResponse, TransportError> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
        {
            return Err(TransportError::Cancelled);
        }
        self.context.check().map_err(transport_control)?;
        request.timeout = request.timeout.min(self.context.remaining());
        if request.timeout.is_zero() {
            return Err(TransportError::Timeout);
        }
        let result = self.inner.post_json(request);
        if self
            .cancellation
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
        {
            return Err(TransportError::Cancelled);
        }
        self.context.check().map_err(transport_control)?;
        // Never propagate transport diagnostics containing URLs/credentials.
        result.map_err(|e| match e {
            TransportError::Io(_) => TransportError::Io("query transport failed".into()),
            other => other,
        })
    }
}

pub struct QueryEncodingInputs {
    pub cache: Arc<QueryVectorCache>,
    pub namespace: String,
    pub spec: QueryEncodingSpec,
    pub budget: InputBudget,
    /// Must be the same fence retired when this project's provider/space closes.
    pub lifecycle: Arc<LifecycleFence>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryEncodingOutcome {
    Ready { cache_hit: bool },
    Skipped { reason: OversizeReason },
}
type ProviderFactory =
    dyn Fn(QueryCallContext) -> CcResult<Arc<dyn EmbeddingProvider>> + Send + Sync;
enum CapacitySource {
    Reserved(Arc<QueryNetworkCapacity>),
    // Exact composition interface: no waiting queue, immediate QueryBusy.
    // Main owns the class partition behind this dedicated query semaphore.
    Direct(Arc<Semaphore>),
}
impl CapacitySource {
    async fn acquire(&self, control: &QueryControl) -> CcResult<NetworkPermit> {
        match self {
            Self::Reserved(capacity) => capacity.query.acquire(control).await,
            Self::Direct(capacity) => {
                control.check()?;
                let running = capacity
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| CcError::QueryBusy)?;
                Ok(NetworkPermit {
                    _admission: None,
                    _running: running,
                })
            }
        }
    }
}
pub struct QueryEncodingService {
    inputs: QueryEncodingInputs,
    capacity: CapacitySource,
    factory: Arc<ProviderFactory>,
    // The blocking closure owns this service until physical exit.
    _keep_alive: Option<Arc<dyn Send + Sync>>,
    #[cfg(test)]
    before_publish: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl QueryEncodingService {
    pub fn new(
        inputs: QueryEncodingInputs,
        capacity: Arc<QueryNetworkCapacity>,
        factory: impl Fn(QueryCallContext) -> CcResult<Arc<dyn EmbeddingProvider>>
            + Send
            + Sync
            + 'static,
    ) -> CcResult<Arc<Self>> {
        Self::build(
            inputs,
            CapacitySource::Reserved(capacity),
            Arc::new(factory),
            None,
        )
    }
    fn build(
        inputs: QueryEncodingInputs,
        capacity: CapacitySource,
        factory: Arc<ProviderFactory>,
        keep_alive: Option<Arc<dyn Send + Sync>>,
    ) -> CcResult<Arc<Self>> {
        InputBudget::validated(
            inputs.budget.max_items,
            inputs.budget.max_bytes,
            inputs.budget.max_tokens,
        )?;
        inputs.spec.digest()?;
        if inputs.budget.max_tokens > inputs.spec.max_tokens() as usize {
            return Err(CcError::InvalidParams(
                "query token budget exceeds encoding spec".into(),
            ));
        }
        QueryCacheKey::new(
            &inputs.namespace,
            &inputs.spec,
            &QueryInput::from_bytes(b"validation")?,
        )?;
        Ok(Arc::new(Self {
            inputs,
            capacity,
            factory,
            _keep_alive: keep_alive,
            #[cfg(test)]
            before_publish: None,
        }))
    }
    /// Run only for an opted-in nonlocal, nonempty semantic request. The same
    /// child control must subsequently govern ExactRecallService's scan.
    pub async fn encode(
        self: &Arc<Self>,
        query: Vec<u8>,
        control: QueryControl,
    ) -> CcResult<QueryEncodingOutcome> {
        control.check()?;
        if !self.inputs.lifecycle.is_open() {
            return Err(CcError::QueryCancelled);
        }
        // Admission first: oversize inputs must not create a provider or wait.
        if query.len() > self.inputs.budget.max_bytes {
            return Ok(QueryEncodingOutcome::Skipped {
                reason: OversizeReason::BytesTooLarge {
                    bytes: query.len(),
                    max_bytes: self.inputs.budget.max_bytes,
                },
            });
        }
        let plan = cc_semantic::admission::plan_query_batches(
            &[((), query.clone())],
            &self.inputs.budget,
            self.inputs.spec.tokenizer(),
        )?;
        if let Some(cc_semantic::admission::PlannedQueryInput::Skipped { reason, .. }) =
            plan.into_iter().next()
        {
            return Ok(QueryEncodingOutcome::Skipped { reason });
        }
        let input = QueryInput::from_bytes(&query)?;
        let key = QueryCacheKey::new(&self.inputs.namespace, &self.inputs.spec, &input)?;
        {
            let _lifecycle = self
                .inputs
                .lifecycle
                .enter()
                .ok_or(CcError::QueryCancelled)?;
            if self.inputs.cache.get(&key).is_some() {
                control.check()?;
                return Ok(QueryEncodingOutcome::Ready { cache_hit: true });
            }
        }
        let permit = self.capacity.acquire(&control).await?;
        let abandoned = QueryControl::new(control.remaining())?;
        let mut cancellation = abandoned.cancel_on_drop();
        let context = QueryCallContext {
            control: control.clone(),
            abandoned,
            lifecycle: self.inputs.lifecycle.clone(),
        };
        let service = self.clone();
        let job = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            context.check()?;
            // A queued identical query can consume the newly filled cache.
            if service.inputs.cache.get(&key).is_some() {
                context.check()?;
                return Ok(QueryEncodingOutcome::Ready { cache_hit: true });
            }
            let provider = (service.factory)(context.clone()).map_err(redact_error)?;
            context.check()?;
            if provider.space() != service.inputs.spec.space() {
                return Err(CcError::InvalidParams(
                    "query provider space mismatch".into(),
                ));
            }
            let staging_bytes = (service.inputs.spec.space().dimension() as usize)
                .checked_mul(4)
                .ok_or_else(|| CcError::InvalidParams("query vector size exceeds bounds".into()))?;
            let staging = QueryVectorCache::new(1, staging_bytes);
            let outcomes = encode_queries(
                &RedactedProvider(provider),
                &staging,
                &service.inputs.namespace,
                &service.inputs.spec,
                &service.inputs.budget,
                &[((), query)],
            )
            .map_err(redact_error)?;
            context.check()?;
            let Some(QueryEncodeOutcome::Encoded { vector, .. }) = outcomes.into_iter().next()
            else {
                return Err(CcError::Other("query encoding produced no vector".into()));
            };
            #[cfg(test)]
            if let Some(barrier) = &service.before_publish {
                barrier();
            }
            // No provider, DB, callback, or await under either publication fence.
            // Acquire lifecycle first so a contended retirement fence cannot
            // block QueryControl::cancel's publication mutex. Both controls
            // are rechecked AFTER lifecycle acquisition, before cache put.
            let _lifecycle = context.lifecycle.enter().ok_or(CcError::QueryCancelled)?;
            context.control.publish(|| {
                context
                    .abandoned
                    .publish(|| service.inputs.cache.put(key, vector))
            })???;
            Ok(QueryEncodingOutcome::Ready { cache_hit: false })
        });
        let result = cc_search::execution::until(&control, job)
            .await?
            .map_err(|_| CcError::Other("query encoding worker failed".into()))?;
        cancellation.disarm();
        result
    }
}
/// Exact interface agreed with the composition-root owner. This type and its
/// fields stay crate-private; no MCP priming or injection surface is added.
pub(crate) type QueryProviderFactory = Arc<
    dyn Fn(
            QueryControl,
            tokio_util::sync::CancellationToken,
        ) -> CcResult<Arc<dyn EmbeddingProvider>>
        + Send
        + Sync,
>;
pub(crate) struct QueryEncodingContext {
    pub(crate) factory: QueryProviderFactory,
    pub(crate) cache: Arc<QueryVectorCache>,
    pub(crate) namespace: String,
    pub(crate) spec: QueryEncodingSpec,
    pub(crate) input_budget: InputBudget,
    pub(crate) lifecycle: Arc<LifecycleFence>,
    pub(crate) cancellation: tokio_util::sync::CancellationToken,
    /// Dedicated foreground running slots: immediate refusal, zero queue.
    /// Main must separately reserve background capacity / align provider gate.
    pub(crate) capacity: Arc<Semaphore>,
}
/// Main gates this context with semantic.allow_query_network (default false),
/// semantic-http, enabled and network_opt_in. Call only for nonlocal/nonempty
/// scope, before ExactRecall, with the SAME semantic child control. Factory
/// must clamp HTTP deadlines to control.remaining() and use the child token;
/// do not use the unbounded default 30s timeout or cancel the shared control.
pub(crate) async fn ensure_query_vector(
    context: Arc<QueryEncodingContext>,
    query: String,
    control: QueryControl,
    keep_alive: Arc<crate::service_factory::QueryPin>,
) -> CcResult<()> {
    let child = context.cancellation.child_token();
    let _cancel_child = child.clone().drop_guard();
    if child.is_cancelled() {
        return Err(CcError::QueryCancelled);
    }
    let factory = context.factory.clone();
    let token = child.clone();
    let service = QueryEncodingService::build(
        QueryEncodingInputs {
            cache: context.cache.clone(),
            namespace: context.namespace.clone(),
            spec: context.spec.clone(),
            budget: context.input_budget.clone(),
            lifecycle: context.lifecycle.clone(),
        },
        CapacitySource::Direct(context.capacity.clone()),
        Arc::new(move |call| factory(call.control.clone(), token.clone())),
        Some(keep_alive),
    )?;
    let result = tokio::select! {
        biased;
        _ = child.cancelled() => return Err(CcError::QueryCancelled),
        result = service.encode(query.into_bytes(), control) => result?,
    };
    match result {
        QueryEncodingOutcome::Ready { .. } => Ok(()),
        QueryEncodingOutcome::Skipped { .. } => Err(CcError::InvalidParams(
            "query encoding input refused".into(),
        )),
    }
}

fn redact_error(error: CcError) -> CcError {
    match error {
        CcError::QueryCancelled | CcError::QueryTimedOut | CcError::QueryBusy => error,
        _ => CcError::Other("query encoding failed".into()),
    }
}
struct RedactedProvider(Arc<dyn EmbeddingProvider>);
impl EmbeddingProvider for RedactedProvider {
    fn space(&self) -> &VectorSpace {
        self.0.space()
    }
    fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Err(ProviderError::InvalidInput(
            "document encoding is unavailable on query service".into(),
        ))
    }
    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.0.embed_queries(batch).map_err(|e| match e {
            ProviderError::InvalidInput(_) => {
                ProviderError::InvalidInput("query input rejected".into())
            }
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
        time::Duration,
    };

    fn fake(model: &str) -> Arc<FakeProvider> {
        Arc::new(FakeProvider::new(FakeProviderConfig::new(
            VectorSpace::new(model, 2).unwrap(),
        )))
    }
    fn new_capacity(queued: usize) -> Arc<QueryNetworkCapacity> {
        QueryNetworkCapacity::new(NetworkLimits {
            query_running: 1,
            query_queued: queued,
            background_running: 1,
            background_queued: 0,
        })
        .unwrap()
    }
    fn inputs(
        provider: &dyn EmbeddingProvider,
        cache: Arc<QueryVectorCache>,
        lifecycle: Arc<LifecycleFence>,
    ) -> QueryEncodingInputs {
        QueryEncodingInputs {
            cache,
            namespace: cc_semantic::cache::namespace_key("synthetic/query-service-test").unwrap(),
            spec: QueryEncodingSpec::new(
                provider.space().clone(),
                None,
                8192,
                cc_model::chunk_policy::TOKEN_ESTIMATOR,
            )
            .unwrap(),
            budget: InputBudget::validated(1, 1024, 8192).unwrap(),
            lifecycle,
        }
    }
    fn new_service(
        provider: Arc<dyn EmbeddingProvider>,
        cache: Arc<QueryVectorCache>,
        lifecycle: Arc<LifecycleFence>,
        capacity: Arc<QueryNetworkCapacity>,
    ) -> Arc<QueryEncodingService> {
        QueryEncodingService::new(
            inputs(provider.as_ref(), cache, lifecycle),
            capacity,
            move |_| Ok(provider.clone()),
        )
        .unwrap()
    }
    fn new_control(ms: u64) -> QueryControl {
        QueryControl::new(Duration::from_millis(ms)).unwrap()
    }
    fn new_cache() -> Arc<QueryVectorCache> {
        Arc::new(QueryVectorCache::new(8, 1024))
    }
    fn new_lifecycle() -> Arc<LifecycleFence> {
        Arc::new(LifecycleFence::default())
    }

    #[tokio::test]
    async fn cold_query_uses_real_encoder_then_cache_hit_is_provider_and_factory_free() {
        let provider = fake("synthetic/version-1");
        let cache = new_cache();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let p = provider.clone();
        let service = QueryEncodingService::new(
            inputs(provider.as_ref(), cache.clone(), new_lifecycle()),
            new_capacity(0),
            move |_| {
                count.fetch_add(1, Ordering::AcqRel);
                Ok(p.clone())
            },
        )
        .unwrap();
        assert_eq!(
            service
                .encode(b"synthetic needle".to_vec(), new_control(1000))
                .await
                .unwrap(),
            QueryEncodingOutcome::Ready { cache_hit: false }
        );
        assert_eq!(
            service
                .encode(b"synthetic needle".to_vec(), new_control(1000))
                .await
                .unwrap(),
            QueryEncodingOutcome::Ready { cache_hit: true }
        );
        assert_eq!(provider.call_count(), 1);
        assert_eq!(calls.load(Ordering::Acquire), 1);
        let key = QueryCacheKey::new(
            &service.inputs.namespace,
            &service.inputs.spec,
            &QueryInput::from_bytes(b"synthetic needle").unwrap(),
        )
        .unwrap();
        let vector = cache.get(&key).unwrap();
        assert_eq!(vector.dimension, 2);
        assert!(vector.data.iter().all(|v| v.is_finite()));
        assert!(vector.data.iter().any(|&v| v != 0.0));
    }

    struct FailedProvider(FakeProvider);
    impl EmbeddingProvider for FailedProvider {
        fn space(&self) -> &VectorSpace {
            self.0.space()
        }
        fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            panic!("query service must never encode documents")
        }
        fn embed_queries(&self, _: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            Err(ProviderError::InvalidInput(
                "synthetic-secret-query-and-credential".into(),
            ))
        }
    }
    #[tokio::test]
    async fn failure_and_assembly_diagnostics_are_redacted_and_never_cached() {
        let provider = Arc::new(FailedProvider(FakeProvider::new(FakeProviderConfig::new(
            VectorSpace::new("synthetic/failure", 2).unwrap(),
        ))));
        let cache = new_cache();
        let service = new_service(provider, cache.clone(), new_lifecycle(), new_capacity(0));
        let error = service
            .encode(
                b"synthetic-secret-query-and-credential".to_vec(),
                new_control(1000),
            )
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "query encoding failed");
        assert!(!format!("{error:?}").contains("secret"));
        assert!(cache.is_empty());
        let provider = fake("synthetic/factory-failure");
        let assembly = QueryEncodingService::new(
            inputs(provider.as_ref(), cache.clone(), new_lifecycle()),
            new_capacity(0),
            |_| {
                Err(CcError::Other(
                    "synthetic-secret-query-and-credential".into(),
                ))
            },
        )
        .unwrap();
        assert_eq!(
            assembly
                .encode(b"synthetic".to_vec(), new_control(1000))
                .await
                .unwrap_err()
                .to_string(),
            "query encoding failed"
        );
        assert!(cache.is_empty());
    }

    struct ErrorProvider {
        inner: FakeProvider,
        error: ProviderError,
    }
    impl EmbeddingProvider for ErrorProvider {
        fn space(&self) -> &VectorSpace {
            self.inner.space()
        }
        fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            unreachable!()
        }
        fn embed_queries(&self, _: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            Err(self.error.clone())
        }
    }
    #[tokio::test]
    async fn typed_provider_timeout_and_cancel_survive_redaction_other_failures_do_not_leak() {
        for error in [
            ProviderError::Timeout,
            ProviderError::Cancelled,
            ProviderError::ServerError,
            ProviderError::AuthError,
            ProviderError::RateLimited {
                retry_after: Duration::from_secs(1),
            },
        ] {
            let provider = Arc::new(ErrorProvider {
                inner: FakeProvider::new(FakeProviderConfig::new(
                    VectorSpace::new("synthetic/typed-fault", 2).unwrap(),
                )),
                error: error.clone(),
            });
            let cache = new_cache();
            let service = new_service(provider, cache.clone(), new_lifecycle(), new_capacity(0));
            let result = service
                .encode(b"synthetic query".to_vec(), new_control(1000))
                .await
                .unwrap_err();
            match error {
                ProviderError::Timeout => assert!(matches!(result, CcError::QueryTimedOut)),
                ProviderError::Cancelled => assert!(matches!(result, CcError::QueryCancelled)),
                _ => assert_eq!(result.to_string(), "query encoding failed"),
            }
            assert!(cache.is_empty());
        }
    }

    struct MalformedProvider(FakeProvider);
    impl EmbeddingProvider for MalformedProvider {
        fn space(&self) -> &VectorSpace {
            self.0.space()
        }
        fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            unreachable!()
        }
        fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            let mut vectors = self.0.embed_queries(inputs)?;
            vectors[0][0] = f32::NAN;
            Ok(vectors)
        }
    }
    #[tokio::test]
    async fn malformed_real_encoder_output_is_rejected_before_shared_cache_publication() {
        let provider = Arc::new(MalformedProvider(FakeProvider::new(
            FakeProviderConfig::new(VectorSpace::new("synthetic/malformed", 2).unwrap()),
        )));
        let cache = new_cache();
        let service = new_service(provider, cache.clone(), new_lifecycle(), new_capacity(0));
        assert!(service
            .encode(b"synthetic query".to_vec(), new_control(1000))
            .await
            .is_err());
        assert!(cache.is_empty());
    }

    struct HeldProvider {
        inner: FakeProvider,
        entered: tokio::sync::Notify,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl EmbeddingProvider for HeldProvider {
        fn space(&self) -> &VectorSpace {
            self.inner.space()
        }
        fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            panic!("no document inputs")
        }
        fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.entered.notify_one();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .expect("test must release provider");
            self.inner.embed_queries(inputs)
        }
    }
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Release {
        fn now(&mut self) {
            if let Some(tx) = self.0.take() {
                tx.send(()).unwrap();
            }
        }
    }
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }
    fn held() -> (Arc<HeldProvider>, Release) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (
            Arc::new(HeldProvider {
                inner: FakeProvider::new(FakeProviderConfig::new(
                    VectorSpace::new("synthetic/held-v1", 2).unwrap(),
                )),
                entered: tokio::sync::Notify::new(),
                release: Mutex::new(receiver),
            }),
            Release(Some(sender)),
        )
    }
    async fn wait_free(capacity: &QueryNetworkCapacity) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while capacity.query.running.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_cancel_and_future_drop_refuse_late_writes_and_hold_capacity_until_exit() {
        for fault in ["timeout", "cancel", "drop"] {
            let (provider, mut release) = held();
            let cache = new_cache();
            let capacity = new_capacity(0);
            let service = new_service(
                provider.clone(),
                cache.clone(),
                new_lifecycle(),
                capacity.clone(),
            );
            let control = new_control(if fault == "timeout" { 100 } else { 2000 });
            let inside = control.clone();
            let request = tokio::spawn({
                let service = service.clone();
                async move {
                    service
                        .encode(b"synthetic held query".to_vec(), inside)
                        .await
                }
            });
            tokio::time::timeout(Duration::from_secs(1), provider.entered.notified())
                .await
                .unwrap();
            match fault {
                "timeout" => assert!(matches!(
                    request.await.unwrap(),
                    Err(CcError::QueryTimedOut)
                )),
                "cancel" => {
                    control.cancel();
                    assert!(matches!(
                        request.await.unwrap(),
                        Err(CcError::QueryCancelled)
                    ));
                }
                "drop" => {
                    request.abort();
                    assert!(request.await.unwrap_err().is_cancelled());
                    assert!(
                        !control.is_cancelled(),
                        "abandonment must preserve parent's local fallback"
                    );
                }
                _ => unreachable!(),
            }
            assert!(cache.is_empty());
            assert_eq!(capacity.query.running.available_permits(), 0);
            assert!(matches!(
                service
                    .encode(b"another synthetic query".to_vec(), new_control(100))
                    .await,
                Err(CcError::QueryBusy)
            ));
            let background = capacity
                .acquire_background(&new_control(100))
                .await
                .unwrap();
            assert!(matches!(
                capacity.acquire_background(&new_control(100)).await,
                Err(CcError::QueryBusy)
            ));
            drop(background);
            release.now();
            wait_free(&capacity).await;
            assert!(
                cache.is_empty(),
                "physically late successful result must never be published"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queue_deadline_prevents_factory_call_and_reserved_query_slot_survives_background_load()
    {
        let (provider, mut release) = held();
        let cache = new_cache();
        let capacity = new_capacity(1);
        let service = new_service(provider.clone(), cache, new_lifecycle(), capacity.clone());
        let job = tokio::spawn({
            let service = service.clone();
            async move {
                service
                    .encode(b"first synthetic query".to_vec(), new_control(2000))
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), provider.entered.notified())
            .await
            .unwrap();
        assert!(matches!(
            service
                .encode(b"queued synthetic query".to_vec(), new_control(10))
                .await,
            Err(CcError::QueryTimedOut)
        ));
        let background = capacity
            .acquire_background(&new_control(100))
            .await
            .unwrap();
        release.now();
        job.await.unwrap().unwrap();
        wait_free(&capacity).await;
        assert_eq!(
            provider.inner.call_count(),
            1,
            "queued deadline must not invoke provider"
        );
        let provider = fake("synthetic/query-reservation");
        let service = new_service(
            provider.clone(),
            new_cache(),
            new_lifecycle(),
            capacity.clone(),
        );
        service
            .encode(
                b"foreground with background occupied".to_vec(),
                new_control(1000),
            )
            .await
            .unwrap();
        assert_eq!(provider.call_count(), 1);
        drop(background);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retired_instance_cannot_publish_or_serve_hits_and_new_instance_can_encode() {
        let (old_provider, mut release) = held();
        let cache = new_cache();
        let capacity = new_capacity(0);
        let old_lifecycle = new_lifecycle();
        let old = new_service(
            old_provider.clone(),
            cache.clone(),
            old_lifecycle.clone(),
            capacity.clone(),
        );
        let job = tokio::spawn({
            let old = old.clone();
            async move {
                old.encode(b"same synthetic query".to_vec(), new_control(2000))
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), old_provider.entered.notified())
            .await
            .unwrap();
        old_lifecycle.close();
        release.now();
        assert!(matches!(job.await.unwrap(), Err(CcError::QueryCancelled)));
        assert!(cache.is_empty());
        wait_free(&capacity).await;
        let provider = fake("synthetic/held-v1");
        let new = new_service(provider.clone(), cache.clone(), new_lifecycle(), capacity);
        new.encode(b"same synthetic query".to_vec(), new_control(1000))
            .await
            .unwrap();
        assert_eq!(provider.call_count(), 1);
        assert!(matches!(
            old.encode(b"same synthetic query".to_vec(), new_control(1000))
                .await,
            Err(CcError::QueryCancelled)
        ));
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn versioned_space_keys_and_oversize_skips_are_provider_safe() {
        let cache = new_cache();
        let one = fake("synthetic/revision-1");
        let two = fake("synthetic/revision-2");
        let first = new_service(one.clone(), cache.clone(), new_lifecycle(), new_capacity(0));
        let second = new_service(two.clone(), cache.clone(), new_lifecycle(), new_capacity(0));
        for service in [&first, &second] {
            service
                .encode(b"same synthetic query".to_vec(), new_control(1000))
                .await
                .unwrap();
        }
        assert_eq!(one.call_count(), 1);
        assert_eq!(two.call_count(), 1);
        assert_eq!(cache.len(), 2);
        for bytes in [vec![], vec![b'x'; 1025]] {
            assert!(matches!(
                first.encode(bytes, new_control(1000)).await.unwrap(),
                QueryEncodingOutcome::Skipped { .. }
            ));
        }
        assert_eq!(one.call_count(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn publication_barrier_drop_cancel_deadline_and_retirement_are_atomic() {
        for fault in ["drop", "cancel", "deadline", "retire"] {
            let provider = fake("synthetic/publication-barrier");
            let cache = new_cache();
            let capacity = new_capacity(0);
            let lifecycle = new_lifecycle();
            let mut service = new_service(
                provider.clone(),
                cache.clone(),
                lifecycle.clone(),
                capacity.clone(),
            );
            let entered = Arc::new(tokio::sync::Notify::new());
            let signal = entered.clone();
            let (sender, receiver) = std::sync::mpsc::channel();
            let receiver = Mutex::new(receiver);
            let mut release = Release(Some(sender));
            Arc::get_mut(&mut service).unwrap().before_publish = Some(Arc::new(move || {
                signal.notify_one();
                receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .expect("test must release publication barrier");
            }));
            let control = new_control(if fault == "deadline" { 100 } else { 2000 });
            let inside = control.clone();
            let request = tokio::spawn({
                let service = service.clone();
                async move {
                    service
                        .encode(b"synthetic query already encoded".to_vec(), inside)
                        .await
                }
            });
            tokio::time::timeout(Duration::from_secs(1), entered.notified())
                .await
                .unwrap();
            assert_eq!(
                provider.call_count(),
                1,
                "real encoding completed BEFORE the cancellation barrier"
            );
            assert!(cache.is_empty());
            match fault {
                "drop" => {
                    request.abort();
                    assert!(request.await.unwrap_err().is_cancelled());
                    assert!(!control.is_cancelled());
                }
                "cancel" => {
                    control.cancel();
                    assert!(matches!(
                        request.await.unwrap(),
                        Err(CcError::QueryCancelled)
                    ));
                }
                "deadline" => assert!(matches!(
                    request.await.unwrap(),
                    Err(CcError::QueryTimedOut)
                )),
                "retire" => {
                    lifecycle.close();
                    release.now();
                    assert!(matches!(
                        request.await.unwrap(),
                        Err(CcError::QueryCancelled)
                    ));
                }
                _ => unreachable!(),
            }
            if fault != "retire" {
                assert_eq!(capacity.query.running.available_permits(), 0);
                release.now();
            }
            wait_free(&capacity).await;
            assert!(
                cache.is_empty(),
                "post-barrier result must lose publication to drop/cancel/deadline/retirement"
            );
        }
    }

    fn root_context(
        provider: Arc<dyn EmbeddingProvider>,
        cache: Arc<QueryVectorCache>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Arc<QueryEncodingContext> {
        let inputs = inputs(provider.as_ref(), cache, new_lifecycle());
        Arc::new(QueryEncodingContext {
            factory: Arc::new(move |_, _| Ok(provider.clone())),
            cache: inputs.cache,
            namespace: inputs.namespace,
            spec: inputs.spec,
            input_budget: inputs.budget,
            lifecycle: inputs.lifecycle,
            cancellation,
            capacity: Arc::new(Semaphore::new(1)),
        })
    }
    #[tokio::test]
    async fn exact_root_interface_encodes_then_hits_cache_even_when_direct_capacity_is_full() {
        let provider = fake("synthetic/root-interface");
        let context = root_context(
            provider.clone(),
            new_cache(),
            tokio_util::sync::CancellationToken::new(),
        );
        let services = crate::service_factory::QueryServices::default();
        ensure_query_vector(
            context.clone(),
            "synthetic root query".into(),
            new_control(1000),
            services.pin(),
        )
        .await
        .unwrap();
        let occupied = context.capacity.clone().acquire_owned().await.unwrap();
        ensure_query_vector(
            context.clone(),
            "synthetic root query".into(),
            new_control(1000),
            services.pin(),
        )
        .await
        .unwrap();
        assert_eq!(provider.call_count(), 1);
        assert_eq!(services.query_pins(), 0);
        assert!(!context.cancellation.is_cancelled());
        drop(occupied);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exact_root_drop_and_project_cancel_hold_pin_capacity_and_cancel_only_child_transport()
    {
        for fault in ["drop", "project_cancel"] {
            let (provider, mut release) = held();
            let project = tokio_util::sync::CancellationToken::new();
            let mut context = root_context(provider.clone(), new_cache(), project.clone());
            let observed = Arc::new(Mutex::new(None));
            let token = observed.clone();
            let p = provider.clone();
            Arc::get_mut(&mut context).unwrap().factory = Arc::new(move |control, child| {
                assert!(!control.is_cancelled());
                *token.lock().unwrap() = Some(child);
                Ok(p.clone())
            });
            let control = new_control(2000);
            let original = control.clone();
            let services = crate::service_factory::QueryServices::default();
            let pin = services.pin();
            let request = tokio::spawn({
                let context = context.clone();
                async move {
                    ensure_query_vector(context, "synthetic held root query".into(), control, pin)
                        .await
                }
            });
            tokio::time::timeout(Duration::from_secs(1), provider.entered.notified())
                .await
                .unwrap();
            if fault == "drop" {
                request.abort();
                assert!(request.await.unwrap_err().is_cancelled());
                assert!(!project.is_cancelled());
            } else {
                project.cancel();
                assert!(matches!(
                    request.await.unwrap(),
                    Err(CcError::QueryCancelled)
                ));
            }
            assert!(
                !original.is_cancelled(),
                "optional lane must never cancel shared local fallback"
            );
            assert!(observed.lock().unwrap().as_ref().unwrap().is_cancelled());
            assert_eq!(context.capacity.available_permits(), 0);
            assert_eq!(services.query_pins(), 1);
            release.now();
            tokio::time::timeout(Duration::from_secs(1), async {
                while context.capacity.available_permits() != 1 || services.query_pins() != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(context.cache.is_empty());
        }
    }

    struct TransportRecorder(Mutex<Option<Duration>>);
    impl EmbeddingHttpTransport for TransportRecorder {
        fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            *self.0.lock().unwrap() = Some(request.timeout);
            Err(TransportError::Io(
                "synthetic-secret-query-and-credential".into(),
            ))
        }
    }
    #[test]
    fn transport_clamps_each_request_to_absolute_budget_and_redacts_errors() {
        let control = new_control(50);
        let context = QueryCallContext {
            control: control.clone(),
            abandoned: new_control(50),
            lifecycle: new_lifecycle(),
        };
        let recorder = Arc::new(TransportRecorder(Mutex::new(None)));
        let transport = context.wrap_transport(recorder.clone());
        let request = || HttpRequest {
            url: "https://synthetic.invalid/embeddings".into(),
            headers: vec![],
            body: b"synthetic".to_vec(),
            timeout: Duration::from_secs(30),
        };
        let error = transport.post_json(request()).unwrap_err();
        assert_eq!(error, TransportError::Io("query transport failed".into()));
        assert!(recorder.0.lock().unwrap().unwrap() <= Duration::from_millis(50));
        let child = tokio_util::sync::CancellationToken::new();
        let root_transport =
            query_deadline_transport(recorder.clone(), control.clone(), child.clone());
        assert!(matches!(
            root_transport.post_json(request()),
            Err(TransportError::Io(_))
        ));
        assert!(recorder.0.lock().unwrap().unwrap() <= Duration::from_millis(50));
        child.cancel();
        assert_eq!(
            root_transport.post_json(request()).unwrap_err(),
            TransportError::Cancelled
        );
        assert!(!control.is_cancelled());
        control.cancel();
        assert_eq!(
            transport.post_json(request()).unwrap_err(),
            TransportError::Cancelled
        );
    }

    #[test]
    fn capacity_rejects_invalid_limits_and_service_validates_namespace() {
        assert!(QueryNetworkCapacity::new(NetworkLimits {
            query_running: 0,
            query_queued: 1,
            background_running: 1,
            background_queued: 0
        })
        .is_err());
        assert!(QueryNetworkCapacity::new(NetworkLimits {
            query_running: 1,
            query_queued: usize::MAX,
            background_running: 1,
            background_queued: 0
        })
        .is_err());
        let provider = fake("synthetic/validation");
        let mut input = inputs(provider.as_ref(), new_cache(), new_lifecycle());
        input.namespace = "../invalid".into();
        assert!(
            QueryEncodingService::new(input, new_capacity(0), move |_| Ok(provider.clone()))
                .is_err()
        );
    }
}
