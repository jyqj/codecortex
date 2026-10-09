//! Opt-in, fixed-storage observations of actual database acquisitions.
//!
//! The default build does not compile this module or change its lock types.
//! Counts are process-lifetime aggregates across every IndexDb, so retiring a
//! handle cannot discard its final counters. A thread-local scope separates
//! status-observer acquisitions from simultaneous work on other threads.
//! Acquisition wall includes clock/lock overhead. Connection checkout wall
//! also includes validation/creation; none of these measures SQLite busy wait.
//! Recording uses fixed atomics, never a second mutex or an allocation while
//! holding the acquired guard. Atomic retries have no hard wall-time bound.
//! This overhead belongs to the explicit diagnostic build, not to default-build
//! performance measurements.
use serde::Serialize;
use std::{
    cell::Cell,
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::SeqCst},
        LockResult, Mutex, MutexGuard,
    },
    time::Instant,
};

thread_local! {
    static OBSERVER: Cell<bool> = const { Cell::new(false) };
}

// Tests can give the real acquisition methods isolated counters without
// resetting the process-global production accumulator or racing other tests.
#[cfg(test)]
thread_local! {
    static TEST_OBSERVATIONS: Cell<Option<&'static Observations>> = const { Cell::new(None) };
}

/// Must remain on its creating thread and must not span an async await.
pub struct ObserverScope {
    previous: bool,
    _not_send: PhantomData<Rc<()>>,
}

/// Apply inside the complete synchronous status handler, not around its future.
pub fn enter_observer_scope() -> ObserverScope {
    ObserverScope {
        previous: OBSERVER.with(|value| value.replace(true)),
        _not_send: PhantomData,
    }
}

impl Drop for ObserverScope {
    fn drop(&mut self) {
        OBSERVER.with(|value| value.set(self.previous));
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    WriterMutex = 0,
    ReadPoolLock = 1,
    ReadConnectionCheckout = 2,
}

#[derive(Clone, Copy)]
pub(crate) enum Outcome {
    Acquired,
    Poisoned,
    Failed,
    #[cfg(test)]
    WouldBlock,
}

#[derive(Default)]
struct Metric {
    attempts: AtomicU64,
    acquired: AtomicU64,
    poisoned: AtomicU64,
    failed: AtomicU64,
    would_block: AtomicU64,
    in_flight: AtomicU64,
    elapsed_ns_total: AtomicU64,
    elapsed_ns_max: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AcquisitionMetric {
    pub attempts: u64,
    pub acquired: u64,
    pub poisoned: u64,
    pub failed: u64,
    pub would_block: u64,
    pub in_flight: u64,
    pub elapsed_ns_total: u64,
    pub elapsed_ns_max: u64,
}

impl Metric {
    const fn new() -> Self {
        Self {
            attempts: AtomicU64::new(0),
            acquired: AtomicU64::new(0),
            poisoned: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            would_block: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            elapsed_ns_total: AtomicU64::new(0),
            elapsed_ns_max: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> AcquisitionMetric {
        AcquisitionMetric {
            attempts: self.attempts.load(SeqCst),
            acquired: self.acquired.load(SeqCst),
            poisoned: self.poisoned.load(SeqCst),
            failed: self.failed.load(SeqCst),
            would_block: self.would_block.load(SeqCst),
            in_flight: self.in_flight.load(SeqCst),
            elapsed_ns_total: self.elapsed_ns_total.load(SeqCst),
            elapsed_ns_max: self.elapsed_ns_max.load(SeqCst),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AcquisitionGroup {
    pub writer_mutex_acquire: AcquisitionMetric,
    pub read_pool_lock_acquire: AcquisitionMetric,
    pub read_connection_checkout: AcquisitionMetric,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbLockObservationSnapshot {
    pub schema_version: u32,
    pub process_id: u32,
    pub db_instance_id: u64,
    pub created_instances: u64,
    pub scope: &'static str,
    pub sqlite_busy_wait_observed: bool,
    pub coherent: bool,
    pub overflowed: bool,
    pub update_sequence: u64,
    pub workload: AcquisitionGroup,
    pub observer: AcquisitionGroup,
}

#[derive(Default)]
struct Observations {
    // Each begin/finish update brackets its fixed set of atomic writes. A
    // snapshot is coherent only if no updater overlaps any of its field reads.
    // SeqCst gives the snapshot and all writers one total order. In-flight
    // acquisitions are different from these brief active recorders.
    active_recorders: AtomicU64,
    sequence: AtomicU64,
    overflowed: AtomicBool,
    created_instances: AtomicU64,
    metrics: [[Metric; 3]; 2],
}

impl Observations {
    const fn new() -> Self {
        Self {
            active_recorders: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            overflowed: AtomicBool::new(false),
            created_instances: AtomicU64::new(0),
            metrics: [
                [Metric::new(), Metric::new(), Metric::new()],
                [Metric::new(), Metric::new(), Metric::new()],
            ],
        }
    }

    fn add(&self, value: &AtomicU64, amount: u64) {
        if value
            .fetch_update(SeqCst, SeqCst, |old| old.checked_add(amount))
            .is_err()
        {
            self.overflowed.store(true, SeqCst);
        }
    }

    fn subtract_one(&self, value: &AtomicU64) {
        if value
            .fetch_update(SeqCst, SeqCst, |old| old.checked_sub(1))
            .is_err()
        {
            self.overflowed.store(true, SeqCst);
        }
    }

    fn update(&self, work: impl FnOnce()) {
        self.add(&self.active_recorders, 1);
        self.add(&self.sequence, 1);
        work();
        self.add(&self.sequence, 1);
        self.subtract_one(&self.active_recorders);
    }

    fn begin(&self, kind: Kind) -> Acquisition<'_> {
        let role = usize::from(OBSERVER.with(Cell::get));
        let metric = &self.metrics[role][kind as usize];
        self.update(|| {
            self.add(&metric.attempts, 1);
            self.add(&metric.in_flight, 1);
        });
        Acquisition {
            owner: self,
            metric,
            started: Instant::now(),
            finished: false,
        }
    }

    fn group(&self, role: usize) -> AcquisitionGroup {
        AcquisitionGroup {
            writer_mutex_acquire: self.metrics[role][0].snapshot(),
            read_pool_lock_acquire: self.metrics[role][1].snapshot(),
            read_connection_checkout: self.metrics[role][2].snapshot(),
        }
    }

    fn snapshot(&self, db_instance_id: u64) -> DbLockObservationSnapshot {
        let before = self.sequence.load(SeqCst);
        let idle_before = self.active_recorders.load(SeqCst) == 0;
        let created_instances = self.created_instances.load(SeqCst);
        let workload = self.group(0);
        let observer = self.group(1);
        let idle_after = self.active_recorders.load(SeqCst) == 0;
        let after = self.sequence.load(SeqCst);
        let overflowed = self.overflowed.load(SeqCst);
        DbLockObservationSnapshot {
            schema_version: 1,
            process_id: std::process::id(),
            db_instance_id,
            created_instances,
            scope: "process_lifetime_all_index_db_handles",
            sqlite_busy_wait_observed: false,
            coherent: idle_before && idle_after && before == after && !overflowed,
            overflowed,
            update_sequence: after,
            workload,
            observer,
        }
    }
}

pub(crate) struct Acquisition<'a> {
    owner: &'a Observations,
    metric: &'a Metric,
    started: Instant,
    finished: bool,
}

impl Acquisition<'_> {
    pub(crate) fn finish(mut self, outcome: Outcome) {
        self.record(outcome);
    }

    fn record(&mut self, outcome: Outcome) {
        // Capture the elapsed boundary before accounting. A returned lock's
        // standard guard stays owned by the caller, including poison errors.
        let elapsed = self.started.elapsed().as_nanos();
        let elapsed = u64::try_from(elapsed).unwrap_or_else(|_| {
            self.owner.overflowed.store(true, SeqCst);
            u64::MAX
        });
        self.owner.update(|| {
            let outcome_count = match outcome {
                Outcome::Acquired => &self.metric.acquired,
                Outcome::Poisoned => &self.metric.poisoned,
                Outcome::Failed => &self.metric.failed,
                #[cfg(test)]
                Outcome::WouldBlock => &self.metric.would_block,
            };
            self.owner.add(outcome_count, 1);
            self.owner.add(&self.metric.elapsed_ns_total, elapsed);
            self.metric.elapsed_ns_max.fetch_max(elapsed, SeqCst);
            self.owner.subtract_one(&self.metric.in_flight);
        });
        self.finished = true;
    }
}

impl Drop for Acquisition<'_> {
    fn drop(&mut self) {
        if !self.finished {
            // Keep an unwind/abandoned attempt in the denominator.
            self.record(Outcome::Failed);
        }
    }
}

fn observations() -> &'static Observations {
    #[cfg(test)]
    if let Some(owner) = TEST_OBSERVATIONS.with(Cell::get) {
        return owner;
    }
    static VALUE: Observations = Observations::new();
    &VALUE
}

pub(crate) fn acquire(kind: Kind) -> Acquisition<'static> {
    observations().begin(kind)
}

pub(crate) fn record_instance_created() {
    let owner = observations();
    owner.update(|| owner.add(&owner.created_instances, 1));
}

pub(crate) fn snapshot(db_instance_id: u64) -> DbLockObservationSnapshot {
    observations().snapshot(db_instance_id)
}

/// Only the opt-in build substitutes this field. It returns the standard guard
/// and standard poison result, with no additional lock in the observer itself.
pub(crate) struct ObservedMutex<T> {
    inner: Mutex<T>,
}

impl<T> ObservedMutex<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(value),
        }
    }

    pub(crate) fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        self.lock_observed(observations())
    }

    fn lock_observed(&self, owner: &Observations) -> LockResult<MutexGuard<'_, T>> {
        let timer = owner.begin(Kind::WriterMutex);
        let result = self.inner.lock();
        timer.finish(if result.is_ok() {
            Outcome::Acquired
        } else {
            Outcome::Poisoned
        });
        result
    }

    #[cfg(test)]
    pub(crate) fn try_lock(&self) -> std::sync::TryLockResult<MutexGuard<'_, T>> {
        let timer = acquire(Kind::WriterMutex);
        let result = self.inner.try_lock();
        timer.finish(match &result {
            Ok(_) => Outcome::Acquired,
            Err(std::sync::TryLockError::Poisoned(_)) => Outcome::Poisoned,
            Err(std::sync::TryLockError::WouldBlock) => Outcome::WouldBlock,
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Duration};

    fn with_owner<T>(owner: &'static Observations, work: impl FnOnce() -> T) -> T {
        struct Restore(Option<&'static Observations>);
        impl Drop for Restore {
            fn drop(&mut self) {
                TEST_OBSERVATIONS.with(|value| value.set(self.0));
            }
        }
        let _restore = Restore(TEST_OBSERVATIONS.with(|value| value.replace(Some(owner))));
        work()
    }

    fn balanced(metric: &AcquisitionMetric) {
        assert_eq!(
            metric.attempts,
            metric.acquired + metric.poisoned + metric.failed + metric.would_block + metric.in_flight
        );
        assert!(metric.elapsed_ns_total >= metric.elapsed_ns_max);
    }

    #[test]
    fn held_writer_is_observed_before_release_without_changing_guard_ownership() {
        let owner = Arc::new(Observations::default());
        let lock = Arc::new(ObservedMutex::new(7));
        let held = lock.inner.lock().unwrap();
        let other_owner = owner.clone();
        let other_lock = lock.clone();
        let thread = std::thread::spawn(move || {
            let mut guard = other_lock.lock_observed(&other_owner).unwrap();
            *guard += 1;
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = owner.snapshot(1);
            if snapshot.coherent && snapshot.workload.writer_mutex_acquire.in_flight == 1 {
                assert_eq!(snapshot.workload.writer_mutex_acquire.acquired, 0);
                balanced(&snapshot.workload.writer_mutex_acquire);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!thread.is_finished());
        drop(held);
        thread.join().unwrap();
        assert_eq!(*lock.inner.lock().unwrap(), 8);
        let snapshot = owner.snapshot(1);
        assert!(snapshot.coherent);
        assert_eq!(snapshot.workload.writer_mutex_acquire.acquired, 1);
        assert_eq!(snapshot.workload.writer_mutex_acquire.in_flight, 0);
        balanced(&snapshot.workload.writer_mutex_acquire);
    }

    #[test]
    fn poisoned_guard_and_original_error_are_returned() {
        let lock = Arc::new(ObservedMutex::new(9));
        let other = lock.clone();
        assert!(std::thread::spawn(move || {
            let _guard = other.inner.lock().unwrap();
            panic!("original poison");
        })
        .join()
        .is_err());
        let owner = Observations::default();
        let poison = lock.lock_observed(&owner).unwrap_err();
        assert_eq!(*poison.into_inner(), 9);
        let snapshot = owner.snapshot(1);
        assert!(snapshot.coherent);
        assert_eq!(snapshot.workload.writer_mutex_acquire.poisoned, 1);
        assert_eq!(snapshot.workload.writer_mutex_acquire.acquired, 0);
        balanced(&snapshot.workload.writer_mutex_acquire);
    }

    #[test]
    fn observer_scope_is_nested_thread_local_and_restored_after_unwind() {
        let owner = Arc::new(Observations::default());
        let outer = enter_observer_scope();
        {
            let _inner = enter_observer_scope();
            owner.begin(Kind::ReadPoolLock).finish(Outcome::Acquired);
        }
        let other = owner.clone();
        std::thread::spawn(move || other.begin(Kind::ReadPoolLock).finish(Outcome::Acquired))
            .join()
            .unwrap();
        drop(outer);
        assert!(std::panic::catch_unwind(|| {
            let _scope = enter_observer_scope();
            panic!("observer unwind");
        })
        .is_err());
        owner.begin(Kind::ReadPoolLock).finish(Outcome::Acquired);
        let snapshot = owner.snapshot(1);
        assert_eq!(snapshot.observer.read_pool_lock_acquire.acquired, 1);
        assert_eq!(snapshot.workload.read_pool_lock_acquire.acquired, 2);
    }

    #[test]
    fn failed_and_abandoned_attempts_remain_in_the_denominator() {
        let owner = Observations::default();
        owner
            .begin(Kind::ReadConnectionCheckout)
            .finish(Outcome::Failed);
        drop(owner.begin(Kind::ReadConnectionCheckout));
        let snapshot = owner.snapshot(1);
        assert_eq!(snapshot.workload.read_connection_checkout.failed, 2);
        balanced(&snapshot.workload.read_connection_checkout);
    }

    #[test]
    fn overlap_and_overflow_are_explicitly_not_coherent() {
        let owner = Observations::default();
        owner.active_recorders.store(1, SeqCst);
        assert!(!owner.snapshot(1).coherent);
        owner.active_recorders.store(0, SeqCst);
        owner.metrics[0][0].attempts.store(u64::MAX, SeqCst);
        owner.begin(Kind::WriterMutex).finish(Outcome::Acquired);
        let snapshot = owner.snapshot(1);
        assert!(snapshot.overflowed);
        assert!(!snapshot.coherent);
        assert_eq!(snapshot.workload.writer_mutex_acquire.attempts, u64::MAX);
    }

    #[test]
    fn concurrent_snapshots_never_claim_torn_balances_as_coherent() {
        let owner = Arc::new(Observations::default());
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let owner = owner.clone();
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        owner.begin(Kind::WriterMutex).finish(Outcome::Acquired);
                    }
                })
            })
            .collect();
        while threads.iter().any(|thread| !thread.is_finished()) {
            let snapshot = owner.snapshot(1);
            if snapshot.coherent {
                balanced(&snapshot.workload.writer_mutex_acquire);
            }
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let snapshot = owner.snapshot(1);
        assert!(snapshot.coherent);
        assert_eq!(snapshot.workload.writer_mutex_acquire.acquired, 2000);
        balanced(&snapshot.workload.writer_mutex_acquire);
    }

    #[test]
    fn real_single_connection_checkout_waits_and_retired_handles_are_not_lost() {
        use crate::index_db::IndexDb;
        let owner: &'static Observations = Box::leak(Box::default());
        let temp = tempfile::tempdir().unwrap();
        with_owner(owner, || {
            let (db, _) = IndexDb::open_with_read_pool_size(&temp.path().join("a.db"), 1).unwrap();
            let db = Arc::new(db);
            let held = db.read_conn().unwrap();
            let before = db.lock_observation_snapshot();
            let other = db.clone();
            let thread = std::thread::spawn(move || {
                with_owner(owner, || {
                    let connection = other.read_conn().unwrap();
                    connection
                        .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                        .unwrap()
                })
            });
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let snapshot = db.lock_observation_snapshot();
                if snapshot.coherent && snapshot.workload.read_connection_checkout.in_flight == 1 {
                    assert_eq!(
                        snapshot.workload.read_connection_checkout.acquired,
                        before.workload.read_connection_checkout.acquired
                    );
                    balanced(&snapshot.workload.read_connection_checkout);
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(!thread.is_finished());
            drop(held);
            assert_eq!(thread.join().unwrap(), 1);
            let final_first = db.lock_observation_snapshot();
            assert!(final_first.coherent);
            assert_eq!(final_first.workload.read_connection_checkout.in_flight, 0);
            assert_eq!(
                final_first.workload.read_connection_checkout.acquired,
                before.workload.read_connection_checkout.acquired + 1
            );
            let first_id = final_first.db_instance_id;
            drop(db);
            let (second, _) = IndexDb::open(&temp.path().join("b.db")).unwrap();
            let second_snapshot = second.lock_observation_snapshot();
            assert_ne!(second_snapshot.db_instance_id, first_id);
            assert_eq!(second_snapshot.created_instances, 2);
            assert_eq!(
                second_snapshot.workload.read_connection_checkout.attempts,
                final_first.workload.read_connection_checkout.attempts
            );
        });
    }

    #[test]
    fn real_unavailable_read_pool_preserves_error_and_records_failed_checkout() {
        use crate::index_db::IndexDb;
        let owner: &'static Observations = Box::leak(Box::default());
        let temp = tempfile::tempdir().unwrap();
        with_owner(owner, || {
            let (db, _) = IndexDb::open(&temp.path().join("unavailable.db")).unwrap();
            *db.pool.write().unwrap() = None;
            let before = db.lock_observation_snapshot();
            let error = match db.read_conn() {
                Ok(_) => panic!("unavailable pool was accepted"),
                Err(error) => error,
            };
            assert!(error
                .to_string()
                .contains("database rebuild reopen failed; reopen the database handle"));
            let after = db.lock_observation_snapshot();
            assert!(after.coherent);
            assert_eq!(
                after.workload.read_pool_lock_acquire.acquired,
                before.workload.read_pool_lock_acquire.acquired + 1
            );
            assert_eq!(
                after.workload.read_connection_checkout.failed,
                before.workload.read_connection_checkout.failed + 1
            );
            balanced(&after.workload.read_connection_checkout);
        });
    }
}
