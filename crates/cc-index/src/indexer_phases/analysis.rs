use std::path::Path;

use cc_db::index_db::FileWriteUnit;
use cc_model::edge::{CoChangeEdgeRecord, RouteNodeRecord};
use cc_model::infra::{InfraEdge, InfraNode};
use cc_model::{BuildExplainCollector, CcResult};

use crate::indexer::Indexer;
use crate::pass_gate::{
    log_gate_decision, DeferredSignatureRecord, FileSignatureGate, PassGate, StringCacheGate,
};

use super::{time_step, ADR_SIG_ALGORITHM, INFRA_SIG_ALGORITHM};

/// Metadata key for the git co-change HEAD-skip gate. Shared by the gate
/// construction (compute) and the deferred record (apply).
const COCHANGE_HEAD_KEY: &str = "last_cochange_head";

/// Conventional ADR directories scanned by the ADR pass (direct children
/// only, `.md` files).
const ADR_DIRS: [&str; 4] = [
    "docs/adr",
    "docs/decisions",
    "doc/architecture/decisions",
    "doc/adr",
];

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

struct AdrStage {
    /// May be empty: the apply still writes (clearing stale docs after the
    /// last ADR file is removed).
    docs: Vec<serde_json::Value>,
    record: DeferredSignatureRecord,
}

/// Phase 8-11 deltas: git co-change, infrastructure, ADR documents.
pub(crate) struct AnalysisPlan {
    cochange: Option<CoChangeStage>,
    infra: Option<InfraStage>,
    /// ADR rescan behind a file-set signature gate; `None` when unchanged.
    adr: Option<AdrStage>,
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
        walk_manifest: Option<&crate::scanner::WalkManifest>,
        build_explain: &mut BuildExplainCollector,
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
        build_explain.record_gate(
            cochange_gate.id(),
            cochange_decision.run,
            cochange_decision.reason,
        );
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
                    build_explain.record_degraded("cochange_unavailable");
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
                time_step("analysis", "infra_signature", || match walk_manifest {
                    // Shared-walk manifest: candidate set + stats without
                    // another tree walk (value-equal to the walk fallback).
                    Some(manifest) => {
                        crate::infra_pass::infra_signature_from_manifest(project_path, manifest)
                    }
                    None => crate::infra_pass::infra_signature(project_path),
                })
            },
        );
        let infra_decision = infra_gate.should_run()?;
        log_gate_decision(&infra_gate, infra_decision);
        build_explain.record_gate(infra_gate.id(), infra_decision.run, infra_decision.reason);
        let infra = if infra_decision.run {
            let (mut infra_nodes, mut infra_edges) =
                time_step("analysis", "infra_scan", || match walk_manifest {
                    Some(manifest) => {
                        crate::infra_pass::run_infra_pass_with_manifest(project_path, manifest)
                    }
                    None => crate::infra_pass::run_infra_pass(project_path),
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

        // Phase 10: Architecture Decision Records (ADR) indexing, behind a
        // file-set signature gate (paths + mtime + size over the conventional
        // ADR directories) so unchanged ADR trees skip the per-file reads.
        let adr_gate = FileSignatureGate::new(
            "adr",
            &self.db,
            "last_adr_sig",
            "last_adr_sig_algo",
            ADR_SIG_ALGORITHM,
            || adr_files_signature(project_path),
        );
        let adr_decision = adr_gate.should_run()?;
        log_gate_decision(&adr_gate, adr_decision);
        build_explain.record_gate(adr_gate.id(), adr_decision.run, adr_decision.reason);
        let adr = if adr_decision.run {
            let docs = time_step("analysis", "adr_scan", || {
                Self::collect_adr_docs(project_path)
            });
            Some(AdrStage {
                docs,
                record: adr_gate.deferred_record(),
            })
        } else {
            None
        };

        Ok(AnalysisPlan {
            cochange,
            infra,
            adr,
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

        if let Some(stage) = &plan.adr {
            // Always write when the pass ran — an empty list must clear the
            // previously recorded docs (all ADR files removed), not leave
            // them dangling until the next full build.
            if !stage.docs.is_empty() {
                tracing::info!(count = stage.docs.len(), "indexed ADR documents");
            }
            self.db.writes().set_metadata(
                "adr_documents",
                &serde_json::to_string(&stage.docs).unwrap_or_default(),
            )?;
            stage.record.record(&self.db)?;
        }
        Ok(())
    }

    /// Scan the conventional ADR directories and extract MADR-format headers.
    /// Pure filesystem read.
    fn collect_adr_docs(project_path: &Path) -> Vec<serde_json::Value> {
        let mut adr_docs = Vec::new();

        for dir in &ADR_DIRS {
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

/// Stat-signature over the ADR input set: sorted `.md` direct children of the
/// conventional ADR directories, hashed with mtime + size (the shared
/// change-detection contract). Cheap — four `read_dir` calls, no tree walk.
fn adr_files_signature(project_path: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut candidates: Vec<(String, u64, u64)> = Vec::new();
    for dir in &ADR_DIRS {
        let adr_path = project_path.join(dir);
        let Ok(entries) = std::fs::read_dir(&adr_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e == "md") {
                continue;
            }
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let rel = path
                .strip_prefix(project_path)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            candidates.push((rel, metadata.len(), mtime));
        }
    }
    candidates.sort();

    let mut hasher = DefaultHasher::new();
    candidates.len().hash(&mut hasher);
    for (rel, size, mtime) in &candidates {
        rel.hash(&mut hasher);
        size.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod adr_gate_tests {
    use crate::indexer::Indexer;
    use cc_db::index_db::IndexDb;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;

    const ADR_DOC: &str = "# Use SQLite\n\nStatus: accepted\nDate: 2024-01-01\n";

    fn setup() -> (tempfile::TempDir, Arc<IndexDb>, Indexer) {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join("docs/adr")).unwrap();
        std::fs::write(project.join("docs/adr/0001-use-sqlite.md"), ADR_DOC).unwrap();
        std::fs::write(project.join("lib.py"), "def handler():\n    return 1\n").unwrap();
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    fn adr_metadata(db: &IndexDb) -> Option<String> {
        db.reads().get_metadata("adr_documents").unwrap()
    }

    /// First build scans and records; an unchanged ADR tree skips the rescan
    /// (signature stays put); removing the last ADR doc clears the recorded
    /// docs instead of leaving them dangling.
    #[test]
    fn adr_gate_records_skips_and_clears() {
        let (tmp, db, indexer) = setup();
        let project = tmp.path();

        indexer.build_index(project, false).unwrap();
        let docs = adr_metadata(&db).expect("ADR docs recorded on first build");
        assert!(docs.contains("Use SQLite"), "recorded ADR title");
        let sig = db
            .reads()
            .get_metadata("last_adr_sig")
            .unwrap()
            .expect("ADR signature recorded");

        // Unchanged ADR tree + a source edit: gate must skip (signature
        // unchanged) and the recorded docs must survive.
        std::fs::write(
            project.join("lib.py"),
            "def handler():\n    return 1\n\n\ndef extra():\n    return 2\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            db.reads().get_metadata("last_adr_sig").unwrap().as_deref(),
            Some(sig.as_str()),
            "unchanged ADR tree keeps the recorded signature"
        );
        assert_eq!(
            adr_metadata(&db).as_deref(),
            Some(docs.as_str()),
            "skipped pass must not disturb the recorded docs"
        );

        // Removing the last ADR doc must clear the recorded list.
        std::fs::remove_file(project.join("docs/adr/0001-use-sqlite.md")).unwrap();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            adr_metadata(&db).as_deref(),
            Some("[]"),
            "removal must clear the recorded ADR docs"
        );
    }
}
