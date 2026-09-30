//! Runtime-neutral query policy inputs, deadlines and cooperative cancellation.
use crate::{CcError, CcResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalStrategy {
    #[default]
    Local,
    Auto,
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueryConfig {
    pub strategy: RetrievalStrategy,
    pub deadline_ms: u64,
    pub lane_timeout_ms: u64,
    pub semantic_timeout_ms: u64,
    pub semantic_top_k: usize,
}
impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            strategy: RetrievalStrategy::Local,
            deadline_ms: 30_000,
            lane_timeout_ms: 20_000,
            semantic_timeout_ms: 5_000,
            semantic_top_k: 24,
        }
    }
}
impl QueryConfig {
    pub fn validate(&self) -> CcResult<()> {
        if [
            self.deadline_ms,
            self.lane_timeout_ms,
            self.semantic_timeout_ms,
        ]
        .iter()
        .any(|v| !(1..=600_000).contains(v))
            || !(1..=4096).contains(&self.semantic_top_k)
        {
            return Err(CcError::Config(
                "query budgets must be 1..=600000 ms and semantic_top_k 1..=4096".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    next_waiter: AtomicU64,
    waiters: Mutex<HashMap<u64, Waker>>,
    publication: Mutex<()>,
}
/// Shared cancellation, immutable absolute deadline. Child budgets cannot extend
/// the parent deadline. No scheduler, network, SQL connection or thread is owned.
#[derive(Debug, Clone)]
pub struct QueryControl {
    state: Arc<CancellationState>,
    started: Instant,
    deadline: Instant,
}
impl QueryControl {
    pub fn new(timeout: Duration) -> CcResult<Self> {
        let started = Instant::now();
        let deadline = started
            .checked_add(timeout)
            .ok_or_else(|| CcError::InvalidParams("query deadline overflow".into()))?;
        Ok(Self {
            state: Arc::default(),
            started,
            deadline,
        })
    }
    pub fn child(&self, timeout: Duration) -> Self {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or(self.deadline)
            .min(self.deadline);
        Self {
            state: self.state.clone(),
            started: self.started,
            deadline,
        }
    }
    pub fn limit_total(&self, timeout: Duration) -> Self {
        let deadline = self
            .started
            .checked_add(timeout)
            .unwrap_or(self.deadline)
            .min(self.deadline);
        Self {
            state: self.state.clone(),
            started: self.started,
            deadline,
        }
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }
    pub fn check(&self) -> CcResult<()> {
        if self.is_cancelled() {
            Err(CcError::QueryCancelled)
        } else if Instant::now() >= self.deadline {
            Err(CcError::QueryTimedOut)
        } else {
            Ok(())
        }
    }
    pub fn cancel(&self) {
        let waiters = {
            let _fence = self
                .state
                .publication
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            self.state.cancelled.store(true, Ordering::Release);
            std::mem::take(&mut *self.state.waiters.lock().unwrap_or_else(|p| p.into_inner()))
        };
        for (_, waker) in waiters {
            waker.wake();
        }
    }
    /// Linearize a small cache publication against cancellation. Never run SQL,
    /// user callbacks or await while holding this fence.
    pub fn publish<T>(&self, write: impl FnOnce() -> T) -> CcResult<T> {
        let _fence = self
            .state
            .publication
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.check()?;
        Ok(write())
    }
    pub fn cancelled(&self) -> Cancelled {
        Cancelled {
            state: self.state.clone(),
            id: None,
        }
    }
    pub fn cancel_on_drop(&self) -> CancelOnDrop {
        CancelOnDrop {
            control: Some(self.clone()),
        }
    }
}
#[derive(Debug)]
pub struct CancelOnDrop {
    control: Option<QueryControl>,
}
impl CancelOnDrop {
    pub fn disarm(&mut self) {
        self.control = None;
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(c) = &self.control {
            c.cancel();
        }
    }
}

pub struct Cancelled {
    state: Arc<CancellationState>,
    id: Option<u64>,
}
impl Future for Cancelled {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        let id = match self.id {
            Some(id) => id,
            None => {
                let id = self.state.next_waiter.fetch_add(1, Ordering::Relaxed);
                self.id = Some(id);
                id
            }
        };
        let mut waiters = self.state.waiters.lock().unwrap_or_else(|p| p.into_inner());
        if self.state.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        waiters.insert(id, cx.waker().clone());
        Poll::Pending
    }
}
impl Drop for Cancelled {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.state
                .waiters
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_cannot_extend_deadline_and_cancel_blocks_publication() {
        let root = QueryControl::new(Duration::from_secs(5)).unwrap();
        assert_eq!(
            root.deadline(),
            root.child(Duration::from_secs(50)).deadline()
        );
        let child = root.child(Duration::from_secs(1));
        child.cancel();
        assert!(matches!(
            root.publish(|| panic!("late publication")),
            Err(CcError::QueryCancelled)
        ));
    }
    #[test]
    fn expired_control_and_bad_config_fail_closed() {
        let c = QueryControl::new(Duration::ZERO).unwrap();
        assert!(matches!(c.check(), Err(CcError::QueryTimedOut)));
        let cfg = QueryConfig {
            deadline_ms: 0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
        assert!(serde_json::from_str::<QueryConfig>(r#"{"strategy":"invented"}"#).is_err());
    }
}
