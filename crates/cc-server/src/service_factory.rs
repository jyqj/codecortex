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
pub struct QueryServices {
    pub pool: ExecutionPool,
    semantic: RwLock<Option<Arc<dyn SemanticRecall>>>,
    pins: Arc<AtomicUsize>,
}
impl Default for QueryServices {
    fn default() -> Self {
        Self {
            pool: query_pool(),
            semantic: RwLock::new(None),
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
}
