//! Request-local work receipts. These are originating work, not cache-hit work or
//! global resource limits. SQL counters cover only the explicitly named statements.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqlWork {
    pub statements: usize,
    /// Rows yielded to Rust, including metadata-only probes and duplicate rows.
    pub rows: usize,
    /// SQLite outer-statement counters; not FTS internal work, I/O bytes or time.
    /// None denotes a negative counter or aggregate overflow, never a measured
    /// zero. SQLite counters beyond their signed 32-bit range are undefined;
    /// this does not detect every possible wraparound or certify huge workloads.
    pub vm_steps: Option<u64>,
    pub fullscan_steps: Option<u64>,
    pub sorts: Option<u64>,
}
impl Default for SqlWork {
    fn default() -> Self {
        Self {
            statements: 0,
            rows: 0,
            vm_steps: Some(0),
            fullscan_steps: Some(0),
            sorts: Some(0),
        }
    }
}
impl SqlWork {
    pub fn merge(&mut self, other: Self) {
        self.statements = self.statements.saturating_add(other.statements);
        self.rows = self.rows.saturating_add(other.rows);
        self.vm_steps = self
            .vm_steps
            .zip(other.vm_steps)
            .and_then(|(a, b)| a.checked_add(b));
        self.fullscan_steps = self
            .fullscan_steps
            .zip(other.fullscan_steps)
            .and_then(|(a, b)| a.checked_add(b));
        self.sorts = self
            .sorts
            .zip(other.sorts)
            .and_then(|(a, b)| a.checked_add(b));
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextWork {
    pub storage_reads: usize,
    pub zstd_decodes: usize,
    pub legacy_auto_reads: usize,
    pub cache_hits: usize,
    pub utf8_bytes: u64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadWork {
    pub sql: SqlWork,
    pub text: TextWork,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrepStageWork {
    pub stage: String,
    pub work: ReadWork,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalCost {
    pub schema_version: u32,
    pub coverage: String,
    pub lexical_sql: SqlWork,
    pub hydration: ReadWork,
    pub lane_candidates: BTreeMap<String, usize>,
    pub fused_candidates: usize,
    pub hydrate_requested: usize,
}
impl Default for RetrievalCost {
    fn default() -> Self {
        Self {
            schema_version: 1,
            coverage: "originating query work; lexical FTS and hydration SQL here; grep stage SQL in grep.stages; excludes preselection/exact lookup/graph SQL, FTS internal work and storage I/O; not total cost or a hard limit".into(),
            lexical_sql: SqlWork::default(), hydration: ReadWork::default(),
            lane_candidates: BTreeMap::new(), fused_candidates: 0, hydrate_requested: 0,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_and_overflow_are_not_zero() {
        let mut a = SqlWork {
            vm_steps: Some(u64::MAX),
            ..Default::default()
        };
        a.merge(SqlWork {
            vm_steps: Some(1),
            ..Default::default()
        });
        assert_eq!(a.vm_steps, None);
        a.merge(SqlWork::default());
        assert_eq!(a.vm_steps, None);
        assert_eq!(a.sorts, Some(0));
    }
}
