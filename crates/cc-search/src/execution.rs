//! Bounded query admission. Dropping a blocking JoinHandle is not termination:
//! permits are owned by the closure until it actually exits.
use cc_model::{query::QueryControl, CcError, CcResult};
use serde::Serialize;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::Semaphore;

pub async fn until<T>(control: &QueryControl, future: impl Future<Output = T>) -> CcResult<T> {
    control.check()?;
    let value = tokio::select! {
        biased;
        _ = control.cancelled() => return Err(CcError::QueryCancelled),
        _ = tokio::time::sleep_until(control.deadline().into()) => return Err(CcError::QueryTimedOut),
        value = future => value,
    };
    control.check()?;
    Ok(value)
}

#[derive(Debug, Clone)]
pub struct ExecutionPool {
    inner: Arc<Inner>,
}
#[derive(Debug)]
struct Inner {
    cpu: Arc<Semaphore>,
    cpu_admission: Arc<Semaphore>,
    asynchronous: Arc<Semaphore>,
    async_admission: Arc<Semaphore>,
    cpu_limit: usize,
    async_limit: usize,
    queue_limit: usize,
    rejected: AtomicU64,
    completed: AtomicU64,
}
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionStats {
    pub cpu_limit: usize,
    pub async_limit: usize,
    pub queue_limit: usize,
    pub cpu_in_flight: usize,
    pub cpu_admitted: usize,
    pub async_in_flight: usize,
    pub async_admitted: usize,
    pub rejected: u64,
    pub completed: u64,
}
impl ExecutionPool {
    pub fn new(cpu: usize, queued: usize, asynchronous: usize) -> CcResult<Self> {
        if !(1..=64).contains(&cpu) || !(1..=64).contains(&asynchronous) || queued > 4096 {
            return Err(CcError::Config(
                "invalid bounded query executor limits".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                cpu: Arc::new(Semaphore::new(cpu)),
                cpu_admission: Arc::new(Semaphore::new(cpu + queued)),
                asynchronous: Arc::new(Semaphore::new(asynchronous)),
                async_admission: Arc::new(Semaphore::new(asynchronous + queued)),
                cpu_limit: cpu,
                async_limit: asynchronous,
                queue_limit: queued,
                rejected: AtomicU64::new(0),
                completed: AtomicU64::new(0),
            }),
        })
    }
    pub fn stats(&self) -> ExecutionStats {
        let i = &self.inner;
        ExecutionStats {
            cpu_limit: i.cpu_limit,
            async_limit: i.async_limit,
            queue_limit: i.queue_limit,
            cpu_in_flight: i.cpu_limit - i.cpu.available_permits(),
            cpu_admitted: i.cpu_limit + i.queue_limit - i.cpu_admission.available_permits(),
            async_in_flight: i.async_limit - i.asynchronous.available_permits(),
            async_admitted: i.async_limit + i.queue_limit - i.async_admission.available_permits(),
            rejected: i.rejected.load(Ordering::Relaxed),
            completed: i.completed.load(Ordering::Relaxed),
        }
    }
    fn busy(&self) -> CcError {
        self.inner.rejected.fetch_add(1, Ordering::Relaxed);
        CcError::QueryBusy
    }
    pub async fn run_cpu<T: Send + 'static>(
        &self,
        control: QueryControl,
        work: impl FnOnce() -> CcResult<T> + Send + 'static,
    ) -> CcResult<T> {
        control.check()?;
        let admission = self
            .inner
            .cpu_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| self.busy())?;
        let mut cancel = control.cancel_on_drop();
        let permit = until(&control, self.inner.cpu.clone().acquire_owned())
            .await?
            .map_err(|_| self.busy())?;
        let inside = control.clone();
        let inner = self.inner.clone();
        let job = tokio::task::spawn_blocking(move || {
            let (_admission, _permit) = (admission, permit);
            inside.check()?;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .map_err(|_| CcError::Search("query worker panicked".into()))?;
            inner.completed.fetch_add(1, Ordering::Relaxed);
            inside.check()?;
            result
        });
        let result = until(&control, job)
            .await?
            .map_err(|e| CcError::Search(format!("query worker join failed: {e}")))?;
        cancel.disarm();
        result
    }
    /// The optional async port has separate capacity and holds no CPU permit.
    /// Its child timeout does not cancel the whole request's local fallback.
    pub async fn run_async<T>(
        &self,
        control: &QueryControl,
        work: impl Future<Output = CcResult<T>>,
    ) -> CcResult<T> {
        control.check()?;
        let _admission = self
            .inner
            .async_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| self.busy())?;
        let _permit = until(control, self.inner.asynchronous.clone().acquire_owned())
            .await?
            .map_err(|_| self.busy())?;
        let mut work = std::pin::pin!(work);
        let guarded = std::future::poll_fn(|cx| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work.as_mut().poll(cx)))
                .unwrap_or_else(|_| {
                    std::task::Poll::Ready(Err(CcError::Search("async query port panicked".into())))
                })
        });
        until(control, guarded).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_worker_retains_permit_and_cannot_publish_late() {
        let pool = ExecutionPool::new(1, 0, 1).unwrap();
        let control = QueryControl::new(Duration::from_secs(3)).unwrap();
        let published = Arc::new(AtomicBool::new(false));
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker_pool = pool.clone();
        let worker_control = control.clone();
        let inside = control.clone();
        let flag = published.clone();
        let job = tokio::spawn(async move {
            worker_pool
                .run_cpu(worker_control, move || {
                    let _ = start_tx.send(());
                    release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    inside.publish(|| flag.store(true, Ordering::SeqCst))?;
                    Ok(())
                })
                .await
        });
        start_rx.await.unwrap();
        control.cancel();
        assert!(matches!(job.await.unwrap(), Err(CcError::QueryCancelled)));
        assert_eq!(pool.stats().cpu_in_flight, 1);
        let next = QueryControl::new(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            pool.run_cpu(next, || Ok(())).await,
            Err(CcError::QueryBusy)
        ));
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while pool.stats().cpu_in_flight != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!published.load(Ordering::SeqCst));
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_queue_rejects_and_aborted_waiter_releases_admission() {
        let pool = ExecutionPool::new(1, 1, 1).unwrap();
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first_pool = pool.clone();
        let first = tokio::spawn(async move {
            first_pool
                .run_cpu(
                    QueryControl::new(Duration::from_secs(5)).unwrap(),
                    move || {
                        start_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                        Ok(())
                    },
                )
                .await
        });
        start_rx.await.unwrap();
        let queued_pool = pool.clone();
        let queued = tokio::spawn(async move {
            queued_pool
                .run_cpu::<()>(QueryControl::new(Duration::from_secs(5)).unwrap(), || {
                    panic!("cancelled queued work ran")
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while pool.stats().cpu_admitted != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            pool.run_cpu(
                QueryControl::new(Duration::from_secs(1)).unwrap(),
                || Ok(())
            )
            .await,
            Err(CcError::QueryBusy)
        ));
        // Async capacity is independent even while CPU and queue are full.
        assert_eq!(
            pool.run_async(&QueryControl::new(Duration::from_secs(1)).unwrap(), async {
                Ok(7)
            })
            .await
            .unwrap(),
            7
        );
        queued.abort();
        assert!(queued.await.unwrap_err().is_cancelled());
        assert_eq!(pool.stats().cpu_admitted, 1);
        release_tx.send(()).unwrap();
        first.await.unwrap().unwrap();
        assert_eq!(pool.stats().cpu_admitted, 0);
        pool.run_cpu(
            QueryControl::new(Duration::from_secs(1)).unwrap(),
            || Ok(()),
        )
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deadline_discards_late_worker_result_and_reclaims_when_it_exits() {
        let pool = ExecutionPool::new(1, 0, 1).unwrap();
        let published = Arc::new(AtomicBool::new(false));
        let flag = published.clone();
        let control = QueryControl::new(Duration::from_millis(20)).unwrap();
        let inside = control.clone();
        let result = pool
            .run_cpu(control, move || {
                std::thread::sleep(Duration::from_millis(60));
                inside.publish(|| flag.store(true, Ordering::SeqCst))?;
                Ok(())
            })
            .await;
        assert!(matches!(result, Err(CcError::QueryTimedOut)));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let stats = pool.stats();
                // Separate permit destructors/counters are not one atomic
                // observation. Wait for both resources, not an intermediate
                // CPU-only release, before asserting complete reclamation.
                if stats.cpu_in_flight == 0 && stats.cpu_admitted == 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!published.load(Ordering::SeqCst));
        assert_eq!(pool.stats().cpu_admitted, 0);
    }

    #[tokio::test]
    async fn async_timeout_keeps_parent_usable() {
        let pool = ExecutionPool::new(1, 1, 1).unwrap();
        let parent = QueryControl::new(Duration::from_secs(1)).unwrap();
        let child = parent.child(Duration::from_millis(10));
        let result: CcResult<()> = pool.run_async(&child, std::future::pending()).await;
        assert!(matches!(result, Err(CcError::QueryTimedOut)));
        parent.check().unwrap();
        assert_eq!(pool.stats().async_admitted, 0);
    }
}
