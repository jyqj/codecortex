//! Index build plan orchestration.
//!
//! This module owns the ordering invariants around a build:
//! scan/diff → parse → dirty closure → enrichment → resolution →
//! route-node/report snapshot → write → postprocess → analysis.
//! Phase implementations stay on `Indexer`; this module keeps the plumbing
//! concentrated so full and incremental builds cannot drift apart.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use cc_db::index_db::FileWriteUnit;
use cc_model::edge::{RouteNodeRecord, SemanticEdgeRecord};
use cc_model::CcResult;

use crate::indexer::{FileAction, IndexReport, Indexer, ParseResult, ScanDiffResult, WriteResult};

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
    actions: HashMap<String, FileAction>,
    output_snapshot: OutputSnapshot,
    hierarchy_edges: Vec<SemanticEdgeRecord>,
    parse_report: ParseReport,
    start: Instant,
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

        let mut scan_result =
            indexer.phase_scan_and_diff(project_path, self.mode.is_full(), self.auto_file_limit)?;
        let to_parse = std::mem::take(&mut scan_result.to_parse);
        let parse_result = indexer.phase_parse(project_path, to_parse)?;
        let ParsedBuildState {
            mut write_units,
            parse_report,
        } = ParsedBuildState::from(parse_result);

        let dirty_closure =
            DirtyClosure::prepare(indexer, self.mode, &scan_result, &mut write_units)?;

        // Framework enrichment must run after dirty reload so dirty units
        // participate in project context, and before resolution so resolvers
        // can bind framework-specific edges.
        let fw_context =
            indexer.phase_framework_enrichment(project_path, &mut write_units)?;

        let resolve_result = indexer.phase_resolve(
            project_path,
            self.mode.is_full(),
            &mut write_units,
            &scan_result.to_remove,
            &fw_context,
        )?;

        // Capture report totals and route nodes from the resolved in-memory
        // units before `phase_write` consumes them. Route nodes must be the
        // same snapshot used by both DB write and infra route matching.
        let output_snapshot = OutputSnapshot::from_resolved_units(indexer, &write_units);

        Ok(PreparedBuild {
            scan_result,
            write_units,
            actions: dirty_closure.into_actions(),
            output_snapshot,
            hierarchy_edges: resolve_result.hierarchy_edges,
            parse_report,
            start,
        })
    }

    /// Write half of a build: `phase_write` → `run_after_write` → report.
    /// Postprocess/analysis stay here, alongside `phase_write`, because the
    /// incremental write path is a sequence of independent batch writes rather
    /// than a single transaction; the caller's write lock is what keeps readers
    /// from observing the intermediate state.
    pub(crate) fn commit(
        &self,
        indexer: &Indexer,
        project_path: &Path,
        prepared: PreparedBuild,
    ) -> CcResult<IndexReport> {
        let PreparedBuild {
            scan_result,
            write_units,
            actions,
            output_snapshot,
            hierarchy_edges,
            parse_report,
            start,
        } = prepared;

        let write_result = indexer.phase_write(
            project_path,
            self.mode.is_full(),
            write_units,
            &actions,
            &scan_result.to_remove,
            &output_snapshot.route_nodes,
            &hierarchy_edges,
        )?;

        self.run_after_write(
            indexer,
            project_path,
            &scan_result,
            &write_result,
            &output_snapshot,
        )?;

        Ok(self.report(scan_result, parse_report, output_snapshot, start.elapsed()))
    }

    fn run_after_write(
        &self,
        indexer: &Indexer,
        project_path: &Path,
        scan_result: &ScanDiffResult,
        write_result: &WriteResult,
        output_snapshot: &OutputSnapshot,
    ) -> CcResult<()> {
        // Post-processing observes the live DB after full/direct-writer or
        // incremental writes have completed.
        indexer.phase_postprocess(
            project_path,
            self.mode.is_full(),
            &write_result.write_units,
            &write_result.config_units,
            &scan_result.to_remove,
        )?;

        // Analysis intentionally reuses the pre-write route-node snapshot used
        // for the DB write so infra route matching cannot drift from persisted
        // route nodes in the same build.
        indexer.phase_analysis(
            project_path,
            &write_result.write_units,
            &output_snapshot.route_nodes,
        )
    }

    fn report(
        &self,
        scan_result: ScanDiffResult,
        parse_report: ParseReport,
        output_snapshot: OutputSnapshot,
        elapsed: Duration,
    ) -> IndexReport {
        IndexReport {
            files_scanned: scan_result.files_scanned,
            files_added: scan_result.files_added,
            files_updated: scan_result.files_updated,
            files_removed: scan_result.to_remove.len(),
            files_skipped: scan_result.files_skipped,
            symbols_total: output_snapshot.symbols_total,
            chunks_total: output_snapshot.chunks_total,
            parse_errors: parse_report.parse_errors,
            elapsed_ms: elapsed.as_millis() as u64,
            files_parsed: parse_report.files_to_parse,
            used_parallel_parse: parse_report.used_parallel,
        }
    }
}

struct DirtyClosure {
    actions: HashMap<String, FileAction>,
}

impl DirtyClosure {
    fn prepare(
        indexer: &Indexer,
        mode: IndexBuildMode,
        scan_result: &ScanDiffResult,
        write_units: &mut Vec<FileWriteUnit>,
    ) -> CcResult<Self> {
        let mut actions = indexer.build_actions_map(
            write_units,
            &scan_result.existing,
            &scan_result.scanned_paths,
        );

        // Full builds never promote skipped files; incremental builds may
        // close over importers whose dependency exports changed.
        let dirty_count = if mode.is_incremental() {
            indexer.run_dirty_propagation(&mut actions, write_units)?
        } else {
            0
        };

        // Dirty reload must happen before enrichment/resolution and after the
        // dirty closure is complete, otherwise the catalog would miss
        // re-resolve-only units.
        indexer.phase_dirty_reload(write_units, &actions, &scan_result.existing, dirty_count)?;

        Ok(Self { actions })
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
        let prepared = plan_b.prepare(&indexer_b, dir_b.path()).expect("prepare build");
        let report_b = plan_b
            .commit(&indexer_b, dir_b.path(), prepared)
            .expect("commit build");

        // Key report fields must be identical across both paths.
        assert_eq!(report_a.files_added, report_b.files_added, "files_added");
        assert_eq!(report_a.files_updated, report_b.files_updated, "files_updated");
        assert_eq!(report_a.files_parsed, report_b.files_parsed, "files_parsed");
        assert_eq!(report_a.symbols_total, report_b.symbols_total, "symbols_total");
        assert_eq!(report_a.chunks_total, report_b.chunks_total, "chunks_total");
        assert!(report_a.symbols_total > 0, "fixture should yield symbols");

        // Persisted DB state must match: symbol, file, and chunk counts.
        let stats_a = db_a.stats(dir_a.path()).expect("stats a");
        let stats_b = db_b.stats(dir_b.path()).expect("stats b");
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
    }
}
