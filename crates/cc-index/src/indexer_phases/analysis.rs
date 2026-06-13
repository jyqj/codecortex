use std::path::Path;

use cc_db::index_db::FileWriteUnit;
use cc_model::edge::{CoChangeEdgeRecord, RouteNodeRecord};
use cc_model::infra::{InfraEdge, InfraNode};
use cc_model::CcResult;

use crate::indexer::Indexer;
use crate::pass_gate::{
    log_gate_decision, DeferredSignatureRecord, FileSignatureGate, PassGate, StringCacheGate,
};

use super::{time_step, INFRA_SIG_ALGORITHM};

/// Metadata key for the git co-change HEAD-skip gate. Shared by the gate
/// construction (compute) and the deferred record (apply).
const COCHANGE_HEAD_KEY: &str = "last_cochange_head";

struct CoChangeStage {
    co_changes: Vec<CoChangeEdgeRecord>,
    /// HEAD sha to record after the apply; `None` when git was unavailable or
    /// the analysis degraded — nothing is recorded so the next build retries.
    record_head: Option<String>,
}

struct InfraStage {
    nodes: Vec<InfraNode>,
    edges: Vec<InfraEdge>,
    record: DeferredSignatureRecord,
}

/// Phase 8-11 deltas: git co-change, infrastructure, ADR documents.
pub(crate) struct AnalysisPlan {
    cochange: Option<CoChangeStage>,
    infra: Option<InfraStage>,
    /// ADR docs are re-scanned unconditionally; an empty list writes nothing.
    adr_docs: Vec<serde_json::Value>,
}

impl Indexer {
    /// Phase 8-11 (compute half): git co-change, infrastructure, and ADR
    /// indexing. Reads git, the filesystem, and the read pool only — no index
    /// writes, so callers may run it without holding any index lock.
    pub(crate) fn phase_analysis_compute(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
    ) -> CcResult<AnalysisPlan> {
        // Phase 8: Git co-change analysis. HEAD-skip: co-change edges only
        // depend on commit history. If HEAD has not advanced since the last
        // successful analysis, the result is unchanged (the `--since=1.year`
        // window drifts but produces equivalent output while HEAD is fixed),
        // so the git log + parse + write can be skipped.
        let cochange_gate =
            StringCacheGate::new("git_cochange", &self.db, COCHANGE_HEAD_KEY, || {
                crate::git_cochange::current_git_head(project_path)
            });
        let cochange_decision = cochange_gate.should_run()?;
        log_gate_decision(&cochange_gate, cochange_decision);
        let cochange = if cochange_decision.run {
            match time_step("analysis", "cochange_scan", || {
                crate::git_cochange::analyze_cochanges(project_path, 2, 0.2, 500)
            }) {
                Ok(co_changes) => Some(CoChangeStage {
                    co_changes,
                    record_head: cochange_gate.record_key(),
                }),
                Err(err) => {
                    // Non-fatal: git may not be available or the project may
                    // not be a git repo. The HEAD marker stays unrecorded so a
                    // transient failure never poisons the skip cache.
                    tracing::warn!(error = %err, "skipping git co-change analysis");
                    Some(CoChangeStage {
                        co_changes: Vec::new(),
                        record_head: None,
                    })
                }
            }
        } else {
            None
        };

        // Phase 9: Infrastructure pass.
        //
        // The infra pass scans the whole project (Dockerfiles, compose, K8s,
        // terraform, compile_commands) independently of the language parser
        // pipeline — so infra files generally never appear in `write_units` and
        // their changes cannot be inferred from it. To stay strictly correct
        // *and* skip when unchanged, gate the pass on a signature over the infra
        // candidate set (paths + mtime + size); see `infra_pass::infra_signature`.
        let infra_gate = FileSignatureGate::new(
            "infra",
            &self.db,
            "last_infra_sig",
            "last_infra_sig_algo",
            INFRA_SIG_ALGORITHM,
            || {
                time_step("analysis", "infra_signature", || {
                    crate::infra_pass::infra_signature(project_path)
                })
            },
        );
        let infra_decision = infra_gate.should_run()?;
        log_gate_decision(&infra_gate, infra_decision);
        let infra = if infra_decision.run {
            let (mut infra_nodes, mut infra_edges) = time_step("analysis", "infra_scan", || {
                crate::infra_pass::run_infra_pass(project_path)
            });
            if !infra_nodes.is_empty() || !infra_edges.is_empty() {
                // Bind infra nodes to code symbols before persisting
                let bind_symbols: Vec<_> = write_units
                    .iter()
                    .flat_map(|u| u.outcome.symbols.iter().cloned())
                    .collect();
                crate::infra_pass::bind_infra_to_symbols(&mut infra_nodes, &bind_symbols);

                // Match binding target URLs to known route nodes
                crate::infra_pass::match_bindings_to_routes(&mut infra_edges, route_nodes);
            }
            Some(InfraStage {
                nodes: infra_nodes,
                edges: infra_edges,
                record: infra_gate.deferred_record(),
            })
        } else {
            None
        };

        // Phase 10: Architecture Decision Records (ADR) indexing — no skip
        // condition, rescanned every build.
        let adr_docs = time_step("analysis", "adr_scan", || {
            Self::collect_adr_docs(project_path)
        });

        Ok(AnalysisPlan {
            cochange,
            infra,
            adr_docs,
        })
    }

    /// Phase 8-11 (apply half): short DB transactions only. Per-pass record
    /// ordering matches the historical immediate-record loop: each pass's
    /// marker is persisted right after its own write, so a later pass failure
    /// never unrecords an earlier completed pass.
    pub(crate) fn phase_analysis_apply(&self, plan: &AnalysisPlan) -> CcResult<()> {
        if let Some(stage) = &plan.cochange {
            if !stage.co_changes.is_empty() {
                self.db
                    .writes()
                    .insert_co_change_edges_batch(&stage.co_changes)?;
                tracing::info!(
                    count = stage.co_changes.len(),
                    "indexed git co-change edges"
                );
            }
            if let Some(head) = &stage.record_head {
                self.db.writes().set_metadata(COCHANGE_HEAD_KEY, head)?;
            }
        }

        if let Some(stage) = &plan.infra {
            if !stage.nodes.is_empty() || !stage.edges.is_empty() {
                self.db
                    .writes()
                    .replace_infra_data(&stage.nodes, &stage.edges)?;
                let bound_count = stage
                    .nodes
                    .iter()
                    .filter(|n| n.bound_symbol_uid.is_some())
                    .count();
                let binding_count = stage
                    .edges
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.kind,
                            cc_model::infra::InfraEdgeKind::BindsTopic
                                | cc_model::infra::InfraEdgeKind::ConsumesQueue
                        )
                    })
                    .count();
                tracing::info!(
                    nodes = stage.nodes.len(),
                    edges = stage.edges.len(),
                    bound = bound_count,
                    bindings = binding_count,
                    "indexed infra graph"
                );
            }
            stage.record.record(&self.db)?;
        }

        if !plan.adr_docs.is_empty() {
            tracing::info!(count = plan.adr_docs.len(), "indexed ADR documents");
            self.db.writes().set_metadata(
                "adr_documents",
                &serde_json::to_string(&plan.adr_docs).unwrap_or_default(),
            )?;
        }
        Ok(())
    }

    /// Scan the conventional ADR directories and extract MADR-format headers.
    /// Pure filesystem read.
    fn collect_adr_docs(project_path: &Path) -> Vec<serde_json::Value> {
        let adr_dirs = [
            "docs/adr",
            "docs/decisions",
            "doc/architecture/decisions",
            "doc/adr",
        ];
        let mut adr_docs = Vec::new();

        for dir in &adr_dirs {
            let adr_path = project_path.join(dir);
            if adr_path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&adr_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_some_and(|e| e == "md") {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                // Extract MADR-format header
                                let mut title = None;
                                let mut status = None;
                                let mut date = None;
                                for line in content.lines().take(20) {
                                    if title.is_none() && line.starts_with("# ") {
                                        title = Some(line.trim_start_matches("# ").to_string());
                                    }
                                    if line.to_lowercase().starts_with("status:") {
                                        status = Some(
                                            line.split(':').nth(1).unwrap_or("").trim().to_string(),
                                        );
                                    }
                                    if line.to_lowercase().starts_with("date:") {
                                        date = Some(
                                            line.split(':').nth(1).unwrap_or("").trim().to_string(),
                                        );
                                    }
                                }
                                if let Some(t) = title {
                                    let rel = path
                                        .strip_prefix(project_path)
                                        .unwrap_or(&path)
                                        .to_string_lossy()
                                        .to_string();
                                    adr_docs.push(serde_json::json!({
                                        "file": rel,
                                        "title": t,
                                        "status": status,
                                        "date": date,
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
        adr_docs
    }
}
