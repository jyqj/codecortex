//! Opt-in, process-local runtime timing. No query text, SQL, paths or parameters.
//!
//! The executable enables only this target when CODECORTEX_RUNTIME_DIAGNOSTICS=1.
//! Times are monotonic nanoseconds relative to this process's first observation.
//! A started phase without a terminal event is unknown, never an inferred zero.
//! These waits are Rust mutex/pool waits, not SQLite's internal busy-handler time.
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LockResult, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use cc_model::{CcError, CcResult};
use rusqlite::Connection;
use tracing::{Dispatch, Span};

pub const TARGET: &str = "codecortex_runtime_diagnostics";
static ENABLED: OnceLock<bool> = OnceLock::new();
static ORIGIN: OnceLock<Instant> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var_os("CODECORTEX_RUNTIME_DIAGNOSTICS").is_some_and(|value| value == "1")
    }) && tracing::enabled!(target: "codecortex_runtime_diagnostics", tracing::Level::DEBUG)
}

fn now_ns() -> u64 {
    ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}

/// A bounded, typed wire identity. Long string IDs retain a byte digest instead
/// of being truncated or made equal to a numeric ID.
pub fn request_span(number: Option<i64>, string: Option<&str>) -> Span {
    if !enabled() {
        return Span::none();
    }
    let request_seq = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (kind, id, bytes, digest) = match (number, string) {
        (Some(value), None) => ("number", serde_json::json!(value), 0, None),
        (None, Some(value)) if value.len() <= 128 => {
            ("string", serde_json::json!(value), value.len(), None)
        }
        (None, Some(value)) => (
            "string_hash",
            serde_json::Value::Null,
            value.len(),
            Some(blake3::hash(value.as_bytes()).to_hex().to_string()),
        ),
        _ => ("invalid", serde_json::Value::Null, 0, None),
    };
    let correlation = serde_json::json!({
        "kind": "mcp_origin",
        "request_seq": request_seq,
        "request_id_kind": kind,
        "request_id": id,
        "request_id_bytes": bytes,
        "request_id_blake3": digest,
    });
    tracing::debug_span!(
        target: "codecortex_runtime_diagnostics",
        "mcp_call",
        correlation_json = %correlation
    )
}

/// Correlates direct benchmark API calls without inventing an MCP request ID.
/// Callers supply only fixed scenario labels, seed and ordinal, never query text.
pub fn local_span(seed: u64, scenario: &'static str, ordinal: u64, concurrency: u64) -> Span {
    if !enabled() {
        return Span::none();
    }
    let correlation = serde_json::json!({
        "kind": "local_operation",
        "operation_seq": NEXT_ID.fetch_add(1, Ordering::Relaxed),
        "seed": seed,
        "scenario": scenario,
        "ordinal": ordinal,
        "concurrency": concurrency,
    });
    tracing::debug_span!(
        target: "codecortex_runtime_diagnostics",
        "local_operation",
        correlation_json = %correlation
    )
}

struct Started {
    id: u64,
    start_ns: u64,
    span: Span,
    dispatch: Dispatch,
}

/// One observed interval. Dropping a future is not worker termination:
/// each worker owns a separate phase until its synchronous closure exits.
pub struct Phase {
    name: &'static str,
    started: Option<Started>,
}
impl Phase {
    pub fn start(name: &'static str) -> Self {
        let started = enabled().then(|| Started {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            start_ns: now_ns(),
            span: Span::current(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        });
        let phase = Self { name, started };
        if let Some(start) = &phase.started {
            tracing::dispatcher::with_default(&start.dispatch, || {
                start.span.in_scope(|| {
                    tracing::debug!(
                        target: "codecortex_runtime_diagnostics",
                        phase = phase.name,
                        phase_id = start.id,
                        event = "start",
                        start_ns = start.start_ns
                    );
                });
            });
        }
        phase
    }

    fn id(&self) -> Option<u64> {
        self.started.as_ref().map(|started| started.id)
    }

    pub fn finish(mut self, outcome: &'static str) {
        self.emit_finish(outcome);
    }

    pub fn finish_result<T>(self, result: &CcResult<T>) {
        self.finish(outcome(result));
    }

    fn emit_finish(&mut self, outcome: &'static str) {
        if let Some(start) = self.started.take() {
            let end_ns = now_ns();
            tracing::dispatcher::with_default(&start.dispatch, || {
                start.span.in_scope(|| {
                    tracing::debug!(
                        target: "codecortex_runtime_diagnostics",
                        phase = self.name,
                        phase_id = start.id,
                        event = "finish",
                        start_ns = start.start_ns,
                        end_ns,
                        elapsed_ns = end_ns.saturating_sub(start.start_ns),
                        outcome
                    );
                });
            });
        }
    }
}
impl Drop for Phase {
    fn drop(&mut self) {
        self.emit_finish(if std::thread::panicking() {
            "unwinding"
        } else {
            "dropped"
        });
    }
}

pub fn outcome<T>(result: &CcResult<T>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(CcError::QueryBusy) => "busy",
        Err(CcError::QueryCancelled) => "cancelled",
        Err(CcError::QueryTimedOut) => "timed_out",
        Err(_) => "error",
    }
}

/// Observes the original future without spawning it or changing cancellation.
pub async fn observe<T>(
    phase: &'static str,
    future: impl std::future::Future<Output = CcResult<T>>,
) -> CcResult<T> {
    let timing = Phase::start(phase);
    let result = future.await;
    timing.finish_result(&result);
    result
}

/// Captured before spawn_blocking. The dispatcher and span enter only inside
/// the synchronous closure; no entered-span guard ever crosses an await.
pub struct BlockingTask {
    queued: Phase,
    service: &'static str,
    context: Option<(Span, Dispatch)>,
}
impl BlockingTask {
    pub fn queued(queue: &'static str, service: &'static str) -> Self {
        Self {
            queued: Phase::start(queue),
            service,
            context: enabled().then(|| {
                (
                    Span::current(),
                    tracing::dispatcher::get_default(Clone::clone),
                )
            }),
        }
    }

    pub fn run<T>(self, work: impl FnOnce() -> CcResult<T>) -> CcResult<T> {
        let Self {
            queued,
            service,
            context,
        } = self;
        Self::scope(context, || {
            queued.finish("entered");
            let timing = Phase::start(service);
            let waits = WaitScope::start(timing.id());
            let result = work();
            drop(waits);
            timing.finish_result(&result);
            result
        })
    }

    /// A background scheduler may handle its own errors internally. "returned"
    /// means physical closure exit, not that all background work succeeded.
    pub fn run_background(self, work: impl FnOnce()) {
        let Self {
            queued,
            service,
            context,
        } = self;
        Self::scope(context, || {
            queued.finish("entered");
            let timing = Phase::start(service);
            let waits = WaitScope::start(timing.id());
            work();
            drop(waits);
            timing.finish("returned");
        });
    }

    fn scope<T>(context: Option<(Span, Dispatch)>, work: impl FnOnce() -> T) -> T {
        match &context {
            Some((span, dispatch)) => {
                tracing::dispatcher::with_default(dispatch, || span.in_scope(work))
            }
            None => work(),
        }
    }
}

/// Fixed wait categories. A worker emits one bounded aggregate rather than
/// one event per SQL statement. Sum is work across acquisitions, not E2E.
#[derive(Clone, Copy)]
pub enum WaitKind {
    DbWrite,
    DbPoolGuard,
    DbPool,
    IndexRead,
    IndexWrite,
    BuildGate,
    ProviderGate,
}
impl WaitKind {
    fn slot(self) -> usize {
        self as usize
    }
    fn name(self) -> &'static str {
        match self {
            Self::DbWrite => "db_write_mutex",
            Self::DbPoolGuard => "db_read_pool_guard",
            Self::DbPool => "db_read_pool",
            Self::IndexRead => "index_read_lock",
            Self::IndexWrite => "index_write_lock",
            Self::BuildGate => "build_gate",
            Self::ProviderGate => "provider_gate_attempt",
        }
    }
}
#[derive(Clone, Copy, Default)]
struct WaitMetric {
    count: u64,
    sum_ns: u64,
    max_ns: u64,
    failed: u64,
    incomplete: u64,
}
struct Aggregate {
    id: u64,
    metrics: [WaitMetric; 7],
}
thread_local! {
    static WAITS: RefCell<Vec<Aggregate>> = const { RefCell::new(Vec::new()) };
}
struct WaitScope(Option<u64>);
impl WaitScope {
    fn start(id: Option<u64>) -> Self {
        let Some(id) = id else {
            return Self(None);
        };
        WAITS.with(|stack| {
            stack.borrow_mut().push(Aggregate {
                id,
                metrics: [WaitMetric::default(); 7],
            })
        });
        Self(Some(id))
    }
}
impl Drop for WaitScope {
    fn drop(&mut self) {
        let Some(id) = self.0 else {
            return;
        };
        let aggregate = WAITS.with(|stack| {
            let mut stack = stack.borrow_mut();
            let position = stack.iter().rposition(|item| item.id == id)?;
            Some(stack.remove(position))
        });
        let Some(aggregate) = aggregate else {
            return;
        };
        let kinds = [
            WaitKind::DbWrite,
            WaitKind::DbPoolGuard,
            WaitKind::DbPool,
            WaitKind::IndexRead,
            WaitKind::IndexWrite,
            WaitKind::BuildGate,
            WaitKind::ProviderGate,
        ];
        let complete = !std::thread::panicking()
            && aggregate
                .metrics
                .iter()
                .all(|metric| metric.incomplete == 0);
        let waits: Vec<_> = kinds
            .into_iter()
            .map(|kind| {
                let metric = aggregate.metrics[kind.slot()];
                serde_json::json!({
                    "wait": kind.name(),
                    "count": metric.count,
                    "sum_ns": (metric.count != 0).then_some(metric.sum_ns),
                    "max_ns": (metric.count != 0).then_some(metric.max_ns),
                    "failed": metric.failed,
                    "incomplete": metric.incomplete,
                })
            })
            .collect();
        let waits = serde_json::Value::Array(waits);
        tracing::debug!(
            target: "codecortex_runtime_diagnostics",
            event = "worker_waits",
            worker_phase_id = id,
            complete,
            waits_json = %waits
        );
    }
}

/// Synchronous lock acquisition only; never held across an await.
/// result_ok records the API result. A poisoned standard lock can return Err
/// while its PoisonError still owns the acquired guard; this never changes
/// whether the original caller recovers that guard.
pub struct Wait {
    kind: WaitKind,
    start_ns: Option<u64>,
}
impl Wait {
    pub fn start(kind: WaitKind) -> Self {
        Self {
            kind,
            start_ns: enabled().then(now_ns),
        }
    }
    pub fn finish(mut self, result_ok: bool) {
        let Some(start_ns) = self.start_ns.take() else {
            return;
        };
        let end_ns = now_ns();
        let elapsed_ns = end_ns.saturating_sub(start_ns);
        let recorded = WAITS.with(|stack| {
            let mut stack = stack.borrow_mut();
            let Some(scope) = stack.last_mut() else {
                return false;
            };
            let metric = &mut scope.metrics[self.kind.slot()];
            metric.count = metric.count.saturating_add(1);
            metric.sum_ns = metric.sum_ns.saturating_add(elapsed_ns);
            metric.max_ns = metric.max_ns.max(elapsed_ns);
            metric.failed = metric.failed.saturating_add(u64::from(!result_ok));
            true
        });
        if !recorded {
            // Startup/maintenance outside a blocking worker remains explicit.
            tracing::debug!(
                target: "codecortex_runtime_diagnostics",
                event = "unscoped_wait",
                wait = self.kind.name(),
                start_ns, end_ns, elapsed_ns, result_ok
            );
        }
    }
}

impl Drop for Wait {
    fn drop(&mut self) {
        let Some(start_ns) = self.start_ns.take() else {
            return;
        };
        let recorded = WAITS.with(|stack| {
            let mut stack = stack.borrow_mut();
            let Some(scope) = stack.last_mut() else {
                return false;
            };
            let metric = &mut scope.metrics[self.kind.slot()];
            metric.incomplete = metric.incomplete.saturating_add(1);
            true
        });
        if !recorded {
            let end_ns = now_ns();
            tracing::debug!(
                target: "codecortex_runtime_diagnostics",
                event = "wait_incomplete",
                wait = self.kind.name(),
                start_ns, end_ns,
                elapsed_ns = end_ns.saturating_sub(start_ns),
                outcome = if std::thread::panicking() { "unwinding" } else { "dropped" },
            );
        }
    }
}

/// Crate-private seam: preserve the exact standard lock result/guard and poison
/// contract used by every existing write caller and UnitOfWork.
pub(crate) struct ConnectionMutex(Mutex<Connection>);
impl ConnectionMutex {
    pub(crate) fn new(connection: Connection) -> Self {
        Self(Mutex::new(connection))
    }

    pub(crate) fn lock(&self) -> LockResult<MutexGuard<'_, Connection>> {
        let wait = Wait::start(WaitKind::DbWrite);
        let result = self.0.lock();
        wait.finish(result.is_ok());
        result
    }

    #[cfg(test)]
    pub(crate) fn try_lock(&self) -> std::sync::TryLockResult<MutexGuard<'_, Connection>> {
        self.0.try_lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_mutex_preserves_guard_and_poison_recovery_contract() {
        let mutex = ConnectionMutex::new(Connection::open_in_memory().unwrap());
        mutex
            .lock()
            .unwrap()
            .execute_batch("CREATE TABLE proof(value);")
            .unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("poison the original write mutex");
        }))
        .is_err());
        let poisoned = match mutex.lock() {
            Err(poisoned) => poisoned,
            Ok(_) => panic!("poison must not be cleared"),
        };
        let guard = poisoned.into_inner();
        guard.execute("INSERT INTO proof VALUES(7)", []).unwrap();
        let value: i64 = guard
            .query_row("SELECT value FROM proof", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, 7);
        drop(guard);
        assert!(mutex.lock().is_err());
    }
}
