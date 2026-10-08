//! Shared query execution services. Construction starts neither threads nor network.
use cc_model::semantic::SemanticRecall;
use cc_search::execution::ExecutionPool;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock, RwLock,
};

/// One owned view pin; clones share the same lease until the last worker exits.
pub(crate) struct QueryPin(Arc<AtomicUsize>);
impl Drop for QueryPin {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn query_pool() -> ExecutionPool {
    static POOL: OnceLock<ExecutionPool> = OnceLock::new();
    POOL.get_or_init(|| ExecutionPool::new(4, 32, 8).expect("valid fixed process query limits"))
        .clone()
}

// ── shared provider concurrency gate (P7-005) ──────────────────────────────
//
// The provider gate is a PROCESS SINGLETON held by the composition root
// ("避免每调用创建独立无限信号量"): every project's worker drains acquire
// their permits from this one instance, so the global cap and the
// per-project fair share actually bind across projects. Until the semantic
// wiring lands (P7-010/P7-014), the lazy default is the PERMISSIVE gate —
// 限流默认关闭, admission never waits, and zero state exists before the
// first call (no thread, no daemon: the gate is a passive, called
// component).
#[cfg(feature = "semantic")]
mod provider_gate {
    use cc_semantic::admission::{GateLimits, ProviderGate};
    use cc_semantic::providers::openai_compatible::{BreakerLimits, CircuitBreaker};
    use std::sync::{Arc, OnceLock};

    fn slot() -> &'static OnceLock<Arc<ProviderGate>> {
        static GATE: OnceLock<Arc<ProviderGate>> = OnceLock::new();
        &GATE
    }

    /// The process-wide shared gate. Read-only getters create a permissive
    /// instance without committing an explicit policy. All callers share it.
    pub fn semantic_provider_gate() -> Arc<ProviderGate> {
        slot()
            .get_or_init(|| Arc::new(ProviderGate::new(GateLimits::permissive())))
            .clone()
    }

    /// Adopt explicit caps only while the shared gate is idle. Equal explicit
    /// caps are idempotent; conflicts and busy adoption propagate as config errors.
    pub fn init_semantic_provider_gate(
        limits: GateLimits,
    ) -> cc_model::CcResult<Arc<ProviderGate>> {
        let gate = semantic_provider_gate();
        gate.configure_limits(limits)?;
        Ok(gate)
    }

    /// Default/unlimited providers inherit the shared policy, including caps
    /// adopted after their construction. Never give production wrappers None.
    pub fn configured_semantic_provider_gate(
        config: &cc_model::config::SemanticProviderConfig,
    ) -> cc_model::CcResult<Arc<ProviderGate>> {
        let limits = if config.enabled && config.max_concurrent > 0 {
            GateLimits::validated(
                config.max_concurrent as usize,
                (config.max_concurrent_per_project > 0)
                    .then_some(config.max_concurrent_per_project as usize),
            )
            .map_err(|error| {
                cc_model::CcError::Config(format!("semantic.max_concurrent*: {error}"))
            })?
        } else {
            GateLimits::permissive()
        };
        init_semantic_provider_gate(limits)
    }

    // ── shared provider circuit breaker (P7-006) ────────────────────────
    //
    // The breaker is a PROCESS SINGLETON held at the SAME composition-root
    // point as the gate ("断路器状态由组合根单例持有，与 ProviderGate 同点
    // 装配，共享生命周期"): every worker's retrying provider admits through
    // this one instance, so consecutive-failure counts and open windows are
    // one shared failure picture, never one private breaker per call.
    // Lazy default = the conservative breaker (5 consecutive failures,
    // 30s window); it is a passive, called component — no thread, timer,
    // or daemon, and its clock is injected by the caller.

    fn breaker_slot() -> &'static OnceLock<Arc<CircuitBreaker>> {
        static BREAKER: OnceLock<Arc<CircuitBreaker>> = OnceLock::new();
        &BREAKER
    }

    /// The process-wide shared circuit breaker. First initialization wins;
    /// later callers (and the default path) share the same instance.
    pub fn semantic_circuit_breaker() -> Arc<CircuitBreaker> {
        breaker_slot()
            .get_or_init(|| {
                Arc::new(CircuitBreaker::new(
                    BreakerLimits::validated(
                        cc_model::config::SemanticProviderConfig::default()
                            .breaker_failure_threshold,
                        std::time::Duration::from_millis(
                            cc_model::config::SemanticProviderConfig::default().breaker_open_ms,
                        ),
                    )
                    .expect("default breaker limits are valid"),
                ))
            })
            .clone()
    }

    /// Initialize the shared breaker with explicit limits (the semantic
    /// configuration's `breaker_*` keys, mapped by
    /// `BreakerLimits::from_provider_config` at wiring time). Idempotent,
    /// first-wins, sharing the gate's lifecycle point.
    pub fn init_semantic_circuit_breaker(limits: BreakerLimits) -> Arc<CircuitBreaker> {
        breaker_slot()
            .get_or_init(|| Arc::new(CircuitBreaker::new(limits)))
            .clone()
    }
}

#[cfg(feature = "semantic")]
pub use provider_gate::{
    configured_semantic_provider_gate, init_semantic_circuit_breaker, init_semantic_provider_gate,
    semantic_circuit_breaker, semantic_provider_gate,
};
/// Plain-data evidence that the attached semantic port came from a
/// successful composition-root wiring (`semantic.enabled = true`, config
/// validated at `set_project`), not from a host-side test injection.
/// P7-014: the capability state machine upgrades an attached+wired port to
/// the real `ready`/`backfilling` states, while a bare attached port keeps
/// the honest `port_attached_unverified` wording. Absent in the default
/// build — the default probe wording is byte-identical (V18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticWiredInfo {
    pub model_id: String,
    pub dimensions: u32,
}

/// Worker failure state owned by one runtime incarnation. A retired worker
/// updates only its own Arc, so it cannot overwrite a newly wired project.
#[derive(Default)]
pub struct SemanticWorkerStatus(std::sync::atomic::AtomicU8);
impl SemanticWorkerStatus {
    #[cfg(feature = "semantic")]
    pub(crate) fn clear(&self) {
        self.0.store(0, Ordering::Release);
    }
    #[cfg(feature = "semantic")]
    pub(crate) fn assembly_failed(&self) {
        self.0.store(1, Ordering::Release);
    }
    #[cfg(feature = "semantic")]
    pub(crate) fn round_failed(&self) {
        self.0.store(2, Ordering::Release);
    }
    #[cfg(feature = "semantic")]
    pub(crate) fn gc_failed(&self) {
        self.0.store(3, Ordering::Release);
    }
    pub fn failure_reason(&self) -> Option<&'static str> {
        match self.0.load(Ordering::Acquire) {
            1 => Some("semantic_provider_assembly_failed"),
            2 => Some("semantic_worker_round_failed"),
            3 => Some("semantic_worker_gc_failed"),
            _ => None,
        }
    }
}
pub struct QueryServices {
    pub pool: ExecutionPool,
    semantic: RwLock<Option<Arc<dyn SemanticRecall>>>,
    /// Composition-root wiring evidence for the attached port (see
    /// [`SemanticWiredInfo`]); `None` = attached-only or unattached.
    semantic_wired: RwLock<Option<SemanticWiredInfo>>,
    semantic_worker: RwLock<Option<Arc<SemanticWorkerStatus>>>,
    query_encoding_lifecycle: RwLock<Option<Arc<cc_db::semantic_publish::LifecycleFence>>>,
    /// Optional semantic-subsystem degradation snapshot (P6-018). Plain
    /// data, no cc-semantic dependency: the composition root (when the
    /// optional `semantic` feature is wired) forwards
    /// `cc_semantic::degrade::DegradationLedger::snapshot()` here;
    /// `capability_status` surfaces it as `semantic_state: "degraded"` +
    /// `degraded_reason`. Absent by default — the default build reports no
    /// degradation and never touches a cache path.
    semantic_degradation: RwLock<Option<SemanticDegradation>>,
    pins: Arc<AtomicUsize>,
}

/// Degradation snapshot of the optional semantic subsystem (P6-018 口径,
/// plain data on purpose): `degraded` is true once a corrupt cache artifact
/// was detected (or the re-embed budget ran out); plain cache misses never
/// degrade — they are the normal cold-cache state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticDegradation {
    pub degraded: bool,
    pub degraded_reasons: Vec<String>,
    pub corrupt_events: u64,
    pub quarantined_objects: u64,
    pub reembeds_used: u64,
    pub reembed_budget: Option<u64>,
    pub budget_exhausted: bool,
}

impl Default for QueryServices {
    fn default() -> Self {
        Self {
            pool: query_pool(),
            semantic: RwLock::new(None),
            semantic_wired: RwLock::new(None),
            semantic_worker: RwLock::new(None),
            query_encoding_lifecycle: RwLock::new(None),
            semantic_degradation: RwLock::new(None),
            pins: Arc::default(),
        }
    }
}
impl QueryServices {
    pub(crate) fn pin(&self) -> Arc<QueryPin> {
        self.pins.fetch_add(1, Ordering::AcqRel);
        Arc::new(QueryPin(self.pins.clone()))
    }
    pub fn query_pins(&self) -> usize {
        self.pins.load(Ordering::Acquire)
    }

    pub fn with_pool(pool: ExecutionPool) -> Self {
        Self {
            pool,
            semantic: RwLock::new(None),
            semantic_wired: RwLock::new(None),
            semantic_worker: RwLock::new(None),
            query_encoding_lifecycle: RwLock::new(None),
            semantic_degradation: RwLock::new(None),
            pins: Arc::default(),
        }
    }
    pub fn semantic(&self) -> Option<Arc<dyn SemanticRecall>> {
        self.semantic
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn set_semantic(&self, port: Option<Arc<dyn SemanticRecall>>) {
        *self.semantic.write().unwrap_or_else(|p| p.into_inner()) = port;
    }
    /// Wiring evidence of the attached port (P7-014); `None` detaches it.
    pub fn semantic_wired(&self) -> Option<SemanticWiredInfo> {
        self.semantic_wired
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn set_semantic_wired(&self, wired: Option<SemanticWiredInfo>) {
        *self
            .semantic_wired
            .write()
            .unwrap_or_else(|p| p.into_inner()) = wired;
    }
    pub fn semantic_worker(&self) -> Option<Arc<SemanticWorkerStatus>> {
        self.semantic_worker
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub(crate) fn query_encoding_active(&self) -> bool {
        self.query_encoding_lifecycle
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.is_open())
    }
    #[cfg(feature = "semantic")]
    pub(crate) fn set_query_encoding_lifecycle(
        &self,
        lifecycle: Option<Arc<cc_db::semantic_publish::LifecycleFence>>,
    ) {
        *self
            .query_encoding_lifecycle
            .write()
            .unwrap_or_else(|p| p.into_inner()) = lifecycle;
    }
    #[cfg(feature = "semantic")]
    pub(crate) fn set_semantic_worker(&self, state: Option<Arc<SemanticWorkerStatus>>) {
        *self
            .semantic_worker
            .write()
            .unwrap_or_else(|p| p.into_inner()) = state;
    }
    pub fn semantic_degradation(&self) -> Option<SemanticDegradation> {
        self.semantic_degradation
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn set_semantic_degradation(&self, snapshot: Option<SemanticDegradation>) {
        *self
            .semantic_degradation
            .write()
            .unwrap_or_else(|p| p.into_inner()) = snapshot;
    }
    /// Hold the port's read guard through projection update. Replacement
    /// takes its write guard, so an old instance cannot overwrite live state.
    #[cfg(feature = "semantic")]
    pub(crate) fn set_semantic_degradation_for(
        &self,
        owner: &Arc<dyn SemanticRecall>,
        snapshot: SemanticDegradation,
    ) {
        let port = self.semantic.read().unwrap_or_else(|p| p.into_inner());
        if port.as_ref().is_some_and(|live| Arc::ptr_eq(live, owner)) {
            self.set_semantic_degradation(Some(snapshot));
        }
    }
}

#[cfg(feature = "semantic")]
#[cfg(test)]
mod provider_gate_tests {
    use super::{
        init_semantic_circuit_breaker, init_semantic_provider_gate, semantic_circuit_breaker,
        semantic_provider_gate,
    };
    use cc_semantic::admission::GateLimits;
    use std::sync::Arc;

    #[test]
    fn the_provider_gate_is_one_process_wide_shared_instance() {
        // 组合根单例: repeated access hands out the SAME gate, so per-project
        // workers share one global cap instead of building private
        // unlimited semaphores.
        let first = semantic_provider_gate();
        let second = semantic_provider_gate();
        assert!(Arc::ptr_eq(&first, &second));
        let initialized = init_semantic_provider_gate(GateLimits::permissive()).unwrap();
        assert!(Arc::ptr_eq(&initialized, &semantic_provider_gate()));
        // Explicit-policy adoption is exercised in fresh test processes.
    }

    #[test]
    fn the_circuit_breaker_is_one_process_wide_shared_instance() {
        // P7-006: same composition-root point as the gate, same lifecycle —
        // repeated access and explicit init hand out the SAME breaker, so
        // every retrying provider sees one shared failure picture.
        let first = semantic_circuit_breaker();
        let second = semantic_circuit_breaker();
        assert!(Arc::ptr_eq(&first, &second));
        let initialized = init_semantic_circuit_breaker(
            cc_semantic::providers::openai_compatible::BreakerLimits::validated(
                3,
                std::time::Duration::from_millis(1_000),
            )
            .unwrap(),
        );
        assert!(
            Arc::ptr_eq(&initialized, &semantic_circuit_breaker()),
            "init is idempotent first-wins and later getters share the instance"
        );
        // The lazy default is the conservative breaker, and it shares no
        // storage slot confusion with the gate (independent singletons).
        assert_eq!(
            semantic_circuit_breaker().limits().failure_threshold,
            5,
            "conservative default: 5 consecutive failures"
        );
    }
}
