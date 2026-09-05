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

/// Immutable inputs to analysis, separated from its mutable explanation sink.
/// Scanning provenance travels together; no positional unused parsed-path input.
pub(crate) struct AnalysisInputs<'a> {
    pub project_path: &'a Path,
    pub full: bool,
    pub write_units: &'a [FileWriteUnit],
    pub route_nodes: &'a [RouteNodeRecord],
    pub walk_manifest: Option<&'a crate::scanner::WalkManifest>,
    pub scope_hints: Option<&'a crate::indexer::ScopeSignatureHints>,
}

impl Indexer {
    /// Phase 8-11 (compute half): git co-change, infrastructure, and ADR
    /// indexing. Reads git, the filesystem, and the read pool only — no index
    /// writes, so callers may run it without holding any index lock.
    pub(crate) fn phase_analysis_compute(
        &self,
        inputs: AnalysisInputs<'_>,
        build_explain: &mut BuildExplainCollector,
    ) -> CcResult<AnalysisPlan> {
        let AnalysisInputs {
            project_path,
            full,
            write_units,
            route_nodes,
            walk_manifest,
            scope_hints,
        } = inputs;
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
        // Event-scoped fast path: a walk-free build whose event set provably
        // contains no infra candidate cannot have changed the infra
        // signature — skip the fallback walk when a comparable record
        // exists (first build / algo upgrades still run).
        let scoped_infra_unaffected =
            walk_manifest.is_none() && scope_hints.is_some_and(|h| h.infra_files_unaffected);
        let infra_decision = if scoped_infra_unaffected {
            infra_gate.should_run_assuming_unchanged("scoped: no infra candidate events")?
        } else {
            infra_gate.should_run()?
        };
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
                let bind_symbols: Vec<_> = if full {
                    self.db.retrieval().symbol_records_for_infra_binding()?
                } else {
                    write_units
                        .iter()
                        .flat_map(|u| u.outcome.symbols.iter().cloned())
                        .collect()
                };
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
            if path.extension().is_none_or(|e| e != "md") {
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

#[cfg(test)]
mod analysis_phase_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Fixture project on disk plus an `IndexDb` in a separate tempdir, so
    /// the analysis filesystem walks (infra, ADR) never see DB/WAL files.
    fn setup(files: &[(&str, &str)]) -> (TempDir, TempDir, Arc<IndexDb>, Indexer) {
        let project = TempDir::new().unwrap();
        for (rel, content) in files {
            let path = project.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let db_dir = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&db_dir.path().join("analysis.db")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project.path(), &IndexingConfig::default());
        (project, db_dir, db, indexer)
    }

    /// Incremental compute with an empty batch — the shape a no-change
    /// build hands to phase 8-11.
    fn compute(
        indexer: &Indexer,
        project: &Path,
        build_explain: &mut BuildExplainCollector,
    ) -> AnalysisPlan {
        indexer
            .phase_analysis_compute(
                AnalysisInputs {
                    project_path: project,
                    full: false,
                    write_units: &[],
                    route_nodes: &[],
                    walk_manifest: None,
                    scope_hints: None,
                },
                build_explain,
            )
            .unwrap()
    }

    fn count(db: &IndexDb, sql: &str) -> i64 {
        db.reads()
            .query_json(sql, &[])
            .unwrap()
            .first()
            .and_then(|row| row.get("cnt"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }

    fn gate<'a>(
        explain: &'a cc_model::BuildExplain,
        pass: &str,
    ) -> &'a cc_model::GateDecisionRecord {
        explain
            .gate_decisions
            .iter()
            .find(|g| g.pass == pass)
            .unwrap_or_else(|| panic!("gate decision for '{pass}' must be recorded"))
    }

    fn git(project: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(project)
            .args(args)
            .output()
            .expect("git must be runnable in the test environment");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_commit(project: &Path, message: &str) {
        git(
            project,
            &[
                "-c",
                "user.name=cc-test",
                "-c",
                "user.email=cc@test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                message,
            ],
        );
    }

    /// (a) A project without a git repository must not fail the analysis
    /// phase: the HEAD gate reads as unavailable (so the pass runs), the
    /// scan yields no edges, and — crucially — nothing is recorded, so the
    /// next build retries instead of permanently skipping. The gate
    /// decision lands in build_explain with the unavailable reason.
    #[test]
    fn missing_git_repo_skips_cochange_gracefully_without_error() {
        let (project, _db_dir, db, indexer) = setup(&[("src/a.py", "def a():\n    return 1\n")]);
        let mut build_explain = BuildExplainCollector::new();

        let plan = compute(&indexer, project.path(), &mut build_explain);

        let stage = plan
            .cochange
            .as_ref()
            .expect("unavailable HEAD must run the pass, never skip it");
        assert!(
            stage.co_changes.is_empty(),
            "no git history can produce no co-change edges"
        );
        assert_eq!(
            stage.record_head, None,
            "no HEAD marker may be recorded when git is unavailable"
        );

        let explain = build_explain.finish();
        let decision = gate(&explain, "git_cochange");
        assert!(
            decision.run,
            "the pass runs when the cache key is unavailable"
        );
        assert_eq!(decision.reason, "cache key unavailable");

        indexer.phase_analysis_apply(&plan).unwrap();
        assert_eq!(
            db.reads().get_metadata(COCHANGE_HEAD_KEY).unwrap(),
            None,
            "apply must not poison the skip cache with a marker"
        );
        assert_eq!(
            count(&db, "SELECT COUNT(*) AS cnt FROM co_change_edges"),
            0,
            "no co-change rows may be written"
        );
    }

    /// (b) The infra pass must produce persisted `infra_nodes`/`infra_edges`
    /// rows for every supported infra file type — Dockerfile, docker-compose,
    /// K8s manifest, terraform — and record its file-set signature after the
    /// apply so the next unchanged build can skip.
    #[test]
    fn infra_pass_persists_nodes_and_edges_for_all_infra_file_types() {
        let (project, _db_dir, db, indexer) = setup(&[
            ("Dockerfile", "FROM python:3.11\nEXPOSE 8080\n"),
            (
                "docker-compose.yml",
                "services:\n  api:\n    image: redis:7\n",
            ),
            (
                "deploy/app.yaml",
                "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web-api\nspec:\n  replicas: 1\n",
            ),
            (
                "main.tf",
                "resource \"aws_s3_bucket\" \"assets\" {\n  bucket = \"cc-assets\"\n}\n",
            ),
        ]);
        let mut build_explain = BuildExplainCollector::new();

        let plan = compute(&indexer, project.path(), &mut build_explain);
        assert!(
            plan.infra.is_some(),
            "no recorded signature: the infra pass must run"
        );
        indexer.phase_analysis_apply(&plan).unwrap();

        let nodes: Vec<(String, String, String)> = db
            .reads()
            .query_json(
                "SELECT file_path, kind, name FROM infra_nodes ORDER BY file_path, kind, name",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["file_path"].as_str().unwrap().to_string(),
                    row["kind"].as_str().unwrap().to_string(),
                    row["name"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        for expected in [
            ("Dockerfile", "docker_image", "python:3.11"),
            ("Dockerfile", "docker_stage", "stage-0"),
            ("Dockerfile", "docker_expose", "8080"),
            ("docker-compose.yml", "compose_service", "api"),
            ("docker-compose.yml", "docker_image", "redis:7"),
            ("deploy/app.yaml", "k8s_deployment", "web-api"),
            ("main.tf", "terraform_resource", "aws_s3_bucket.assets"),
        ] {
            assert!(
                nodes
                    .iter()
                    .any(|(f, k, n)| { (f.as_str(), k.as_str(), n.as_str()) == expected }),
                "expected infra node {expected:?} in {nodes:?}"
            );
        }

        let edge_kinds: Vec<String> = db
            .reads()
            .query_json("SELECT DISTINCT kind FROM infra_edges ORDER BY kind", &[])
            .unwrap()
            .iter()
            .map(|row| row["kind"].as_str().unwrap().to_string())
            .collect();
        for kind in ["depends_on", "exposes_port", "uses_image"] {
            assert!(
                edge_kinds.iter().any(|k| k == kind),
                "expected infra edge kind '{kind}' in {edge_kinds:?}"
            );
        }

        assert!(
            db.reads().get_metadata("last_infra_sig").unwrap().is_some(),
            "apply must record the infra signature"
        );
        assert_eq!(
            db.reads()
                .get_metadata("last_infra_sig_algo")
                .unwrap()
                .as_deref(),
            Some(INFRA_SIG_ALGORITHM),
            "apply must record the signature algorithm version"
        );
    }

    /// (c) MADR-style documents under docs/adr are indexed into the
    /// `adr_documents` metadata blob with file/title/status/date extracted;
    /// an .md file without a `# ` title line is excluded.
    #[test]
    fn adr_docs_with_madr_headers_are_indexed_into_metadata() {
        let (project, _db_dir, db, indexer) = setup(&[
            (
                "docs/adr/0001-use-sqlite.md",
                "# Use SQLite for the index store\n\nStatus: accepted\nDate: 2024-03-01\n\n## Context\n...\n",
            ),
            (
                "docs/adr/0002-no-title.md",
                "just some notes without a heading\n\nStatus: draft\n",
            ),
        ]);
        let mut build_explain = BuildExplainCollector::new();

        let plan = compute(&indexer, project.path(), &mut build_explain);
        let adr = plan
            .adr
            .as_ref()
            .expect("first ADR scan must run (file-set signature changed)");
        assert_eq!(
            adr.docs.len(),
            1,
            "only documents with a title heading are indexed"
        );
        indexer.phase_analysis_apply(&plan).unwrap();

        let raw = db
            .reads()
            .get_metadata("adr_documents")
            .unwrap()
            .expect("apply must persist the ADR metadata blob");
        let docs: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(
            docs[0]["file"].as_str(),
            Some("docs/adr/0001-use-sqlite.md")
        );
        assert_eq!(
            docs[0]["title"].as_str(),
            Some("Use SQLite for the index store")
        );
        assert_eq!(docs[0]["status"].as_str(), Some("accepted"));
        assert_eq!(docs[0]["date"].as_str(), Some("2024-03-01"));

        // Deleting the last ADR doc must un-index it: the next apply with an
        // empty scan clears the stored metadata instead of serving stale ADRs.
        std::fs::remove_file(project.path().join("docs/adr/0001-use-sqlite.md")).unwrap();
        let mut build_explain = BuildExplainCollector::new();
        let plan = compute(&indexer, project.path(), &mut build_explain);
        assert!(plan.adr.as_ref().unwrap().docs.is_empty());
        indexer.phase_analysis_apply(&plan).unwrap();
        assert_eq!(
            db.reads().get_metadata("adr_documents").unwrap().as_deref(),
            Some("[]"),
            "empty ADR scan must clear previously stored metadata"
        );
    }

    /// (d) Gate behavior across two same-content builds: the first build
    /// runs git co-change (two commits touching a.py+b.py produce a
    /// persisted edge) and the infra pass, and records both markers on
    /// apply; the second build's compute must skip all three gated passes
    /// (HEAD unchanged, infra signature unchanged, ADR file set unchanged).
    /// Applying the skip plan leaves the persisted artifacts intact.
    #[test]
    fn unchanged_content_second_build_skips_cochange_and_infra_gates() {
        let (project, _db_dir, db, indexer) = setup(&[
            ("a.py", "def a():\n    return 1\n"),
            ("b.py", "def b():\n    return 1\n"),
            ("Dockerfile", "FROM python:3.11\nEXPOSE 8080\n"),
            (
                "docs/adr/0001-use-sqlite.md",
                "# Use SQLite\n\nStatus: accepted\nDate: 2024-03-01\n",
            ),
        ]);
        let root = project.path();

        // Two commits both touching a.py and b.py: pair count 2 with
        // confidence 1.0 clears the analyzer thresholds (2, 0.2).
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git_commit(root, "c1");
        std::fs::write(root.join("a.py"), "def a():\n    return 2\n").unwrap();
        std::fs::write(root.join("b.py"), "def b():\n    return 2\n").unwrap();
        git(root, &["add", "."]);
        git_commit(root, "c2");
        let head = crate::git_cochange::current_git_head(root)
            .expect("fixture repo must expose a HEAD sha");

        // Build 1: both gates run, artifacts and markers land on apply.
        let mut explain1 = BuildExplainCollector::new();
        let plan1 = compute(&indexer, root, &mut explain1);
        let stage = plan1.cochange.as_ref().expect("first build runs co-change");
        assert!(
            stage
                .co_changes
                .iter()
                .any(|e| e.file_a == "a.py" && e.file_b == "b.py" && e.co_change_count == 2),
            "two shared commits must yield the a.py/b.py edge; got {:?}",
            stage.co_changes
        );
        assert_eq!(stage.record_head.as_deref(), Some(head.as_str()));
        assert!(plan1.infra.is_some(), "first build runs the infra pass");
        assert!(plan1.adr.is_some(), "ADR docs are collected");
        indexer.phase_analysis_apply(&plan1).unwrap();

        assert_eq!(
            count(&db, "SELECT COUNT(*) AS cnt FROM co_change_edges"),
            1,
            "exactly the a.py/b.py pair clears the thresholds"
        );
        assert_eq!(
            db.reads()
                .get_metadata(COCHANGE_HEAD_KEY)
                .unwrap()
                .as_deref(),
            Some(head.as_str()),
            "apply must record the analyzed HEAD"
        );
        let infra_nodes_after_first = count(&db, "SELECT COUNT(*) AS cnt FROM infra_nodes");
        assert!(
            infra_nodes_after_first > 0,
            "Dockerfile produces infra nodes"
        );

        // Build 2 (same content): both gated passes must skip; ADR rescans.
        let mut explain2 = BuildExplainCollector::new();
        let plan2 = compute(&indexer, root, &mut explain2);
        assert!(
            plan2.cochange.is_none(),
            "unchanged HEAD must skip the co-change pass"
        );
        assert!(
            plan2.infra.is_none(),
            "unchanged infra file set must skip the infra pass"
        );
        assert!(
            plan2.adr.is_none(),
            "unchanged ADR file set must skip the rescan gate"
        );

        let explain2 = explain2.finish();
        let cochange_decision = gate(&explain2, "git_cochange");
        assert!(!cochange_decision.run);
        assert_eq!(cochange_decision.reason, "cache key unchanged");
        let infra_decision = gate(&explain2, "infra");
        assert!(!infra_decision.run);
        assert_eq!(infra_decision.reason, "signature unchanged");

        // Applying the skip plan is a no-op for the persisted artifacts.
        indexer.phase_analysis_apply(&plan2).unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) AS cnt FROM co_change_edges"),
            1,
            "skipped pass must leave the co-change rows intact"
        );
        assert_eq!(
            count(&db, "SELECT COUNT(*) AS cnt FROM infra_nodes"),
            infra_nodes_after_first,
            "skipped pass must leave the infra rows intact"
        );
    }
}
