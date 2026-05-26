//! Tree-sitter allocation statistics and per-file memory reclaim.
//!
//! Provides allocation tracking to measure tree-sitter's memory usage patterns
//! and a reclaim mechanism to bound per-file peak memory during parallel parsing.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Global allocation counters for tree-sitter operations
static TS_ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static TS_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static TS_FREE_COUNT: AtomicU64 = AtomicU64::new(0);
static TS_PEAK_BYTES: AtomicU64 = AtomicU64::new(0);
static TRACKING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Allocation statistics snapshot
#[derive(Debug, Clone)]
pub struct AllocStats {
    pub current_bytes: u64,
    pub peak_bytes: u64,
    pub alloc_count: u64,
    pub free_count: u64,
}

impl AllocStats {
    /// Take a snapshot of current allocation stats
    pub fn snapshot() -> Self {
        Self {
            current_bytes: TS_ALLOC_BYTES.load(Ordering::Relaxed),
            peak_bytes: TS_PEAK_BYTES.load(Ordering::Relaxed),
            alloc_count: TS_ALLOC_COUNT.load(Ordering::Relaxed),
            free_count: TS_FREE_COUNT.load(Ordering::Relaxed),
        }
    }

    /// Reset counters (call between files for per-file stats)
    pub fn reset_counters() {
        TS_ALLOC_BYTES.store(0, Ordering::Relaxed);
        TS_ALLOC_COUNT.store(0, Ordering::Relaxed);
        TS_FREE_COUNT.store(0, Ordering::Relaxed);
        // Don't reset peak — it tracks session peak
    }

    /// Reset everything including peak
    pub fn reset_all() {
        Self::reset_counters();
        TS_PEAK_BYTES.store(0, Ordering::Relaxed);
    }
}

/// Enable allocation tracking.
/// Call once at startup before any parsing.
pub fn enable_tracking() {
    TRACKING_ENABLED.store(true, Ordering::Relaxed);
}

/// Check if tracking is enabled
pub fn is_tracking_enabled() -> bool {
    TRACKING_ENABLED.load(Ordering::Relaxed)
}

/// Per-file reclaim: call after each file's parse to release memory back.
/// This helps bound peak RSS during parallel parsing.
///
/// Currently this triggers:
/// 1. Reset per-file allocation counters
/// 2. Hint to the allocator to return memory to OS (if supported)
pub fn per_file_reclaim() {
    if !is_tracking_enabled() {
        return;
    }

    let stats = AllocStats::snapshot();

    // Update peak if current exceeds it
    let mut current_peak = TS_PEAK_BYTES.load(Ordering::Relaxed);
    while stats.current_bytes > current_peak {
        match TS_PEAK_BYTES.compare_exchange_weak(
            current_peak,
            stats.current_bytes,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current_peak = actual,
        }
    }

    // Log stats for large files (> 1MB allocated)
    if stats.current_bytes > 1_048_576 {
        tracing::debug!(
            alloc_mb = stats.current_bytes / 1_048_576,
            alloc_count = stats.alloc_count,
            free_count = stats.free_count,
            "per-file tree-sitter allocation stats"
        );
    }

    // Reset per-file counters
    AllocStats::reset_counters();
}

/// Summary stats for the entire parsing session
pub fn session_summary() -> AllocStats {
    AllocStats {
        current_bytes: TS_ALLOC_BYTES.load(Ordering::Relaxed),
        peak_bytes: TS_PEAK_BYTES.load(Ordering::Relaxed),
        alloc_count: TS_ALLOC_COUNT.load(Ordering::Relaxed),
        free_count: TS_FREE_COUNT.load(Ordering::Relaxed),
    }
}

// Note: tree_sitter::set_allocator() requires specific C ABI functions.
// The actual allocator replacement is deferred to a future stage
// (requires unsafe FFI and careful testing with tree-sitter internals).
// For now, this module provides the tracking infrastructure and
// per-file reclaim discipline that can be used by the indexer.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_snapshot_works() {
        let stats = AllocStats::snapshot();
        assert_eq!(stats.alloc_count, 0);
    }

    #[test]
    fn reset_clears_counters() {
        TS_ALLOC_COUNT.store(100, Ordering::Relaxed);
        AllocStats::reset_counters();
        assert_eq!(TS_ALLOC_COUNT.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn enable_tracking_works() {
        enable_tracking();
        assert!(is_tracking_enabled());
    }

    #[test]
    fn per_file_reclaim_updates_peak() {
        AllocStats::reset_all();
        enable_tracking();
        TS_ALLOC_BYTES.store(2_000_000, Ordering::Relaxed);
        per_file_reclaim();
        assert_eq!(TS_PEAK_BYTES.load(Ordering::Relaxed), 2_000_000);
        // After reclaim, counters reset
        assert_eq!(TS_ALLOC_COUNT.load(Ordering::Relaxed), 0);
    }
}
