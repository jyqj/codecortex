//! P7-010 minimal semantic-subsystem assembly, extended by P7-014 into the
//! FULL composition-root wiring (组合根全链贯通腿).
//!
//! This module is the production wiring of the optional semantic subsystem
//! into the composition root:
//!
//! - **try_init / wire** (`enabled` only): `resolve_provider` (P7-002 config
//!   parsing, every capability mismatch is a startup-refusing config error)
//!   → shared provider gate/breaker init from the same `semantic.*` keys
//!   (P7-005/P7-006 singletons, first-wins) → artifact cache open under
//!   `namespace_key(project_identity)` at the resolved cache root (待办 5;
//!   `open` is side-effect free) → bounded query vector cache (P7-009) →
//!   degradation ledger with the configured re-embed budget (待办 6 预算键)
//!   → one [`ExactRecallService`]. `wire` additionally stamps the
//!   wiring-evidence slot the capability state machine reads (P7-014:
//!   attached+wired ≠ bare attached). P7-014 adds `try_init`/`teardown`
//!   as the explicit, symmetric init/teardown pair (待办 1 完成).
//! - **teardown / wire(None)**: detach symmetrically — every slot cleared,
//!   the capability probe back to its exact `not_configured` wording (V18
//!   口径零漂移), never a half-open state.
//! - **worker drain** ([`drain_worker_batch`], 待办 2/3): one explicit,
//!   bounded `drain_pending` round whose per-task pre-check quarantines a
//!   detected-corrupt artifact through the P6-018 sanctioned facade
//!   ([`cc_semantic::degrade::quarantine_detected`] +
//!   [`cc_semantic::degrade::requeue_after_degrade`] — a cache fault is not
//!   the task's fault, so the attempt budget is not consumed) and whose
//!   outer layer re-forwards the ledger snapshot into the
//!   `set_semantic_degradation` slot after every round (状态轮询转写).
//! - **space switch** ([`drain_revocations_with_reclaim`], 待办 13): the
//!   composition-root revocation-drain trigger — guarded against misuse
//!   (the ACTIVE space is refused; only revoked/diagnostic spaces are
//!   drainable) and followed by the explicit final-reclaim GC pass
//!   (待办 7: the revoked space's manifest rows are gone, so the pass's
//!   counters are the audit trail returned to the caller).
//! - **GC** ([`run_gc_until_exhausted`], 待办 6): `min_retention_secs`
//!   comes from the `semantic.gc_min_retention_secs` key (non-zero floor
//!   validated at assembly, key-naming config error).
//!
//! There is still NO resident thread, timer, or daemon (ADR 红线): whether
//! and when to call the drain / recovery / reconcile / GC entry points
//! stays the explicit composition-root decision (`cc-semantic` lib docs
//! "whether and when to drain stays the composition root's decision");
//! these functions ARE that decision, one call = one bounded round.
//!
//! The recall implementation is deliberately provider-FREE: the query path
//! consumes the P7-009 query vector cache ONLY. A cache miss surfaces as an
//! explicit `Unavailable` lane outcome (`query_vector_not_encoded`), never an
//! inline provider call (P7-013 ruling). No HTTP client type exists anywhere
//! on this path (acceptance: "搜索不依赖具体HTTP客户端"); the local strategy
//! never reaches the port at all.
//!
//! P7-013 degradation口径: the recall reads (never acquires) the shared
//! gate/breaker singletons and the project ledger before every scan — a
//! provider-side failure silently degrades the dense lane to an explicit
//! `Unavailable` receipt while the query keeps its local lanes, and the
//! semantic lane runs under a named share of the query-total deadline
//! (`cc_search::execution::semantic_child_budget`, consumed in
//! `query_handle.rs`).

use crate::service_factory::{QueryServices, SemanticDegradation, SemanticWiredInfo};
use cc_db::index_db::IndexDb;
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_db::semantic_outbox::ClaimedTask;
use cc_model::config::ProjectConfig;
use cc_model::query::QueryControl;
use cc_model::retrieval::{
    CandidateRef, LaneCoverage, LaneOutcome, LaneStatus, CANDIDATE_REF_SCHEMA_VERSION,
    LANE_OUTCOME_SCHEMA_VERSION,
};
use cc_model::semantic::{SemanticRecall, SemanticRequest};
use cc_model::{CcError, CcResult};
use cc_semantic::cache::{
    namespace_key, resolve_cache_root_with, ArtifactCache, CacheRead, QueryCacheKey,
    QueryVectorCache, CACHE_ROOT_ENV,
};
use cc_semantic::capability::resolve_provider;
use cc_semantic::degrade::{quarantine_detected, requeue_after_degrade, DegradationLedger};
use cc_semantic::gc::{run_gc_pass, GcConfig, GcCounters, GcPosition};
use cc_semantic::ports::{DocumentInput, EmbeddingProvider, QueryInput};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, BatchReport, EmbedHandler, LeaseGuard, TaskExit};
use cc_semantic::space_switch::{drain_space_revocations, RevocationDrainReport};
use cc_semantic::spec::{QueryEncodingSpec, VectorSpace};
use cc_semantic::types::DocSpecDigest;
use cc_semantic::vector::exact::{search_controlled, space_manifest_reads, ExactSearch};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Query-vector cache bounds (P7-009 deviation 4: programmatic assembly, no
/// `semantic.*` config key this round — the key surface stays frozen at the
/// P7-002 set until P7-014). 4096 entries / 64 MiB payload: a small multiple
/// of `semantic_top_k` session working sets, hard-bounded either way.
const QUERY_CACHE_MAX_ENTRIES: usize = 4096;
const QUERY_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Exact-scan candidate load batch upper bound (the bounded-memory knob of
/// [`ExactSearch`]; matches the P6-010 oracle-backend口径, small-scale).
const EXACT_BATCH_ROWS: usize = 256;
/// Diagnostic scoring spec of the dense lane's native scale (cosine over the
/// frozen space; fusion never mixes it with lexical scores).
const SCORING_SPEC: &str = "cosine-exact-v1";
/// The one lane id every semantic receipt must carry.
const LANE_ID: &str = "semantic";

/// Fenced-retry backoff of one composition-root worker drain round (the
/// bounded programmatic default; `semantic.worker_lease_secs` owns the
/// lease, the remaining worker bounds stay constants until a key demands
/// otherwise).
const WORKER_BACKOFF_SECS: f64 = 30.0;
/// Outbox attempt bound handed to the fenced retry (the DB-side attempt
/// budget, P6-007; the call-layer retry budget is separate, P7-006).
const WORKER_MAX_ATTEMPTS: u32 = 3;
/// GC pass batch and round caps (`collect_candidates` batch + the bounded
/// convergence loop of one final reclaim; the 256×64 shape mirrors the
/// scope guard's keyset paging budget).
const GC_BATCH_ENTRIES: usize = 256;
const GC_MAX_ROUNDS: usize = 64;

// ── provider-failure degradation (P7-013) ───────────────────────────────────
//
// Degradation口径: a provider-side failure (circuit breaker open/half-open,
// 429 gate suspension, degradation-ledger `degraded`) silently degrades the
// dense lane to an explicit `Unavailable` receipt — the whole query keeps
// running its local lanes (P7-012: non-fusable receipts cast zero RRF
// votes), and the reason stays visible on the lane surface / capability
// probe. The query path NEVER acquires a gate permit, a breaker probe slot
// or an outbox lease (P6-007 stays worker-side): it only READS health
// state, so there is no nested budget and no waiting across a transaction
// (C11).

/// Receipt reason when the shared circuit breaker is open (or half-open —
/// the provider is not proven healthy until a probe closes it again;
/// conservative, mirroring the P7-011 "疑问永不利 Complete" precedent).
pub const BREAKER_OPEN_REASON: &str = "semantic_breaker_open";
/// Receipt reason when the shared gate is inside a 429 cooldown.
pub const GATE_SUSPENDED_REASON: &str = "semantic_gate_suspended";
/// Receipt reason when the worker-side degradation ledger reports a corrupt
/// artifact (or an exhausted re-embed budget) — cache integrity is in
/// question, so the honest receipt is unavailable rather than a possibly
/// incomplete Complete.
pub const PROVIDER_DEGRADED_REASON: &str = "semantic_provider_degraded";

/// Read-only provider health view: `Some(reason)` = the dense lane must
/// degrade to `Unavailable(reason)` without scanning. Plain data seam so
/// tests inject failure states without touching the process-wide
/// gate/breaker singletons.
pub type ProviderHealth = Arc<dyn Fn() -> Option<&'static str> + Send + Sync>;

/// The degradation ruling over one health reading (order is the diagnosis
/// priority): breaker first, then the 429 gate, then the ledger.
pub fn provider_unhealthy_reason(
    breaker: &cc_semantic::providers::openai_compatible::CircuitBreaker,
    gate: &cc_semantic::admission::ProviderGate,
    ledger_degraded: bool,
) -> Option<&'static str> {
    use cc_semantic::providers::openai_compatible::{CircuitState, SystemRetryClock};
    if !matches!(breaker.state(&SystemRetryClock), CircuitState::Closed) {
        return Some(BREAKER_OPEN_REASON);
    }
    if gate.snapshot().suspended_for.is_some() {
        return Some(GATE_SUSPENDED_REASON);
    }
    if ledger_degraded {
        return Some(PROVIDER_DEGRADED_REASON);
    }
    None
}

/// One assembled semantic subsystem. Everything the full wiring (P7-014)
/// needs to reach — the recall port, the worker-side ledger and caches —
/// plus the frozen space/query/doc specs and the configured worker/GC
/// bounds the composition-root scheduling points consume.
pub struct SemanticSubsystem {
    /// The `SemanticRecall` supplier for the query-services slot.
    pub recall: Arc<ExactRecallService>,
    /// Process-lifetime degradation ledger; the re-embed budget comes from
    /// the `semantic.reembed_budget_max` key (`None` = unbounded).
    pub ledger: Arc<DegradationLedger>,
    /// The artifact cache of this project namespace (side-effect-free open).
    pub cache: Arc<ArtifactCache>,
    /// The bounded query vector cache (P7-009) the recall consumes.
    pub query_cache: Arc<QueryVectorCache>,
    /// Cache-namespace isolation key (`namespace_key(project_identity)`).
    pub namespace: String,
    /// The frozen encoding space resolved from the configuration.
    pub space: VectorSpace,
    /// The frozen query spec (space + estimator; no instruction key yet).
    pub query_spec: QueryEncodingSpec,
    /// The frozen document spec digest the worker publishes against (same
    /// space, declared input limit, workspace estimator; no instruction).
    pub doc_spec: DocSpecDigest,
    /// Worker drain lease seconds (`semantic.worker_lease_secs`).
    pub lease_secs: f64,
    /// GC fresh-timestamp grace seconds (`semantic.gc_min_retention_secs`,
    /// validated ≥ 1 at assembly).
    pub gc_min_retention_secs: i64,
}

/// Assemble the optional semantic subsystem for one project.
///
/// `Ok(None)` = the `semantic.enabled` master switch is false: nothing is
/// resolved, nothing is attached, no cache path is created (C14 default
/// no-network; V18 `not_configured` unchanged). `Err` = a config error that
/// must refuse startup (missing required keys, capability mismatches, a
/// derivable-cache-root absence): P7-002's "支持差异不能吞" — never a silent
/// default. Reads no credential and builds no transport.
pub fn assemble(
    project_identity: &str,
    config: &ProjectConfig,
    db: Arc<IndexDb>,
) -> CcResult<Option<SemanticSubsystem>> {
    assemble_with(
        project_identity,
        config,
        db,
        |var| {
            std::env::var(var)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        },
        cfg!(target_os = "macos"),
    )
}

/// Injectable form of [`assemble`] (the cache-root lookup and platform
/// layout are the test/deployment seams, mirroring
/// `cc_semantic::cache::resolve_cache_root_with`).
pub fn assemble_with(
    project_identity: &str,
    config: &ProjectConfig,
    db: Arc<IndexDb>,
    env_lookup: impl Fn(&str) -> Option<String>,
    macos_layout: bool,
) -> CcResult<Option<SemanticSubsystem>> {
    // Master switch first: with `enabled = false` the whole section is inert
    // (C14) — nothing below may run, and no cache path is created.
    if !config.semantic.enabled {
        return Ok(None);
    }
    // P7-014 (待办 6/3): the worker/GC bounds are configuration keys with a
    // structural floor — a zero GC grace is refused by name (the synchronous
    // publish/GC point alone is not sufficient at 0, P6-016 口径); the lease
    // must be a whole positive number of seconds.
    if config.semantic.gc_min_retention_secs == 0 {
        return Err(CcError::Config(
            "semantic.gc_min_retention_secs must be at least 1 (a zero grace lets a \
             concurrent publish's just-put artifact be collected)"
                .into(),
        ));
    }
    if config.semantic.worker_lease_secs == 0 {
        return Err(CcError::Config(
            "semantic.worker_lease_secs must be at least 1".into(),
        ));
    }
    // P7-002: config → (frozen space, capability sheet). Missing keys and
    // capability mismatches are config errors that name the key.
    let (space, capability) = resolve_provider(&config.semantic)?;
    // Shared provider gate / circuit breaker (P7-005/P7-006): the SAME
    // composition-root singletons the worker side uses. First-wins: if the
    // permissive/breaker defaults were already claimed by a lazy getter, the
    // explicit limits do not replace them (documented pre-existing caveat).
    // `from_provider_config` validates the `semantic.max_concurrent*` keys
    // with key-naming config errors; the limits are then re-derived for the
    // OnceLock init (same mapping, cannot fail after that validation).
    if cc_semantic::admission::ProviderGate::from_provider_config(&config.semantic)?.is_some() {
        let per_project = match config.semantic.max_concurrent_per_project {
            0 => None,
            cap => Some(cap as usize),
        };
        let limits = cc_semantic::admission::GateLimits::validated(
            config.semantic.max_concurrent as usize,
            per_project,
        )?;
        crate::service_factory::init_semantic_provider_gate(limits);
    }
    crate::service_factory::init_semantic_circuit_breaker(
        cc_semantic::providers::openai_compatible::BreakerLimits::from_provider_config(
            &config.semantic,
        )?,
    );
    // 待办 5: cache root + project identity. `None` = no derivable default —
    // a config error naming the override variable, never a silent path.
    let root = resolve_cache_root_with(env_lookup, macos_layout).ok_or_else(|| {
        CcError::Config(format!(
            "no semantic cache root derivable: set {CACHE_ROOT_ENV} to a writable directory"
        ))
    })?;
    let namespace = namespace_key(project_identity)?;
    // Side-effect free: the cache layout is created lazily by the first put,
    // so an enabled-but-unused project creates nothing on disk.
    let cache = Arc::new(ArtifactCache::open(root, namespace.clone())?);
    let query_cache = Arc::new(QueryVectorCache::new(
        QUERY_CACHE_MAX_ENTRIES,
        QUERY_CACHE_MAX_BYTES,
    ));
    // Worker-side ledger; the re-embed budget comes from the configured
    // `semantic.reembed_budget_max` key (待办 6; `None` = unbounded).
    let ledger = Arc::new(DegradationLedger::new(config.semantic.reembed_budget_max));
    // The frozen query spec the recall encodes cache keys against: bound by
    // the declared model input limit, no instruction (no config key yet),
    // and the one estimator the query planner's tokenizer gate admits.
    let query_spec = QueryEncodingSpec::new(
        space.clone(),
        None,
        capability.max_input_tokens,
        cc_model::chunk_policy::TOKEN_ESTIMATOR,
    )?;
    // The frozen document spec the worker publishes against: the same
    // frozen surface as the query spec (the estimator is the declared
    // workspace estimator, so the render side's stamped numbers and this
    // side can never disagree).
    let doc_spec = cc_semantic::spec::DocumentEncodingSpec::new(
        space.clone(),
        None,
        capability.max_input_tokens,
        cc_model::chunk_policy::TOKEN_ESTIMATOR,
    )?
    .digest()?;
    Ok(Some(SemanticSubsystem {
        recall: Arc::new(ExactRecallService {
            db,
            cache: cache.clone(),
            query_cache: query_cache.clone(),
            namespace: namespace.clone(),
            space: space.clone(),
            query_spec: query_spec.clone(),
            // P7-013: the SAME process-wide failure picture the workers use
            // (gate + breaker singletons) plus this project's ledger. Read
            // per query, never mutated here; no permit/lease is taken.
            health: {
                let gate = crate::service_factory::semantic_provider_gate();
                let breaker = crate::service_factory::semantic_circuit_breaker();
                let ledger = ledger.clone();
                Arc::new(move || {
                    provider_unhealthy_reason(&breaker, &gate, ledger.snapshot().degraded)
                })
            },
        }),
        ledger,
        cache,
        query_cache,
        namespace,
        space,
        query_spec,
        doc_spec,
        lease_secs: config.semantic.worker_lease_secs as f64,
        gc_min_retention_secs: config.semantic.gc_min_retention_secs as i64,
    }))
}

/// Wire (or unwire) the assembled subsystem into the query services.
///
/// This is the composition-root leg of wiring todo 1 (构造子系统),
/// todo 4 (degradation snapshot 桥接) and todo 5 (cache 根与项目身份传入).
/// `Ok(())` with the switch off leaves the slots empty — the capability
/// probe keeps its exact pre-wiring disabled wording.
pub fn wire(
    services: &QueryServices,
    project_identity: &str,
    config: &ProjectConfig,
    db: Arc<IndexDb>,
) -> CcResult<()> {
    wire_with(
        services,
        project_identity,
        config,
        db,
        |var| {
            std::env::var(var)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        },
        cfg!(target_os = "macos"),
    )
}

/// Injectable form of [`wire`]: the cache-root lookup is a parameter, so
/// tests never touch process environment.
pub fn wire_with(
    services: &QueryServices,
    project_identity: &str,
    config: &ProjectConfig,
    db: Arc<IndexDb>,
    env_lookup: impl Fn(&str) -> Option<String>,
    macos_layout: bool,
) -> CcResult<()> {
    // Assemble errors propagate BEFORE any slot is touched: a refused
    // config never leaves a half-wired state behind.
    match assemble_with(project_identity, config, db, env_lookup, macos_layout)? {
        Some(subsystem) => attach(services, &subsystem),
        // Off switch detaches symmetrically (P7-014 `teardown`): the
        // capability probe returns to its exact `not_configured` wording
        // (V18 口径零漂移), never a half-open state.
        None => teardown(services),
    }
    Ok(())
}

/// Attach one assembled subsystem to the query services (待办 4 bridge plus
/// the P7-014 wiring-evidence stamp): recall port, degradation snapshot,
/// and the `SemanticWiredInfo` marker that upgrades the capability state
/// machine from `port_attached_unverified` to the real states.
pub(crate) fn attach(services: &QueryServices, subsystem: &SemanticSubsystem) {
    services.set_semantic(Some(subsystem.recall.clone()));
    services.set_semantic_wired(Some(SemanticWiredInfo {
        model_id: subsystem.space.model_id().to_owned(),
        dimensions: subsystem.space.dimension(),
    }));
    services.set_semantic_degradation(Some(SemanticDegradation::from(subsystem.ledger.snapshot())));
}

/// Detach the semantic subsystem from the query services: every slot is
/// cleared symmetrically — recall port, wiring evidence, degradation
/// snapshot — so the capability probe reports its exact `not_configured` /
/// `dense_state: "disabled"` wording again (V18 口径零漂移). Idempotent and
/// side-effect free: no cache content is touched, no worker state is lost
/// (the outbox and the artifact cache outlive the process).
pub fn teardown(services: &QueryServices) {
    services.set_semantic_worker(None);
    services.set_semantic(None);
    services.set_semantic_wired(None);
    services.set_semantic_degradation(None);
}

/// Explicit configuration-driven initialization (待办 1 完成): assemble the
/// optional subsystem and attach it, returning the assembled handle for the
/// composition root's later scheduling calls. `Ok(None)` = the
/// `semantic.enabled` master switch is false: the slots are torn down
/// symmetrically (the same end state as [`teardown`]). `Err` = a config
/// error that must refuse startup; the slots are untouched.
pub fn try_init(
    services: &QueryServices,
    project_identity: &str,
    config: &ProjectConfig,
    db: Arc<IndexDb>,
) -> CcResult<Option<SemanticSubsystem>> {
    let subsystem = assemble(project_identity, config, db)?;
    match subsystem {
        Some(subsystem) => {
            attach(services, &subsystem);
            Ok(Some(subsystem))
        }
        None => {
            teardown(services);
            Ok(None)
        }
    }
}

impl From<cc_semantic::degrade::DegradationSnapshot> for SemanticDegradation {
    fn from(snapshot: cc_semantic::degrade::DegradationSnapshot) -> Self {
        Self {
            degraded: snapshot.degraded,
            degraded_reasons: snapshot.degraded_reasons,
            corrupt_events: snapshot.corrupt_events,
            quarantined_objects: snapshot.quarantined_objects,
            reembeds_used: snapshot.reembeds_used,
            reembed_budget: snapshot.reembed_budget,
            budget_exhausted: snapshot.budget_exhausted,
        }
    }
}

// ── composition-root scheduling (P7-014, 接线待办 2/3/6/7/13) ───────────────
//
// One call = one bounded round; no resident thread, timer, or daemon (ADR
// 红线). Recovery (`cc_semantic::recovery::recover_scan`) and post-rebuild
// reconcile (`cc_semantic::reconcile::reconcile_after_rebuild`) stay the
// caller-driven P6-014/P6-015 primitives this module documents as the
// composition root's remaining explicit decision points.

/// What one [`drain_worker_batch`] round did: the queue's own
/// [`BatchReport`] plus the degrade-facade outcome (待办 2/3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrainOutcome {
    pub batch: BatchReport,
    /// Tasks whose artifact pre-checked corrupt: quarantined (evidence +
    /// report preserved, address reads Miss again) through the sanctioned
    /// P6-018 facade.
    pub quarantined: usize,
    /// Of those, the tasks handed back to `pending` WITHOUT consuming the
    /// attempt budget (`requeue_after_degrade`); the rest were lease-lost
    /// no-ops.
    pub requeued_after_degrade: usize,
}

/// One explicit, bounded worker-drain round with the degrade facade
/// attached (接线待办 2/3). Per claimed task, before any provider contact:
/// the task's artifact is presence-checked against the subsystem's frozen
/// spec — a `Corrupt` read is quarantined through
/// `cc_semantic::degrade::quarantine_detected` (ledger degrades, object
/// moves to `<root>/quarantine/`) and the task is handed back to `pending`
/// through `requeue_after_degrade` (a cache fault is not the task's fault,
/// so the attempt budget is not consumed). Everything else runs through the
/// P6-013 reference [`EmbedHandler`]. After the round the ledger snapshot is
/// re-forwarded into `set_semantic_degradation` (状态轮询转写, 待办 4 的
/// 重转写半), so the capability probe's `degraded` state tracks the worker's
/// findings instead of staying an assembly-time snapshot.
///
/// The record-schema read (rendered embedding input for a task) stays the
/// caller's closure — the same decoupling `EmbedHandler` prescribes.
#[derive(Debug, Clone, Copy)]
pub struct WorkerDrainOptions<'a> {
    pub owner: &'a str,
    pub max_batch: usize,
    pub now_unix: i64,
}

pub fn drain_worker_batch(
    db: &Arc<IndexDb>,
    subsystem: &SemanticSubsystem,
    services: &QueryServices,
    provider: &dyn EmbeddingProvider,
    resolve_input: &dyn Fn(&ClaimedTask) -> CcResult<Option<DocumentInput>>,
    options: WorkerDrainOptions<'_>,
) -> CcResult<DrainOutcome> {
    let WorkerDrainOptions {
        owner,
        max_batch,
        now_unix,
    } = options;
    let limits = cc_semantic::queue::WorkerLimits::validated(
        max_batch,
        subsystem.lease_secs,
        WORKER_BACKOFF_SECS,
        WORKER_MAX_ATTEMPTS,
    )?;
    // The publish CAS fence compares against the incarnation on the spot
    // (a ghost process is refused with zero writes, P6-014 fence).
    let incarnation = db.reads().read_generation()?.incarnation;
    let publisher = Publisher::new(
        db,
        &subsystem.cache,
        &subsystem.space,
        &subsystem.doc_spec,
        incarnation,
    )?;
    let handler = EmbedHandler::new(publisher, provider, resolve_input);
    let mut quarantined = 0usize;
    let mut requeued_after_degrade = 0usize;
    let report = drain_pending(db, owner, &limits, &mut |guard: &LeaseGuard<'_>| {
        // Degrade pre-check: a corrupt artifact must never reach the
        // provider. Quarantine + requeue WITHOUT consuming the attempt
        // budget; the task was disposed by the degrade path itself, so the
        // loop must not touch it again.
        let task = guard.task();
        let input = cc_semantic::degrade::task_input(task);
        if let CacheRead::Corrupt(report) =
            subsystem
                .cache
                .get(&subsystem.space, &input, &subsystem.doc_spec)?
        {
            let record = quarantine_detected(
                &subsystem.cache,
                &subsystem.ledger,
                &subsystem.space,
                &input,
                &subsystem.doc_spec,
                &report,
                now_unix,
            )?;
            quarantined += usize::from(record.is_some());
            if requeue_after_degrade(
                db,
                task,
                "semantic degrade: corrupt cache artifact quarantined, task requeued",
            )? {
                requeued_after_degrade += 1;
            }
            return Ok(TaskExit::Disposed);
        }
        handler.handle(guard)
    })?;
    // 状态轮询转写: the probe's degradation view follows the worker, not
    // the assembly instant.
    services.set_semantic_degradation(Some(SemanticDegradation::from(subsystem.ledger.snapshot())));
    Ok(DrainOutcome {
        batch: report,
        quarantined,
        requeued_after_degrade,
    })
}

/// Explicit GC convergence (接线待办 6): bounded rounds of `run_gc_pass`
/// until the namespace tree is exhausted or the round cap trips. `now_unix`
/// is the caller's clock (P6-007 discipline); the grace comes from the
/// configured `semantic.gc_min_retention_secs`. The accumulated
/// [`GcCounters`] are the audit trail (待办 7) returned to the caller.
pub fn run_gc_until_exhausted(
    db: &IndexDb,
    subsystem: &SemanticSubsystem,
    now_unix: i64,
) -> CcResult<GcCounters> {
    let cfg = GcConfig {
        min_retention_secs: subsystem.gc_min_retention_secs,
        batch_entries: GC_BATCH_ENTRIES,
        now_unix,
    };
    let mut total = GcCounters::default();
    let mut after: Option<GcPosition> = None;
    for _ in 0..GC_MAX_ROUNDS {
        let (counters, resume, _exhausted) =
            run_gc_pass(db, &subsystem.cache, &cfg, after.as_ref())?;
        total.kept_fresh += counters.kept_fresh;
        total.kept_referenced += counters.kept_referenced;
        total.kept_live_task += counters.kept_live_task;
        total.deleted_objects += counters.deleted_objects;
        total.deleted_halves += counters.deleted_halves;
        total.deleted_temps += counters.deleted_temps;
        total.pruned_dirs += counters.pruned_dirs;
        match resume {
            Some(position) => after = Some(position),
            None => return Ok(total),
        }
    }
    Ok(total)
}

/// What one [`drain_revocations_with_reclaim`] round did: the space-switch
/// revocation drain plus the final-reclaim GC pass over the now-unmarked
/// objects (待办 13 / 待办 7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RevocationReclaimReport {
    pub revocations: RevocationDrainReport,
    pub gc: GcCounters,
}

/// The composition-root space-switch revocation trigger (接线待办 13):
/// drain one (usually `revoked`) space's revoke tasks, then run the
/// explicit final reclaim — the revoked space's manifest rows are gone, so
/// the GC pass's mark finds nothing and the objects become collectable.
///
/// Misuse guard (待办 13): the ACTIVE space is refused by name. The
/// revocation drain exists for the revocation window only — claiming the
/// active space here would steal embed tasks from the real worker
/// (`claim_semantic_space` is explicitly the non-active-space primitive).
pub fn drain_revocations_with_reclaim(
    db: &Arc<IndexDb>,
    subsystem: &SemanticSubsystem,
    services: &QueryServices,
    space_id: &str,
    owner: &str,
    max_batch: usize,
    now_unix: i64,
) -> CcResult<RevocationReclaimReport> {
    if db.semantic_active_space()?.as_deref() == Some(space_id) {
        return Err(CcError::InvalidParams(format!(
            "revocation drain refuses the ACTIVE space {space_id}: only revoked or \
             diagnostic spaces are drainable (use drain_worker_batch for the active space)"
        )));
    }
    let revocations = drain_space_revocations(
        db,
        space_id,
        owner,
        subsystem.lease_secs,
        WORKER_BACKOFF_SECS,
        WORKER_MAX_ATTEMPTS,
        max_batch,
    )?;
    let gc = run_gc_until_exhausted(db, subsystem, now_unix)?;
    // The reclaim can move the visible set only inside the drain's own
    // consume transactions; the snapshot refresh keeps the probe current.
    services.set_semantic_degradation(Some(SemanticDegradation::from(subsystem.ledger.snapshot())));
    Ok(RevocationReclaimReport { revocations, gc })
}

/// The production `SemanticRecall` supplier: query vector from the P7-009
/// cache only → P6 filtered exact search over the published manifest ×
/// artifact cache (C09 hard scope before top-k) → versioned candidate
/// receipts (doc version / source span / coverage). Contains no provider and
/// no transport type; the caller (lane adapter + `append_semantic_outcome`)
/// re-verifies generation and identity against its own DB.
#[derive(Clone)]
pub struct ExactRecallService {
    db: Arc<IndexDb>,
    cache: Arc<ArtifactCache>,
    query_cache: Arc<QueryVectorCache>,
    namespace: String,
    space: VectorSpace,
    query_spec: QueryEncodingSpec,
    /// P7-013 degradation seam: consulted before any scan; `Some(reason)`
    /// short-circuits the lane into an `Unavailable(reason)` receipt.
    health: ProviderHealth,
}

impl ExactRecallService {
    /// Cache-miss outcome: the query path never encodes inline. This is a
    /// per-query `Unavailable` (C10: "no result" ≠ "did not run"), cacheable
    /// no, retried no — P7-012/P7-013 own the inline-encoding ruling.
    fn unavailable(&self, reason: &str, started: std::time::Instant) -> LaneOutcome {
        LaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: LANE_ID.into(),
            weight: 1.0,
            status: LaneStatus::Unavailable,
            elapsed_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            candidate_count: 0,
            coverage: LaneCoverage::not_run(),
            truncation_reason: Some(reason.into()),
            candidates: Vec::new(),
        }
    }
}

impl ExactRecallService {
    async fn recall_on(
        &self,
        request: SemanticRequest,
        control: QueryControl,
        pool: cc_search::execution::ExecutionPool,
    ) -> CcResult<LaneOutcome> {
        control.check()?;
        let service = self.clone();
        // run_cpu cancels its own control when the waiting future is
        // dropped. Give admission an independent token so a lane timeout
        // cannot cancel the parent's local fallback. The worker still
        // checks the original child deadline and parent cancellation.
        let admission = QueryControl::new(control.remaining())?;
        let inside = control.clone();
        cc_search::execution::until(
            &control,
            pool.run_cpu(admission, move || service.recall_blocking(request, inside)),
        )
        .await?
    }

    /// Entire synchronous scan/receipt assembly lives on the bounded worker.
    fn recall_blocking(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> CcResult<LaneOutcome> {
        control.check()?;
        let started = std::time::Instant::now();
        // 0. P7-013 degradation gate: a provider-side failure (breaker
        //    open/half-open, 429 gate pause, ledger degraded) silently
        //    degrades THIS query's dense lane to an explicit
        //    `Unavailable` receipt — before any scan, before any lock,
        //    before the query vector cache is even read. The query
        //    keeps its local lanes; the reason stays on the receipt.
        let unhealthy = (self.health)();
        control.check()?;
        if let Some(reason) = unhealthy {
            return Ok(self.unavailable(reason, started));
        }
        // 1. Query vector from the P7-009 cache ONLY — no provider, no
        //    transport, nothing HTTP-shaped on this path.
        let input = QueryInput::from_bytes(request.query.as_bytes())?;
        let key = QueryCacheKey::new(&self.namespace, &self.query_spec, &input)?;
        let Some(vector) = self.query_cache.get(&key) else {
            return Ok(self.unavailable("query_vector_not_encoded", started));
        };
        control.check()?;
        // 2. Filtered exact top-k (C09: hard scope before top-k) over the
        //    published manifest × artifact cache, under one short DB read
        //    connection. Freshness stays with the caller: the lane
        //    adapter re-verifies `request.generation` against the live
        //    generation after this returns. The scope declaration (P7-011,
        //    wiring todo 12) runs on its own checkout AFTER the scan
        //    released its connection (never a nested checkout); the
        //    adapter's generation re-verification covers the interleave.
        let space_digest = self.space.digest()?;
        let scored = {
            let conn = self.db.read_conn()?;
            let manifest = SemanticManifestReads::on(&conn);
            let scoped = space_manifest_reads(&manifest, &space_digest);
            search_controlled(
                &self.cache,
                &scoped,
                ExactSearch {
                    space: &self.space,
                    query: &vector.data,
                    filter: &request.scope,
                    k: request.limit,
                    batch_rows: EXACT_BATCH_ROWS,
                },
                &control,
            )?
        };
        // P7-013 recall checkpoint: the total deadline owns the whole
        // lane (encoding → recall → fusion contribution); expiry here
        // propagates as `QueryTimedOut`, which the lane adapter turns
        // into a Timeout receipt instead of blocking the query.
        control.check()?;
        let declaration = crate::semantic_scope_guard::declare(
            &self.db,
            space_digest.as_str(),
            &request.scope,
            scored.len(),
        )?;
        // P7-011 scope fence: a recall whose space is not the active
        // publishing space returns nothing (P6-017 混排拒绝), never a
        // Complete-over-nothing receipt.
        if !declaration.status.is_fusable() {
            return Ok(LaneOutcome {
                schema_version: LANE_OUTCOME_SCHEMA_VERSION,
                lane_id: LANE_ID.into(),
                weight: 1.0,
                status: declaration.status,
                elapsed_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                candidate_count: 0,
                coverage: declaration.coverage,
                truncation_reason: declaration.truncation_reason,
                candidates: Vec::new(),
            });
        }
        // 3. Versioned identity for the receipts (doc version / source
        //    span). The exact backend speaks doc_keys; the sanctioned
        //    candidate reader speaks chunk_ids, so the doc_key →
        //    chunk_id mapping comes from the document manifest first
        //    (a manifest row without a current document is an integrity
        //    error — the caller re-verifies every reference against its
        //    own DB regardless).
        let doc_keys: Vec<&str> = scored.iter().map(|doc| doc.doc_key.as_str()).collect();
        control.check()?;
        let chunk_ids = self.db.retrieval().chunk_ids_by_doc_keys(&doc_keys)?;
        let mut missing: std::collections::HashSet<&str> = doc_keys.iter().copied().collect();
        for (doc_key, _) in &chunk_ids {
            missing.remove(doc_key.as_str());
        }
        if !missing.is_empty() {
            return Err(CcError::Search(
                "semantic candidate is not a current document".into(),
            ));
        }
        let chunk_id_refs: Vec<&str> = chunk_ids.iter().map(|(_, id)| id.as_str()).collect();
        let rows = self
            .db
            .retrieval()
            .chunk_candidate_rows_by_ids(&chunk_id_refs)?;
        let mut rows_by_doc_key: HashMap<&str, &_> = rows
            .iter()
            .map(|row| (row.document.doc_key.as_str(), row))
            .collect();
        let mut candidates = Vec::with_capacity(scored.len());
        for (rank, doc) in scored.iter().enumerate() {
            control.check()?;
            let row = rows_by_doc_key
                .remove(doc.doc_key.as_str())
                .ok_or_else(|| {
                    CcError::Search("semantic candidate is not a current document".into())
                })?;
            candidates.push(CandidateRef {
                schema_version: CANDIDATE_REF_SCHEMA_VERSION,
                document: row.document.clone(),
                source_span: row.source_evidence.span,
                legacy_chunk_id: row.chunk_id.clone(),
                lane_id: LANE_ID.into(),
                lane_rank: rank + 1,
                raw_score: doc.score,
                scoring_spec: SCORING_SPEC.into(),
                exact_identity: false,
            });
        }
        // The scope declaration owns the receipt's honesty: an exact scan
        // over a fully published hard scope stays Complete (top-k is the
        // requested limit, not a truncation); partial publication
        // coverage of the scope surfaces as Partial +
        // `semantic_coverage_uncovered` — never a Complete receipt over
        // missing vectors (P6-018 口径, recall layer).
        control.check()?;
        Ok(LaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: LANE_ID.into(),
            weight: 1.0,
            status: declaration.status,
            elapsed_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            candidate_count: candidates.len(),
            coverage: declaration.coverage,
            truncation_reason: declaration.truncation_reason,
            candidates,
        })
    }
}

impl SemanticRecall for ExactRecallService {
    fn recall(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(self.recall_on(request, control, crate::service_factory::query_pool()))
    }
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "semantic"))]
mod tests {
    use super::*;
    use crate::service_factory::query_pool;
    use cc_model::query::{QueryConfig, RetrievalStrategy};
    use cc_model::retrieval::HardScope;
    use cc_model::search::SearchRequest;
    use cc_semantic::admission::InputBudget;
    use cc_semantic::cache::{encode_queries, QueryEncodeOutcome};
    use cc_semantic::ports::EmbeddingProvider;
    use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
    use cc_semantic::spec::{input_bytes_digest, DocumentEncodingSpec};
    use cc_semantic::types::InputDigest;

    fn base_config() -> ProjectConfig {
        ProjectConfig::default()
    }

    fn enabled_config() -> ProjectConfig {
        let mut config = ProjectConfig::default();
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/model-wiring".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8_192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        config
    }

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cc-server-p7010-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn fixed_lookup(root: &std::path::Path) -> impl Fn(&str) -> Option<String> + '_ {
        move |var| {
            if var == CACHE_ROOT_ENV {
                Some(root.to_string_lossy().to_string())
            } else {
                None
            }
        }
    }

    fn query_services() -> QueryServices {
        QueryServices::default()
    }

    /// Unattached services must keep the exact pre-wiring disabled wording.
    fn assert_v18_disabled(retrieval: &serde_json::Value) {
        assert_eq!(retrieval["semantic_state"], "not_configured");
        assert_eq!(retrieval["dense_state"], "disabled");
        assert_eq!(
            retrieval["dense_reason"],
            "provider_and_vector_publication_not_implemented"
        );
    }

    fn probe(services: &QueryServices, config: Option<&ProjectConfig>) -> serde_json::Value {
        crate::capability_status::snapshot(None, None, config, services)
    }

    #[test]
    fn disabled_config_keeps_the_port_unattached_and_v18_wording_unchanged() {
        let services = query_services();
        let config = base_config();
        let (dir, db) = test_db();
        let root = temp_root("disabled");
        wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &config,
            db,
            fixed_lookup(&root),
            true,
        )
        .unwrap();
        assert!(!services_attached(&services));
        assert_v18_disabled(&probe(&services, Some(&config))["retrieval"]);
    }

    #[test]
    fn missing_required_keys_are_config_errors_that_name_the_key() {
        let services = query_services();
        let (dir, db) = test_db();
        let root = temp_root("missing-keys");
        let mut config = enabled_config();
        config.semantic.model_id = String::new();
        let error = wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &config,
            db.clone(),
            fixed_lookup(&root),
            true,
        )
        .expect_err("missing model_id must refuse");
        assert!(error.to_string().contains("semantic.model_id"));
        assert!(
            !services_attached(&services),
            "a refused config never wires"
        );

        let mut config = enabled_config();
        config.semantic.dimensions = None;
        let error = wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &config,
            db,
            fixed_lookup(&root),
            true,
        )
        .expect_err("missing dimensions must refuse");
        assert!(error.to_string().contains("semantic.dimensions"));
    }

    #[test]
    fn capability_mismatch_is_a_config_error_never_a_silent_default() {
        let services = query_services();
        let (dir, db) = test_db();
        let root = temp_root("metric-mismatch");
        let mut config = enabled_config();
        config.semantic.metric = "dot".into();
        let error = wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &config,
            db,
            fixed_lookup(&root),
            true,
        )
        .expect_err("metric mismatch must refuse");
        assert!(error.to_string().contains("metric"));
        assert!(!services_attached(&services));
    }

    #[test]
    fn enabled_config_assembles_attaches_and_creates_no_cache_path() {
        let root = temp_root("assemble");
        let services = query_services();
        let config = enabled_config();
        let (dir, db) = test_db();
        wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &config,
            db,
            fixed_lookup(&root),
            true,
        )
        .unwrap();
        assert!(services_attached(&services));
        // `ArtifactCache::open` is side-effect free: no directory, no file.
        assert!(
            std::fs::read_dir(&root).unwrap().next().is_none(),
            "assembly must not create cache content before any put"
        );
        // Attached + healthy keeps the pre-P7-014 honest state string.
        assert_eq!(
            probe(&services, Some(&config))["retrieval"]["semantic_state"],
            "port_attached_unverified"
        );
        // Dense-state wording is untouched by this leg (P7-014 owns it).
        assert_eq!(
            probe(&services, Some(&config))["retrieval"]["dense_state"],
            "disabled"
        );
    }

    #[test]
    fn switching_the_switch_off_detaches_again() {
        let root = temp_root("detach");
        let services = query_services();
        let (dir, db) = test_db();
        wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &enabled_config(),
            db.clone(),
            fixed_lookup(&root),
            true,
        )
        .unwrap();
        assert!(services_attached(&services));
        wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &base_config(),
            db,
            fixed_lookup(&root),
            true,
        )
        .unwrap();
        assert!(!services_attached(&services));
        assert!(services.semantic_degradation().is_none());
        assert_v18_disabled(&probe(&services, None)["retrieval"]);
    }

    #[test]
    fn degradation_ledger_snapshot_bridges_into_the_capability_probe() {
        let root = temp_root("degrade");
        let services = query_services();
        let config = enabled_config();
        let (dir, db) = test_db();
        wire_with(
            &services,
            &dir.path().to_string_lossy(),
            &config,
            db.clone(),
            fixed_lookup(&root),
            true,
        )
        .unwrap();
        let subsystem = assemble_with(
            &dir.path().to_string_lossy(),
            &config,
            db,
            fixed_lookup(&root),
            true,
        )
        .unwrap()
        .expect("enabled config assembles");
        subsystem
            .ledger
            .note_corrupt(&InputDigest::of_input(b"corrupt-input").expect("digest"));
        // The composition root re-forwards the snapshot (one-line bridge).
        services
            .set_semantic_degradation(Some(SemanticDegradation::from(subsystem.ledger.snapshot())));
        let degradation = services.semantic_degradation().expect("bridged");
        assert!(degradation.degraded);
        let retrieval = &probe(&services, Some(&config))["retrieval"];
        assert_eq!(retrieval["semantic_state"], "degraded");
        assert!(!retrieval["degraded_reason"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fake_provider_full_chain_publish_encode_recall_hit() {
        let (world, subsystem, services) = seeded_world("full-chain").await;
        let config = enabled_config();

        // 1. document embedding (FakeProvider, no network) is already
        //    published inside seeded_world; encode the query into the
        //    subsystem's query cache (P7-009 path).
        let provider = FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone()));
        let budget = InputBudget::validated(1, cc_semantic::spec::MAX_INPUT_BYTES, 8_192).unwrap();
        let outcomes = encode_queries(
            &provider,
            &subsystem.query_cache,
            &subsystem.namespace,
            &subsystem.query_spec,
            &budget,
            &[((), QUERY_TEXT.as_bytes().to_vec())],
        )
        .unwrap();
        assert!(
            matches!(outcomes.as_slice(), [QueryEncodeOutcome::Encoded { .. }]),
            "query encoding must succeed: {outcomes:?}"
        );

        // 2. dense recall through the production port + lane receipt gate.
        let generation = world.db.reads().read_generation().expect("read generation");
        let control = QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap();
        let raw = subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY_TEXT.into(),
                    scope: HardScope::default(),
                    limit: 8,
                    policy_fingerprint: "test-fingerprint".into(),
                    generation,
                },
                control.clone(),
            )
            .await;
        let raw = match raw {
            Ok(outcome) => outcome,
            Err(error) => panic!("raw recall failed: {error:?}"),
        };
        raw.validate().expect("raw receipt validates");
        let response = cc_search::semantic_adapter::recall(
            &query_pool(),
            subsystem.recall.as_ref(),
            SemanticRequest {
                query: QUERY_TEXT.into(),
                scope: HardScope::default(),
                limit: 8,
                policy_fingerprint: "test-fingerprint".into(),
                generation,
            },
            control,
        )
        .await
        .expect("recall");
        assert_eq!(response.generation, generation);
        let outcome = &response.outcome;
        assert_eq!(outcome.lane_id, "semantic");
        assert_eq!(outcome.status, LaneStatus::Complete);
        assert_eq!(outcome.candidate_count, 1);
        assert!(outcome.coverage.complete);
        let candidate = &outcome.candidates[0];
        assert_eq!(candidate.document.doc_key, world.doc_key);
        assert_eq!(candidate.document.doc_version, world.doc_version);
        assert!(candidate.raw_score.is_finite());
        assert!(!candidate.exact_identity);

        // 3. the attached port is visible through the capability probe.
        assert_eq!(
            probe(&services, Some(&config))["retrieval"]["semantic_state"],
            "port_attached_unverified"
        );
    }

    #[tokio::test]
    async fn uncached_query_vector_is_unavailable_never_an_inline_encoding() {
        let (_world, subsystem, _services) = seeded_world("uncached").await;
        let generation = _world.db.reads().read_generation().unwrap();
        let control = QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap();
        let outcome = subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY_TEXT.into(),
                    scope: HardScope::default(),
                    limit: 8,
                    policy_fingerprint: "test".into(),
                    generation,
                },
                control,
            )
            .await
            .expect("recall result");
        assert_eq!(outcome.status, LaneStatus::Unavailable);
        assert_eq!(
            outcome.truncation_reason.as_deref(),
            Some("query_vector_not_encoded")
        );
        assert_eq!(outcome.candidate_count, 0);
        outcome.validate().expect("unavailable receipt validates");
    }

    #[tokio::test]
    async fn hard_scope_is_applied_before_top_k_on_the_wired_path() {
        let (world, subsystem, _services) = seeded_world("scoped").await;
        let provider = FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone()));
        let budget = InputBudget::validated(1, cc_semantic::spec::MAX_INPUT_BYTES, 8_192).unwrap();
        encode_queries(
            &provider,
            &subsystem.query_cache,
            &subsystem.namespace,
            &subsystem.query_spec,
            &budget,
            &[((), QUERY_TEXT.as_bytes().to_vec())],
        )
        .unwrap();
        let generation = world.db.reads().read_generation().unwrap();
        let control = QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap();
        let outcome = subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY_TEXT.into(),
                    scope: HardScope {
                        path_prefix: None,
                        languages: None,
                        file_paths: Some(vec!["src/other.rs".into()]),
                    },
                    limit: 8,
                    policy_fingerprint: "test".into(),
                    generation,
                },
                control,
            )
            .await
            .expect("recall result");
        // The published doc lives at src/d1.rs; the narrowed scope denies it.
        assert_eq!(outcome.status, LaneStatus::Complete);
        assert_eq!(outcome.candidate_count, 0);
    }

    #[test]
    fn local_strategy_never_reaches_the_semantic_port() {
        // Acceptance "local 策略不调用端口": the policy resolves Local before
        // the recall; the port exists but the query path short-circuits.
        let request = SearchRequest {
            query: "anything".into(),
            retrieval_strategy: Some(RetrievalStrategy::Local),
            ..Default::default()
        };
        let policy = cc_search::query_policy::QueryPolicy::resolve(
            &QueryConfig::default(),
            &request,
            true, // port attached
        )
        .expect("policy");
        assert_eq!(policy.effective, RetrievalStrategy::Local);
    }

    #[tokio::test]
    async fn partial_publication_coverage_is_declared_never_masqueraded() {
        // P7-011 scope guard, full chain (FakeProvider): one published plus
        // one eligible-but-unpublished document. The recall keeps its
        // candidate but must NOT claim Complete — P6-018 "绝不把缺向量当完整
        // 空结果" extended to the recall layer.
        let (world, subsystem, _services) = seeded_world("partial-coverage").await;
        seed_uncovered_document(&world.db, "src/d2.rs");

        let provider = FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone()));
        let budget = InputBudget::validated(1, cc_semantic::spec::MAX_INPUT_BYTES, 8_192).unwrap();
        encode_queries(
            &provider,
            &subsystem.query_cache,
            &subsystem.namespace,
            &subsystem.query_spec,
            &budget,
            &[((), QUERY_TEXT.as_bytes().to_vec())],
        )
        .unwrap();
        let generation = world.db.reads().read_generation().unwrap();
        let outcome = subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY_TEXT.into(),
                    scope: HardScope::default(),
                    limit: 8,
                    policy_fingerprint: "test".into(),
                    generation,
                },
                QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap(),
            )
            .await
            .expect("recall result");
        assert_eq!(outcome.status, LaneStatus::Partial);
        assert_eq!(
            outcome.truncation_reason.as_deref(),
            Some(crate::semantic_scope_guard::PARTIAL_COVERAGE_REASON)
        );
        assert!(
            !outcome.coverage.complete,
            "partial must never read complete"
        );
        assert_eq!(outcome.candidate_count, 1);
        assert_eq!(outcome.candidates[0].document.doc_key, world.doc_key);
        outcome.validate().expect("partial receipt validates");

        // The consumer-side receipt gate (semantic_adapter) admits the same
        // Partial receipt unchanged.
        let response = cc_search::semantic_adapter::recall(
            &query_pool(),
            subsystem.recall.as_ref(),
            SemanticRequest {
                query: QUERY_TEXT.into(),
                scope: HardScope::default(),
                limit: 8,
                policy_fingerprint: "test".into(),
                generation,
            },
            QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap(),
        )
        .await
        .expect("adapter recall");
        assert_eq!(response.outcome.status, LaneStatus::Partial);
        assert_eq!(response.outcome.candidate_count, 1);
    }

    #[tokio::test]
    async fn recall_from_a_non_active_space_is_unavailable_never_complete_zero() {
        // Space switch left the recall's space non-active: the vectors of a
        // non-active space are never returned (P6-017), and the honest
        // receipt is Unavailable — not Complete over nothing.
        let (world, subsystem, _services) = seeded_world("non-active-space").await;
        let space_digest = subsystem.space.digest().expect("space digest");
        let conn = rusqlite::Connection::open(world.db.admin().db_path()).expect("seed conn");
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute(
            "UPDATE semantic_spaces SET space_id='sp-switched-away' WHERE space_id=?1",
            [space_digest.as_str()],
        )
        .unwrap();
        drop(conn);

        let provider = FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone()));
        let budget = InputBudget::validated(1, cc_semantic::spec::MAX_INPUT_BYTES, 8_192).unwrap();
        encode_queries(
            &provider,
            &subsystem.query_cache,
            &subsystem.namespace,
            &subsystem.query_spec,
            &budget,
            &[((), QUERY_TEXT.as_bytes().to_vec())],
        )
        .unwrap();
        let generation = world.db.reads().read_generation().unwrap();
        let outcome = subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY_TEXT.into(),
                    scope: HardScope::default(),
                    limit: 8,
                    policy_fingerprint: "test".into(),
                    generation,
                },
                QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap(),
            )
            .await
            .expect("recall result");
        assert_eq!(outcome.status, LaneStatus::Unavailable);
        assert_eq!(
            outcome.truncation_reason.as_deref(),
            Some(crate::semantic_scope_guard::SPACE_NOT_ACTIVE_REASON)
        );
        assert_eq!(outcome.candidate_count, 0);
        outcome.validate().expect("unavailable receipt validates");
    }

    // ── P7-013 provider-failure degradation matrix ───────────────────────

    /// The recall service under test, rebuilt from the seeded world with an
    /// INJECTED health view (the production closure reads the process-wide
    /// gate/breaker singletons; tests must not trip those shared objects).
    fn service_with_health(
        world: &World,
        subsystem: &SemanticSubsystem,
        health: ProviderHealth,
    ) -> ExactRecallService {
        ExactRecallService {
            db: world.db.clone(),
            cache: subsystem.cache.clone(),
            query_cache: subsystem.query_cache.clone(),
            namespace: subsystem.namespace.clone(),
            space: subsystem.space.clone(),
            query_spec: subsystem.query_spec.clone(),
            health,
        }
    }

    fn unhealthy(reason: &'static str) -> ProviderHealth {
        Arc::new(move || Some(reason))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recall_worker_timeout_and_cancellation_keep_capacity_until_exit() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Mutex,
        };
        use std::time::Duration;

        for cancel_parent in [false, true] {
            let (world, subsystem, _services) = seeded_world("recall-cancel-worker").await;
            warm_query_cache(&subsystem).await;
            let pool = cc_search::execution::ExecutionPool::new(1, 0, 1).unwrap();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let started_tx = Mutex::new(Some(started_tx));
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let release_rx = Mutex::new(release_rx);
            let exited = Arc::new(AtomicBool::new(false));
            let health: ProviderHealth = {
                let exited = exited.clone();
                Arc::new(move || {
                    started_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                    // Synthetic blocking I/O at the worker boundary. The safety
                    // timeout prevents a failed test from leaking a stuck thread.
                    let _ = release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3));
                    exited.store(true, Ordering::Release);
                    None
                })
            };
            let service = service_with_health(&world, &subsystem, health);
            let parent = QueryControl::new(Duration::from_secs(5)).unwrap();
            let child = parent.child(Duration::from_millis(200));
            let request = SemanticRequest {
                query: QUERY_TEXT.into(),
                scope: HardScope::default(),
                limit: 8,
                policy_fingerprint: "worker-cancel-test".into(),
                generation: world.db.reads().read_generation().unwrap(),
            };
            let worker_pool = pool.clone();
            let query =
                tokio::spawn(async move { service.recall_on(request, child, worker_pool).await });
            tokio::time::timeout(Duration::from_secs(1), started_rx)
                .await
                .unwrap()
                .unwrap();
            // This current-thread runtime remains schedulable while synchronous
            // work is blocked. Parent cancellation uses the original token.
            if cancel_parent {
                parent.cancel();
            }
            let result = tokio::time::timeout(Duration::from_secs(1), query)
                .await
                .unwrap()
                .unwrap();
            assert!(if cancel_parent {
                matches!(result, Err(CcError::QueryCancelled))
            } else {
                matches!(result, Err(CcError::QueryTimedOut))
            });
            assert!(
                !exited.load(Ordering::Acquire),
                "caller returned before blocking I/O exited"
            );
            assert_eq!(pool.stats().cpu_in_flight, 1);
            assert_eq!(pool.stats().cpu_admitted, 1);
            let local = QueryControl::new(Duration::from_secs(1)).unwrap();
            assert!(matches!(
                pool.run_cpu(local, || Ok(())).await,
                Err(CcError::QueryBusy)
            ));
            if !cancel_parent {
                parent
                    .check()
                    .expect("lane timeout must leave the local fallback usable");
            }
            release_tx.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                while {
                    let stats = pool.stats();
                    stats.cpu_in_flight != 0 || stats.cpu_admitted != 0
                } {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(exited.load(Ordering::Acquire));
            let local = if cancel_parent {
                QueryControl::new(Duration::from_secs(1)).unwrap()
            } else {
                parent.clone()
            };
            assert_eq!(pool.run_cpu(local, || Ok(42)).await.unwrap(), 42);
        }
    }

    async fn warm_query_cache(subsystem: &SemanticSubsystem) {
        let provider = FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone()));
        let budget = InputBudget::validated(1, cc_semantic::spec::MAX_INPUT_BYTES, 8_192).unwrap();
        let outcomes = encode_queries(
            &provider,
            &subsystem.query_cache,
            &subsystem.namespace,
            &subsystem.query_spec,
            &budget,
            &[((), QUERY_TEXT.as_bytes().to_vec())],
        )
        .unwrap();
        assert!(matches!(
            outcomes.as_slice(),
            [QueryEncodeOutcome::Encoded { .. }]
        ));
    }

    #[test]
    fn provider_unhealthy_reason_names_the_three_failure_classes_in_priority_order() {
        use cc_semantic::admission::{GateLimits, ProviderGate};
        use cc_semantic::degrade::DegradationLedger;
        use cc_semantic::providers::openai_compatible::{
            BreakerLimits, CircuitBreaker, MockRetryClock,
        };
        use cc_semantic::types::InputDigest;
        use std::time::Duration;

        let healthy_breaker =
            CircuitBreaker::new(BreakerLimits::validated(2, Duration::from_secs(1)).unwrap());
        let healthy_gate = ProviderGate::new(GateLimits::permissive());
        let healthy_ledger = DegradationLedger::new(None);
        assert_eq!(
            provider_unhealthy_reason(
                &healthy_breaker,
                &healthy_gate,
                healthy_ledger.snapshot().degraded
            ),
            None,
            "closed breaker + live gate + clean ledger is healthy"
        );

        let tripped =
            CircuitBreaker::new(BreakerLimits::validated(2, Duration::from_secs(60)).unwrap());
        let clock = MockRetryClock::new(1_000);
        tripped.record_failure(&clock);
        tripped.record_failure(&clock);
        assert_eq!(
            provider_unhealthy_reason(&tripped, &healthy_gate, false),
            Some(BREAKER_OPEN_REASON),
            "open breaker wins the diagnosis"
        );

        let paused_gate = ProviderGate::new(GateLimits::permissive());
        paused_gate.note_rate_limited(Duration::from_secs(30));
        assert_eq!(
            provider_unhealthy_reason(&healthy_breaker, &paused_gate, false),
            Some(GATE_SUSPENDED_REASON)
        );

        let degraded_ledger = DegradationLedger::new(None);
        degraded_ledger.note_corrupt(&InputDigest::of_input(b"corrupt").unwrap());
        assert_eq!(
            provider_unhealthy_reason(&healthy_breaker, &healthy_gate, true),
            Some(PROVIDER_DEGRADED_REASON)
        );
        assert!(degraded_ledger.snapshot().degraded);
        // Breaker outranks the other two readings.
        assert_eq!(
            provider_unhealthy_reason(&tripped, &paused_gate, true),
            Some(BREAKER_OPEN_REASON)
        );
    }

    #[tokio::test]
    async fn provider_failure_states_silently_degrade_the_lane_with_distinct_reasons() {
        // The degradation matrix × the query path: breaker open / gate
        // suspended / ledger degraded each short-circuit the lane into an
        // explicit Unavailable receipt — even with a WARM query vector cache
        // that would otherwise recall fine. Local lanes are untouched by
        // construction (the receipt is simply non-fusable).
        for (reason, expected) in [
            (BREAKER_OPEN_REASON, BREAKER_OPEN_REASON),
            (GATE_SUSPENDED_REASON, GATE_SUSPENDED_REASON),
            (PROVIDER_DEGRADED_REASON, PROVIDER_DEGRADED_REASON),
        ] {
            let (world, subsystem, _services) = seeded_world("degrade-matrix").await;
            warm_query_cache(&subsystem).await;
            let generation = world.db.reads().read_generation().unwrap();
            let service = service_with_health(&world, &subsystem, unhealthy(reason));
            let outcome = service
                .recall(
                    SemanticRequest {
                        query: QUERY_TEXT.into(),
                        scope: HardScope::default(),
                        limit: 8,
                        policy_fingerprint: "test".into(),
                        generation,
                    },
                    QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap(),
                )
                .await
                .expect("degraded receipt, not an error");
            assert_eq!(outcome.status, LaneStatus::Unavailable, "reason={reason}");
            assert_eq!(outcome.truncation_reason.as_deref(), Some(expected));
            assert_eq!(outcome.candidate_count, 0);
            assert!(!outcome.coverage.complete);
            assert!(!outcome.status.is_fusable());
            outcome.validate().expect("degraded receipt validates");
        }
    }

    #[tokio::test]
    async fn degraded_receipt_flows_through_the_lane_adapter_unchanged() {
        // "explicit semantic 明确不足" face: the adapter (the query path's
        // receipt gate) passes the degraded receipt through — the failure
        // stays visible on the lane surface, nothing masquerades as
        // complete, and the envelope generation is echoed.
        let (world, subsystem, _services) = seeded_world("degrade-adapter").await;
        warm_query_cache(&subsystem).await;
        let generation = world.db.reads().read_generation().unwrap();
        let service = service_with_health(&world, &subsystem, unhealthy(BREAKER_OPEN_REASON));
        let response = cc_search::semantic_adapter::recall(
            &query_pool(),
            &service,
            SemanticRequest {
                query: QUERY_TEXT.into(),
                scope: HardScope::default(),
                limit: 8,
                policy_fingerprint: "test".into(),
                generation,
            },
            QueryControl::new(std::time::Duration::from_millis(5_000)).unwrap(),
        )
        .await
        .expect("adapter recall");
        assert_eq!(response.generation, generation);
        assert_eq!(response.outcome.status, LaneStatus::Unavailable);
        assert_eq!(
            response.outcome.truncation_reason.as_deref(),
            Some(BREAKER_OPEN_REASON)
        );
    }

    /// Seed one eligible-but-never-published document (full FK chain, the
    /// seeded_world pattern): it enters the P6-012 eligible denominator and
    /// the uncovered list, but no dense candidate can ever come from it.
    fn seed_uncovered_document(db: &Arc<IndexDb>, file_path: &str) {
        let doc_text = "fn uncovered_document() {}\n";
        let doc_key = input_bytes_digest(file_path.as_bytes()).expect("doc key hex");
        let doc_version = input_bytes_digest(b"uncovered-v1").expect("doc version hex");
        let input_digest = input_bytes_digest(doc_text.as_bytes()).expect("input digest");
        let conn = rusqlite::Connection::open(db.admin().db_path()).expect("seed connection");
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at,\
             document_spec) VALUES(?1,'rust',?2,1.0,1,'2026-01-01','spec')",
            rusqlite::params![file_path, doc_key],
        )
        .unwrap();
        let source_json = format!(
            "{{\"source\":{{\"snapshot_id\":\"{}\",\"content_digest\":\"{}\",\
             \"byte_len\":{},\"encoding\":\"utf8\"}},\"span\":{{\"start\":0,\"end\":{}}},\
             \"slice_digest\":\"{}\",\"boundary\":\"whole\",\"owner\":null,\"signature\":null}}",
            doc_key,
            doc_key,
            doc_text.len(),
            doc_text.len(),
            doc_version,
        );
        conn.execute(
            "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,\
             text,source_json) VALUES(?1,?2,'rust',0,1,1,?3,?4)",
            rusqlite::params![format!("c-{doc_key}"), file_path, doc_text, source_json],
        )
        .unwrap();
        let reference_json = format!(
            "{{\"doc_key\":\"{doc_key}\",\"doc_version\":\"{doc_version}\",\
             \"encoding_key\":\"{input_digest}\"}}"
        );
        let record_json = format!("{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}");
        conn.execute(
            "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
             reference_json,record_json) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                doc_key,
                doc_version,
                file_path,
                format!("c-{doc_key}"),
                input_digest,
                reference_json,
                record_json
            ],
        )
        .unwrap();
    }

    // ── seeded world: one project, one published document ────────────────

    const QUERY_TEXT: &str = "how does the index handle a rebuild";

    struct World {
        _dir: tempfile::TempDir,
        db: Arc<IndexDb>,
        doc_key: String,
        doc_version: String,
    }

    async fn seeded_world(tag: &str) -> (World, SemanticSubsystem, QueryServices) {
        let dir = tempfile::tempdir().expect("temp project");
        let db_path = dir.path().join("index.sqlite3");
        let db = Arc::new(IndexDb::open(&db_path).expect("open db").0);

        let space = VectorSpace::new("fake/model-wiring", 2).expect("space");
        let space_digest = space.digest().expect("space digest");
        let doc_text = "fn indexed_document() {}\n";
        let input_digest = input_bytes_digest(doc_text.as_bytes()).expect("input digest");
        let doc_key = input_bytes_digest(b"d1").expect("doc key hex");
        let doc_version = input_bytes_digest(b"d1-v1").expect("doc version hex");

        // Seed the FK chain with valid identity/source JSON, then enqueue the
        // desired embed task through the real outbox path.
        let conn = rusqlite::Connection::open(&db_path).expect("seed connection");
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space_digest.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at,\
             document_spec) VALUES('src/d1.rs','rust',?1,1.0,1,'2026-01-01','spec')",
            rusqlite::params![doc_key],
        )
        .unwrap();
        let source_json = format!(
            "{{\"source\":{{\"snapshot_id\":\"{}\",\"content_digest\":\"{}\",\
             \"byte_len\":{},\"encoding\":\"utf8\"}},\"span\":{{\"start\":0,\"end\":{}}},\
             \"slice_digest\":\"{}\",\"boundary\":\"whole\",\"owner\":null,\"signature\":null}}",
            doc_key,
            doc_key,
            doc_text.len(),
            doc_text.len(),
            doc_version,
        );
        conn.execute(
            "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,\
             text,source_json) VALUES('c-d1','src/d1.rs','rust',0,1,1,?1,?2)",
            rusqlite::params![doc_text, source_json],
        )
        .unwrap();
        let reference_json = format!(
            "{{\"doc_key\":\"{doc_key}\",\"doc_version\":\"{doc_version}\",\
             \"encoding_key\":\"{input_digest}\"}}"
        );
        let record_json = format!("{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}");
        conn.execute(
            "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
             reference_json,record_json) VALUES(?1,?2,'src/d1.rs','c-d1',?3,?4,?5)",
            rusqlite::params![
                doc_key,
                doc_version,
                input_digest,
                reference_json,
                record_json
            ],
        )
        .unwrap();
        cc_db::semantic_outbox::supersede_and_enqueue_on(
            &conn,
            &cc_db::semantic_outbox::OutboxPlan {
                upserts: &[cc_db::semantic_outbox::OutboxUpsert {
                    doc_key: doc_key.clone(),
                    doc_version: doc_version.clone(),
                    input_digest: input_digest.clone(),
                }],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .expect("enqueue");
        drop(conn);

        // Assemble the subsystem against a fixed cache root.
        let root = temp_root(tag);
        let config = enabled_config();
        let subsystem = assemble_with(
            dir.path().to_string_lossy().as_ref(),
            &config,
            db.clone(),
            fixed_lookup(&root),
            true,
        )
        .expect("assemble")
        .expect("enabled");

        // Publish the embedding through the real P6 publish CAS.
        let incarnation = incarnation_of(&db_path);
        let doc_spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
            .expect("doc spec")
            .digest()
            .expect("doc spec digest");
        let publisher = cc_semantic::publish::Publisher::new(
            &db,
            &subsystem.cache,
            &subsystem.space,
            &doc_spec,
            incarnation,
        )
        .expect("publisher");
        let conn = rusqlite::Connection::open(&db_path).expect("claim connection");
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        let task = cc_db::semantic_outbox::claim_next_on(
            &conn,
            space_digest.as_str(),
            "wiring-test",
            1_000.0,
            600.0,
        )
        .expect("claim")
        .expect("one claimed task");
        let provider = FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone()));
        let doc_input = cc_semantic::ports::DocumentInput::from_bytes(doc_text.as_bytes())
            .expect("document input");
        let vectors = provider
            .embed_documents(&[doc_input])
            .expect("fake embedding");
        let verdict = publisher
            .publish_embedding(&task, &vectors[0], 1_100)
            .expect("publish");
        assert!(
            matches!(
                verdict,
                cc_semantic::publish::PublishVerdict::Published { .. }
            ),
            "publish must succeed: {verdict:?}"
        );
        drop(conn);

        // Wire the assembled subsystem exactly as the composition root does.
        let services = query_services();
        wire_with(
            &services,
            dir.path().to_string_lossy().as_ref(),
            &config,
            db.clone(),
            fixed_lookup(&root),
            true,
        )
        .expect("wire");

        let world = World {
            _dir: dir,
            db,
            doc_key,
            doc_version,
        };
        (world, subsystem, services)
    }

    fn incarnation_of(db_path: &std::path::Path) -> [u8; 16] {
        let conn = rusqlite::Connection::open(db_path).expect("incarnation connection");
        let hex: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='index_incarnation'",
                [],
                |r| r.get(0),
            )
            .expect("incarnation metadata");
        let mut bytes = [0u8; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .expect("incarnation hex digit");
        }
        bytes
    }

    // `wire`/`wire_with` receive a real (empty) database handle; config
    // errors fire before it is ever touched.
    fn test_db() -> (tempfile::TempDir, Arc<IndexDb>) {
        let dir = tempfile::tempdir().expect("temp project");
        let db = Arc::new(
            IndexDb::open(&dir.path().join("index.sqlite3"))
                .expect("open db")
                .0,
        );
        (dir, db)
    }

    fn services_attached(services: &QueryServices) -> bool {
        services.semantic().is_some()
    }
}
