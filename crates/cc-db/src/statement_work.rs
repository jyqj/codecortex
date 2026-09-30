//! Reset cached-statement counters before each measured execution. Never installs
//! connection-global tracing callbacks or shares query counters between callers.
use cc_model::retrieval_cost::{SqlWork, TextWork};
use rusqlite::{Statement, StatementStatus};

pub(crate) fn reset(stmt: &Statement<'_>) {
    for s in [
        StatementStatus::VmStep,
        StatementStatus::FullscanStep,
        StatementStatus::Sort,
    ] {
        stmt.reset_status(s);
    }
}
pub(crate) fn finish(stmt: &Statement<'_>, rows: usize) -> SqlWork {
    SqlWork {
        statements: 1,
        rows,
        vm_steps: u64::try_from(stmt.get_status(StatementStatus::VmStep)).ok(),
        fullscan_steps: u64::try_from(stmt.get_status(StatementStatus::FullscanStep)).ok(),
        sorts: u64::try_from(stmt.get_status(StatementStatus::Sort)).ok(),
    }
}
pub(crate) fn text(
    work: &mut TextWork,
    row: &rusqlite::Row<'_>,
    encoding: usize,
    bytes: usize,
    cached: bool,
) {
    work.utf8_bytes = work.utf8_bytes.saturating_add(bytes as u64);
    if cached {
        work.cache_hits += 1;
        return;
    }
    work.storage_reads += 1;
    match row
        .get::<_, Option<String>>(encoding)
        .ok()
        .flatten()
        .as_deref()
    {
        Some("plain") => {}
        Some("zstd") => work.zstd_decodes += 1,
        _ => work.legacy_auto_reads += 1,
    }
}
