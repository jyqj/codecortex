//! Per-domain handler functions for code indexing, search, and graph.

pub mod context;
pub mod core;
pub mod facade;
pub mod graph;
pub mod output_budget;

use crate::engine::CodeIndex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Shared runtime handle used by all MCP handlers.
///
/// Keeping the concrete `Arc<RwLock<CodeIndex>>` behind one alias prevents
/// handler signatures from drifting and gives future lock/context refactors a
/// single seam.
pub type SharedCodeIndex = Arc<RwLock<CodeIndex>>;

/// Set once a poisoned CodeIndex lock has been recovered. The recovered value
/// may have been mid-mutation when the panic hit, so surfacing this in
/// diagnostics lets clients see the runtime is degraded and a rebuild is
/// advisable, instead of silently serving from possibly-inconsistent state.
static POISON_RECOVERED: AtomicBool = AtomicBool::new(false);

/// Whether a poisoned lock has ever been recovered in this process.
pub fn poison_recovered() -> bool {
    POISON_RECOVERED.load(Ordering::Relaxed)
}

/// Acquire a read lock on the CodeIndex, recovering from RwLock poisoning.
///
/// If a previous handler panicked inside `spawn_blocking`, the RwLock becomes
/// poisoned. Rather than permanently failing all subsequent requests we recover
/// the inner value; the recovery is recorded so diagnostics can flag the
/// runtime as degraded (a panic mid-write may have left partial in-memory
/// state).
pub fn lock_index(index: &SharedCodeIndex) -> Result<RwLockReadGuard<'_, CodeIndex>, String> {
    match index.read() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => {
            POISON_RECOVERED.store(true, Ordering::Relaxed);
            tracing::warn!("CodeIndex RwLock was poisoned — recovering read guard");
            Ok(poisoned.into_inner())
        }
    }
}

/// Acquire a write lock on the CodeIndex, recovering from RwLock poisoning.
///
/// On recovery the search result cache is invalidated: results computed from
/// a half-mutated CodeIndex must not be served after recovery.
pub fn lock_index_write(
    index: &SharedCodeIndex,
) -> Result<RwLockWriteGuard<'_, CodeIndex>, String> {
    match index.write() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => {
            POISON_RECOVERED.store(true, Ordering::Relaxed);
            tracing::warn!("CodeIndex RwLock was poisoned — recovering write guard");
            let mut guard = poisoned.into_inner();
            guard.invalidate_search_cache_after_poison();
            Ok(guard)
        }
    }
}
