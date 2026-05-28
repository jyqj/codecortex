//! Per-domain handler functions for code indexing, search, and graph.

pub mod context;
pub mod core;
pub mod facade;
pub mod graph;

use crate::engine::CodeIndex;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Shared runtime handle used by all MCP handlers.
///
/// Keeping the concrete `Arc<RwLock<CodeIndex>>` behind one alias prevents
/// handler signatures from drifting and gives future lock/context refactors a
/// single seam.
pub type SharedCodeIndex = Arc<RwLock<CodeIndex>>;

/// Acquire a read lock on the CodeIndex, recovering from RwLock poisoning.
///
/// If a previous handler panicked inside `spawn_blocking`, the RwLock becomes
/// poisoned. Rather than permanently failing all subsequent requests we recover
/// the inner value — the `CodeIndex` itself is still in a consistent state
/// because panics happen before any partial mutation is committed.
pub fn lock_index(index: &SharedCodeIndex) -> Result<RwLockReadGuard<'_, CodeIndex>, String> {
    match index.read() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => {
            tracing::warn!("CodeIndex RwLock was poisoned — recovering read guard");
            Ok(poisoned.into_inner())
        }
    }
}

/// Acquire a write lock on the CodeIndex, recovering from RwLock poisoning.
pub fn lock_index_write(
    index: &SharedCodeIndex,
) -> Result<RwLockWriteGuard<'_, CodeIndex>, String> {
    match index.write() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => {
            tracing::warn!("CodeIndex RwLock was poisoned — recovering write guard");
            Ok(poisoned.into_inner())
        }
    }
}
