//! Indexer phase implementations (Phase 3.6 – Phase 11).
//!
//! Split from `indexer.rs` for maintainability. All methods are on `impl Indexer`.

mod analysis;
mod config_link;
mod dirty;
mod postprocess;
mod resolve;
mod snapshot;
mod write;

pub(crate) use analysis::{AnalysisInputs, AnalysisPlan};
pub(crate) use postprocess::PostprocessPlan;

/// Signature algorithm versions, persisted next to each recorded signature
/// (see `pass_gate`). Bump a version when its signature's column set or hash
/// formula changes, so a stale recorded value forces exactly one recompute
/// instead of a wrong skip. Signatures recorded before the version keys
/// existed read as version "1".
///
/// Version "2" (dispatch/interface/community): the table-scan signatures were
/// replaced by commutative aggregates maintained at write time (see
/// `cc_db::signature_agg`), turning the gate decision from O(repo) into
/// O(1) metadata reads. Same input coverage per gate; different hash formula,
/// hence the bump.
/// Version "2" (infra/config): the candidate walks were unified into the
/// scanner's shared walk manifest — rel paths are now `/`-normalized and the
/// global gitignore is excluded everywhere (`git_global(false)`), so the
/// candidate set may differ from version "1" on some setups. Bumped to force
/// exactly one recompute after upgrade.
const DISPATCH_SIG_ALGORITHM: &str = "2";
const INTERFACE_SIG_ALGORITHM: &str = "2";
const COMMUNITY_SIG_ALGORITHM: &str = "2";
const INFRA_SIG_ALGORITHM: &str = "2";
const CONFIG_SIG_ALGORITHM: &str = "2";
const ADR_SIG_ALGORITHM: &str = "1";

/// Time one postprocess/analysis sub-step and emit a `tracing::debug!` event
/// with the elapsed milliseconds, so a slow `postprocess_ms`/`analysis_ms`
/// aggregate can be attributed to a specific sub-step from logs without a
/// profiler.
pub(crate) fn time_step<T>(phase: &'static str, step: &'static str, f: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let result = f();
    tracing::debug!(
        phase,
        step,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "sub-phase timing"
    );
    result
}
