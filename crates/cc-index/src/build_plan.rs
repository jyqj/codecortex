//! Index build plan orchestration.
//!
//! This module owns the ordering invariants around a build:
//! scan/diff → parse → dirty closure → enrichment → resolution →
//! route-node/report snapshot → write → postprocess → analysis.
//! Phase implementations stay on `Indexer`; this module keeps the plumbing
//! concentrated so full and incremental builds cannot drift apart.
//!
//! # Staged commit (lock-scope contract)
//!
//! The commit half is itself split into three stages so the caller's
//! exclusive lock only covers actual index writes:
//!
//! 1. [`IndexBuildPlan::commit_write`] — generation guard + `phase_write`,
//!    under the caller's write lock → [`WrittenBuild`].
//! 2. [`IndexBuildPlan::compute_postprocess`] — postprocess/analysis COMPUTE
//!    with no index lock held: reads the just-committed state through the
//!    read pool (WAL readers) and produces typed deltas →
//!    [`StagedPostprocess`].
//! 3. [`IndexBuildPlan::apply_postprocess`] — applies the deltas in short DB
//!    transactions under a (brief) write lock and produces the report.
//!
//! Stage-2 correctness — no concurrent build mutating the DB between write
//! and apply — relies on the caller holding the per-project build gate across
//! all three stages (every cc-server build entry point does). Stage 3 still
//! rechecks `index_epoch` cheaply so a cross-process writer surfaces as
//! [`CcError::StalePreparedBuild`] instead of silently applying stale deltas.
//!
//! Eventual consistency: readers may observe a window where the index
//! content from stage 1 is committed but postprocess artifacts (synthesized
//! edges, communities, test edges, co-change/infra/ADR outputs) are not yet
//! refreshed. This is accepted behavior — every stage-3 apply transaction
//! bumps `index_epoch` as before, so epoch-keyed caches converge as the
//! deltas land. The bundled [`IndexBuildPlan::commit`] composes the same
//! three stage functions inline (single write-lock behavior unchanged), so
//! each stage has exactly one implementation.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use cc_db::index_db::{FileState, FileWriteUnit, PrecompressedChunks};
use cc_model::edge::{RouteNodeRecord, SemanticEdgeRecord};
use cc_model::{BuildExplain, BuildExplainCollector, CcError, CcResult};

use crate::dirty_closure::DirtyPropagationStatus;
use crate::indexer::{FileAction, IndexReport, Indexer, ParseResult, PhaseTiming, ScanDiffResult};
use crate::indexer_phases::{AnalysisPlan, PostprocessPlan};

/// Owned, read-only output of the prepare phase.
///
/// `prepare` runs scan/diff → parse → dirty closure → enrichment → resolution
/// → snapshot without touching the index, producing this owned bundle. `commit`
/// then consumes it to perform `phase_write` → `run_after_write` under the
/// caller's write lock. Fields stay private so callers only transport the
/// bundle, never inspect it.
pub struct PreparedBuild {
    scan_result: ScanDiffResult,
    write_units: Vec<FileWriteUnit>,
    /// Chunk payloads zstd-compressed during prepare (lock-free), so the
    /// commit-side write transaction only binds pre-computed blobs.
    chunk_blobs: PrecompressedChunks,
    actions: HashMap<String, FileAction>,
    output_snapshot: OutputSnapshot,
    hierarchy_edges: Vec<SemanticEdgeRecord>,
    parse_report: ParseReport,
    dirty_propagation: Option<DirtyPropagationStatus>,
    /// `index_epoch` observed at prepare START, before the first DB read
    /// (`get_file_state` in scan/diff). `commit` re-reads the epoch and
    /// refuses to write on mismatch, so a later-committing stale prepare can
    /// never overwrite newer index content. Deliberately NOT the full
    /// generation pair: `evidence_epoch` is bumped concurrently by
    /// runtime-evidence ingestion and would false-positive.
    prepared_index_epoch: u64,
    /// The resolution catalog + seed-token basis, folded back into the
    /// cross-build cache after a successful incremental write (see
    /// `resolver::catalog_cache`).
    catalog_carry: Option<crate::resolver::catalog_cache::CatalogCarry>,
    start: Instant,
    scan_diff_ms: u64,
    parse_ms: u64,
    resolve_ms: u64,
}

/// Report inputs threaded through the commit stages: everything the final
/// [`IndexReport`] needs, plus the progressively accumulated phase timing
/// (compute and apply durations are added to `postprocess_ms`/`analysis_ms`
/// as the stages run).
struct ReportCarry {
    scan_result: ScanDiffResult,
    output_snapshot: OutputSnapshot,
    parse_report: ParseReport,
    dirty_propagation: Option<DirtyPropagationStatus>,
    start: Instant,
    timing: PhaseTiming,
}

/// Owned output of stage 1 ([`IndexBuildPlan::commit_write`]): the index
/// content is committed; postprocess/analysis have not run. Fields stay
/// private so callers only transport the bundle, mirroring [`PreparedBuild`].
pub struct WrittenBuild {
    carry: ReportCarry,
    write_units: Vec<FileWriteUnit>,
    config_units: Vec<FileWriteUnit>,
    /// Build-side explainability collector, started in stage 1 (config-linker
    /// gate decision) and continued through stage 2 (postprocess/analysis
    /// gates). Finished into `IndexReport.build_explain` in stage 3.
    build_explain: BuildExplainCollector,
    /// `index_epoch` observed after the stage-1 writes committed; stage 3
    /// rechecks it before applying deltas. In-process the build gate makes
    /// the recheck a tautology — it exists to surface cross-process writers
    /// as `CcError::StalePreparedBuild`. Deliberately NOT `evidence_epoch`,
    /// which runtime-evidence ingestion bumps concurrently.
    written_index_epoch: u64,
}

/// Owned output of stage 2 ([`IndexBuildPlan::compute_postprocess`]): the
/// typed postprocess/analysis deltas plus the report carry-through for
/// stage 3. Transport-only, like [`WrittenBuild`].
pub struct StagedPostprocess {
    carry: ReportCarry,
    postprocess: PostprocessPlan,
    analysis: AnalysisPlan,
    written_index_epoch: u64,
    build_explain: Option<BuildExplain>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexBuildMode {
    Full,
    Incremental,
}

impl IndexBuildMode {
    fn from_full(full: bool) -> Self {
        if full {
            Self::Full
        } else {
            Self::Incremental
        }
    }

    fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    fn is_incremental(self) -> bool {
        matches!(self, Self::Incremental)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct IndexBuildPlan {
    mode: IndexBuildMode,
    auto_file_limit: Option<usize>,
}

impl IndexBuildPlan {
    pub(crate) fn new(full: bool, auto_file_limit: Option<usize>) -> Self {
        Self {
            mode: IndexBuildMode::from_full(full),
            auto_file_limit,
        }
    }

    pub(crate) fn execute(&self, indexer: &Indexer, project_path: &Path) -> CcResult<IndexReport> {
        let prepared = self.prepare(indexer, project_path)?;
        self.commit(indexer, project_path, prepared)
    }

    /// Read-only half of a build: scan/diff → parse → dirty closure →
    /// enrichment → resolution → snapshot. Touches the file system and reads
    /// the DB but never writes the index, so it is safe to run without holding
    /// the index write lock.
    pub(crate) fn prepare(
        &self,
        indexer: &Indexer,
        project_path: &Path,
    ) -> CcResult<PreparedBuild> {
        let start = Instant::now();

        // Generation snapshot must precede every DB read in this prepare
        // (the first is `get_file_state` inside scan/diff): any index write
        // that lands after our snapshot reads is then detected at commit.
        let prepared_index_epoch = indexer.db.reads().generation()?.index_epoch;

        let phase_start = Instant::now();
        let mut scan_result =
            indexer.phase_scan_and_diff(project_path, self.mode.is_full(), self.auto_file_limit)?;
        let scan_diff_ms = phase_start.elapsed().as_millis() as u64;

        let phase_start = Instant::now();
        let to_parse = std::mem::take(&mut scan_result.to_parse);
        let parse_result = indexer.phase_parse(project_path, to_parse)?;
        let ParsedBuildState {
            mut write_units,
            parse_report,
        } = ParsedBuildState::from(parse_result);

        // Ordering invariant, enforced by types: the dirty closure must be
        // complete before dirty units are reloaded ([`DirtyClosed::reload`]
        // consumes the closure proof), and framework enrichment is only
        // reachable through the resulting [`Reloaded`] token — so dirty units
        // provably participate in project context before resolvers bind
        // framework-specific edges.
        let dirty_closed = DirtyClosed::compute(indexer, self.mode, &scan_result, &write_units)?;
        let dirty_propagation = dirty_closed.dirty_propagation;
        let reloaded = dirty_closed.reload(indexer, &mut write_units, &scan_result.existing)?;
        let parse_ms = phase_start.elapsed().as_millis() as u64;

        let phase_start = Instant::now();
        let fw_context = reloaded.enrich_frameworks(indexer, project_path, &mut write_units)?;

        let resolve_result = indexer.phase_resolve(
            project_path,
            self.mode.is_full(),
            &mut write_units,
            &scan_result.to_remove,
            &fw_context,
        )?;
        let resolve_ms = phase_start.elapsed().as_millis() as u64;

        // Capture report totals and route nodes from the resolved in-memory
        // units before `phase_write` consumes them. Route nodes must be the
        // same snapshot used by both DB write and infra route matching.
        let output_snapshot = OutputSnapshot::from_resolved_units(indexer, &write_units);

        // Chunk text is final after resolution, so compress here (lock-free)
        // instead of inside the commit-side write transaction.
        let chunk_blobs = Indexer::precompress_chunks(&write_units);

        Ok(PreparedBuild {
            scan_result,
            write_units,
            chunk_blobs,
            actions: reloaded.into_actions(),
            output_snapshot,
            hierarchy_edges: resolve_result.hierarchy_edges,
            parse_report,
            dirty_propagation,
            prepared_index_epoch,
            catalog_carry: resolve_result.catalog_carry,
            start,
            scan_diff_ms,
            parse_ms,
            resolve_ms,
        })
    }

    /// Write half of a build, composed from the three stage functions so the
    /// bundled (single write-lock) path and the staged path share exactly one
    /// implementation per stage. See the module doc for the staging contract.
    pub(crate) fn commit(
        &self,
        indexer: &Indexer,
        project_path: &Path,
        prepared: PreparedBuild,
    ) -> CcResult<IndexReport> {
        let written = self.commit_write(indexer, project_path, prepared)?;
        let staged = self.compute_postprocess(indexer, project_path, written)?;
        self.apply_postprocess(indexer, staged)
    }

    /// Stage 1: generation guard + `phase_write`. Must run under the caller's
    /// write lock — the incremental write path is a sequence of independent
    /// batch writes rather than a single transaction, and the lock is what
    /// keeps readers from observing that intermediate state.
    pub(crate) fn commit_write(
        &self,
        indexer: &Indexer,
        project_path: &Path,
        prepared: PreparedBuild,
    ) -> CcResult<WrittenBuild> {
        let PreparedBuild {
            scan_result,
            write_units,
            chunk_blobs,
            actions,
            output_snapshot,
            hierarchy_edges,
            parse_report,
            dirty_propagation,
            prepared_index_epoch,
            catalog_carry,
            start,
            scan_diff_ms,
            parse_ms,
            resolve_ms,
        } = prepared;

        // Generation guard: refuse to commit a PreparedBuild whose snapshot
        // reads predate a newer index write. Checked under the caller's write
        // lock, so the epoch cannot move between this check and phase_write.
        let current_epoch = indexer.db.reads().generation()?.index_epoch;
        if current_epoch != prepared_index_epoch {
            return Err(CcError::StalePreparedBuild {
                prepared_epoch: prepared_index_epoch,
                current_epoch,
            });
        }

        let mut build_explain = BuildExplainCollector::new();
        let phase_start = Instant::now();
        let write_result = indexer.phase_write(
            project_path,
            self.mode.is_full(),
            write_units,
            &actions,
            &scan_result.to_remove,
            &output_snapshot.route_nodes,
            &hierarchy_edges,
            &chunk_blobs,
            &mut build_explain,
        )?;
        let write_ms = phase_start.elapsed().as_millis() as u64;

        // Every phase_write write has committed: fold the resolution catalog
        // into the cross-build cache (or clear it — full rebuilds and
        // unprovable bases must not leave a stale catalog parked).
        crate::resolver::catalog_cache::after_write(
            &indexer.db,
            self.mode.is_full(),
            catalog_carry,
            &write_result.write_units,
            &scan_result.to_remove,
            write_result.seed_tokens,
        );

        // Baseline for the stage-3 recheck, read after every stage-1 write
        // committed.
        let written_index_epoch = indexer.db.reads().generation()?.index_epoch;

        Ok(WrittenBuild {
            carry: ReportCarry {
                scan_result,
                output_snapshot,
                parse_report,
                dirty_propagation,
                start,
                timing: PhaseTiming {
                    scan_diff_ms,
                    parse_ms,
                    resolve_ms,
                    write_ms,
                    postprocess_ms: 0,
                    analysis_ms: 0,
                },
            },
            write_units: write_result.write_units,
            config_units: write_result.config_units,
            build_explain,
            written_index_epoch,
        })
    }

    /// Stage 2: postprocess/analysis COMPUTE — signature gates, synthesis
    /// passes, Louvain, git log, infra walk — reading the just-committed
    /// state through the read pool only. Safe to run with no index lock held;
    /// callers must keep holding the build gate (see module doc).
    pub(crate) fn compute_postprocess(
        &self,
        indexer: &Indexer,
        project_path: &Path,
        written: WrittenBuild,
    ) -> CcResult<StagedPostprocess> {
        let WrittenBuild {
            mut carry,
            write_units,
            config_units,
            mut build_explain,
            written_index_epoch,
        } = written;

        let phase_start = Instant::now();
        let postprocess = indexer.phase_postprocess_compute(
            self.mode.is_full(),
            &write_units,
            &config_units,
            &carry.scan_result.to_remove,
            &carry.scan_result.existing,
            &mut build_explain,
        )?;
        carry.timing.postprocess_ms += phase_start.elapsed().as_millis() as u64;

        let phase_start = Instant::now();
        let analysis = indexer.phase_analysis_compute(
            project_path,
            &write_units,
            &carry.output_snapshot.route_nodes,
            &mut build_explain,
        )?;
        carry.timing.analysis_ms += phase_start.elapsed().as_millis() as u64;

        let build_explain = build_explain.finish_non_empty();
        Ok(StagedPostprocess {
            carry,
            postprocess,
            analysis,
            written_index_epoch,
            build_explain,
        })
    }

    /// Stage 3: APPLY the staged deltas — short DB transactions only — and
    /// produce the report. Must run under the caller's write lock.
    pub(crate) fn apply_postprocess(
        &self,
        indexer: &Indexer,
        staged: StagedPostprocess,
    ) -> CcResult<IndexReport> {
        let StagedPostprocess {
            mut carry,
            postprocess,
            analysis,
            written_index_epoch,
            build_explain,
        } = staged;

        // Cheap generation recheck: in-process the build gate guarantees
        // equality; a cross-process writer that committed during stage 2
        // surfaces here instead of having stale deltas applied on top. Only
        // `index_epoch` is compared — `evidence_epoch` moves concurrently
        // with runtime-evidence ingestion and must never guard a build.
        let current_epoch = indexer.db.reads().generation()?.index_epoch;
        if current_epoch != written_index_epoch {
            return Err(CcError::StalePreparedBuild {
                prepared_epoch: written_index_epoch,
                current_epoch,
            });
        }

        let phase_start = Instant::now();
        indexer.phase_postprocess_apply(&postprocess)?;
        carry.timing.postprocess_ms += phase_start.elapsed().as_millis() as u64;

        let phase_start = Instant::now();
        indexer.phase_analysis_apply(&analysis)?;
        carry.timing.analysis_ms += phase_start.elapsed().as_millis() as u64;

        Ok(self.report(carry, build_explain))
    }

    fn report(&self, carry: ReportCarry, build_explain: Option<BuildExplain>) -> IndexReport {
        let ReportCarry {
            scan_result,
            output_snapshot,
            parse_report,
            dirty_propagation,
            start,
            timing,
        } = carry;
        IndexReport {
            files_scanned: scan_result.files_scanned,
            files_added: scan_result.files_added,
            files_updated: scan_result.files_updated,
            files_removed: scan_result.to_remove.len(),
            files_skipped: scan_result.files_skipped,
            symbols_total: output_snapshot.symbols_total,
            chunks_total: output_snapshot.chunks_total,
            parse_errors: parse_report.parse_errors,
            elapsed_ms: start.elapsed().as_millis() as u64,
            files_parsed: parse_report.files_to_parse,
            used_parallel_parse: parse_report.used_parallel,
            dirty_propagation,
            phase_timing: Some(timing),
            build_explain,
        }
    }
}

/// Proof that the dirty-propagation closure has completed for this build's
/// actions map. Constructed only by [`DirtyClosed::compute`]; the only way to
/// proceed is [`DirtyClosed::reload`], which consumes the proof — so dirty
/// reload provably runs after the closure is complete (it would otherwise
/// miss re-resolve-only units in the catalog).
struct DirtyClosed {
    actions: HashMap<String, FileAction>,
    dirty_count: usize,
    /// Closure status for the report; `None` for full builds, where
    /// propagation does not apply.
    dirty_propagation: Option<DirtyPropagationStatus>,
}

/// Proof that dirty units have been reloaded into `write_units`. Framework
/// enrichment in this plan is only reachable through
/// [`Reloaded::enrich_frameworks`], so "dirty reload before framework
/// enrichment" is a compile-time fact rather than a comment.
struct Reloaded {
    actions: HashMap<String, FileAction>,
}

impl DirtyClosed {
    fn compute(
        indexer: &Indexer,
        mode: IndexBuildMode,
        scan_result: &ScanDiffResult,
        write_units: &[FileWriteUnit],
    ) -> CcResult<Self> {
        let mut actions = indexer.build_actions_map(
            write_units,
            &scan_result.existing,
            &scan_result.scanned_paths,
        );

        // Full builds never promote skipped files; incremental builds may
        // close over importers whose dependency exports changed.
        let (dirty_count, dirty_propagation) = if mode.is_incremental() {
            let outcome = indexer.run_dirty_propagation(&mut actions, write_units)?;
            (outcome.marked, Some(outcome.status))
        } else {
            (0, None)
        };

        Ok(Self {
            actions,
            dirty_count,
            dirty_propagation,
        })
    }

    fn reload(
        self,
        indexer: &Indexer,
        write_units: &mut Vec<FileWriteUnit>,
        existing: &HashMap<String, FileState>,
    ) -> CcResult<Reloaded> {
        indexer.phase_dirty_reload(write_units, &self.actions, existing, self.dirty_count)?;
        Ok(Reloaded {
            actions: self.actions,
        })
    }
}

impl Reloaded {
    /// Framework enrichment must run after dirty reload so dirty units
    /// participate in project context, and before resolution so resolvers can
    /// bind framework-specific edges (the latter is enforced by data flow:
    /// `phase_resolve` requires the returned context).
    fn enrich_frameworks(
        &self,
        indexer: &Indexer,
        project_path: &Path,
        write_units: &mut [FileWriteUnit],
    ) -> CcResult<crate::framework_resolvers::ProjectFrameworkContext> {
        indexer.phase_framework_enrichment(project_path, write_units)
    }

    fn into_actions(self) -> HashMap<String, FileAction> {
        self.actions
    }
}

struct ParsedBuildState {
    write_units: Vec<FileWriteUnit>,
    parse_report: ParseReport,
}

impl From<ParseResult> for ParsedBuildState {
    fn from(parse_result: ParseResult) -> Self {
        Self {
            write_units: parse_result.write_units,
            parse_report: ParseReport {
                parse_errors: parse_result.parse_errors,
                files_to_parse: parse_result.files_to_parse,
                used_parallel: parse_result.used_parallel,
            },
        }
    }
}

struct ParseReport {
    parse_errors: Vec<String>,
    files_to_parse: usize,
    used_parallel: bool,
}

struct OutputSnapshot {
    symbols_total: usize,
    chunks_total: usize,
    route_nodes: Vec<RouteNodeRecord>,
}

impl OutputSnapshot {
    fn from_resolved_units(indexer: &Indexer, write_units: &[FileWriteUnit]) -> Self {
        let mut symbols_total = 0;
        let mut chunks_total = 0;
        for unit in write_units {
            symbols_total += unit.outcome.symbols.len();
            chunks_total += unit.outcome.chunks.len();
        }

        Self {
            symbols_total,
            chunks_total,
            route_nodes: indexer.collect_route_nodes(write_units),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use cc_db::index_db::IndexDb;
    use cc_model::config::IndexingConfig;

    use super::IndexBuildPlan;
    use crate::indexer::Indexer;

    const FIXTURE_FILE: &str = "lib.py";
    const FIXTURE_SOURCE: &str = r#"
def helper(value):
    return value + 1


def main():
    total = 0
    for n in range(10):
        total = helper(total)
    return total


class Accumulator:
    def __init__(self):
        self.total = 0

    def add(self, value):
        self.total = helper(value)
        return self.total
"#;

    /// Write the fixture project into `dir` and return the project path.
    fn write_fixture(dir: &Path) {
        std::fs::write(dir.join(FIXTURE_FILE), FIXTURE_SOURCE).expect("write fixture source");
    }

    fn open_db(dir: &Path) -> Arc<IndexDb> {
        let (db, _) = IndexDb::open(&dir.join("index.sqlite3")).expect("open index db");
        Arc::new(db)
    }

    /// Snapshot of the persisted graph state a build produces: per-edge-type
    /// counts plus node/community totals. Labeled so an assertion failure
    /// names the drifting table directly.
    fn graph_state(db: &IndexDb) -> Vec<(&'static str, i64)> {
        let count = |sql: &str| -> i64 {
            db.reads()
                .query_json(sql, &[])
                .expect("graph state query")
                .first()
                .and_then(|row| row.get("cnt"))
                .and_then(|value| value.as_i64())
                .unwrap_or(0)
        };
        vec![
            ("files", count("SELECT COUNT(*) AS cnt FROM files")),
            ("symbols", count("SELECT COUNT(*) AS cnt FROM symbols")),
            ("chunks", count("SELECT COUNT(*) AS cnt FROM chunks")),
            (
                "call_edges",
                count("SELECT COUNT(*) AS cnt FROM call_edges"),
            ),
            (
                "semantic_edges",
                count("SELECT COUNT(*) AS cnt FROM semantic_edges"),
            ),
            (
                "test_edges",
                count("SELECT COUNT(*) AS cnt FROM test_edges"),
            ),
            (
                "co_change_edges",
                count("SELECT COUNT(*) AS cnt FROM co_change_edges"),
            ),
            ("routes", count("SELECT COUNT(*) AS cnt FROM routes")),
            (
                "community_assigned_symbols",
                count("SELECT COUNT(*) AS cnt FROM symbols WHERE community_id IS NOT NULL"),
            ),
            (
                "communities",
                count("SELECT COUNT(*) AS cnt FROM communities"),
            ),
        ]
    }

    /// `prepare` + `commit` must produce the same `IndexReport` and persisted DB
    /// state as the single-shot `execute` path, proving the split introduces no
    /// behavioral drift.
    #[test]
    fn prepare_commit_matches_execute() {
        let config = IndexingConfig::default();

        // Path A: single-shot execute (== build_index).
        let dir_a = tempfile::tempdir().expect("tempdir a");
        write_fixture(dir_a.path());
        let db_a = open_db(dir_a.path());
        let indexer_a = Indexer::new(db_a.clone(), dir_a.path(), &config);
        let report_a = IndexBuildPlan::new(false, None)
            .execute(&indexer_a, dir_a.path())
            .expect("execute build");

        // Path B: prepare (read-only) then commit (write).
        let dir_b = tempfile::tempdir().expect("tempdir b");
        write_fixture(dir_b.path());
        let db_b = open_db(dir_b.path());
        let indexer_b = Indexer::new(db_b.clone(), dir_b.path(), &config);
        let plan_b = IndexBuildPlan::new(false, None);
        let prepared = plan_b
            .prepare(&indexer_b, dir_b.path())
            .expect("prepare build");
        let report_b = plan_b
            .commit(&indexer_b, dir_b.path(), prepared)
            .expect("commit build");

        // Key report fields must be identical across both paths.
        assert_eq!(report_a.files_added, report_b.files_added, "files_added");
        assert_eq!(
            report_a.files_updated, report_b.files_updated,
            "files_updated"
        );
        assert_eq!(report_a.files_parsed, report_b.files_parsed, "files_parsed");
        assert_eq!(
            report_a.symbols_total, report_b.symbols_total,
            "symbols_total"
        );
        assert_eq!(report_a.chunks_total, report_b.chunks_total, "chunks_total");
        assert!(report_a.symbols_total > 0, "fixture should yield symbols");

        // Persisted DB state must match: symbol, file, and chunk counts.
        let stats_a = db_a.reads().stats(dir_a.path()).expect("stats a");
        let stats_b = db_b.reads().stats(dir_b.path()).expect("stats b");
        assert_eq!(
            stats_a.indexed_symbols, stats_b.indexed_symbols,
            "db indexed_symbols"
        );
        assert_eq!(
            stats_a.indexed_files, stats_b.indexed_files,
            "db indexed_files"
        );
        assert_eq!(
            stats_a.indexed_chunks, stats_b.indexed_chunks,
            "db indexed_chunks"
        );
        assert_eq!(
            stats_a.indexed_symbols, report_b.symbols_total,
            "report vs db symbol count"
        );

        // Final graph state must match table by table: edge counts by type,
        // community assignments, and node totals. This catches postprocess /
        // analysis drift (synthesis, Louvain, test edges, co-change, routes)
        // that the report counters above cannot observe.
        assert_eq!(
            graph_state(&db_a),
            graph_state(&db_b),
            "execute vs prepare+commit persisted graph state"
        );
    }

    /// The staged commit (commit_write → compute_postprocess →
    /// apply_postprocess, with no lock semantics attached here) must produce
    /// the same report and persisted graph state as the bundled single-shot
    /// path — the staging is a lock-scope restructure only.
    #[test]
    fn staged_commit_matches_bundled_commit() {
        let config = IndexingConfig::default();

        // Path A: bundled execute (prepare + composed commit).
        let dir_a = tempfile::tempdir().expect("tempdir a");
        write_fixture(dir_a.path());
        let db_a = open_db(dir_a.path());
        let indexer_a = Indexer::new(db_a.clone(), dir_a.path(), &config);
        let report_a = IndexBuildPlan::new(false, None)
            .execute(&indexer_a, dir_a.path())
            .expect("bundled build");

        // Path B: the three stages driven explicitly.
        let dir_b = tempfile::tempdir().expect("tempdir b");
        write_fixture(dir_b.path());
        let db_b = open_db(dir_b.path());
        let indexer_b = Indexer::new(db_b.clone(), dir_b.path(), &config);
        let plan_b = IndexBuildPlan::new(false, None);
        let prepared = plan_b
            .prepare(&indexer_b, dir_b.path())
            .expect("prepare build");
        let written = plan_b
            .commit_write(&indexer_b, dir_b.path(), prepared)
            .expect("stage 1: commit_write");

        // Eventual-consistency window: after stage 1 the index content is
        // already committed and reader-visible, while postprocess artifacts
        // (communities) are not yet refreshed.
        let mid_stats = db_b.reads().stats(dir_b.path()).expect("mid-stage stats");
        assert!(
            mid_stats.indexed_files >= 1,
            "stage-1 content must be reader-visible before stage 3"
        );

        let staged = plan_b
            .compute_postprocess(&indexer_b, dir_b.path(), written)
            .expect("stage 2: compute_postprocess");
        let report_b = plan_b
            .apply_postprocess(&indexer_b, staged)
            .expect("stage 3: apply_postprocess");

        assert_eq!(report_a.files_added, report_b.files_added, "files_added");
        assert_eq!(report_a.files_parsed, report_b.files_parsed, "files_parsed");
        assert_eq!(
            report_a.symbols_total, report_b.symbols_total,
            "symbols_total"
        );
        assert_eq!(report_a.chunks_total, report_b.chunks_total, "chunks_total");
        assert!(report_a.symbols_total > 0, "fixture should yield symbols");

        // BuildExplain carries the postprocess/analysis gate decisions — a
        // first build has no recorded signatures, so every gate runs and the
        // envelope must be non-empty, and the two paths must agree.
        let be_a = report_a
            .build_explain
            .as_ref()
            .expect("bundled build_explain");
        let be_b = report_b
            .build_explain
            .as_ref()
            .expect("staged build_explain");
        assert!(
            !be_a.gate_decisions.is_empty(),
            "first build records gate decisions"
        );
        assert_eq!(be_a.gate_decisions, be_b.gate_decisions, "gate decisions");
        assert_eq!(be_a.degraded, be_b.degraded, "degrade notes");

        // Final persisted graph state must match table by table — this is
        // what catches a staged pass computing against the wrong snapshot
        // (e.g. community detection missing the synthesis overlay).
        assert_eq!(
            graph_state(&db_a),
            graph_state(&db_b),
            "bundled vs staged persisted graph state"
        );
    }

    /// Chunk payloads are zstd-compressed during prepare (incremental) or
    /// inside the rebuild closure fallback (full). Both strategies share one
    /// deterministic policy, so the persisted chunk rows — blob bytes and
    /// text_encoding — must be byte-identical across build modes, and the
    /// read path must restore the original text.
    #[test]
    fn full_and_incremental_builds_store_identical_chunk_rows() {
        let config = IndexingConfig::default();
        // Repetitive content well above the 128-byte floor guarantees at
        // least one zstd-encoded chunk.
        let repetitive: String = format!(
            "def repeated_handler():\n    return [\n{}    ]\n",
            "        \"the same compressible line of payload text\",\n".repeat(30)
        );

        let chunk_rows = |db: &IndexDb| -> Vec<(String, i64, rusqlite::types::Value, String)> {
            let conn = db.reads().read_conn().expect("read conn");
            let mut stmt = conn
                .prepare(
                    "SELECT file_path, chunk_index, text, text_encoding FROM chunks \
                     ORDER BY file_path, chunk_index",
                )
                .expect("prepare chunk query");
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, rusqlite::types::Value>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .expect("query chunks")
                .map(|r| r.expect("chunk row"))
                .collect();
            rows
        };

        // Path A: incremental build (precompressed side-car through the
        // single write transaction).
        let dir_a = tempfile::tempdir().expect("tempdir a");
        write_fixture(dir_a.path());
        std::fs::write(dir_a.path().join("big.py"), &repetitive).expect("write big fixture");
        let db_a = open_db(dir_a.path());
        let indexer_a = Indexer::new(db_a.clone(), dir_a.path(), &IndexingConfig::default());
        IndexBuildPlan::new(false, None)
            .execute(&indexer_a, dir_a.path())
            .expect("incremental build");

        // Path B: full rebuild (temp-db / direct-writer rebuild protocol).
        let dir_b = tempfile::tempdir().expect("tempdir b");
        write_fixture(dir_b.path());
        std::fs::write(dir_b.path().join("big.py"), &repetitive).expect("write big fixture");
        let db_b = open_db(dir_b.path());
        let indexer_b = Indexer::new(db_b.clone(), dir_b.path(), &config);
        IndexBuildPlan::new(true, None)
            .execute(&indexer_b, dir_b.path())
            .expect("full build");

        let rows_a = chunk_rows(&db_a);
        let rows_b = chunk_rows(&db_b);
        assert!(!rows_a.is_empty(), "fixture must produce chunks");
        assert_eq!(
            rows_a, rows_b,
            "full vs incremental on-disk chunk data must be identical"
        );
        assert!(
            rows_a.iter().any(|(_, _, _, enc)| enc == "zstd"),
            "expected at least one zstd-compressed chunk; got encodings {:?}",
            rows_a
                .iter()
                .map(|(_, _, _, enc)| enc.as_str())
                .collect::<Vec<_>>()
        );

        // Read path restores the original text from the compressed blob:
        // every chunk of big.py must decode to a slice of the source.
        let conn = db_a.reads().read_conn().expect("read conn");
        let mut stmt = conn
            .prepare(
                "SELECT text, text_encoding FROM chunks \
                 WHERE file_path = 'big.py' ORDER BY chunk_index",
            )
            .expect("prepare big.py chunk query");
        let restored: Vec<String> = stmt
            .query_map([], |row| {
                cc_db::index_db::read_chunk_text_with_encoding(row, 0, 1)
            })
            .expect("query big.py chunks")
            .map(|r| r.expect("chunk text"))
            .collect();
        assert!(!restored.is_empty(), "big.py must produce chunks");
        for text in &restored {
            assert!(
                repetitive.contains(text.trim_end_matches('\n')),
                "restored chunk text must be a slice of the original source"
            );
        }
    }

    /// The cross-build seed cache (see `cc_db`'s `seed_symbol_cache`) must
    /// be content-equivalent to a full DB reload after every incremental
    /// batch shape: initial add, export-signature modification of an
    /// imported file (the dirty-closure trigger), file addition, and file
    /// removal. Ground truth comes from a second `IndexDb` handle on the
    /// same file, whose first read is always a fresh SQL load.
    #[test]
    fn seed_cache_matches_full_reload_across_incremental_builds() {
        let config = IndexingConfig::default();
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("index.sqlite3");
        std::fs::write(
            dir.path().join("lib.py"),
            "def helper(value):\n    return value + 1\n\n\ndef util():\n    return 0\n",
        )
        .expect("write lib.py");
        std::fs::write(
            dir.path().join("main.py"),
            "from lib import helper\n\n\ndef main():\n    return helper(1)\n",
        )
        .expect("write main.py");
        std::fs::write(
            dir.path().join("extra.py"),
            "def standalone():\n    return 2\n",
        )
        .expect("write extra.py");

        let db = open_db(dir.path());
        let indexer = Indexer::new(db.clone(), dir.path(), &config);
        let plan = IndexBuildPlan::new(false, None);

        let assert_seed_equivalent = |label: &str| {
            assert!(
                db.seed_cache_len().is_some(),
                "{label}: seed cache must be warm before the equivalence read"
            );
            let (fresh, _) =
                cc_db::index_db::IndexDb::open(&db_path).expect("open ground-truth handle");
            for excluded in [Vec::new(), vec!["main.py".to_string()]] {
                let fingerprints = |rows: &[cc_model::symbol::SymbolRecord]| {
                    let mut keys: Vec<String> = rows
                        .iter()
                        .map(|s| serde_json::to_string(s).expect("serialize symbol"))
                        .collect();
                    keys.sort();
                    keys
                };
                let cached = db
                    .reads()
                    .resolver_seed_symbols_excluding(&excluded)
                    .expect("cached seed read");
                let direct = fresh
                    .reads()
                    .resolver_seed_symbols_excluding(&excluded)
                    .expect("direct seed read");
                assert_eq!(
                    fingerprints(&cached),
                    fingerprints(&direct),
                    "{label}: cached seed rows diverged from a full reload (excluded: {excluded:?})"
                );
                assert!(
                    !cached.is_empty(),
                    "{label}: fixture must yield persisted seed symbols"
                );
            }
        };

        plan.execute(&indexer, dir.path()).expect("initial build");
        // Warm the cache through the read path: the initial build runs on a
        // fresh database whose aggregate baseline is created mid-batch, so
        // the batch itself cannot prove its pre-state.
        db.reads()
            .resolver_seed_symbols_excluding(&[])
            .expect("warming read");
        assert_seed_equivalent("after initial build");

        // Export-signature change of an imported file: triggers the dirty
        // closure (main.py re-resolves) and rewrites lib.py's seed rows.
        std::fs::write(
            dir.path().join("lib.py"),
            "def helper(value, scale):\n    return value * scale\n\n\ndef util():\n    return 0\n",
        )
        .expect("modify lib.py");
        plan.execute(&indexer, dir.path()).expect("modify build");
        assert_seed_equivalent("after export-signature change");

        std::fs::write(
            dir.path().join("newcomer.py"),
            "def newcomer():\n    return 3\n",
        )
        .expect("write newcomer.py");
        plan.execute(&indexer, dir.path()).expect("add build");
        assert_seed_equivalent("after file addition");

        std::fs::remove_file(dir.path().join("extra.py")).expect("remove extra.py");
        plan.execute(&indexer, dir.path()).expect("remove build");
        assert_seed_equivalent("after file removal");
    }

    /// Cross-build resolver catalog cache lifecycle: an incremental build
    /// with a provable token basis parks the folded catalog, the next build
    /// takes it (observed via the per-handle hit counter), removal-only
    /// batches fold in place, full rebuilds clear the slot — and throughout,
    /// the resolved call-edge rows (UIDs included, not just counts) stay
    /// identical to a same-content full rebuild on a fresh handle.
    ///
    /// TypeScript fixture on purpose: the dirty closure keys on export
    /// fingerprints, which only exported symbols contribute to — a lib.ts
    /// signature change must promote main.ts to DirtyResolveOnly and push
    /// the dirty-reload path through the reused catalog.
    #[test]
    fn catalog_cache_reuse_matches_full_rebuild() {
        use crate::resolver::catalog_cache;

        let config = IndexingConfig::default();
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path();
        std::fs::write(
            project.join("lib.ts"),
            "export function helper(value: number): number {\n    return value + 1;\n}\n\nexport function utilFn(): number {\n    return 0;\n}\n",
        )
        .expect("write lib.ts");
        std::fs::write(
            project.join("main.ts"),
            "import { helper } from './lib';\n\nexport function mainEntry(): number {\n    return helper(1);\n}\n",
        )
        .expect("write main.ts");
        std::fs::write(
            project.join("extra.ts"),
            "export function standaloneFn(): number {\n    return 2;\n}\n",
        )
        .expect("write extra.ts");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = Arc::new(
            IndexDb::open(&db_dir.path().join("index.sqlite3"))
                .expect("open db")
                .0,
        );
        let indexer = Indexer::new(db.clone(), project, &config);
        let plan = IndexBuildPlan::new(false, None);

        // Build 1: fresh database — the aggregate baseline is created
        // mid-batch, so the fold cannot prove its pre-state; nothing parks.
        plan.execute(&indexer, project).expect("build 1");

        // Build 2 (edit main.ts): the seed load carries a token now; the
        // post-write fold must park the catalog.
        std::fs::write(
            project.join("main.ts"),
            "import { helper } from './lib';\n\nexport function mainEntry(): number {\n    return helper(2);\n}\n\nexport function secondEntry(): number {\n    return helper(3);\n}\n",
        )
        .expect("edit main.ts");
        plan.execute(&indexer, project).expect("build 2");
        assert!(
            catalog_cache::parked_live_len(&db).is_some(),
            "build 2 must park the folded catalog"
        );

        // Build 3: export-signature change in lib.ts — helper's uid moves,
        // the dirty closure promotes main.ts, and the dirty re-resolution
        // must run against the reused catalog.
        let hits_before = catalog_cache::cache_hits(&db);
        std::fs::write(
            project.join("lib.ts"),
            "export function helper(value: number, scale: number): number {\n    return value * scale;\n}\n\nexport function utilFn(): number {\n    return 0;\n}\n",
        )
        .expect("edit lib.ts");
        plan.execute(&indexer, project).expect("build 3");
        assert_eq!(
            catalog_cache::cache_hits(&db),
            hits_before + 1,
            "build 3 must reuse the parked catalog"
        );
        assert!(
            catalog_cache::parked_live_len(&db).is_some(),
            "build 3 must re-park after its fold"
        );

        // Removal-only batch (nothing parsed): folds the removal into the
        // parked catalog without taking it through a resolve.
        let live_before = catalog_cache::parked_live_len(&db).expect("parked before removal");
        std::fs::remove_file(project.join("extra.ts")).expect("remove extra.ts");
        plan.execute(&indexer, project).expect("removal build");
        let live_after =
            catalog_cache::parked_live_len(&db).expect("removal must keep the catalog parked");
        assert!(
            live_after < live_before,
            "removal fold must shrink live entries ({live_before} -> {live_after})"
        );

        // Resolution equivalence against a same-content full rebuild: the
        // fixture is ambiguity-free, so resolved rows must match exactly.
        // This is what catches a stale reused catalog: a dangling old uid on
        // main.ts's helper calls would differ from the rebuilt rows.
        let resolved_rows = |db: &IndexDb| -> Vec<String> {
            db.reads()
                .query_json(
                    "SELECT file_path || '|' || callee_symbol || '|' || \
                     COALESCE(callee_symbol_uid,'') || '|' || COALESCE(target_file_path,'') \
                     AS row FROM call_edges ORDER BY row",
                    &[],
                )
                .expect("call edge rows")
                .iter()
                .filter_map(|r| r.get("row").and_then(|v| v.as_str()).map(String::from))
                .collect()
        };
        let incremental_edges = resolved_rows(&db);
        assert!(
            incremental_edges
                .iter()
                .any(|row| row.contains("helper|") && row.contains("lib.ts")),
            "cross-file helper call must resolve to lib.ts; got {incremental_edges:?}"
        );

        let db_full = Arc::new(
            IndexDb::open(&db_dir.path().join("index_full.sqlite3"))
                .expect("open full db")
                .0,
        );
        let indexer_full = Indexer::new(db_full.clone(), project, &config);
        IndexBuildPlan::new(true, None)
            .execute(&indexer_full, project)
            .expect("full rebuild");
        assert_eq!(
            incremental_edges,
            resolved_rows(&db_full),
            "cached-path resolution must match a same-content full rebuild"
        );

        // A full rebuild on the caching handle replaces the whole symbol
        // table — the slot must be cleared, not left to rot.
        IndexBuildPlan::new(true, None)
            .execute(&indexer, project)
            .expect("full build on caching handle");
        assert!(
            catalog_cache::parked_live_len(&db).is_none(),
            "full rebuild must clear the parked catalog"
        );
    }

    /// A `PreparedBuild` whose snapshot predates a newer index write must be
    /// rejected at commit time with the typed stale error — otherwise a
    /// later-committing stale prepare would overwrite fresher index content.
    #[test]
    fn stale_prepared_build_is_rejected_at_commit() {
        let config = IndexingConfig::default();
        let dir = tempfile::tempdir().expect("tempdir");
        write_fixture(dir.path());
        let db = open_db(dir.path());
        let indexer = Indexer::new(db.clone(), dir.path(), &config);

        IndexBuildPlan::new(false, None)
            .execute(&indexer, dir.path())
            .expect("initial build");

        // Snapshot a prepare, then let a concurrent build land first.
        let plan = IndexBuildPlan::new(false, None);
        let prepared = plan.prepare(&indexer, dir.path()).expect("prepare build");

        std::fs::write(
            dir.path().join("newcomer.py"),
            "def newcomer():\n    return 1\n",
        )
        .expect("write interleaved file");
        IndexBuildPlan::new(true, None)
            .execute(&indexer, dir.path())
            .expect("interleaved full build");

        let err = plan
            .commit(&indexer, dir.path(), prepared)
            .expect_err("stale prepared build must be rejected");
        match err {
            cc_model::CcError::StalePreparedBuild {
                prepared_epoch,
                current_epoch,
            } => assert!(
                current_epoch > prepared_epoch,
                "interleaved build must have advanced the epoch: prepared {prepared_epoch}, current {current_epoch}"
            ),
            other => panic!("expected StalePreparedBuild, got: {other}"),
        }
    }
}
