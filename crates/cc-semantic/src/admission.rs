//! Caller-side input-size admission and deterministic batch planning (P7-003).
//!
//! `ports.rs:94-95` froze the split of responsibility — "the caller
//! guarantees batch size (P6-013 admission), implementations do not split
//! batches". This module is that caller-side guarantee as code: it measures
//! every input at its **final rendered size** (the exact bytes the provider
//! will see), explicitly skips inputs that can never fit any batch, and cuts
//! the remainder into legal batches.
//!
//! ## Measurement and estimation口径 (honest declaration)
//!
//! - **bytes are hard.** `max_bytes` is enforced on exact rendered input
//!   bytes, and the frozen per-input structural bound
//!   ([`crate::spec::MAX_INPUT_BYTES`]) is re-checked as a backstop. A bytes
//!   verdict from this module is a guarantee, not an approximation.
//! - **tokens are an estimate, never a count.** No real tokenizer ships in
//!   this workspace. The estimate reuses the codebase-wide frozen estimator
//!   `utf8-bytes-div-ceil-4-v1` ([`cc_model::chunk_policy::TOKEN_ESTIMATOR`]:
//!   1 token ≈ 4 UTF-8 bytes, ceiling). That estimator can **undercount**
//!   real tokenizers on multi-byte scripts (CJK: ≈ 1 token per character at
//!   3 bytes per character ⇒ bytes/4 < characters). `max_tokens` is
//!   therefore an advisory budget under the declared estimator only; the
//!   planner refuses any other tokenizer name ([`tokenizer_gate`]) rather
//!   than silently approximating a tokenizer it does not have. Differences
//!   against provider-reported/billed tokens are the receipt layer's concern
//!   (P7-008 reported/estimated split), not admission's.
//!
//! ## Skip, never truncate, never pool
//!
//! An input that cannot fit any batch is returned as
//! [`PlannedInput::Skipped`] with a structured [`OversizeReason`] — it is
//! never silently dropped, averaged into a pool, or truncated here.
//! (Render-layer metadata truncation is a separate, explicitly flagged
//! mechanism: `cc_index::documents::render` stamps `metadata_truncated`;
//! source text is never truncated at all — it re-chunks or fails.)
//!
//! ## Determinism
//!
//! The same input slice under the same budget always yields the same plan:
//! order-preserving first-fit greedy fill, a batch closed exactly when the
//! next item cannot join it, skipped items recorded in place.
//!
//! ## Worker seam (P6-013 drain, unchanged by design)
//!
//! The delivered worker drain embeds ONE input per attempt
//! (`queue::EmbedHandler` — a single-input batch). That口径 trivially
//! satisfies every [`InputBudget`] bound (1 ≤ any valid `max_items`), so no
//! worker upgrade is required this round. The batching seam for a
//! composition root that aggregates several claimed/rendered inputs is
//! exactly [`plan_document_batches`] → feed each [`BatchPlan`] to
//! `EmbeddingProvider::embed_documents` unsplit.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use cc_model::chunk_policy::TOKEN_ESTIMATOR;
use cc_model::config::SemanticProviderConfig;
use cc_model::{CcError, CcResult};

use crate::capability::ModelCapability;
use crate::ports::{DocumentInput, QueryInput};
use crate::spec::MAX_INPUT_BYTES;

/// Batch admission budget: the per-batch bounds the caller guarantees to the
/// provider. Every bound is structural (≥ 1) after [`InputBudget::validated`]
/// and is defensively re-checked by the planners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBudget {
    /// Maximum number of inputs in one batch.
    pub max_items: usize,
    /// Maximum total rendered input bytes in one batch (hard bound).
    pub max_bytes: usize,
    /// Maximum total estimated tokens in one batch, under the declared
    /// [`TOKEN_ESTIMATOR`] estimator. Advisory: see the module docs.
    pub max_tokens: usize,
}

impl InputBudget {
    /// Validated constructor: every bound must be structurally meaningful.
    pub fn validated(max_items: usize, max_bytes: usize, max_tokens: usize) -> CcResult<Self> {
        let budget = Self {
            max_items,
            max_bytes,
            max_tokens,
        };
        budget.check()?;
        Ok(budget)
    }

    /// Derive the batch budget from a P7-002 declared [`ModelCapability`]:
    /// item and token bounds come straight from the capability sheet; the
    /// bytes bound stays an operator decision (capabilities declare tokens,
    /// not bytes).
    pub fn from_capability(
        capability: &ModelCapability,
        max_batch_bytes: usize,
    ) -> CcResult<Self> {
        Self::validated(
            capability.max_batch_items as usize,
            max_batch_bytes,
            capability.max_input_tokens as usize,
        )
    }

    fn check(&self) -> CcResult<()> {
        if self.max_items == 0 || self.max_bytes == 0 || self.max_tokens == 0 {
            return Err(CcError::InvalidParams(
                "admission budget bounds must all be at least 1".into(),
            ));
        }
        Ok(())
    }
}

/// Why one input can never be admitted, in any batch. Structured so the
/// skip is auditable and countable (coverage denominators), never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OversizeReason {
    /// Empty input: never a legal embedding input (spec v1 requires
    /// non-empty canonical UTF-8).
    EmptyInput,
    /// The input alone exceeds the byte bound (`budget.max_bytes`, or the
    /// frozen [`MAX_INPUT_BYTES`] backstop reported as `max_bytes`).
    BytesTooLarge {
        bytes: usize,
        max_bytes: usize,
    },
    /// The input's estimated token count exceeds `budget.max_tokens` under
    /// the declared estimator. An estimate — see the module docs.
    TokensTooLarge {
        estimated_tokens: u32,
        max_tokens: usize,
    },
}

impl OversizeReason {
    /// Stable one-line description for logs, coverage reports and dead-letter
    /// reasons.
    pub fn describe(&self) -> String {
        match self {
            OversizeReason::EmptyInput => "empty input".into(),
            OversizeReason::BytesTooLarge { bytes, max_bytes } => {
                format!("{bytes} bytes exceed the {max_bytes}-byte budget")
            }
            OversizeReason::TokensTooLarge {
                estimated_tokens,
                max_tokens,
            } => format!(
                "estimated {estimated_tokens} tokens exceed the {max_tokens}-token budget \
                 (estimator {TOKEN_ESTIMATOR}, not a tokenizer count)"
            ),
        }
    }
}

/// One planned document batch: key-paired, provider-ready inputs in input
/// order. The planner guarantees every bound of the driving [`InputBudget`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchPlan<K> {
    pub items: Vec<(K, DocumentInput)>,
}

impl<K> BatchPlan<K> {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.items.iter().map(|(_, input)| input.bytes.len()).sum()
    }

    /// Sum of per-input estimates under the declared estimator. Not a
    /// tokenizer count.
    pub fn total_estimated_tokens(&self) -> u32 {
        self.items
            .iter()
            .map(|(_, input)| estimate_tokens(&input.bytes))
            .sum()
    }
}

/// One document-path planning outcome, in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedInput<K> {
    /// A legal batch: feed `items` to `embed_documents` unsplit.
    Batch(BatchPlan<K>),
    /// The input can never fit any batch; explicit, never silent.
    Skipped {
        key: K,
        reason: OversizeReason,
    },
}

/// One planned query batch (query-path dual of [`BatchPlan`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryBatchPlan<K> {
    pub items: Vec<(K, QueryInput)>,
}

impl<K> QueryBatchPlan<K> {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One query-path planning outcome, in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedQueryInput<K> {
    Batch(QueryBatchPlan<K>),
    Skipped {
        key: K,
        reason: OversizeReason,
    },
}

/// Declared token estimate for one input: `utf8-bytes-div-ceil-4-v1`
/// (bytes/4, ceiling) — the codebase-wide frozen estimator. An estimate, not
/// a tokenizer count; see the module docs.
pub fn estimate_tokens(bytes: &[u8]) -> u32 {
    u32::try_from(bytes.len().div_ceil(4)).unwrap_or(u32::MAX)
}

/// Admission knows exactly one estimation basis: the declared workspace
/// estimator. A caller that declares any other tokenizer is refused with a
/// named-config error instead of being served a mislabeled estimate.
pub fn tokenizer_gate(tokenizer: &str) -> CcResult<()> {
    if tokenizer != TOKEN_ESTIMATOR {
        return Err(CcError::InvalidParams(format!(
            "admission has no real tokenizer; only the declared workspace estimator \
             \"{TOKEN_ESTIMATOR}\" is supported (requested: \"{tokenizer}\")"
        )));
    }
    Ok(())
}

// ── planning core ──────────────────────────────────────────────────────────

/// Per-item admission of FINAL rendered input bytes against the batch
/// budget. Returns `(bytes, estimated_tokens)` on admit; a structured
/// [`OversizeReason`] on refuse. Bounds are checked budget-first, with the
/// frozen [`MAX_INPUT_BYTES`] as an explicit structural backstop (a loose
/// operator budget can never smuggle an over-frozen-bound input through).
fn admit_item(bytes: &[u8], budget: &InputBudget) -> Result<(usize, u32), OversizeReason> {
    if bytes.is_empty() {
        return Err(OversizeReason::EmptyInput);
    }
    let tokens = estimate_tokens(bytes);
    if bytes.len() > budget.max_bytes {
        return Err(OversizeReason::BytesTooLarge {
            bytes: bytes.len(),
            max_bytes: budget.max_bytes,
        });
    }
    if tokens as usize > budget.max_tokens {
        return Err(OversizeReason::TokensTooLarge {
            estimated_tokens: tokens,
            max_tokens: budget.max_tokens,
        });
    }
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(OversizeReason::BytesTooLarge {
            bytes: bytes.len(),
            max_bytes: MAX_INPUT_BYTES,
        });
    }
    Ok((bytes.len(), tokens))
}

/// Deterministic order-preserving first-fit fill over the rendered inputs.
/// Returns `(batch index groups, skips)` — groups are contiguous-in-input
/// index lists; skips carry the input position and its structured reason.
fn plan_order<K>(
    rendered: &[(K, Vec<u8>)],
    budget: &InputBudget,
) -> (Vec<Vec<usize>>, Vec<(usize, OversizeReason)>) {
    let len = rendered.len();
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut skips: Vec<(usize, OversizeReason)> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_bytes = 0usize;
    let mut current_tokens = 0usize;
    for index in 0..len {
        match admit_item(rendered[index].1.as_slice(), budget) {
            Err(reason) => skips.push((index, reason)),
            Ok((item_bytes, item_tokens)) => {
                let must_close = current.len() == budget.max_items
                    || current_bytes + item_bytes > budget.max_bytes
                    || current_tokens + item_tokens as usize > budget.max_tokens;
                if must_close && !current.is_empty() {
                    batches.push(std::mem::take(&mut current));
                    current_bytes = 0;
                    current_tokens = 0;
                }
                current.push(index);
                current_bytes += item_bytes;
                current_tokens += item_tokens as usize;
            }
        }
    }
    if !current.is_empty() {
        batches.push(current);
    }
    (batches, skips)
}

fn check_plan_prelude(budget: &InputBudget, tokenizer: &str) -> CcResult<()> {
    tokenizer_gate(tokenizer)?;
    budget.check()
}

/// Plan the document path: measure every rendered input at its final size,
/// cut the admissible ones into legal [`BatchPlan`] batches (order kept,
/// first-fit greedy), and surface every inadmissible input as an explicit
/// [`PlannedInput::Skipped`]. Deterministic: same inputs + budget + tokenizer
/// ⇒ same plan.
pub fn plan_document_batches<K: Clone>(
    rendered: &[(K, Vec<u8>)],
    budget: &InputBudget,
    tokenizer: &str,
) -> CcResult<Vec<PlannedInput<K>>> {
    check_plan_prelude(budget, tokenizer)?;
    let (batches, skips) = plan_order(rendered, budget);
    // Interleave batches (positioned at their first input) and skips by
    // input position so the plan reads in input order.
    let mut entries: Vec<(usize, PlannedInput<K>)> = Vec::with_capacity(batches.len() + skips.len());
    for batch in batches {
        let position = batch[0];
        let mut items = Vec::with_capacity(batch.len());
        for index in batch {
            let (key, bytes) = &rendered[index];
            // Empty/oversized inputs were already refused above; from_bytes
            // can only fail on non-UTF-8 here, which is a caller bug and
            // fails the plan loudly rather than smuggling bad bytes through.
            let input = DocumentInput::from_bytes(bytes)?;
            items.push((key.clone(), input));
        }
        entries.push((position, PlannedInput::Batch(BatchPlan { items })));
    }
    for (index, reason) in skips {
        entries.push((
            index,
            PlannedInput::Skipped {
                key: rendered[index].0.clone(),
                reason,
            },
        ));
    }
    entries.sort_by_key(|(position, _)| *position);
    Ok(entries.into_iter().map(|(_, planned)| planned).collect())
}

/// Query-path dual of [`plan_document_batches`]: identical measurement and
/// cut, [`QueryInput`] digest binding on the outputs.
pub fn plan_query_batches<K: Clone>(
    rendered: &[(K, Vec<u8>)],
    budget: &InputBudget,
    tokenizer: &str,
) -> CcResult<Vec<PlannedQueryInput<K>>> {
    check_plan_prelude(budget, tokenizer)?;
    let (batches, skips) = plan_order(rendered, budget);
    let mut entries: Vec<(usize, PlannedQueryInput<K>)> =
        Vec::with_capacity(batches.len() + skips.len());
    for batch in batches {
        let position = batch[0];
        let mut items = Vec::with_capacity(batch.len());
        for index in batch {
            let (key, bytes) = &rendered[index];
            let input = QueryInput::from_bytes(bytes)?;
            items.push((key.clone(), input));
        }
        entries.push((position, PlannedQueryInput::Batch(QueryBatchPlan { items })));
    }
    for (index, reason) in skips {
        entries.push((
            index,
            PlannedQueryInput::Skipped {
                key: rendered[index].0.clone(),
                reason,
            },
        ));
    }
    entries.sort_by_key(|(position, _)| *position);
    Ok(entries.into_iter().map(|(_, planned)| planned).collect())
}

// ── provider concurrency gate (P7-005) ─────────────────────────────────────
//
// ## Thread model (Q4 定案)
//
// `EmbeddingProvider` is a SYNCHRONOUS trait (`ports.rs:99-106`, frozen):
// an embed call blocks its calling thread for the network round-trip. C11
// (`02-CONTRACTS.md:87-91`) forbids running that call while holding any DB
// lock, connection, or SQL transaction. The split of responsibility this
// round freezes is:
//
// - **The gate only admits.** [`ProviderGate`] is a passive, called
//   component: it bounds how many provider calls may be in flight and in
//   which fair order admission is granted. It spawns no thread, timer, or
//   daemon (ADR red line: no implicit resident process) — a caller waits on
//   its OWN thread inside [`ProviderGate::try_acquire_permit`], and the wait
//   ends in bounded time ([`GateAcquireError::Timeout`]) or immediately
//   ([`GateAcquireError::Suspended`], the 429 pause).
// - **The caller decides the execution context.** This round's only callers
//   are the composition root's EXPLICIT drains (`queue.rs`: claim → renew →
//   handler → fenced ack; no resident loop). Whether those handlers run
//   directly on the drain caller's thread or on a dedicated bounded blocking
//   pool (`std::thread` pool / `spawn_blocking`) is entirely the composition
//   root's decision (P7-010/P7-014 wiring); the gate neither knows nor cares.
//   The query path (P7-013) must NOT inline a blocking encode on a query
//   thread — the planning default (OPEN-QUESTIONS Q4 ②) stands: cache misses
//   leave the dense lane `Unavailable`, encoding happens worker-side.
// - **Admission waits never span DB state.** The gate holds no DB handle,
//   lock, or transaction (structural: its public surface accepts none), and
//   the wired worker order is acquire-then-claim: a caller acquires its
//   permit BEFORE `LeaseGuard::claim` so the wait can never outlive a lease
//   or sit inside a transaction. The tests below fix that call shape.

/// Limits of the shared provider concurrency gate (P7-005). Validated by
/// [`GateLimits::validated`]; [`GateLimits::permissive`] is the default-off
/// configuration (`semantic.max_concurrent = 0`): unlimited concurrency,
/// admission never waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateLimits {
    /// Hard process-wide cap on concurrent provider calls.
    pub max_concurrent: usize,
    /// Per-project share. `Some(cap)` with `cap < max_concurrent` is what
    /// makes "one project cannot starve the others" structural (V20): a
    /// single project can never hold every permit. `None` = no per-project
    /// split (the global cap alone applies).
    pub max_concurrent_per_project: Option<usize>,
}

impl GateLimits {
    /// Default-off limits: unlimited concurrency, no per-project split.
    pub fn permissive() -> Self {
        Self {
            max_concurrent: usize::MAX,
            max_concurrent_per_project: None,
        }
    }

    /// Validated constructor. `max_concurrent` must be ≥ 1; a per-project
    /// cap, when set, must be ≥ 1 and STRICTLY smaller than
    /// `max_concurrent` (a cap ≥ max could never prevent one project from
    /// holding the whole gate, which is exactly the starvation V20 forbids).
    pub fn validated(
        max_concurrent: usize,
        max_concurrent_per_project: Option<usize>,
    ) -> CcResult<Self> {
        if max_concurrent == 0 {
            return Err(CcError::InvalidParams(
                "provider gate max_concurrent must be at least 1 (0 means unlimited: \
                 leave the gate unconfigured instead)"
                    .into(),
            ));
        }
        match max_concurrent_per_project {
            None => Ok(Self {
                max_concurrent,
                max_concurrent_per_project: None,
            }),
            Some(0) => Err(CcError::InvalidParams(
                "provider gate max_concurrent_per_project must be at least 1 \
                 (0 means unlimited: omit it instead)"
                    .into(),
            )),
            Some(cap) if cap >= max_concurrent => Err(CcError::InvalidParams(format!(
                "provider gate max_concurrent_per_project ({cap}) must be strictly \
                 smaller than max_concurrent ({max_concurrent}) or a single project \
                 can starve every other one"
            ))),
            Some(cap) => Ok(Self {
                max_concurrent,
                max_concurrent_per_project: Some(cap),
            }),
        }
    }
}

/// Why an admission attempt did not produce a permit. Never a provider
/// error: these are admission outcomes the caller routes explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateAcquireError {
    /// No permit within the caller's wait budget. Nothing was consumed and
    /// nothing is pending — the caller retries or hands its task back.
    Timeout {
        /// Wall-clock time actually spent waiting.
        waited: Duration,
    },
    /// The gate is PAUSED because the provider answered
    /// `RateLimited` (P7-001 maps 429 + `Retry-After`): admission fails fast
    /// instead of parking threads, so the worker can hand its task back
    /// through the fenced retry (P7-006 owns the retry loop). The pause
    /// expires by itself when the cooldown elapses ([`ProviderGate::resume`]
    /// exists for tests/operators).
    Suspended {
        /// Remaining global cooldown.
        cooldown_remaining: Duration,
    },
    /// The project identity is a caller-supplied string; an empty one has no
    /// namespace and is refused instead of being lumped into a fake bucket.
    EmptyProjectIdentity,
}

#[derive(Debug)]
struct Waiter {
    ticket: u64,
    project: String,
}

#[derive(Debug, Default)]
struct GateState {
    in_flight: usize,
    per_project: HashMap<String, usize>,
    /// FIFO of waiting callers; a freshly arriving caller joins the BACK
    /// whenever anyone waits (no barging). [`core_scan`] serves from the
    /// front but SKIPS a temporarily blocked head (project cap full), so one
    /// project's full share never blocks the others.
    queue: VecDeque<Waiter>,
    granted_tickets: HashSet<u64>,
    /// Global 429 cooldown deadline (`None` = running normally).
    cooldown_until: Option<Instant>,
    next_ticket: u64,
}

#[derive(Debug)]
struct GateCore {
    limits: GateLimits,
    state: Mutex<GateState>,
    released: Condvar,
}

/// The process-wide shared provider gate (P7-005): ONE instance per
/// process, assembled and owned by the composition root
/// (`cc-server::service_factory::semantic_provider_gate`) — never one
/// semaphore per call ("避免每调用创建独立无限信号量"). Semaphore-style
/// admission over provider calls: RAII [`ProviderPermit`], global cap,
/// per-project fair share, and the 429 pause.
///
/// Passive by construction: no thread, timer, or daemon lives here (the
/// red line "限流器是被调用组件"); all blocking happens on the CALLING
/// thread inside [`ProviderGate::try_acquire_permit`].
#[derive(Debug, Clone)]
pub struct ProviderGate {
    core: std::sync::Arc<GateCore>,
}

/// RAII admission permit: one provider call slot. Dropping it releases the
/// slot and wakes the fair queue. A permit is bound to the project identity
/// it was acquired for and is neither `Clone` nor transferable.
#[derive(Debug)]
pub struct ProviderPermit {
    gate: std::sync::Arc<GateCore>,
    project: String,
}

impl ProviderGate {
    /// Construct from validated limits; see [`GateLimits::validated`] for
    /// the structural rules (and [`ProviderGate::validated`] for the
    /// fallible shorthand).
    pub fn new(limits: GateLimits) -> Self {
        Self {
            core: std::sync::Arc::new(GateCore {
                limits,
                state: Mutex::new(GateState::default()),
                released: Condvar::new(),
            }),
        }
    }

    /// Fallible constructor shorthand over [`GateLimits::validated`].
    pub fn validated(
        max_concurrent: usize,
        max_concurrent_per_project: Option<usize>,
    ) -> CcResult<Self> {
        Ok(Self::new(GateLimits::validated(
            max_concurrent,
            max_concurrent_per_project,
        )?))
    }

    /// Map the `semantic.*` configuration keys (P7-002 section, P7-005
    /// keys) onto a gate. `Ok(None)` = 限流关闭: the provider is not enabled
    /// or `max_concurrent` is left at the default `0` (unlimited) — callers
    /// then run without a gate at all. Any invalid combination is a config
    /// error that names the key, never a silent fallback.
    pub fn from_provider_config(config: &SemanticProviderConfig) -> CcResult<Option<Self>> {
        if !config.enabled || config.max_concurrent == 0 {
            return Ok(None);
        }
        let per_project = match config.max_concurrent_per_project {
            0 => None,
            cap => Some(cap as usize),
        };
        Self::validated(config.max_concurrent as usize, per_project).map(Some)
    }

    /// Try to acquire one permit for `project`, waiting at most `wait`.
    /// Blocking happens on the CALLING thread and ends in one of: a permit
    /// (RAII), [`GateAcquireError::Timeout`],
    /// [`GateAcquireError::Suspended`] (429 pause — fail fast, do not park
    /// provider-bound threads), or [`GateAcquireError::EmptyProjectIdentity`].
    ///
    /// Order of admission: strict FIFO over waiting callers, except a head
    /// whose project share is momentarily full is skipped (公平轮转) —
    /// waiting never starves behind a single project's backlog, and a fresh
    /// caller never barges ahead of an already-waiting one.
    pub fn try_acquire_permit(
        &self,
        project: &str,
        wait: Duration,
    ) -> Result<ProviderPermit, GateAcquireError> {
        let project = project.trim();
        if project.is_empty() {
            return Err(GateAcquireError::EmptyProjectIdentity);
        }
        let started = Instant::now();
        let deadline = Instant::now().checked_add(wait);
        let core = &*self.core;
        let mut st = core.state.lock().unwrap_or_else(|p| p.into_inner());

        // Fast path — only when nobody waits, so a fresh caller can never
        // barge ahead of the fair queue.
        if Self::suspended_for(&mut st).is_none()
            && st.queue.is_empty()
            && Self::admits(&core.limits, &st, project)
        {
            Self::consume(&mut st, project);
            return Ok(ProviderPermit {
                gate: self.core.clone(),
                project: project.to_owned(),
            });
        }

        let ticket = st.next_ticket;
        st.next_ticket += 1;
        st.queue.push_back(Waiter {
            ticket,
            project: project.to_owned(),
        });
        loop {
            core_scan(core, &mut st);
            if st.granted_tickets.remove(&ticket) {
                return Ok(ProviderPermit {
                    gate: self.core.clone(),
                    project: project.to_owned(),
                });
            }
            if let Some(remaining) = Self::suspended_for(&mut st) {
                withdraw(&mut st, ticket);
                wake(core, &st);
                return Err(GateAcquireError::Suspended { cooldown_remaining: remaining });
            }
            if let Some(deadline) = deadline {
                let now = Instant::now();
                if now >= deadline {
                    withdraw(&mut st, ticket);
                    wake(core, &st);
                    return Err(GateAcquireError::Timeout {
                        waited: started.elapsed(),
                    });
                }
                let (guard, _) = core
                    .released
                    .wait_timeout(st, deadline - now)
                    .unwrap_or_else(|p| p.into_inner());
                st = guard;
            } else {
                // Unbounded wait ([`Self::acquire_permit`]): re-check the
                // fair queue periodically.
                let (guard, _) = core
                    .released
                    .wait_timeout(st, Duration::from_secs(1))
                    .unwrap_or_else(|p| p.into_inner());
                st = guard;
            }
        }
    }

    /// Acquire a permit, waiting indefinitely. Only the 429 pause and an
    /// empty project identity can fail this — a timeout cannot (the caller
    /// asked to wait). Callers that need bounded admission should prefer
    /// [`ProviderGate::try_acquire_permit`].
    pub fn acquire_permit(&self, project: &str) -> Result<ProviderPermit, GateAcquireError> {
        loop {
            match self.try_acquire_permit(project, Duration::from_secs(1)) {
                Ok(permit) => return Ok(permit),
                Err(GateAcquireError::Timeout { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// 429 协同 (pause/降档): the provider answered `RateLimited` carrying a
    /// `Retry-After` (P7-001 maps the header into
    /// `ProviderError::RateLimited`). The gate records a GLOBAL cooldown —
    /// the quota being exhausted is the shared provider account's, not one
    /// project's — and admission then fails fast for everyone until it
    /// expires. Repeated reports EXTEND the pause to the latest deadline
    /// (max, never shorter). In-flight permits are not revoked; they finish
    /// naturally. The retry/backoff loop that consumes this signal is
    /// P7-006's.
    pub fn note_rate_limited(&self, retry_after: Duration) {
        let core = &*self.core;
        let mut st = core.state.lock().unwrap_or_else(|p| p.into_inner());
        // A pathological (huge) Retry-After saturates at one hour instead of
        // either overflowing or silently disabling the pause.
        let until = Instant::now()
            .checked_add(retry_after)
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
        let changed = match st.cooldown_until {
            Some(current) if current > until => false,
            _ => {
                st.cooldown_until = Some(until);
                true
            }
        };
        drop(st);
        if changed {
            core.released.notify_all();
        }
    }

    /// Clear a 429 pause early (operator/test escape hatch; the pause also
    /// expires by itself).
    pub fn resume(&self) {
        let core = &*self.core;
        let mut st = core.state.lock().unwrap_or_else(|p| p.into_inner());
        st.cooldown_until = None;
        core_scan(core, &mut st);
        drop(st);
        core.released.notify_all();
    }

    /// Read-only liveness view (status/observability surface).
    pub fn snapshot(&self) -> GateSnapshot {
        let core = &*self.core;
        let mut st = core.state.lock().unwrap_or_else(|p| p.into_inner());
        let suspended_for = Self::suspended_for(&mut st);
        GateSnapshot {
            max_concurrent: core.limits.max_concurrent,
            max_concurrent_per_project: core.limits.max_concurrent_per_project,
            in_flight: st.in_flight,
            waiting: st.queue.len(),
            per_project_in_flight: st.per_project.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            suspended_for,
        }
    }

    fn admits(limits: &GateLimits, st: &GateState, project: &str) -> bool {
        if st.in_flight >= limits.max_concurrent {
            return false;
        }
        match limits.max_concurrent_per_project {
            None => true,
            Some(cap) => st.per_project.get(project).copied().unwrap_or(0) < cap,
        }
    }

    fn consume(st: &mut GateState, project: &str) {
        st.in_flight += 1;
        *st.per_project.entry(project.to_owned()).or_insert(0) += 1;
    }

    /// Remaining global cooldown; lazily clears an expired one. Callers
    /// hold the state lock.
    fn suspended_for(st: &mut GateState) -> Option<Duration> {
        let until = st.cooldown_until?;
        let now = Instant::now();
        if now >= until {
            st.cooldown_until = None;
            return None;
        }
        Some(until - now)
    }
}

/// Grant pass over the fair queue: serve from the front while global
/// capacity lasts, SKIPPING a head whose project share is full (no
/// head-of-line starvation). Grants are recorded by ticket; the waiter
/// removes its own ticket on wake. No grants during a 429 pause.
fn core_scan(core: &GateCore, st: &mut GateState) {
    if st.cooldown_until.is_some() {
        return;
    }
    let mut index = 0;
    while index < st.queue.len() && st.in_flight < core.limits.max_concurrent {
        let project_ok = match core.limits.max_concurrent_per_project {
            None => true,
            Some(cap) => {
                st.per_project.get(&st.queue[index].project).copied().unwrap_or(0) < cap
            }
        };
        if project_ok {
            let waiter = st.queue.remove(index).expect("waiter present");
            ProviderGate::consume(st, &waiter.project);
            st.granted_tickets.insert(waiter.ticket);
        } else {
            index += 1;
        }
    }
}

fn withdraw(st: &mut GateState, ticket: u64) {
    st.queue.retain(|waiter| waiter.ticket != ticket);
}

fn wake(core: &GateCore, _st: &GateState) {
    core.released.notify_all();
}

impl Drop for ProviderPermit {
    fn drop(&mut self) {
        let core = &*self.gate;
        let mut st = core.state.lock().unwrap_or_else(|p| p.into_inner());
        st.in_flight = st.in_flight.saturating_sub(1);
        if let Some(count) = st.per_project.get_mut(&self.project) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                st.per_project.remove(&self.project);
            }
        }
        core_scan(core, &mut st);
        drop(st);
        core.released.notify_all();
    }
}

/// Point-in-time view of the shared gate (`snapshot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSnapshot {
    pub max_concurrent: usize,
    pub max_concurrent_per_project: Option<usize>,
    pub in_flight: usize,
    pub waiting: usize,
    pub per_project_in_flight: std::collections::BTreeMap<String, usize>,
    pub suspended_for: Option<Duration>,
}

// ── provider cost & uncertain-attempt receipts (P7-008) ────────────────────
//
// ## Receipt 口径 (frozen for the implementation record)
//
// - **Cost units are placeholder-rate.** [`PROVIDER_ATTEMPT_COST_UNITS`] —
//   one abstract unit per actual provider attempt, the exact rate P7-006's
//   `retry_max_cost_units` cap has been charging. Real tariffs replace the
//   constant, never the structure.
// - **Billed tokens are reported, estimated, or UNKNOWN — never zero.**
//   [`UsageReceipt`] keeps the P7-003 split honest: `reported` is what the
//   provider says it billed (the frozen port does not surface usage yet, so
//   producer-side receipts carry `None` until that leg lands); `estimated`
//   is the declared workspace estimator over the exact batch bytes. When
//   both are missing the cost column reads `unknown`
//   ([`UsageReceipt::billed_is_unknown`]); nothing anywhere fills a 0.
// - **Uncertain attempts are explicit, not inferred.** A breaker-open
//   refusal and a timed-out attempt both leave an `uncertain` marker
//   ([`UncertainReason`]): from the caller's seat it is undecidable whether
//   the provider received / processed / billed the work, and a later retry
//   (possibly after a restart) may pay for it twice.
// - **Cache reuse bills nothing.** A `cache_reuse` hit is recorded with
//   zero cost units. Provider-layer receipts are always `cache_reuse:
//   false` (cache hits never reach the provider); the column exists for
//   the aggregation / report surface.
// - **In-memory only, bounded.** Receipts aggregate inside this process
//   and are deliberately dropped on restart (the
//   [`crate::degrade::DegradationLedger`] precedent: process-lifetime
//   budgets do not survive restarts). The cross-restart ledger is the
//   outbox's own `attempt_count`. The ring buffer keeps the most recent
//   `capacity` receipts; the lifetime cost total and budget-refusal
//   counter survive eviction (they feed the shutdown threshold), while
//   [`ReceiptLedger::aggregate`] describes the retained window only.
// - **Zero sensitive content.** A receipt carries counters, durations,
//   outcome labels, and the space's `model_id` — never input bytes, input
//   digests, prompts, or credentials. Tests fix this for the Debug
//   surface.

/// Placeholder tariff: cost units charged per actual provider attempt
/// (P7-006's rate, now owned by the receipt layer). Real pricing swaps
/// this constant and nothing else.
pub const PROVIDER_ATTEMPT_COST_UNITS: u64 = 1;

/// What became of one provider attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// The provider returned vectors.
    Succeeded,
    /// The provider returned an error (retryable or not).
    Failed,
    /// Refused before any call: the circuit breaker was open.
    RejectedByBreaker,
    /// Refused before any call: the process cost budget is exhausted
    /// (the shutdown threshold, checked pre-call).
    RejectedByBudget,
}

/// Why an attempt's billing outcome is undecidable from the caller's seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UncertainReason {
    /// The breaker refused this call, so THIS call cost nothing — but the
    /// failures that tripped the breaker may still have been processed and
    /// billed server-side, and the work will be retried (possibly after a
    /// restart) into possible double billing.
    BreakerOpen,
    /// The attempt ended in `Timeout`: whether the provider received,
    /// processed, and billed the request is undecidable from here.
    TimeoutIndeterminate,
}

/// Which port path a receipted call traveled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptPath {
    Documents,
    Queries,
}

/// Reported-vs-estimated usage of one attempt (the P7-003 split, receipt
/// side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageReceipt {
    /// Tokens the PROVIDER reports having billed. `None` = unknown (the
    /// frozen port does not surface usage yet). Never defaulted to 0.
    pub reported: Option<u64>,
    /// Declared-estimator count over the exact batch bytes — an estimate of
    /// the INPUT, so it is present even on failures and refusals.
    pub estimated: Option<u64>,
    /// A cache hit — bills nothing by definition.
    pub cache_reuse: bool,
}

impl UsageReceipt {
    /// True when neither the provider nor the estimator can say what this
    /// attempt cost in tokens: the cost column reads `unknown`, never 0.
    pub fn billed_is_unknown(&self) -> bool {
        self.reported.is_none() && self.estimated.is_none()
    }

    /// `attempt` is the 1-based ordinal inside ONE retry sequence. Any
    /// retry whose reported usage is unknown may duplicate an
    /// already-billed prior attempt — our own estimate cannot clear that
    /// doubt, only the provider's report can.
    pub fn unknown_duplicate_risk(&self, attempt: u64) -> bool {
        attempt > 1 && self.reported.is_none()
    }
}

/// One structured receipt for ONE provider attempt (retries included —
/// every attempt gets its own receipt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCallReceipt {
    /// `VectorSpace::model_id` — configuration identity, the only label a
    /// receipt carries.
    pub space_model: String,
    pub path: ReceiptPath,
    /// Inputs in the batch (a COUNT — never their content).
    pub batch_items: usize,
    pub usage: UsageReceipt,
    /// 1-based ordinal inside ONE retry sequence (P7-006 layering: a whole
    /// sequence lives inside one outbox attempt).
    pub attempt: u64,
    pub outcome: AttemptOutcome,
    /// Attempt duration measured on the injected retry clock (0 for
    /// pre-call refusals — no round-trip happened).
    pub duration_ms: u64,
    /// Placeholder-rate charge: [`PROVIDER_ATTEMPT_COST_UNITS`] per actual
    /// provider call; 0 for pre-call refusals and cache hits.
    pub cost_units: u64,
    pub uncertain: Option<UncertainReason>,
}

/// The cost shutdown threshold: once `used_units` has reached the cap,
/// calls are refused BEFORE they are made (the
/// [`crate::degrade::DegradationLedger`] "budget rejects before the call"
/// pattern). `Some(0)` is legal and means "refuse everything".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostBudget {
    pub max_units: Option<u64>,
}

/// Outcome of a pre-call budget check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAdmission {
    Allowed,
    Exhausted { used_units: u64, cap_units: u64 },
}

impl CostBudget {
    pub fn new(max_units: Option<u64>) -> Self {
        Self { max_units }
    }

    pub fn admit(&self, used_units: u64) -> BudgetAdmission {
        match self.max_units {
            None => BudgetAdmission::Allowed,
            Some(cap) if used_units < cap => BudgetAdmission::Allowed,
            Some(cap) => BudgetAdmission::Exhausted {
                used_units,
                cap_units: cap,
            },
        }
    }
}

/// Aggregated receipt read surface over a retained time window (and, one
/// level down, per `space_model`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiptAggregate {
    pub attempts: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub rejected_by_breaker: u64,
    pub rejected_by_budget: u64,
    pub uncertain_attempts: u64,
    pub uncertain_breaker_open: u64,
    pub uncertain_timeout: u64,
    /// Placeholder-rate units charged by the retained receipts.
    pub cost_units: u64,
    pub estimated_tokens: u64,
    /// Sum of provider-reported tokens; `None` while NO retained attempt
    /// reported usage (never `Some(0)` by construction).
    pub reported_tokens: Option<u64>,
    /// Retained attempts whose reported usage is unknown — the explicit
    /// "no usage ⇒ unknown, not 0" column.
    pub attempts_with_unknown_reported: u64,
    /// Retained retries (`attempt > 1`) whose reported usage is unknown:
    /// the observable duplicate-billing risk.
    pub unknown_duplicate_risk: u64,
    pub cache_reuse_hits: u64,
    /// The same aggregation restricted to one `space_model`. Nested maps
    /// are always empty.
    pub per_space: std::collections::BTreeMap<String, ReceiptAggregate>,
}

/// Bounded in-process receipt ledger: producers record one receipt per
/// provider attempt (retries included), readers aggregate over space /
/// time windows. A passive, called component — no thread, no timer; the
/// clock reading (`now_ms`) is supplied by the caller at record time.
pub struct ReceiptLedger {
    capacity: usize,
    inner: Mutex<VecDeque<(u64, ProviderCallReceipt)>>,
    lifetime_cost_units: std::sync::atomic::AtomicU64,
    lifetime_budget_refusals: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for ReceiptLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiptLedger")
            .field("capacity", &self.capacity)
            .field("retained", &self.retained_count())
            .field("lifetime_cost_units", &self.total_cost_units())
            .field("lifetime_budget_refusals", &self.budget_refusals())
            .finish_non_exhaustive()
    }
}

impl ReceiptLedger {
    /// `capacity` must be ≥ 1; the ledger keeps the most recent
    /// `capacity` receipts and drops older ones.
    pub fn new(capacity: usize) -> CcResult<Self> {
        if capacity == 0 {
            return Err(CcError::InvalidParams(
                "receipt ledger capacity must be at least 1".into(),
            ));
        }
        Ok(Self {
            capacity,
            inner: Mutex::new(VecDeque::new()),
            lifetime_cost_units: std::sync::atomic::AtomicU64::new(0),
            lifetime_budget_refusals: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Records one receipt, stamped `now_ms` by the caller's clock. The
    /// lifetime cost / budget-refusal counters are updated even when the
    /// ring buffer evicts the receipt itself.
    pub fn record(&self, receipt: ProviderCallReceipt, now_ms: u64) {
        use std::sync::atomic::Ordering as AtomicOrdering;
        self.lifetime_cost_units
            .fetch_add(receipt.cost_units, AtomicOrdering::SeqCst);
        if receipt.outcome == AttemptOutcome::RejectedByBudget {
            self.lifetime_budget_refusals
                .fetch_add(1, AtomicOrdering::SeqCst);
        }
        let mut ring = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        ring.push_back((now_ms, receipt));
        while ring.len() > self.capacity {
            ring.pop_front();
        }
    }

    /// Lifetime charged units (survives ring-buffer eviction; feeds the
    /// [`CostBudget`] shutdown threshold).
    pub fn total_cost_units(&self) -> u64 {
        self.lifetime_cost_units
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Lifetime pre-call budget refusals (survives eviction).
    pub fn budget_refusals(&self) -> u64 {
        self.lifetime_budget_refusals
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn retained_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
    }

    /// Snapshot of the retained receipts, oldest first (the raw read
    /// surface behind [`ReceiptLedger::aggregate`]).
    pub fn retained_receipts(&self) -> Vec<ProviderCallReceipt> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|(_, receipt)| receipt.clone())
            .collect()
    }

    /// Aggregates the retained receipts recorded at `>= since_ms` (pass
    /// `None` for "everything retained"). Evicted receipts are invisible
    /// here BY DESIGN — this is the window read surface, not the lifetime
    /// budget counter.
    pub fn aggregate(&self, since_ms: Option<u64>) -> ReceiptAggregate {
        let ring = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut whole = ReceiptAggregate::default();
        for (at_ms, receipt) in ring.iter() {
            if let Some(since) = since_ms {
                if *at_ms < since {
                    continue;
                }
            }
            fold_receipt(&mut whole, receipt);
            let per_space = whole
                .per_space
                .entry(receipt.space_model.clone())
                .or_default();
            fold_receipt(per_space, receipt);
        }
        whole
    }
}

/// Folds one receipt into an aggregate (used for both the whole-window
/// total and the per-space views).
fn fold_receipt(acc: &mut ReceiptAggregate, receipt: &ProviderCallReceipt) {
    acc.attempts += 1;
    match receipt.outcome {
        AttemptOutcome::Succeeded => acc.succeeded += 1,
        AttemptOutcome::Failed => acc.failed += 1,
        AttemptOutcome::RejectedByBreaker => acc.rejected_by_breaker += 1,
        AttemptOutcome::RejectedByBudget => acc.rejected_by_budget += 1,
    }
    match receipt.uncertain {
        Some(UncertainReason::BreakerOpen) => {
            acc.uncertain_attempts += 1;
            acc.uncertain_breaker_open += 1;
        }
        Some(UncertainReason::TimeoutIndeterminate) => {
            acc.uncertain_attempts += 1;
            acc.uncertain_timeout += 1;
        }
        None => {}
    }
    acc.cost_units += receipt.cost_units;
    acc.estimated_tokens += receipt.usage.estimated.unwrap_or(0);
    match receipt.usage.reported {
        Some(tokens) => {
            acc.reported_tokens = Some(acc.reported_tokens.unwrap_or(0) + tokens);
        }
        None => acc.attempts_with_unknown_reported += 1,
    }
    if receipt
        .usage
        .unknown_duplicate_risk(receipt.attempt)
    {
        acc.unknown_duplicate_risk += 1;
    }
    if receipt.usage.cache_reuse {
        acc.cache_reuse_hits += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{DimensionsMode, EncodingFormat};
    use crate::providers::fake::{FakeProvider, FakeProviderConfig};
    use crate::providers::openai_compatible::{
        EmbeddingApiKey, EmbeddingHttpTransport, HttpRequest, HttpResponse,
        OpenAiCompatibleConfig, OpenAiCompatibleProvider, TransportError,
    };
    use crate::ports::EmbeddingProvider;
    use crate::spec::{DistanceMetric, MAX_INPUT_BYTES};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    const TOKENIZER: &str = TOKEN_ESTIMATOR;

    fn budget(max_items: usize, max_bytes: usize, max_tokens: usize) -> InputBudget {
        InputBudget::validated(max_items, max_bytes, max_tokens).expect("valid budget")
    }

    fn capability() -> ModelCapability {
        ModelCapability {
            model_id: "fake/model-a".into(),
            dimensions: 1,
            metric: DistanceMetric::Cosine,
            dimensions_mode: DimensionsMode::Configurable,
            max_input_tokens: 8_192,
            max_batch_items: 3,
            encoding_formats: vec![EncodingFormat::Float],
            supports_instruction: false,
        }
    }

    fn rendered<'a>(keys: &[&'a str], sizes: &[usize]) -> Vec<(String, Vec<u8>)> {
        keys.iter()
            .zip(sizes)
            .map(|(key, size)| (key.to_string(), vec![b'a'; *size]))
            .collect()
    }

    fn batch_keys<K: Clone>(plan: &[PlannedInput<K>]) -> Vec<Vec<K>> {
        plan.iter()
            .filter_map(|entry| match entry {
                PlannedInput::Batch(batch) => {
                    Some(batch.items.iter().map(|(key, _)| key.clone()).collect())
                }
                PlannedInput::Skipped { .. } => None,
            })
            .collect()
    }

    fn skips_of<K: Clone>(plan: &[PlannedInput<K>]) -> Vec<(&K, &OversizeReason)> {
        plan.iter()
            .filter_map(|entry| match entry {
                PlannedInput::Skipped { key, reason } => Some((key, reason)),
                PlannedInput::Batch(_) => None,
            })
            .collect()
    }

    #[test]
    fn zero_bound_budgets_are_rejected() {
        assert!(InputBudget::validated(0, 100, 100).is_err());
        assert!(InputBudget::validated(2, 0, 100).is_err());
        assert!(InputBudget::validated(2, 100, 0).is_err());
        assert!(budget(1, 1, 1).check().is_ok());
    }

    #[test]
    fn exact_limits_pass_and_one_over_is_skipped() {
        // Byte boundary: an input exactly at the batch byte budget fits a
        // single batch; one byte more can never fit and is skipped.
        let b = budget(4, 16, 1_000);
        let at_limit = vec![("ok".to_string(), vec![b'a'; 16])];
        let plan = plan_document_batches(&at_limit, &b, TOKENIZER).expect("plan");
        assert!(matches!(&plan[..], [PlannedInput::Batch(batch)] if batch.len() == 1));

        let over = vec![("big".to_string(), vec![b'a'; 17])];
        let plan = plan_document_batches(&over, &b, TOKENIZER).expect("plan");
        assert!(matches!(
            &plan[..],
            [PlannedInput::Skipped {
                reason: OversizeReason::BytesTooLarge {
                    bytes: 17,
                    max_bytes: 16
                },
                ..
            }]
        ));

        // Token boundary under the declared estimator: 4 bytes == 1 token.
        let b = budget(4, 1_000, 2);
        let at_limit = vec![("ok".to_string(), vec![b'a'; 8])];
        let plan = plan_document_batches(&at_limit, &b, TOKENIZER).expect("plan");
        assert!(matches!(&plan[..], [PlannedInput::Batch(_)]));

        let over = vec![("big".to_string(), vec![b'a'; 9])];
        let plan = plan_document_batches(&over, &b, TOKENIZER).expect("plan");
        assert!(matches!(
            &plan[..],
            [PlannedInput::Skipped {
                reason: OversizeReason::TokensTooLarge {
                    estimated_tokens: 3,
                    max_tokens: 2
                },
                ..
            }]
        ));
    }

    #[test]
    fn empty_input_is_skipped_explicitly() {
        let b = budget(4, 100, 100);
        let plan = plan_document_batches(
            &[("empty".to_string(), Vec::new()), ("ok".to_string(), b"x".to_vec())],
            &b,
            TOKENIZER,
        )
        .expect("plan");
        assert_eq!(skips_of(&plan).len(), 1);
        assert_eq!(skips_of(&plan)[0].0, "empty");
        assert_eq!(skips_of(&plan)[0].1, &OversizeReason::EmptyInput);
        assert_eq!(batch_keys(&plan), vec![vec!["ok".to_string()]]);
    }

    #[test]
    fn frozen_input_byte_bound_is_a_backstop_against_a_loose_budget() {
        // Even a loose operator budget must not smuggle an input past the
        // frozen spec v1 structural bound.
        let b = budget(4, MAX_INPUT_BYTES * 2, 1_000_000);
        let oversized = vec![("huge".to_string(), vec![b'a'; MAX_INPUT_BYTES + 1])];
        let plan = plan_document_batches(&oversized, &b, TOKENIZER).expect("plan");
        let [PlannedInput::Skipped { key, reason }] = &plan[..] else {
            panic!("expected a single skip, got {plan:?}");
        };
        assert_eq!(key, "huge");
        let OversizeReason::BytesTooLarge { bytes, max_bytes } = reason else {
            panic!("expected BytesTooLarge, got {reason:?}");
        };
        assert_eq!(*bytes, MAX_INPUT_BYTES + 1);
        assert_eq!(*max_bytes, MAX_INPUT_BYTES);
    }

    #[test]
    fn planning_is_deterministic_same_input_same_cut() {
        let b = budget(2, 10, 1_000);
        let inputs = rendered(&["a", "b", "c", "d", "e"], &[4, 4, 4, 1, 9]);
        let first = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        let second = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        assert_eq!(first, second);
    }

    #[test]
    fn empty_set_yields_nothing_and_single_item_yields_one_batch() {
        let b = budget(4, 100, 100);
        assert!(
            plan_document_batches::<String>(&[], &b, TOKENIZER)
                .expect("plan")
                .is_empty()
        );

        let single = vec![("only".to_string(), b"payload".to_vec())];
        let plan = plan_document_batches(&single, &b, TOKENIZER).expect("plan");
        assert_eq!(batch_keys(&plan), vec![vec!["only".to_string()]]);
    }

    #[test]
    fn greedy_fill_matches_the_hand_computed_plan_and_preserves_order() {
        // Item bound: 4+4+4 = 12 > 10 bytes ⇒ [a,b], [c,d]; e (9 bytes) fits
        // nowhere with a non-empty batch but alone ≤ 10 ⇒ [e]. Skips none.
        let b = budget(4, 10, 1_000);
        let inputs = rendered(&["a", "b", "c", "d", "e"], &[4, 4, 4, 1, 9]);
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        assert_eq!(
            batch_keys(&plan),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string()],
                vec!["e".to_string()],
            ]
        );

        // Item count bound dominates: max_items 2 over three small inputs.
        let b = budget(2, 100, 1_000);
        let inputs = rendered(&["a", "b", "c"], &[1, 1, 1]);
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        assert_eq!(
            batch_keys(&plan),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string()],
            ]
        );
    }

    #[test]
    fn every_planned_batch_respects_every_budget_bound() {
        let b = budget(3, 12, 5);
        let inputs = rendered(&["a", "b", "c", "d", "e", "f"], &[4, 3, 4, 4, 4, 1]);
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        for entry in &plan {
            match entry {
                PlannedInput::Batch(batch) => {
                    assert!(batch.len() <= b.max_items);
                    assert!(batch.total_bytes() <= b.max_bytes);
                    assert!(batch.total_estimated_tokens() as usize <= b.max_tokens);
                }
                PlannedInput::Skipped { .. } => {}
            }
        }
        // 4+3+4 = 11 bytes / 3 tokens fits; 4 more bytes would breach both
        // byte (15 > 12) and token (4 > 5? no: 3+1=4 ≤ 5 — byte bound closes
        // the batch) — either way every emitted batch is legal.
        assert_eq!(plan.iter().filter(|e| matches!(e, PlannedInput::Batch(_))).count(), 2);
    }

    #[test]
    fn skipped_items_never_reach_batches_and_carry_structured_reasons() {
        let b = budget(2, 10, 100);
        let inputs = rendered(&["ok1", "huge", "ok2"], &[4, 64, 4]);
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        let skips = skips_of(&plan);
        assert_eq!(skips.len(), 1);
        assert_eq!(skips[0].0, "huge");
        assert!(matches!(
            skips[0].1,
            OversizeReason::BytesTooLarge {
                bytes: 64,
                max_bytes: 10
            }
        ));
        // The skipped input's bytes appear in no batch, and the plan keeps
        // the surrounding inputs in order around the skip position.
        let all_batched: Vec<&str> = plan
            .iter()
            .flat_map(|e| match e {
                PlannedInput::Batch(batch) => {
                    batch.items.iter().map(|(key, _)| key.as_str()).collect()
                }
                PlannedInput::Skipped { .. } => Vec::new(),
            })
            .collect();
        assert_eq!(all_batched, vec!["ok1", "ok2"]);
        assert!(!skips[0].1.describe().is_empty());
    }

    #[test]
    fn digest_binding_and_provider_order_survive_planning() {
        let b = budget(2, 100, 1_000);
        let inputs: Vec<(String, Vec<u8>)> = vec![
            ("d1".to_string(), b"alpha".to_vec()),
            ("d2".to_string(), b"beta".to_vec()),
            ("d3".to_string(), b"gamma".to_vec()),
        ];
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");

        // digest ↔ bytes binding identical to a direct DocumentInput::from_bytes.
        for entry in &plan {
            if let PlannedInput::Batch(batch) = entry {
                for (key, input) in &batch.items {
                    input.verify().expect("digest binding");
                    assert_eq!(
                        input.input_digest.as_str(),
                        crate::spec::input_bytes_digest(&input.bytes).expect("digest").as_str()
                    );
                    let expected =
                        DocumentInput::from_bytes(&inputs.iter().find(|(k, _)| k == key).unwrap().1)
                            .expect("direct construction");
                    assert_eq!(input, &expected);
                }
            }
        }

        // Each planned batch feeds the provider unsplit, order preserved.
        let provider = FakeProvider::new(FakeProviderConfig::new(
            crate::spec::VectorSpace::new("fake/model-a", 1).expect("space"),
        ));
        let mut seen_order: Vec<String> = Vec::new();
        let mut provider_calls = 0usize;
        for entry in &plan {
            if let PlannedInput::Batch(batch) = entry {
                let batch_inputs: Vec<DocumentInput> =
                    batch.items.iter().map(|(_, input)| input.clone()).collect();
                let vectors = provider.embed_documents(&batch_inputs).expect("embed");
                assert_eq!(vectors.len(), batch_inputs.len());
                for (key, input) in &batch.items {
                    seen_order.push(key.clone());
                    let _ = input;
                }
                provider_calls += 1;
            }
        }
        assert_eq!(seen_order, vec!["d1", "d2", "d3"]);
        // Planner produced 2 batches → exactly 2 provider calls, no split.
        assert_eq!(provider_calls, 2);
        assert_eq!(provider.call_count(), 2);
    }

    #[test]
    fn adapter_receives_planned_batches_unsplit_in_request_order() {
        struct RecordingTransport {
            bodies: Mutex<Vec<String>>,
        }
        impl EmbeddingHttpTransport for RecordingTransport {
            fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
                let body_text = String::from_utf8(request.body).expect("utf-8 body");
                // One vector per requested input, indexes 0..n, batch order.
                let input_count = serde_json::from_str::<serde_json::Value>(&body_text)
                    .ok()
                    .and_then(|value| {
                        value.get("input").and_then(|input| input.as_array()).map(Vec::len)
                    })
                    .unwrap_or(0);
                let data: Vec<String> = (0..input_count)
                    .map(|index| {
                        format!(r#"{{"index":{index},"embedding":[0.5]}}"#)
                    })
                    .collect();
                self.bodies.lock().unwrap().push(body_text);
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![(
                        "Content-Type".to_owned(),
                        "application/json".to_owned(),
                    )],
                    body: format!(
                        r#"{{"model":"fake/model-a","data":[{}]}}"#,
                        data.join(",")
                    )
                    .into_bytes(),
                })
            }
        }

        let b = budget(2, 100, 1_000);
        let inputs: Vec<(String, Vec<u8>)> = vec![
            ("d1".to_string(), b"alpha".to_vec()),
            ("d2".to_string(), b"beta".to_vec()),
            ("d3".to_string(), b"gamma".to_vec()),
        ];
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");

        let transport = RecordingTransport {
            bodies: Mutex::new(Vec::new()),
        };
        let mut config = OpenAiCompatibleConfig::new(
            crate::spec::VectorSpace::new("fake/model-a", 1).expect("space"),
            "http://127.0.0.1:1/v1",
            EmbeddingApiKey::new("test-key"),
        );
        // Test-only endpoint on a loopback port: both egress opt-ins are
        // explicit here (P7-007 defaults would refuse the assembly).
        config.egress = crate::policy::EgressPolicy {
            network_opt_in: true,
            allow_http: true,
        };
        config.transport = Some(std::sync::Arc::new(transport));
        let provider = OpenAiCompatibleProvider::new(config);

        for entry in &plan {
            if let PlannedInput::Batch(batch) = entry {
                let batch_inputs: Vec<DocumentInput> =
                    batch.items.iter().map(|(_, input)| input.clone()).collect();
                let vectors = provider.embed_documents(&batch_inputs).expect("embed");
                assert_eq!(vectors.len(), batch_inputs.len());
            }
        }
    }

    #[test]
    fn query_path_plans_query_inputs_with_the_same_cut() {
        let b = budget(2, 10, 1_000);
        let inputs = rendered(&["q1", "q2", "q3"], &[4, 4, 4]);
        let doc_plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("doc plan");
        let query_plan = plan_query_batches(&inputs, &b, TOKENIZER).expect("query plan");

        let doc_keys: Vec<Vec<&str>> = doc_plan
            .iter()
            .filter_map(|e| match e {
                PlannedInput::Batch(batch) => Some(
                    batch
                        .items
                        .iter()
                        .map(|(key, _)| key.as_str())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .collect();
        let query_keys: Vec<Vec<&str>> = query_plan
            .iter()
            .filter_map(|e| match e {
                PlannedQueryInput::Batch(batch) => Some(
                    batch
                        .items
                        .iter()
                        .map(|(key, _)| key.as_str())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(doc_keys, query_keys);

        // Query inputs carry the query-path digest binding.
        for entry in &query_plan {
            if let PlannedQueryInput::Batch(batch) = entry {
                for (_, input) in &batch.items {
                    input.verify().expect("query digest binding");
                }
            }
        }
    }

    #[test]
    fn foreign_tokenizer_is_refused_not_approximated_silently() {
        let b = budget(4, 100, 100);
        let inputs = rendered(&["a"], &[4]);
        let err = plan_document_batches(&inputs, &b, "cl100k_base").expect_err("refused");
        let message = format!("{err}");
        assert!(message.contains(TOKENIZER), "names the supported estimator: {message}");
        assert!(message.contains("cl100k_base"), "names the request: {message}");
        assert!(plan_query_batches(&inputs, &b, "tiktoken").is_err());
    }

    #[test]
    fn estimator_matches_the_declared_bytes_div_ceil_4_v1() {
        assert_eq!(estimate_tokens(b""), 0);
        assert_eq!(estimate_tokens(b"abc"), 1);
        assert_eq!(estimate_tokens(b"abcd"), 1);
        assert_eq!(estimate_tokens(b"abcde"), 2);
        // Identical formula to the codebase-wide estimator on UTF-8 text.
        for text in ["", "a", "嵌入输入", "hello world"] {
            assert_eq!(
                estimate_tokens(text.as_bytes()),
                cc_model::approx_tokens(text)
            );
        }
    }

    #[test]
    fn from_capability_bounds_flow_through_planning() {
        let b = InputBudget::from_capability(&capability(), 1_048_576).expect("budget");
        assert_eq!(b.max_items, 3);
        assert_eq!(b.max_tokens, 8_192);
        let inputs = rendered(&["a", "b", "c", "d"], &[1, 1, 1, 1]);
        let plan = plan_document_batches(&inputs, &b, TOKENIZER).expect("plan");
        let batches = batch_keys(&plan);
        assert_eq!(batches.len(), 2);
        assert!(batches.iter().all(|batch| batch.len() <= 3));
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn budget_construction_from_capability_rejects_zero_bytes_bound() {
        assert!(InputBudget::from_capability(&capability(), 0).is_err());
    }

    // ── P7-005: provider concurrency gate ──────────────────────────────────

    fn gate(max: usize, per_project: Option<usize>) -> ProviderGate {
        ProviderGate::validated(max, per_project).expect("valid gate")
    }

    const SMALL_WAIT: Duration = Duration::from_millis(80);
    const GENEROUS_WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn degenerate_gate_limits_are_rejected_with_named_reasons() {
        assert!(GateLimits::validated(0, None).is_err());
        assert!(GateLimits::validated(2, Some(0)).is_err());
        // A per-project cap >= the global cap can never prevent one project
        // from holding the whole gate — exactly the starvation V20 forbids.
        assert!(GateLimits::validated(2, Some(2)).is_err());
        assert!(GateLimits::validated(2, Some(3)).is_err());
        assert!(GateLimits::validated(4, Some(2)).is_ok());
        assert_eq!(
            GateLimits::permissive(),
            GateLimits {
                max_concurrent: usize::MAX,
                max_concurrent_per_project: None
            }
        );
    }

    #[test]
    fn gate_is_send_sync_and_shareable_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ProviderGate>();
        assert_send_sync::<ProviderPermit>();
        // No thread is spawned by construction or admission: the gate is a
        // passive, called component (Q4 定案; 红线: 无隐式常驻线程). Four
        // racing callers share ONE gate instance; the global cap still holds.
        let shared = gate(2, None);
        let peak = std::sync::Arc::new(AtomicUsize::new(0));
        let live = std::sync::Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let gate = shared.clone();
                let peak = peak.clone();
                let live = live.clone();
                std::thread::spawn(move || {
                    let permit = gate
                        .try_acquire_permit(&format!("p{i}"), GENEROUS_WAIT)
                        .expect("admitted eventually");
                    let now = live.fetch_add(1, AtomicOrdering::AcqRel) + 1;
                    peak.fetch_max(now, AtomicOrdering::AcqRel);
                    std::thread::sleep(Duration::from_millis(30));
                    live.fetch_sub(1, AtomicOrdering::AcqRel);
                    drop(permit);
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("caller thread");
        }
        assert!(
            peak.load(AtomicOrdering::Acquire) <= 2,
            "shared gate peak {} exceeded the cap",
            peak.load(AtomicOrdering::Acquire)
        );
    }

    #[test]
    fn permissive_gate_never_makes_anyone_wait() {
        let gate = ProviderGate::new(GateLimits::permissive());
        let held: Vec<ProviderPermit> = (0..16)
            .map(|i| {
                gate.try_acquire_permit(&format!("p{i}"), Duration::ZERO)
                    .expect("permissive admission is unconditional")
            })
            .collect();
        assert_eq!(held.len(), 16);
        assert_eq!(gate.snapshot().in_flight, 16);
    }

    #[test]
    fn global_cap_bounds_total_concurrent_permits_across_projects() {
        let gate = gate(2, None);
        let peak = std::sync::Arc::new(AtomicUsize::new(0));
        let live = std::sync::Arc::new(AtomicUsize::new(0));
        let done = std::sync::Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..6)
            .map(|i| {
                let gate = gate.clone();
                let peak = peak.clone();
                let live = live.clone();
                let done = done.clone();
                std::thread::spawn(move || {
                    let permit = gate
                        .try_acquire_permit(if i % 2 == 0 { "p-a" } else { "p-b" }, GENEROUS_WAIT)
                        .expect("admitted");
                    let now = live.fetch_add(1, AtomicOrdering::AcqRel) + 1;
                    peak.fetch_max(now, AtomicOrdering::AcqRel);
                    std::thread::sleep(Duration::from_millis(30));
                    live.fetch_sub(1, AtomicOrdering::AcqRel);
                    drop(permit);
                    done.fetch_add(1, AtomicOrdering::AcqRel);
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker thread");
        }
        assert_eq!(done.load(AtomicOrdering::Acquire), 6);
        assert!(
            peak.load(AtomicOrdering::Acquire) <= 2,
            "global peak {} exceeded the cap",
            peak.load(AtomicOrdering::Acquire)
        );
    }

    #[test]
    fn per_project_cap_binds_independently_of_global_capacity() {
        let gate = gate(4, Some(1));
        let long = gate.acquire_permit("p-a").expect("first project permit");
        // Global room exists, but the project share is full: the second
        // p-a admission times out instead of exceeding its share.
        let second = gate.try_acquire_permit("p-a", SMALL_WAIT);
        assert!(
            matches!(
                second,
                Err(GateAcquireError::Timeout { waited }) if waited >= SMALL_WAIT
            ),
            "second project permit must time out, got {second:?}"
        );
        // Other projects still admit up to the global cap.
        let others: Vec<ProviderPermit> = ["p-b", "p-c", "p-d"]
            .iter()
            .map(|project| {
                gate.try_acquire_permit(project, Duration::ZERO)
                    .expect("other projects admit")
            })
            .collect();
        assert_eq!(others.len(), 3);
        assert_eq!(gate.snapshot().in_flight, 4);
        drop((long, others));
        assert_eq!(gate.snapshot().in_flight, 0);
    }

    #[test]
    fn single_project_cannot_starve_the_others() {
        // p-a burns its whole share forever; p-b and p-c keep making progress
        // on the remaining global capacity (V20 acceptance).
        let gate = gate(3, Some(1));
        let holder = gate.acquire_permit("p-a").expect("share holder");
        let progress = std::sync::Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let gate = gate.clone();
                let progress = progress.clone();
                std::thread::spawn(move || {
                    for _ in 0..8 {
                        let permit = gate
                            .try_acquire_permit(if i == 0 { "p-b" } else { "p-c" }, GENEROUS_WAIT)
                            .expect("other projects keep progressing");
                        drop(permit);
                        progress.fetch_add(1, AtomicOrdering::AcqRel);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("progress thread");
        }
        assert_eq!(progress.load(AtomicOrdering::Acquire), 16);
        drop(holder);
    }

    #[test]
    fn acquire_times_out_explicitly_when_capacity_is_held() {
        let gate = gate(1, None);
        let gate_for_holder = gate.clone();
        let holder = std::thread::spawn(move || {
            let permit = gate_for_holder.acquire_permit("p-a").expect("held");
            std::thread::sleep(Duration::from_millis(250));
            permit
        });
        // Let the holder win the (empty-queue) fast path first.
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        let err = gate.try_acquire_permit("p-b", SMALL_WAIT).unwrap_err();
        match err {
            GateAcquireError::Timeout { waited } => {
                assert!(waited >= SMALL_WAIT && waited < Duration::from_secs(1));
                assert!(started.elapsed() >= SMALL_WAIT);
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
        // After the permit is released the same caller admits.
        drop(holder.join().expect("holder"));
        gate.try_acquire_permit("p-b", GENEROUS_WAIT).expect("admitted after release");
    }

    #[test]
    fn fair_queue_serves_fifo_and_skips_a_blocked_head() {
        // Hold both permits of max=2 / per=1, then queue p-a first and p-b
        // second. Releasing p-b's permit must serve p-b (the blocked p-a
        // head is skipped, no head-of-line starvation); releasing p-a's
        // permit afterwards serves p-a (FIFO among admissible heads).
        let gate = gate(2, Some(1));
        let holder_a = gate.acquire_permit("p-a").expect("hold a");
        let holder_b = gate.acquire_permit("p-b").expect("hold b");

        let order = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let spawn_waiter = |gate: &ProviderGate, project: &'static str| {
            let gate = gate.clone();
            let order = order.clone();
            std::thread::spawn(move || {
                let permit = gate
                    .try_acquire_permit(project, GENEROUS_WAIT)
                    .expect("waiter eventually admitted");
                order.lock().unwrap().push(project.to_owned());
                std::thread::sleep(Duration::from_millis(40));
                drop(permit);
            })
        };

        let waiter_a = spawn_waiter(&gate, "p-a");
        std::thread::sleep(Duration::from_millis(60)); // deterministic queue order
        let waiter_b = spawn_waiter(&gate, "p-b");
        std::thread::sleep(Duration::from_millis(60));
        let snap = gate.snapshot();
        assert_eq!(snap.in_flight, 2);
        assert_eq!(snap.waiting, 2);

        // Free p-b's slot: the queued p-a head is skipped (its share is
        // still held), so p-b is served first despite queuing second.
        drop(holder_b);
        waiter_b.join().expect("b waiter");
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            order.lock().unwrap().as_slice(),
            ["p-b"],
            "blocked head must be skipped, not block the queue"
        );
        assert_eq!(gate.snapshot().waiting, 1);

        // Free p-a's slot: the remaining waiter (p-a) is served.
        drop(holder_a);
        waiter_a.join().expect("a waiter");
        assert_eq!(order.lock().unwrap().as_slice(), ["p-b", "p-a"]);
    }

    #[test]
    fn fresh_callers_cannot_barge_ahead_of_the_waiting_queue() {
        let gate = gate(1, None);
        let gate_for_holder = gate.clone();
        let order = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let holder = std::thread::spawn(move || {
            let permit = gate_for_holder.acquire_permit("holder").expect("held");
            std::thread::sleep(Duration::from_millis(250));
            drop(permit);
        });
        std::thread::sleep(Duration::from_millis(50));
        let waiter = {
            let gate = gate.clone();
            let order = order.clone();
            std::thread::spawn(move || {
                gate.try_acquire_permit("first-waiter", GENEROUS_WAIT)
                    .expect("first waiter admitted");
                order.lock().unwrap().push("first-waiter".into());
            })
        };
        std::thread::sleep(Duration::from_millis(60));
        // Queue is non-empty now: a fresh caller must join the queue instead
        // of taking the free-ish fast path.
        gate.try_acquire_permit("bargers", Duration::ZERO)
            .expect_err("no barging past a waiting caller");
        drop(holder);
        waiter.join().expect("waiter");
        assert_eq!(order.lock().unwrap().as_slice(), ["first-waiter"]);
    }

    #[test]
    fn rate_limited_pause_fails_fast_then_expires() {
        let gate = gate(4, None);
        gate.note_rate_limited(Duration::from_millis(120));
        let started = Instant::now();
        let err = gate.try_acquire_permit("p-a", GENEROUS_WAIT).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "the pause must fail fast, not park the caller"
        );
        match err {
            GateAcquireError::Suspended { cooldown_remaining } => {
                assert!(cooldown_remaining <= Duration::from_millis(120));
                assert!(cooldown_remaining > Duration::ZERO);
            }
            other => panic!("expected Suspended, got {other:?}"),
        }
        std::thread::sleep(Duration::from_millis(160));
        gate.try_acquire_permit("p-a", Duration::ZERO)
            .expect("pause expired by itself");
    }

    #[test]
    fn repeated_rate_limit_reports_extend_the_pause_never_shorten_it() {
        let gate = gate(4, None);
        gate.note_rate_limited(Duration::from_millis(400));
        std::thread::sleep(Duration::from_millis(150));
        gate.note_rate_limited(Duration::from_millis(500));
        let remaining = gate.snapshot().suspended_for.expect("still paused");
        assert!(
            remaining >= Duration::from_millis(400),
            "the later, longer Retry-After must win (got {remaining:?})"
        );
        gate.resume();
        assert_eq!(gate.snapshot().suspended_for, None);
        gate.try_acquire_permit("p-a", Duration::ZERO)
            .expect("resume clears the pause");
    }

    #[test]
    fn empty_project_identity_is_refused() {
        let gate = gate(2, None);
        assert!(matches!(
            gate.try_acquire_permit("", Duration::ZERO),
            Err(GateAcquireError::EmptyProjectIdentity)
        ));
        assert!(matches!(
            gate.try_acquire_permit("   ", Duration::ZERO),
            Err(GateAcquireError::EmptyProjectIdentity)
        ));
        assert!(matches!(
            gate.acquire_permit("\t"),
            Err(GateAcquireError::EmptyProjectIdentity)
        ));
    }

    #[test]
    fn snapshot_reflects_liveness_fairness_and_pause() {
        let gate = gate(3, Some(2));
        let first = gate.acquire_permit("p-a").expect("a1");
        let second = gate.acquire_permit("p-a").expect("a2 (share is 2)");
        let _third = gate.acquire_permit("p-b").expect("b1");
        let snap = gate.snapshot();
        assert_eq!(snap.in_flight, 3);
        assert_eq!(snap.waiting, 0);
        assert_eq!(snap.max_concurrent, 3);
        assert_eq!(snap.max_concurrent_per_project, Some(2));
        assert_eq!(snap.per_project_in_flight.get("p-a"), Some(&2));
        assert_eq!(snap.per_project_in_flight.get("p-b"), Some(&1));
        assert_eq!(snap.suspended_for, None);
        gate.note_rate_limited(Duration::from_millis(50));
        assert!(gate.snapshot().suspended_for.is_some());
        gate.resume();
        drop((first, second, _third));
        let snap = gate.snapshot();
        assert_eq!(snap.in_flight, 0);
        assert!(snap.per_project_in_flight.is_empty());
    }

    #[test]
    fn from_provider_config_maps_the_semantic_keys() {
        // Default (disabled / max_concurrent = 0): no gate at all — 限流默认关闭.
        let mut config = SemanticProviderConfig::default();
        assert!(ProviderGate::from_provider_config(&config).unwrap().is_none());

        config.enabled = true;
        assert!(
            ProviderGate::from_provider_config(&config).unwrap().is_none(),
            "enabled but max_concurrent=0 still means unlimited"
        );

        config.max_concurrent = 4;
        config.max_concurrent_per_project = 2;
        config.acquire_timeout_ms = 500;
        let gate = ProviderGate::from_provider_config(&config)
            .expect("valid gate config")
            .expect("limits configured");
        let snap = gate.snapshot();
        assert_eq!(snap.max_concurrent, 4);
        assert_eq!(snap.max_concurrent_per_project, Some(2));

        config.max_concurrent_per_project = 4; // cap >= max can never bind
        assert!(ProviderGate::from_provider_config(&config).is_err());
    }

    /// Structural fixation of the C11 rule for the gate itself: the
    /// admission wait happens strictly OUTSIDE any DB transaction. The
    /// worker-shaped sequence below is the call order the composition root
    /// must reproduce (acquire → provider call → drop, all between the
    /// short transaction's COMMIT and the next fenced write).
    #[test]
    fn admission_wait_never_spans_a_db_transaction() {
        let conn = rusqlite::Connection::open_in_memory().expect("memory db");
        conn.execute_batch("CREATE TABLE t(x INTEGER);").unwrap();

        let gate = gate(1, None);
        let gate_for_holder = gate.clone();
        let holder = std::thread::spawn(move || {
            let permit = gate_for_holder.acquire_permit("p-a").expect("held");
            std::thread::sleep(Duration::from_millis(250));
            drop(permit);
        });
        std::thread::sleep(Duration::from_millis(50));

        // Worker-shaped flow on this thread: the SHORT transaction closes
        // first, then the admission wait happens, then the provider call.
        let txn_open_at = Instant::now();
        conn.execute_batch("BEGIN IMMEDIATE; INSERT INTO t(x) VALUES (1); COMMIT;")
            .expect("short transaction");
        let txn_closed_at = Instant::now();

        let permit = gate
            .try_acquire_permit("p-a", GENEROUS_WAIT)
            .expect("admitted after the holder releases");
        let admitted_at = Instant::now();
        // The transaction was already closed while we were still waiting —
        // the wait can never have spanned it.
        assert!(
            txn_closed_at < admitted_at,
            "txn closed at {txn_closed_at:?}, admitted at {admitted_at:?}"
        );
        assert!(admitted_at > txn_open_at);

        // The conn is free again: the post-provider fenced write lands.
        drop(permit);
        conn.execute_batch("INSERT INTO t(x) VALUES (2);").unwrap();
        drop(holder);
    }

    /// Integration with the P7-003 batch flow: planned batches pass the
    /// gate one at a time per project; the global cap and the per-project
    /// share both hold under parallel workers, and both projects finish.
    #[test]
    fn gated_batch_flow_limits_global_concurrency_and_keeps_projects_moving() {
        let budget = InputBudget::validated(2, 1_000, 10_000).expect("budget");
        let project_batches = move |name: &str| -> Vec<BatchPlan<String>> {
            let rendered: Vec<(String, Vec<u8>)> = (0..8)
                .map(|i| (format!("{name}-{i}"), vec![b'a'; 16]))
                .collect();
            plan_document_batches(&rendered, &budget, TOKEN_ESTIMATOR)
                .expect("plan")
                .into_iter()
                .filter_map(|planned| match planned {
                    PlannedInput::Batch(batch) => Some(batch),
                    PlannedInput::Skipped { .. } => None,
                })
                .collect()
        };

        let gate = gate(2, Some(1));
        let provider = FakeProvider::new({
            let mut config = FakeProviderConfig::new(
                crate::spec::VectorSpace::new("fake/model-a", 1).expect("space"),
            );
            config.delay_per_call = Duration::from_millis(25);
            config
        });
        let peak = std::sync::Arc::new(AtomicUsize::new(0));
        let live = std::sync::Arc::new(AtomicUsize::new(0));

        let run_project = |gate: &ProviderGate,
                           provider: &FakeProvider,
                           project: &'static str,
                           batches: Vec<BatchPlan<String>>,
                           peak: &std::sync::Arc<AtomicUsize>,
                           live: &std::sync::Arc<AtomicUsize>| {
            let mut done = 0usize;
            for batch in batches {
                let permit = gate
                    .try_acquire_permit(project, GENEROUS_WAIT)
                    .expect("admitted within budget");
                let now = live.fetch_add(1, AtomicOrdering::AcqRel) + 1;
                peak.fetch_max(now, AtomicOrdering::AcqRel);
                let inputs: Vec<DocumentInput> =
                    batch.items.iter().map(|(_, input)| input.clone()).collect();
                let vectors = provider.embed_documents(&inputs).expect("embed");
                live.fetch_sub(1, AtomicOrdering::AcqRel);
                drop(permit);
                assert_eq!(vectors.len(), inputs.len());
                done += 1;
            }
            done
        };

        let gate_for_b = gate.clone();
        let provider_for_b = FakeProvider::new({
            let mut config = FakeProviderConfig::new(
                crate::spec::VectorSpace::new("fake/model-a", 1).expect("space"),
            );
            config.delay_per_call = Duration::from_millis(25);
            config
        });
        let peak_for_b = peak.clone();
        let live_for_b = live.clone();
        let batches_b = project_batches("b");
        let worker_b = std::thread::spawn(move || {
            run_project(
                &gate_for_b,
                &provider_for_b,
                "project-b",
                batches_b,
                &peak_for_b,
                &live_for_b,
            )
        });
        let done_a = run_project(
            &gate,
            &provider,
            "project-a",
            project_batches("a"),
            &peak,
            &live,
        );
        let done_b = worker_b.join().expect("project-b worker");

        assert_eq!(done_a, 4, "8 inputs / max_items 2 → 4 batches per project");
        assert_eq!(done_b, 4);
        assert!(
            peak.load(AtomicOrdering::Acquire) <= 2,
            "global peak {} exceeded the cap",
            peak.load(AtomicOrdering::Acquire)
        );
    }

    // ── P7-008 receipts ───────────────────────────────────────────────────

    fn receipt(
        space_model: &str,
        path: ReceiptPath,
        items: usize,
        usage: UsageReceipt,
        attempt: u64,
        outcome: AttemptOutcome,
        duration_ms: u64,
        cost: u64,
        uncertain: Option<UncertainReason>,
    ) -> ProviderCallReceipt {
        ProviderCallReceipt {
            space_model: space_model.to_owned(),
            path,
            batch_items: items,
            usage,
            attempt,
            outcome,
            duration_ms,
            cost_units: cost,
            uncertain,
        }
    }

    #[test]
    fn missing_usage_is_unknown_and_never_zero_filled() {
        // Both columns missing ⇒ the cost column reads `unknown`, not 0.
        assert!(UsageReceipt {
            reported: None,
            estimated: None,
            cache_reuse: false
        }
        .billed_is_unknown());
        // The estimate alone already un-unknowns the column.
        assert!(!UsageReceipt {
            reported: None,
            estimated: Some(7),
            cache_reuse: false
        }
        .billed_is_unknown());
        assert!(!UsageReceipt {
            reported: Some(3),
            estimated: None,
            cache_reuse: false
        }
        .billed_is_unknown());

        // Aggregation keeps the unknown column explicit instead of
        // folding it into a zero-filled reported sum.
        let ledger = ReceiptLedger::new(8).expect("ledger");
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                UsageReceipt {
                    reported: None,
                    estimated: Some(7),
                    cache_reuse: false
                },
                1,
                AttemptOutcome::Succeeded,
                5,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            100,
        );
        let agg = ledger.aggregate(None);
        assert_eq!(agg.attempts, 1);
        assert_eq!(agg.succeeded, 1);
        assert_eq!(agg.cost_units, PROVIDER_ATTEMPT_COST_UNITS);
        assert_eq!(agg.estimated_tokens, 7);
        assert_eq!(agg.attempts_with_unknown_reported, 1);
        assert_eq!(agg.reported_tokens, None, "no usage must NOT read Some(0)");
    }

    #[test]
    fn duplicate_risk_needs_a_retry_and_unknown_reported_usage() {
        let unknown = |reported: Option<u64>| UsageReceipt {
            reported,
            estimated: Some(9),
            cache_reuse: false,
        };
        // First attempt, usage unknown: no prior attempt to duplicate.
        assert!(!unknown(None).unknown_duplicate_risk(1));
        // Retry with unknown usage: the prior attempt may already be billed.
        assert!(unknown(None).unknown_duplicate_risk(2));
        assert!(unknown(None).unknown_duplicate_risk(3));
        // A provider report clears the doubt — our estimate cannot.
        assert!(!unknown(Some(4)).unknown_duplicate_risk(3));

        // Surfaces through aggregation: two attempts, only the retry's
        // usage unknown ⇒ exactly one risky attempt.
        let ledger = ReceiptLedger::new(8).expect("ledger");
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                unknown(Some(4)),
                1,
                AttemptOutcome::Failed,
                5,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            100,
        );
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                unknown(None),
                2,
                AttemptOutcome::Failed,
                5,
                PROVIDER_ATTEMPT_COST_UNITS,
                Some(UncertainReason::TimeoutIndeterminate),
            ),
            110,
        );
        let agg = ledger.aggregate(None);
        assert_eq!(agg.attempts_with_unknown_reported, 1);
        assert_eq!(agg.unknown_duplicate_risk, 1);
        assert_eq!(agg.reported_tokens, Some(4));
    }

    #[test]
    fn aggregation_sums_outcomes_costs_and_tokens() {
        let ledger = ReceiptLedger::new(16).expect("ledger");
        let est = |n: u64| UsageReceipt {
            reported: None,
            estimated: Some(n),
            cache_reuse: false,
        };
        ledger.record(
            receipt(
                "a",
                ReceiptPath::Documents,
                2,
                est(10),
                1,
                AttemptOutcome::Succeeded,
                12,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            100,
        );
        ledger.record(
            receipt(
                "a",
                ReceiptPath::Documents,
                2,
                est(10),
                2,
                AttemptOutcome::Failed,
                30,
                PROVIDER_ATTEMPT_COST_UNITS,
                Some(UncertainReason::TimeoutIndeterminate),
            ),
            200,
        );
        ledger.record(
            receipt(
                "b",
                ReceiptPath::Queries,
                1,
                est(3),
                1,
                AttemptOutcome::RejectedByBreaker,
                0,
                0,
                Some(UncertainReason::BreakerOpen),
            ),
            300,
        );
        ledger.record(
            receipt(
                "b",
                ReceiptPath::Queries,
                1,
                est(3),
                1,
                AttemptOutcome::RejectedByBudget,
                0,
                0,
                None,
            ),
            400,
        );

        let agg = ledger.aggregate(None);
        assert_eq!(agg.attempts, 4);
        assert_eq!(agg.succeeded, 1);
        assert_eq!(agg.failed, 1);
        assert_eq!(agg.rejected_by_breaker, 1);
        assert_eq!(agg.rejected_by_budget, 1);
        assert_eq!(agg.uncertain_attempts, 2);
        assert_eq!(agg.uncertain_breaker_open, 1);
        assert_eq!(agg.uncertain_timeout, 1);
        assert_eq!(agg.cost_units, 2 * PROVIDER_ATTEMPT_COST_UNITS);
        assert_eq!(agg.estimated_tokens, 26);
        assert_eq!(agg.reported_tokens, None);
        assert_eq!(agg.attempts_with_unknown_reported, 4);
        // The attempt-2 timeout retry has unknown reported usage — the
        // observable duplicate-billing risk.
        assert_eq!(agg.unknown_duplicate_risk, 1);
        assert_eq!(agg.cache_reuse_hits, 0);
        assert_eq!(agg.per_space.len(), 2);
        assert_eq!(agg.per_space["a"].attempts, 2);
        assert_eq!(agg.per_space["a"].cost_units, 2 * PROVIDER_ATTEMPT_COST_UNITS);
        assert_eq!(agg.per_space["b"].rejected_by_breaker, 1);
        assert_eq!(agg.per_space["b"].rejected_by_budget, 1);
        assert!(agg.per_space["a"].per_space.is_empty(), "no nested maps");
        // Lifetime counters agree with the retained window here.
        assert_eq!(ledger.total_cost_units(), agg.cost_units);
        assert_eq!(ledger.budget_refusals(), 1);
        assert_eq!(ledger.retained_count(), 4);
    }

    #[test]
    fn aggregate_splits_by_space_and_time_window() {
        let ledger = ReceiptLedger::new(16).expect("ledger");
        let est = |n: u64| UsageReceipt {
            reported: None,
            estimated: Some(n),
            cache_reuse: false,
        };
        // Early window: space "a" only.
        ledger.record(
            receipt(
                "a",
                ReceiptPath::Documents,
                1,
                est(10),
                1,
                AttemptOutcome::Succeeded,
                5,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            100,
        );
        // Late window: space "b".
        ledger.record(
            receipt(
                "b",
                ReceiptPath::Documents,
                1,
                est(20),
                1,
                AttemptOutcome::Succeeded,
                7,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            500,
        );

        let whole = ledger.aggregate(None);
        assert_eq!(whole.attempts, 2);
        assert_eq!(whole.cost_units, 2 * PROVIDER_ATTEMPT_COST_UNITS);

        let late = ledger.aggregate(Some(400));
        assert_eq!(late.attempts, 1, "the window drops the t=100 receipt");
        assert_eq!(late.cost_units, PROVIDER_ATTEMPT_COST_UNITS);
        assert_eq!(late.estimated_tokens, 20);
        assert_eq!(late.per_space.len(), 1);
        assert_eq!(late.per_space["b"].attempts, 1);
        assert!(late.per_space.get("a").is_none());

        // Inclusive lower bound: `since=100` keeps the t=100 receipt.
        let late_a = ledger.aggregate(Some(100));
        assert_eq!(late_a.attempts, 2);
        assert_eq!(late_a.per_space["a"].attempts, 1);
        assert_eq!(late_a.per_space["b"].attempts, 1);
    }

    #[test]
    fn cache_reuse_hits_are_counted_and_bill_nothing() {
        let ledger = ReceiptLedger::new(8).expect("ledger");
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                UsageReceipt {
                    reported: None,
                    estimated: Some(5),
                    cache_reuse: true,
                },
                1,
                AttemptOutcome::Succeeded,
                0,
                0,
                None,
            ),
            100,
        );
        let agg = ledger.aggregate(None);
        assert_eq!(agg.cache_reuse_hits, 1);
        assert_eq!(agg.attempts, 1);
        assert_eq!(
            agg.cost_units, 0,
            "a cache hit must add no cost units"
        );
        assert_eq!(ledger.total_cost_units(), 0);
    }

    #[test]
    fn cost_budget_refuses_at_the_cap_before_any_call() {
        // No cap: everything is allowed.
        assert_eq!(CostBudget::new(None).admit(0), BudgetAdmission::Allowed);
        assert_eq!(
            CostBudget::new(None).admit(u64::MAX),
            BudgetAdmission::Allowed
        );
        // Cap 2: 0 and 1 used units still admit the NEXT call; at 2 the
        // call is refused before being made; `Some(0)` refuses everything.
        let budget = CostBudget::new(Some(2));
        assert_eq!(budget.admit(0), BudgetAdmission::Allowed);
        assert_eq!(budget.admit(1), BudgetAdmission::Allowed);
        assert_eq!(
            budget.admit(2),
            BudgetAdmission::Exhausted {
                used_units: 2,
                cap_units: 2
            }
        );
        assert_eq!(
            budget.admit(9),
            BudgetAdmission::Exhausted {
                used_units: 9,
                cap_units: 2
            }
        );
        assert_eq!(
            CostBudget::new(Some(0)).admit(0),
            BudgetAdmission::Exhausted {
                used_units: 0,
                cap_units: 0
            }
        );
    }

    #[test]
    fn ledger_is_bounded_evicts_oldest_but_keeps_lifetime_totals() {
        assert!(ReceiptLedger::new(0).is_err(), "0 capacity is a config error");
        let ledger = ReceiptLedger::new(2).expect("ledger");
        let est = UsageReceipt {
            reported: None,
            estimated: Some(1),
            cache_reuse: false,
        };
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                est,
                1,
                AttemptOutcome::Succeeded,
                1,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            100,
        );
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                est,
                2,
                AttemptOutcome::Failed,
                1,
                PROVIDER_ATTEMPT_COST_UNITS,
                None,
            ),
            200,
        );
        ledger.record(
            receipt(
                "m",
                ReceiptPath::Documents,
                1,
                est,
                1,
                AttemptOutcome::RejectedByBudget,
                0,
                0,
                None,
            ),
            300,
        );

        assert_eq!(ledger.retained_count(), 2, "oldest receipt evicted");
        let agg = ledger.aggregate(None);
        assert_eq!(agg.attempts, 2, "the aggregate sees retained only");
        assert_eq!(agg.succeeded, 0, "the t=100 success was evicted");
        assert_eq!(agg.failed, 1);
        assert_eq!(agg.rejected_by_budget, 1);
        // Lifetime counters survive eviction: 2 charged attempts total.
        assert_eq!(ledger.total_cost_units(), 2 * PROVIDER_ATTEMPT_COST_UNITS);
        assert_eq!(ledger.budget_refusals(), 1);
    }
}
