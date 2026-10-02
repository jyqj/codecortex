//! Optional semantic persistence seam (P6-002 skeleton, P6-003 frozen
//! encoding space).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`.
//! Authoritative state stays in the single `index.sqlite3` owned by `cc-db`;
//! this crate may own only the *derivable, discardable, verifiable* artifact
//! cache and the worker-side ports around it. As of P6-003 the canonical
//! encoding space and input specification are frozen in [`crate::spec`]
//! (versioned digest surface; see `docs/ENCODING-SPACE.md`), and digest
//! construction is sealed behind validated paths. As of P6-008 the
//! content-addressed artifact cache mechanism itself lives in [`crate::cache`]
//! (global-root + project-identity namespace per the Q5 decision; `open` is
//! side-effect free, the layout is created lazily by `put`). As of P6-009 the
//! deterministic fake provider lives in [`crate::providers::fake`] — the
//! reference `EmbeddingProvider` implementation, pure local blake3 keystream,
//! never a network dependency; its vectors carry no semantic quality claim.
//! As of P6-010 the filtered exact vector backend lives in
//! [`crate::vector::exact`] — brute-force top-k over the published manifest ×
//! artifact cache with filter-before-top-k, bounded batch loads and the
//! `(score desc, doc_key asc)` total order; it is the small-scale oracle
//! backend, not an ANN structure (V22 optional track). As of P6-011 the
//! artifact → manifest publish orchestration lives in [`crate::publish`]:
//! artifact durable first, verified read-back, then the cc-db publish CAS
//! (five-way fencing; Q4 duplicate-ack visible-set diff) — the durability
//! order of ADR-0003, with no cross-store atomicity claim. As of P6-013 the
//! explicit worker queue primitives live in [`crate::queue`]: RAII lease
//! guard, caller-driven drain (claim → renew → handler → fenced ack/retry)
//! and the continuous-edit merge semantics — an EXPLICIT drain with no
//! resident thread, timer, or daemon (ADR: 不引入独立队列服务、常驻进程或网络
//! 端点); whether and when to drain stays the composition root's decision. As
//! of P6-014 the post-rebuild reconcile lives in [`crate::reconcile`]: fence
//! against the authoritative path's incarnation, re-enqueue the rebuilt
//! desired set, and repopulate the visible set from the SAME cache namespace
//! (Q5: namespace excludes the incarnation) — paid vectors are reused, only
//! cache misses reach the provider, and a pre-swap ghost process is fenced
//! with zero writes. As of P6-015 the bounded crash-recovery scan lives in
//! [`crate::recovery`]: one caller-driven pass over the crash residue —
//! page-capped reclaim of expired leases (orphan triage), replay of
//! interrupted publishes from already-durable artifacts through the full
//! publish CAS (0 provider cost, duplicate acks absorbed by Q4), and a
//! dead-letter census that never resurrects — fenced against the
//! authoritative-path incarnation and bounded per call
//! (`converged` drives the caller's rounds; no resident process). As of
//! P6-016 the bounded artifact-cache GC lives in [`crate::gc`]: conservative
//! mark (manifest refs of ALL spaces + live-task inputs + fresh-timestamp
//! retention grace) under ONE short DB read snapshot per batch — the
//! publish/GC synchronization point that makes "刚发布的 artifact 被 GC 删除"
//! unreachable — then unlink sweep with empty-dir pruning, temp/half-file
//! cleanup, keyset-cursor bounded passes and no resident process; GC is
//! DB-read-only and moves no epoch. As of P6-018 the cache miss/corrupt
//! degradation closes the loop in [`crate::degrade`]: detected-corrupt
//! objects are QUARANTINED (moved to `<root>/quarantine/`, evidence and a
//! diagnostic report preserved, address reads Miss again — GC structurally
//! never touches that directory), the re-embed spend for quarantined inputs
//! is bounded by a process-lifetime admission budget
//! ([`crate::degrade::DegradationLedger`] +
//! [`crate::degrade::BudgetedProvider`]; budget-exhausted re-embeds
//! are refused before the provider runs and dead-letter `failed` with the
//! reason), and the degraded state surfaces as a read-only
//! [`crate::degrade::DegradationSnapshot`] for the cc-server capability status
//! (`semantic_state: "degraded"` + `degraded_reason`).
//! As of P7-002 the model capability declaration, its consistency
//! validation against the frozen encoding surface, and the capability
//! probe protocol (mock-transport only this round, D1/D2) live in
//! [`crate::capability`]; declared-vs-observed support differences are
//! config errors that refuse startup, never silent defaults. As of P7-003
//! the caller-side batch admission lives in [`crate::admission`]: final
//! rendered input sizes are measured against a hard byte bound and the
//! declared token estimator, inadmissible inputs are explicitly skipped and
//! the rest is cut into deterministic order-preserving batches — the frozen
//! "caller guarantees batch size" contract (`ports.rs:94-95`) as code. As of
//! P7-004 the OpenAI-compatible adapter's shared response gate
//! ([`crate::providers::openai_compatible`]) is a full strong-validation
//! gate: protocol (status semantics, JSON content type, error-envelope
//! diagnostics without the free-form message field), security (derived
//! body-size bound pre-parse, count gate pre-entry, serde_json depth limit,
//! zero body/payload leakage into errors), schema (strict field types) and
//! semantics (required model echo, dimension, finiteness, non-zero, optional
//! norm policy, index set exactly `0..n`) — every violation rejects the
//! whole batch as non-retryable `InvalidInput`, so error vectors can never
//! reach the artifact cache. As of P7-005 the shared provider concurrency
//! gate lives in [`crate::admission`] beside the batch planner: ONE
//! process-wide semaphore (assembled by the composition root, never one
//! unlimited semaphore per call) with a per-project fair share and
//! no-head-of-line starvation, explicit admission timeout and the 429
//! fail-fast pause (`Retry-After` cooldown), all admission-only — the
//! gate spawns no thread and the blocking provider call's execution
//! context stays the caller's decision (Q4). The worker's claim order is
//! now an injectable closed fairness policy (`cc-db` `ClaimFairness`):
//! arrival FIFO by default, opt-in doc rotation so a continuously
//! re-edited doc yields to waiting docs. As of P7-006 the bounded
//! call-layer retry and the circuit breaker live in
//! [`crate::providers::openai_compatible`]: a [`RetryingProvider`]
//! decorator retries only the retryable error classes within an
//! attempt/deadline/cost-capped sequence (exponential backoff with
//! deterministic jitter, `Retry-After` honored), reports 429s to the
//! shared gate and consumes its `Suspended` signal instead of parking,
//! and admits every attempt through a process-wide three-state
//! [`CircuitBreaker`] (closed → open → half-open single probe) held by
//! the composition root beside the gate — all passive, called
//! components with an injected clock, layered strictly INSIDE one
//! outbox attempt so the call-layer budget never touches the DB
//! attempt budget. As of P7-007 the code-egress and credential policy
//! mechanisms live in [`crate::policy`]: the default no-network /
//! https-only [`crate::policy::EgressPolicy`] gates transport assembly
//! ([`crate::policy::gate_transport_assembly`] + an adapter-constructor
//! fail-fast), every injected transport is wrapped in the
//! [`crate::policy::GuardedTransport`] (mandatory timeout, scheme
//! admission, redirects rejected before the adapter), the declared egress
//! surface is mechanically auditable
//! ([`crate::policy::audit_egress`] — exact body/header field sets, the
//! credential only ever in the `Authorization` header), and
//! `semantic.api_key_ref` resolves to an in-memory key via
//! [`crate::policy::resolve_key_reference`] (`env:`/`file:` external
//! references only, errors name the reference never the value). The live
//! leg (real transport, sensitive-file egress matrix) stays conditional
//! blocked (D1/D2). As of P7-008 the cost and uncertain-attempt receipts
//! live in [`crate::admission`]: every provider attempt — retries, breaker
//! refusals, and pre-call budget refusals included — leaves one
//! structured [`crate::admission::ProviderCallReceipt`] (batch size, the
//! declared token estimate, attempt ordinal, outcome, injected-clock
//! duration, placeholder-rate cost units), breaker-open refusals and
//! timed-out attempts carry an explicit
//! [`crate::admission::UncertainReason`] marker for duplicate-billing
//! risk, reported-vs-estimated usage stays honest (unknown is never
//! zero-filled), and the bounded in-process
//! [`crate::admission::ReceiptLedger`] aggregates by space and time
//! window behind an optional [`CostBudget`] shutdown threshold checked
//! before the call. Receipts are in-memory only by design and carry
//! zero input text or credentials. As of P7-009 the query encoding path
//! closes in [`crate::cache`]: a bounded in-process LRU of encoded query
//! vectors ([`crate::cache::QueryVectorCache`], dual hard bounds — entry
//! count AND payload bytes) keyed by
//! `(namespace, QuerySpecDigest, QueryDigest)` — Q5 ruling: the semantic
//! epoch is deliberately NOT in the key, because a query vector is a pure
//! function of (encoding spec, query text), while dense-recall freshness
//! stays with the generation-verified consumer side — and
//! [`crate::cache::encode_queries`] runs the full path: admission's query
//! planner (P7-003 query-side cut, digest-bound `QueryInput`s) → cache
//! lookup → provider encoding of the misses only → output validation
//! (dimension/finite/non-zero, error vectors never cached) → input-ordered
//! outcomes. Documents never route through this cache, so a query-side spec
//! change can never re-embed a document.
//! Still absent: no wiring into `cc-server` (the
//! optional `semantic` cargo feature exists but is not referenced anywhere
//! by default).
//!
//! Dependency floor: `cc-model` and `cc-db` only (ADR-0003: "depends on
//! `cc-model`/`cc-db` only"); nothing outside that set is ever allowed.

pub mod admission;
pub mod cache;
pub mod capability;
pub mod degrade;
pub mod error;
pub mod gc;
pub mod policy;
pub mod ports;
pub mod providers;
pub mod publish;
pub mod queue;
pub mod reconcile;
pub mod recovery;
pub mod space_switch;
pub mod spec;
pub mod types;
pub mod vector;

/// Deferred-combination-root handle.
///
/// Placeholder for the handle returned by the future lazy initializer
/// (`try_init`, TASK-BRIEFS P6-002 draft). It is intentionally a zero-sized
/// marker today: the acceptance rule "default build/startup must not create
/// an empty cache directory" requires that no initialization or cache path
/// exists before P6-008, so the real constructor is added together with the
/// artifact cache rather than stubbed here.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SemanticHandle {
    _private: (),
}

#[cfg(test)]
mod tests {
    use super::SemanticHandle;

    #[test]
    fn handle_is_a_zero_sized_marker() {
        let handle = SemanticHandle::default();
        assert_eq!(handle, SemanticHandle { _private: () });
        assert_eq!(std::mem::size_of::<SemanticHandle>(), 0);
    }
}
