//! Index build plan orchestration.
//!
//! This module owns the ordering invariants around a build:
//! scan/diff → parse → dirty closure → enrichment → resolution →
//! route-node/report snapshot → write → postprocess → analysis.
//! Phase implementations stay on `Indexer`; this module keeps the plumbing
//! concentrated so full and incremental builds cannot drift apart.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use cc_db::index_db::FileWriteUnit;
use cc_model::edge::RouteNodeRecord;
use cc_model::CcResult;

use crate::indexer::{FileAction, IndexReport, Indexer, ParseResult, ScanDiffResult, WriteResult};

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
        let start = std::time::Instant::now();

        let mut scan_result =
            indexer.phase_scan_and_diff(project_path, self.mode.is_full(), self.auto_file_limit)?;
        let to_parse = std::mem::take(&mut scan_result.to_parse);
        let parse_result = indexer.phase_parse(project_path, to_parse)?;
        let ParsedBuildState {
            mut write_units,
            source_cache,
            parse_report,
        } = ParsedBuildState::from(parse_result);

        let dirty_closure =
            DirtyClosure::prepare(indexer, self.mode, &scan_result, &mut write_units)?;

        // Framework enrichment must run after dirty reload so dirty units
        // participate in project context, and before resolution so resolvers
        // can bind framework-specific edges.
        let fw_context =
            indexer.phase_framework_enrichment(project_path, &mut write_units, &source_cache)?;

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

        let write_result = indexer.phase_write(
            project_path,
            self.mode.is_full(),
            write_units,
            dirty_closure.actions(),
            &scan_result.to_remove,
            &output_snapshot.route_nodes,
            &resolve_result.hierarchy_edges,
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

    fn actions(&self) -> &HashMap<String, FileAction> {
        &self.actions
    }
}

struct ParsedBuildState {
    write_units: Vec<FileWriteUnit>,
    source_cache: HashMap<String, String>,
    parse_report: ParseReport,
}

impl From<ParseResult> for ParsedBuildState {
    fn from(parse_result: ParseResult) -> Self {
        Self {
            write_units: parse_result.write_units,
            source_cache: parse_result.source_cache,
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
