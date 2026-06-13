use std::collections::HashMap;

use cc_db::index_db::{FileState, FileWriteUnit, IndexDb};
use cc_model::CcResult;

use crate::community::{build_community_labels, louvain_communities};
use crate::indexer::Indexer;
use crate::pass_gate::{
    log_gate_decision, DbSignatureGate, DeferredSignatureRecord, PairGate, PassGate,
};
use crate::synthesis_pipeline::SynthesisRound;

use super::{time_step, COMMUNITY_SIG_ALGORITHM, DISPATCH_SIG_ALGORITHM, INTERFACE_SIG_ALGORITHM};

/// Per-build memo of the graph-signature aggregates shared by the dispatch,
/// interface, and community gates, so a build reads (or, for fixture
/// databases without a stored baseline, recomputes) them once instead of
/// once per signature. Builds are single-threaded through the postprocess
/// phase, hence plain interior mutability.
#[derive(Default)]
struct GraphAggCache {
    aggs: std::cell::Cell<Option<cc_db::GraphSignatureAggregates>>,
}

impl GraphAggCache {
    fn get(&self, db: &IndexDb) -> CcResult<cc_db::GraphSignatureAggregates> {
        if let Some(aggs) = self.aggs.get() {
            return Ok(aggs);
        }
        let aggs = match db.reads().stored_graph_signature_aggregates()? {
            // Maintained baseline: O(1) metadata read — the write paths keep
            // it in sync with the committed tables (see cc_db::signature_agg).
            Some(aggs) => aggs,
            // No baseline (database never written through the maintaining
            // writers, e.g. raw-SQL fixtures): recompute from the tables.
            // Value-identical to the maintained baseline by construction.
            None => db.reads().scan_graph_signature_aggregates()?,
        };
        self.aggs.set(Some(aggs));
        Ok(aggs)
    }
}

// ── Staged postprocess/analysis plans (compute → apply seam) ────────────
//
// Phase 7 (postprocess) and Phase 8-11 (analysis) are split into a COMPUTE
// half (pure reads through the read pool, heavy work: signature scans,
// synthesis passes, Louvain, git log, infra walk) and an APPLY half (short
// DB transactions only). The plan structs below are the typed deltas that
// travel between the two halves; the caller decides how much locking each
// half needs (see `build_plan` for the staging contract).

/// 测试边重建指令：计算本身在 cc-db 的 SQL 里完成，compute 阶段只决定
/// apply 阶段执行哪一种重建。
enum TestEdgeRebuild {
    Skip,
    Full,
    Files(Vec<String>),
}

/// What the apply stage executes for the dispatch-synthesis round.
enum SynthesisAction {
    /// Normal round: one atomic batch write (see `synthesis_pipeline`).
    Round(SynthesisRound),
    /// Synthesis disabled after being enabled previously: delete every
    /// synthetic edge kind/prefix declared by the pass registry.
    DisableCleanup,
}

struct SynthesisStage {
    action: SynthesisAction,
    /// Dispatch + interface signatures, persisted only after the community
    /// apply completed (the historical `RecordTiming::Deferred` semantics: a
    /// later community failure leaves no synthesis signature recorded).
    records: Vec<DeferredSignatureRecord>,
}

enum CommunityAction {
    /// 边数超限的降级路径：未分配社区的符号全部归入 community 0。
    Degraded,
    Update {
        assignments: HashMap<String, u32>,
        labels: HashMap<u32, String>,
    },
}

struct CommunityStage {
    action: CommunityAction,
    record: DeferredSignatureRecord,
}

/// Phase 7 deltas: test edges, dispatch synthesis, community detection.
/// `None` stage fields mean the pass's gate decided to skip this build.
pub(crate) struct PostprocessPlan {
    test_edges: TestEdgeRebuild,
    synthesis: Option<SynthesisStage>,
    community: Option<CommunityStage>,
}

impl Indexer {
    /// Phase 7 (compute half): test edges, dispatch synthesis, community
    /// detection. Pure reads through the read pool — the heavy work
    /// (signature table scans, synthesis passes, Louvain) all happens here,
    /// so callers may run it without holding any index lock. The signature
    /// gates decide in this stage; their records travel inside the plan and
    /// are persisted by [`Self::phase_postprocess_apply`].
    pub(crate) fn phase_postprocess_compute(
        &self,
        full: bool,
        write_units: &[FileWriteUnit],
        config_units: &[FileWriteUnit],
        to_remove: &[String],
        pre_batch_files: &HashMap<String, FileState>,
    ) -> CcResult<PostprocessPlan> {
        // Test edges for changed files: the rebuild itself is a cc-db SQL
        // operation, so compute only decides WHICH rebuild apply runs.
        //
        // Update-only batches skip the rebuild outright: test edges are
        // path-derived (endpoints are file paths; matching depends only on
        // the path set plus the path-derived `is_test_file` flag), and the
        // write batch no longer cascades test_edges deletes for in-place
        // replacements — so when the batch removed nothing and every written
        // path already existed before the batch (`pre_batch_files` is the
        // scan-time files snapshot, covering dirty-closure and config units
        // too), the committed edges are already exactly the rebuilt ones.
        let mut changed_paths: Vec<String> =
            write_units.iter().map(|u| u.rel_path.clone()).collect();
        changed_paths.extend(config_units.iter().map(|u| u.rel_path.clone()));
        changed_paths.extend(to_remove.iter().cloned());
        let path_set_unchanged = to_remove.is_empty()
            && write_units
                .iter()
                .chain(config_units.iter())
                .all(|u| pre_batch_files.contains_key(&u.rel_path));
        let test_edges = if full {
            TestEdgeRebuild::Full
        } else if !changed_paths.is_empty() && !path_set_unchanged {
            TestEdgeRebuild::Files(changed_paths)
        } else {
            TestEdgeRebuild::Skip
        };

        // Per-pass signature gates: instead of a single graph_signature that
        // hashes all 4 tables, each pass group carries its own input
        // signature. This avoids re-running all 7 synthesis passes + Louvain
        // when only one input changed (e.g. a new dispatch site does not need
        // interface dispatch recomputation, and vice versa).
        //
        // All three signatures compose the write-time-maintained aggregates
        // (read once per build via `GraphAggCache`), so the gate decision is
        // O(1) metadata reads instead of full table scans. The synthesis
        // round's records are persisted only after the community apply
        // completed (deferred) — a mid-build failure never records a
        // signature for work that did not finish.
        let forced = if full { Some("full rebuild") } else { None };
        let agg_cache = GraphAggCache::default();

        let dispatch_gate = DbSignatureGate::new(
            "dispatch_synthesis",
            &self.db,
            "last_dispatch_sig",
            "last_dispatch_sig_algo",
            DISPATCH_SIG_ALGORITHM,
            forced,
            || {
                time_step("postprocess", "dispatch_signature", || {
                    self.dispatch_synthesis_signature_from(&agg_cache)
                })
            },
        );
        let interface_gate = DbSignatureGate::new(
            "interface_dispatch",
            &self.db,
            "last_interface_sig",
            "last_interface_sig_algo",
            INTERFACE_SIG_ALGORITHM,
            forced,
            || {
                time_step("postprocess", "interface_signature", || {
                    self.interface_dispatch_signature_from(&agg_cache)
                })
            },
        );
        // The two signatures gate one synthesis round: the round runs when
        // either input changed, and the individual decisions route work to
        // the dispatch- vs interface-gated sub-passes inside the round (see
        // `dispatch_synthesis::SynthesisPassSpec`).
        let synthesis_gate = PairGate::new("synthesis_round", &dispatch_gate, &interface_gate);
        let synthesis_decision = synthesis_gate.should_run()?;
        log_gate_decision(&synthesis_gate, synthesis_decision);

        // Phase 7b–7h: Dynamic dispatch synthesis. Compute every pass delta
        // against the committed snapshot; the apply stage writes all deltas
        // in one short atomic unit of work. See `crate::synthesis_pipeline`
        // for the cross-pass overlay and the concurrency notes.
        let synthesis = if synthesis_decision.run {
            let action = if self.dispatch_synthesis {
                let synthesis_config = crate::dispatch_synthesis::SynthesisConfig {
                    enabled: true,
                    event_fanout_cap: self.event_fanout_cap,
                    generic_event_denylist: if self.event_denylist.is_empty() {
                        crate::dispatch_synthesis::SynthesisConfig::default().generic_event_denylist
                    } else {
                        self.event_denylist.iter().cloned().collect()
                    },
                };
                let round = time_step("postprocess", "synthesis_round", || {
                    crate::synthesis_pipeline::compute_synthesis_round(
                        &self.db,
                        &synthesis_config,
                        synthesis_gate.first_changed(),
                        synthesis_gate.second_changed(),
                    )
                })?;
                SynthesisAction::Round(round)
            } else {
                // Synthesis disabled after a previous enabled run: the apply
                // stage removes stale synthetic edges (deletion set derived
                // from each pass's declared owned kinds/prefixes).
                SynthesisAction::DisableCleanup
            };
            Some(SynthesisStage {
                action,
                records: vec![
                    dispatch_gate.deferred_record()?,
                    interface_gate.deferred_record()?,
                ],
            })
        } else {
            None
        };

        // Community detection conceptually runs AFTER synthesis: its inputs
        // include synthetic edges. The staged round has not been applied yet,
        // so the committed call graph is projected forward — the gate
        // signature projects in aggregate space (no edge load; see
        // `community_signature_projected`), and only a RUN decision loads the
        // actual edge list for Louvain (`community_edges_with_overlay`, the
        // same projection in edge space). When the round was skipped the
        // synthetic edges are unchanged and the committed state is already
        // the post-round state.
        let community_gate = DbSignatureGate::new(
            "community",
            &self.db,
            "last_community_sig",
            "last_community_sig_algo",
            COMMUNITY_SIG_ALGORITHM,
            forced,
            || {
                time_step("postprocess", "community_signature", || {
                    self.community_signature_projected(
                        synthesis.as_ref().map(|s| &s.action),
                        &agg_cache,
                    )
                })
            },
        );
        let community_decision = community_gate.should_run()?;
        log_gate_decision(&community_gate, community_decision);
        let community = if community_decision.run {
            let community_edges = time_step("postprocess", "community_edges", || {
                self.community_edges_with_overlay(synthesis.as_ref().map(|s| &s.action))
            })?;
            Some(CommunityStage {
                action: time_step("postprocess", "louvain", || {
                    self.compute_community_action(&community_edges)
                })?,
                record: community_gate.deferred_record()?,
            })
        } else {
            None
        };

        Ok(PostprocessPlan {
            test_edges,
            synthesis,
            community,
        })
    }

    /// Phase 7 (apply half): short DB transactions only — test-edge rebuild,
    /// synthesis round apply, community update, then the deferred signature
    /// records. Record ordering preserves the historical `RecordTiming`
    /// semantics: community records before the synthesis signatures, so a
    /// community failure leaves no synthesis signature recorded.
    pub(crate) fn phase_postprocess_apply(&self, plan: &PostprocessPlan) -> CcResult<()> {
        time_step("postprocess", "test_edges_apply", || {
            match &plan.test_edges {
                TestEdgeRebuild::Full => self.db.writes().rebuild_test_edges()?,
                TestEdgeRebuild::Files(paths) => {
                    self.db.writes().rebuild_test_edges_for_files(paths)?
                }
                TestEdgeRebuild::Skip => {}
            }
            CcResult::Ok(())
        })?;

        if let Some(stage) = &plan.synthesis {
            match &stage.action {
                // All deltas land in one short atomic unit of work; the apply
                // is all-or-nothing.
                SynthesisAction::Round(round) => {
                    time_step("postprocess", "synthesis_apply", || {
                        crate::synthesis_pipeline::apply_synthesis_round(&self.db, round)
                    })?;
                }
                SynthesisAction::DisableCleanup => {
                    // If synthesis was enabled in a previous run and is
                    // disabled now, proactively remove stale synthetic edges.
                    // The deletion set is derived from each pass's declared
                    // owned kinds/prefixes, so a new pass is covered here the
                    // moment its spec is registered.
                    let mut removed_edges = 0usize;
                    for spec in crate::dispatch_synthesis::registry() {
                        for kind in spec.owned_call_kinds {
                            removed_edges += self.db.writes().delete_synthetic_call_edges(kind)?;
                        }
                        for prefix in spec.owned_semantic_prefixes {
                            removed_edges +=
                                self.db.writes().delete_synthetic_semantic_edges(prefix)?;
                        }
                    }
                    if removed_edges > 0 {
                        tracing::info!(
                            removed_edges,
                            "dispatch synthesis disabled; removed stale synthetic edges"
                        );
                    }
                }
            }
        }

        if let Some(stage) = &plan.community {
            time_step("postprocess", "community_apply", || {
                match &stage.action {
                    CommunityAction::Degraded => {
                        self.db.writes().assign_all_symbols_to_community(0)?;
                    }
                    CommunityAction::Update {
                        assignments,
                        labels,
                    } => {
                        self.db.writes().update_communities(assignments, labels)?;
                    }
                }
                CcResult::Ok(())
            })?;
            stage.record.record(&self.db)?;
        }
        if let Some(stage) = &plan.synthesis {
            for record in &stage.records {
                record.record(&self.db)?;
            }
        }
        Ok(())
    }

    /// Louvain (or the OOM-degradation decision) over the projected
    /// post-apply edge set.
    fn compute_community_action(&self, edges: &[(String, String)]) -> CcResult<CommunityAction> {
        // Guard: cap the edge count before running Louvain to prevent OOM.
        let max_community_edges: usize = std::env::var("CODECORTEX_COMMUNITY_MAX_EDGES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2_000_000);

        if edges.len() > max_community_edges {
            tracing::warn!(
                edge_count = edges.len(),
                max_community_edges,
                "community detection: edge count exceeds limit, assigning all symbols to community 0"
            );
            return Ok(CommunityAction::Degraded);
        }

        let assignments = louvain_communities(edges, 20);
        let symbol_names = self.db.reads().symbol_names_by_uid()?;
        let labels = build_community_labels(&assignments, &symbol_names);
        Ok(CommunityAction::Update {
            assignments,
            labels,
        })
    }

    /// Project the committed call graph forward across a staged synthesis
    /// action: committed (caller_uid, callee_uid) pairs minus the synthetic
    /// kinds the action deletes, plus the round's in-memory inserts (both-UID
    /// edges only, mirroring the SQL `NOT NULL` filter). Once the action is
    /// applied, the DB edge set equals this projection — community detection
    /// can therefore compute against post-apply state before the apply runs.
    fn community_edges_with_overlay(
        &self,
        action: Option<&SynthesisAction>,
    ) -> CcResult<Vec<(String, String)>> {
        let deleted_kinds = Self::overlay_deleted_kinds(action);

        let mut edges = if deleted_kinds.is_empty() {
            self.db.reads().call_uid_edges()?
        } else {
            let placeholders = (1..=deleted_kinds.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
                 AND (synthesized_by IS NULL OR synthesized_by NOT IN ({placeholders}))"
            );
            let params: Vec<String> = deleted_kinds.iter().map(|kind| kind.to_string()).collect();
            self.db
                .reads()
                .query_json(&sql, &params)?
                .into_iter()
                .filter_map(|row| {
                    let caller = row.get("caller_symbol_uid")?.as_str()?.to_string();
                    let callee = row.get("callee_symbol_uid")?.as_str()?.to_string();
                    Some((caller, callee))
                })
                .collect()
        };

        if let Some(SynthesisAction::Round(round)) = action {
            for delta in &round.deltas {
                for edge in &delta.insert_call_edges {
                    if let (Some(caller), Some(callee)) =
                        (&edge.caller_symbol_uid, &edge.callee_symbol_uid)
                    {
                        edges.push((caller.clone(), callee.clone()));
                    }
                }
            }
        }
        Ok(edges)
    }

    /// Deterministic signatures over the *inputs* of dispatch synthesis and
    /// community detection, composed from the maintained graph-signature
    /// aggregates (`cc_db::signature_agg`): each gate hashes its input
    /// groups' `(count, sum)` pairs in a fixed order. The aggregates are
    /// multiset-homomorphic, so the signature changes iff some input row
    /// multiset changed (64-bit hash strength) — the same guarantee the
    /// previous full-table scans provided, at O(1) decision cost.
    ///
    /// Synthesis is a pure function of the real call edges + symbols, so its
    /// output (synthetic edges) is fully determined by them; hashing real
    /// edges only (excluding `synthesized_by IS NOT NULL`) is both sufficient
    /// and necessary: necessary because synthesis writes synthetic edges back
    /// into `call_edges`, so a signature that included them would drift every
    /// run and never match.
    ///
    /// `DefaultHasher` (SipHash with a fixed key) is deterministic across
    /// processes, so persisting the resulting u64 across runs is sound.
    ///
    /// Signature covering dispatch synthesis inputs (dispatch_sites +
    /// symbols incl. container). Gates the 6 dispatch synthesis passes.
    #[cfg(test)]
    fn dispatch_synthesis_signature(&self) -> CcResult<u64> {
        self.dispatch_synthesis_signature_from(&GraphAggCache::default())
    }

    /// Same as `dispatch_synthesis_signature`, reading the aggregates
    /// through a shared per-build cache.
    fn dispatch_synthesis_signature_from(&self, agg_cache: &GraphAggCache) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let aggs = agg_cache.get(&self.db)?;
        let mut hasher = DefaultHasher::new();
        aggs.dispatch_sites.hash_into(&mut hasher);
        aggs.symbols_full.hash_into(&mut hasher);
        Ok(hasher.finish())
    }

    /// Signature covering interface dispatch synthesis inputs
    /// (real call_edges + symbols incl. container + real semantic_edges).
    #[cfg(test)]
    fn interface_dispatch_signature(&self) -> CcResult<u64> {
        self.interface_dispatch_signature_from(&GraphAggCache::default())
    }

    /// Same as `interface_dispatch_signature`, reading the aggregates
    /// through a shared per-build cache.
    fn interface_dispatch_signature_from(&self, agg_cache: &GraphAggCache) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let aggs = agg_cache.get(&self.db)?;
        let mut hasher = DefaultHasher::new();
        aggs.call_real.hash_into(&mut hasher);
        aggs.symbols_full.hash_into(&mut hasher);
        aggs.semantic_real.hash_into(&mut hasher);
        Ok(hasher.finish())
    }

    /// Signature covering community detection inputs over the committed DB
    /// state (no staged overlay). Production goes through
    /// [`Self::community_signature_projected`] with the staged synthesis
    /// action; this wrapper feeds the signature-coverage tests.
    #[cfg(test)]
    fn community_signature(&self) -> CcResult<u64> {
        self.community_signature_projected(None, &GraphAggCache::default())
    }

    /// Community signature over the projected post-apply call-edge multiset
    /// (ALL call edges including synthetic — community detection conceptually
    /// runs AFTER the synthesis round) plus the symbol structure.
    ///
    /// The projection mirrors [`Self::community_edges_with_overlay`] in
    /// aggregate space without loading any edge list: committed real +
    /// synthetic aggregates, minus the rows of the kinds the staged action
    /// deletes (one small SELECT over those kinds), plus the round's
    /// in-memory inserts. Like the edge-list overlay, within-batch `edge_id`
    /// collisions are projected as distinct rows; if one occurs, the recorded
    /// signature simply mismatches the post-apply state and the gate reruns
    /// once — never a wrong skip.
    ///
    /// `container` is intentionally excluded from the symbol component:
    /// community output is Louvain over call-edge uid pairs plus labels built
    /// from symbol names by uid, so container is not an input — a
    /// container-only change must not force a Louvain rerun. Locked by
    /// `community_signature_ignores_container_unlike_synthesis_signatures`.
    fn community_signature_projected(
        &self,
        action: Option<&SynthesisAction>,
        agg_cache: &GraphAggCache,
    ) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let aggs = agg_cache.get(&self.db)?;
        let mut projected = aggs.call_real.merged(&aggs.call_synthetic);

        let deleted_kinds = Self::overlay_deleted_kinds(action);
        if !deleted_kinds.is_empty() {
            let removed = self
                .db
                .reads()
                .synthetic_call_kind_aggregate(&deleted_kinds)?;
            projected = projected.minus(&removed);
        }
        if let Some(SynthesisAction::Round(round)) = action {
            for delta in &round.deltas {
                for edge in &delta.insert_call_edges {
                    if let (Some(caller), Some(callee)) =
                        (&edge.caller_symbol_uid, &edge.callee_symbol_uid)
                    {
                        projected.add_row(cc_db::signature_agg::hash_call_uid_pair(caller, callee));
                    }
                }
            }
        }

        let mut hasher = DefaultHasher::new();
        projected.hash_into(&mut hasher);
        aggs.symbols_community.hash_into(&mut hasher);
        Ok(hasher.finish())
    }

    /// The `synthesized_by` kinds a staged synthesis action deletes — shared
    /// by the aggregate projection and the edge-list overlay so the two views
    /// of post-apply state can never disagree.
    fn overlay_deleted_kinds(action: Option<&SynthesisAction>) -> Vec<&'static str> {
        match action {
            None => Vec::new(),
            Some(SynthesisAction::Round(round)) => round
                .deltas
                .iter()
                .flat_map(|delta| delta.delete_call_kinds.iter().copied())
                .collect(),
            Some(SynthesisAction::DisableCleanup) => crate::dispatch_synthesis::registry()
                .iter()
                .flat_map(|spec| spec.owned_call_kinds.iter().copied())
                .collect(),
        }
    }
}

#[cfg(test)]
mod graph_signature_coverage_tests {
    use super::*;
    use cc_db::index_db::PrecompressedChunks;
    use cc_model::config::IndexingConfig;
    use cc_model::parse::ParseOutcome;
    use cc_model::{Language, ParserTier};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_indexer() -> (TempDir, Indexer) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("sig_cov.db")).unwrap().0);
        let cfg = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), tmp.path(), &cfg);

        let conn = crate::test_seed::seed_conn(&db);
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');\
             INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) \
                 VALUES('s1','src/x.rs','A','function',1,1,'uA');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) \
                 VALUES('e1','src/x.rs','B',1,'uA','uB');",
        )
        .unwrap();

        (tmp, indexer)
    }

    /// dispatch_synthesis_signature must change when dispatch_sites change,
    /// but NOT when call_edges or semantic_edges change.
    #[test]
    fn dispatch_synthesis_signature_covers_sites_and_symbols() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let sig_base = indexer.dispatch_synthesis_signature().unwrap();

        // A new dispatch site must change the dispatch signature.
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
             VALUES('ds1','src/x.rs',3,0,'jsx_tag','Foo')",
            [],
        )
        .unwrap();
        let sig_after_site = indexer.dispatch_synthesis_signature().unwrap();
        assert_ne!(
            sig_base, sig_after_site,
            "a new dispatch site must change dispatch_synthesis_signature"
        );

        // A new semantic edge must NOT change the dispatch signature.
        conn.execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('se1','src/x.rs','A','uA','I','uI','implements')",
            [],
        )
        .unwrap();
        let sig_after_sem = indexer.dispatch_synthesis_signature().unwrap();
        assert_eq!(
            sig_after_site, sig_after_sem,
            "semantic edges must NOT affect dispatch_synthesis_signature"
        );
    }

    /// interface_dispatch_signature must change when semantic_edges or real
    /// call_edges change, but NOT when dispatch_sites change.
    #[test]
    fn interface_dispatch_signature_covers_edges_and_semantics() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let sig_base = indexer.interface_dispatch_signature().unwrap();

        // A new dispatch site must NOT change the interface signature.
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
             VALUES('ds1','src/x.rs',3,0,'jsx_tag','Foo')",
            [],
        )
        .unwrap();
        let sig_after_site = indexer.interface_dispatch_signature().unwrap();
        assert_eq!(
            sig_base, sig_after_site,
            "dispatch sites must NOT affect interface_dispatch_signature"
        );

        // A real semantic edge must change the interface signature.
        conn.execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('se1','src/x.rs','A','uA','I','uI','implements')",
            [],
        )
        .unwrap();
        let sig_after_sem = indexer.interface_dispatch_signature().unwrap();
        assert_ne!(
            sig_base, sig_after_sem,
            "a real semantic edge must change interface_dispatch_signature"
        );

        // A synthetic semantic edge ('synth:%') must NOT change the interface
        // signature.
        conn.execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('synth:jsx:1','src/x.rs','A','uA','Foo','uFoo','renders_component')",
            [],
        )
        .unwrap();
        let sig_after_synth = indexer.interface_dispatch_signature().unwrap();
        assert_eq!(
            sig_after_sem, sig_after_synth,
            "synthetic semantic edges must be excluded from interface_dispatch_signature"
        );
    }

    /// community_signature must include ALL call edges (including synthetic),
    /// but must NOT depend on dispatch_sites.
    #[test]
    fn community_signature_includes_all_edges() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let sig_base = indexer.community_signature().unwrap();

        // A synthetic call edge must change the community signature.
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,synthesized_by) \
             VALUES('se1','src/x.rs','C',1,'uA','uC','event_emitter')",
            [],
        )
        .unwrap();
        let sig_after_synth = indexer.community_signature().unwrap();
        assert_ne!(
            sig_base, sig_after_synth,
            "a synthetic call edge must change community_signature"
        );
    }

    /// `community_signature` intentionally excludes `container`: community
    /// output is Louvain over call-edge uid pairs plus labels built from
    /// symbol names by uid, so container is not an input and a container-only
    /// change must not force a Louvain rerun. The dispatch/interface
    /// signatures DO hash container (synthesis passes resolve methods through
    /// their containers), so the same change must move both of them.
    #[test]
    fn community_signature_ignores_container_unlike_synthesis_signatures() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let community_before = indexer.community_signature().unwrap();
        let dispatch_before = indexer.dispatch_synthesis_signature().unwrap();
        let interface_before = indexer.interface_dispatch_signature().unwrap();

        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "UPDATE symbols SET container = 'NewContainer' WHERE symbol_id = 's1'",
            [],
        )
        .unwrap();

        assert_eq!(
            community_before,
            indexer.community_signature().unwrap(),
            "a container-only change must NOT affect community_signature"
        );
        assert_ne!(
            dispatch_before,
            indexer.dispatch_synthesis_signature().unwrap(),
            "a container change must affect dispatch_synthesis_signature"
        );
        assert_ne!(
            interface_before,
            indexer.interface_dispatch_signature().unwrap(),
            "a container change must affect interface_dispatch_signature"
        );
    }

    /// Sharing one aggregate read between the dispatch and interface
    /// signatures must not change their values: computing through a shared
    /// `GraphAggCache` yields the same u64s as independent reads.
    #[test]
    fn shared_aggregate_cache_preserves_signature_values() {
        let (_tmp, indexer) = setup_indexer();
        let shared = GraphAggCache::default();

        assert_eq!(
            indexer.dispatch_synthesis_signature().unwrap(),
            indexer.dispatch_synthesis_signature_from(&shared).unwrap(),
            "dispatch signature must be identical through the shared cache"
        );
        assert_eq!(
            indexer.interface_dispatch_signature().unwrap(),
            indexer.interface_dispatch_signature_from(&shared).unwrap(),
            "interface signature must be identical through the shared cache"
        );
    }

    /// Lock the algorithm-"2" signature formula: per-row SipHash over the
    /// signature columns (NULL → ""), per-group `(count, wrapping sum)`
    /// aggregate, gate signature = SipHash over the input groups' pairs in
    /// gate order. Reproduced verbatim from raw `query_json` scans here, so
    /// the cc-db aggregate maintenance and the cc-index gate composition can
    /// never drift apart silently (a drift would force spurious reruns or —
    /// worse — wrong skips after the recorded value is trusted).
    #[test]
    fn aggregate_signatures_match_declared_formula() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        // One row per scanned table, including NULL text columns (s1 has no
        // container; the site has no enclosing/handler uid) so the NULL → ""
        // mapping participates in the comparison.
        let conn = crate::test_seed::seed_conn(db);
        conn.execute_batch(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
                 VALUES('ds1','src/x.rs',3,0,'jsx_tag','Foo');\
             INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
                 VALUES('se1','src/x.rs','A','uA','I','uI','implements');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,synthesized_by) \
                 VALUES('synth:1','src/x.rs','C',2,'uA','uC','event_emitter');",
        )
        .unwrap();

        let json_str = |row: &serde_json::Value, col: &str| -> String {
            row.get(col)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        // (count, wrapping sum of row hashes) — the declared aggregate.
        let fold = |rows: &[serde_json::Value], hash_row: &dyn Fn(&serde_json::Value) -> u64| {
            rows.iter().fold((0u64, 0u64), |(count, sum), row| {
                (count.wrapping_add(1), sum.wrapping_add(hash_row(row)))
            })
        };
        let text_cols_hash = |row: &serde_json::Value, cols: &[&str]| -> u64 {
            let mut hasher = DefaultHasher::new();
            for col in cols {
                json_str(row, col).hash(&mut hasher);
            }
            hasher.finish()
        };

        let symbols_json = db
            .reads()
            .query_json(
                "SELECT symbol_uid, name, kind, container FROM symbols \
                 WHERE symbol_uid IS NOT NULL",
                &[],
            )
            .unwrap();
        let symbols_full = fold(&symbols_json, &|row| {
            text_cols_hash(row, &["symbol_uid", "name", "kind", "container"])
        });
        let symbols_community = fold(&symbols_json, &|row| {
            text_cols_hash(row, &["symbol_uid", "name", "kind"])
        });

        let sites_json = db
            .reads()
            .query_json(
                "SELECT site_kind, key, file_path, enclosing_symbol_uid, handler_symbol_uid, \
                 line FROM dispatch_sites",
                &[],
            )
            .unwrap();
        let dispatch_sites = fold(&sites_json, &|row| {
            let mut hasher = DefaultHasher::new();
            for col in [
                "site_kind",
                "key",
                "file_path",
                "enclosing_symbol_uid",
                "handler_symbol_uid",
            ] {
                json_str(row, col).hash(&mut hasher);
            }
            row.get("line")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .hash(&mut hasher);
            hasher.finish()
        });

        let uid_pair_hash = &|row: &serde_json::Value| {
            text_cols_hash(row, &["caller_symbol_uid", "callee_symbol_uid"])
        };
        let call_real = fold(
            &db.reads()
                .query_json(
                    "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
                     WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
                     AND synthesized_by IS NULL",
                    &[],
                )
                .unwrap(),
            uid_pair_hash,
        );
        let call_all = fold(
            &db.reads()
                .query_json(
                    "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
                     WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
                    &[],
                )
                .unwrap(),
            uid_pair_hash,
        );

        let semantic_real = fold(
            &db.reads()
                .query_json(
                    "SELECT source_symbol_uid, target_symbol_uid, relation_kind FROM semantic_edges \
                     WHERE edge_id NOT LIKE 'synth:%'",
                    &[],
                )
                .unwrap(),
            &|row| text_cols_hash(row, &["source_symbol_uid", "target_symbol_uid", "relation_kind"]),
        );

        let compose = |groups: &[(u64, u64)]| -> u64 {
            let mut hasher = DefaultHasher::new();
            for (count, sum) in groups {
                count.hash(&mut hasher);
                sum.hash(&mut hasher);
            }
            hasher.finish()
        };

        assert_eq!(
            indexer.dispatch_synthesis_signature().unwrap(),
            compose(&[dispatch_sites, symbols_full]),
            "dispatch signature must match the declared aggregate formula"
        );
        assert_eq!(
            indexer.interface_dispatch_signature().unwrap(),
            compose(&[call_real, symbols_full, semantic_real]),
            "interface signature must match the declared aggregate formula"
        );
        assert_eq!(
            indexer.community_signature().unwrap(),
            compose(&[call_all, symbols_community]),
            "community signature must match the declared aggregate formula"
        );
    }

    /// Per-pass signatures must be independent: changing dispatch_sites only
    /// affects dispatch_synthesis_signature, not interface_dispatch_signature.
    #[test]
    fn per_pass_signatures_are_independent() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let dispatch_before = indexer.dispatch_synthesis_signature().unwrap();
        let interface_before = indexer.interface_dispatch_signature().unwrap();

        // Modify only dispatch_sites
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
             VALUES('ds2','src/x.rs',5,0,'event_emit','click')",
            [],
        )
        .unwrap();

        let dispatch_after = indexer.dispatch_synthesis_signature().unwrap();
        let interface_after = indexer.interface_dispatch_signature().unwrap();

        assert_ne!(
            dispatch_before, dispatch_after,
            "dispatch_synthesis_signature must change when dispatch_sites change"
        );
        assert_eq!(
            interface_before, interface_after,
            "interface_dispatch_signature must NOT change when only dispatch_sites change"
        );
    }

    /// Convergence at the gate level: after incremental writes (add / modify
    /// / remove, all through the maintained batch writer) the signatures
    /// composed from the stored aggregates must equal the signatures
    /// recomputed by a full scan of the final content. A divergence here is
    /// the wrong-skip failure mode the aggregate maintenance must never
    /// allow.
    #[test]
    fn maintained_signatures_equal_full_recompute_after_incremental_writes() {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("sig_conv.db")).unwrap().0);
        let indexer = Indexer::new(db.clone(), tmp.path(), &IndexingConfig::default());

        let symbol = |file: &str, name: &str, uid: &str| cc_model::symbol::SymbolRecord {
            symbol_id: format!("{file}:{name}"),
            file_path: file.to_string(),
            name: name.to_string(),
            kind: cc_model::symbol::SymbolKind::Function,
            container: None,
            start_line: 1,
            end_line: 3,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            qname: Some(format!("{file}.{name}")),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(uid.to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        };
        let unit = |file: &str, outcome: ParseOutcome| FileWriteUnit {
            rel_path: file.to_string(),
            language: Language::Rust,
            content_hash: format!("hash-{file}-{}", outcome.symbols.len()),
            mtime: 1.0,
            size: 1,
            outcome,
        };
        let write_batch = |to_remove: &[String], units: &[FileWriteUnit]| {
            indexer
                .db
                .writes()
                .write_incremental_batch(
                    to_remove,
                    units,
                    &[],
                    &[],
                    &[],
                    &PrecompressedChunks::new(),
                )
                .unwrap();
        };
        let signatures = || {
            (
                indexer.dispatch_synthesis_signature().unwrap(),
                indexer.interface_dispatch_signature().unwrap(),
                indexer.community_signature().unwrap(),
            )
        };
        let assert_converged = |label: &str| {
            let stored = db
                .reads()
                .stored_graph_signature_aggregates()
                .unwrap()
                .unwrap_or_else(|| panic!("{label}: stored baseline must exist"));
            let scanned = db.reads().scan_graph_signature_aggregates().unwrap();
            assert_eq!(
                stored, scanned,
                "{label}: stored aggregates != full recompute"
            );
            // Signature level: production path (stored baseline) vs the
            // fallback recompute (baseline made unreadable, then restored).
            let with_stored = signatures();
            let raw = db
                .reads()
                .get_metadata("graph_sig_aggregates")
                .unwrap()
                .unwrap();
            db.writes()
                .set_metadata("graph_sig_aggregates", "")
                .unwrap();
            let from_scan = signatures();
            db.writes()
                .set_metadata("graph_sig_aggregates", &raw)
                .unwrap();
            assert_eq!(
                with_stored, from_scan,
                "{label}: maintained signatures != full-recompute signatures"
            );
        };

        // Add two files.
        write_batch(
            &[],
            &[
                unit(
                    "src/a.rs",
                    ParseOutcome {
                        symbols: vec![symbol("src/a.rs", "alpha", "uid_alpha")],
                        ..Default::default()
                    },
                ),
                unit(
                    "src/b.rs",
                    ParseOutcome {
                        symbols: vec![symbol("src/b.rs", "beta", "uid_beta")],
                        ..Default::default()
                    },
                ),
            ],
        );
        assert_converged("after add");

        // Modify one file (different symbol set).
        write_batch(
            &[],
            &[unit(
                "src/a.rs",
                ParseOutcome {
                    symbols: vec![
                        symbol("src/a.rs", "alpha", "uid_alpha"),
                        symbol("src/a.rs", "alpha_two", "uid_alpha_two"),
                    ],
                    ..Default::default()
                },
            )],
        );
        assert_converged("after modify");

        // Remove the other file.
        write_batch(&["src/b.rs".to_string()], &[]);
        assert_converged("after remove");
    }
}

#[cfg(test)]
mod community_overlay_tests {
    use super::*;
    use crate::synthesis_pipeline::{apply_synthesis_round, EdgeDelta, SynthesisRound};
    use cc_db::index_db::PrecompressedChunks;
    use cc_model::config::IndexingConfig;
    use cc_model::edge::CallEdgeRecord;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Fixture: one real call edge plus one stale synthetic edge of a kind a
    /// later round replaces.
    fn setup_indexer() -> (TempDir, Indexer) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("overlay.db")).unwrap().0);
        let indexer = Indexer::new(db.clone(), tmp.path(), &IndexingConfig::default());

        let conn = crate::test_seed::seed_conn(&db);
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');\
             INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) \
                 VALUES('s1','src/x.rs','A','function',1,1,'uA');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) \
                 VALUES('e1','src/x.rs','B',1,'uA','uB');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,synthesized_by) \
                 VALUES('synth:old','src/x.rs','Old',2,'uA','uOld','event_emitter');",
        )
        .unwrap();
        // Initialize the maintained aggregate baseline (no-op batch) so the
        // overlay assertions exercise the production stored-baseline path,
        // including the unit-of-work maintenance during the apply.
        db.writes()
            .write_incremental_batch(&[], &[], &[], &[], &[], &PrecompressedChunks::new())
            .unwrap();

        (tmp, indexer)
    }

    fn synthetic_edge(edge_id: &str, callee_uid: &str) -> CallEdgeRecord {
        CallEdgeRecord {
            edge_id: edge_id.to_string(),
            file_path: "src/x.rs".to_string(),
            callee_symbol: callee_uid.to_string(),
            line: 3,
            caller_symbol_uid: Some("uA".to_string()),
            callee_symbol_uid: Some(callee_uid.to_string()),
            synthesized_by: Some("event_emitter".to_string()),
            ..Default::default()
        }
    }

    /// The staged community overlay (computed BEFORE the synthesis apply)
    /// must equal the committed edge set AFTER the apply — both as a multiset
    /// of uid pairs and through the community signature, so the marker the
    /// apply stage records matches what the next build recomputes from the DB
    /// (no spurious Louvain rerun, no wrong skip).
    #[test]
    fn community_overlay_matches_post_apply_state() {
        let (_tmp, indexer) = setup_indexer();

        let round = SynthesisRound {
            deltas: vec![EdgeDelta {
                delete_call_kinds: vec!["event_emitter"],
                delete_semantic_prefixes: vec![],
                insert_call_edges: vec![
                    synthetic_edge("synth:new1", "uNew1"),
                    synthetic_edge("synth:new2", "uNew2"),
                    // No-UID edges are excluded from the community input by
                    // the SQL NOT NULL filter; the overlay must skip them too.
                    CallEdgeRecord {
                        edge_id: "synth:nouid".to_string(),
                        file_path: "src/x.rs".to_string(),
                        callee_symbol: "Anon".to_string(),
                        line: 4,
                        synthesized_by: Some("event_emitter".to_string()),
                        ..Default::default()
                    },
                ],
                insert_semantic_edges: vec![],
            }],
        };
        let action = SynthesisAction::Round(round);

        let mut overlay = indexer.community_edges_with_overlay(Some(&action)).unwrap();
        // The production gate path: aggregate-space projection of the staged
        // action, computed BEFORE the apply.
        let overlay_sig = indexer
            .community_signature_projected(Some(&action), &GraphAggCache::default())
            .unwrap();

        // The stale 'event_emitter' edge is projected out, the new edges in.
        overlay.sort();
        assert_eq!(
            overlay,
            vec![
                ("uA".to_string(), "uB".to_string()),
                ("uA".to_string(), "uNew1".to_string()),
                ("uA".to_string(), "uNew2".to_string()),
            ],
            "overlay must replace the deleted kind with the round's inserts"
        );

        let SynthesisAction::Round(round) = action else {
            unreachable!()
        };
        apply_synthesis_round(&indexer.db, &round).unwrap();

        let mut committed = indexer.db.reads().call_uid_edges().unwrap();
        committed.sort();
        assert_eq!(
            overlay, committed,
            "pre-apply overlay must equal the post-apply committed edge set"
        );
        assert_eq!(
            overlay_sig,
            indexer.community_signature().unwrap(),
            "overlay signature must equal the post-apply DB signature"
        );
    }

    /// With no staged synthesis action the overlay is exactly the committed
    /// edge set (synthetic edges included).
    #[test]
    fn community_overlay_without_round_is_committed_state() {
        let (_tmp, indexer) = setup_indexer();
        let mut overlay = indexer.community_edges_with_overlay(None).unwrap();
        let mut committed = indexer.db.reads().call_uid_edges().unwrap();
        overlay.sort();
        committed.sort();
        assert_eq!(overlay, committed);
    }
}

#[cfg(test)]
mod test_edge_invariant_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> (TempDir, Arc<IndexDb>, Indexer) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        for (rel, content) in files {
            let path = project.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    /// Sorted (test_file_path, code_file_path, reason) triples.
    fn edges(db: &IndexDb) -> Vec<(String, String, String)> {
        let mut rows: Vec<(String, String, String)> = db
            .reads()
            .query_json(
                "SELECT test_file_path, code_file_path, reason FROM test_edges",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["test_file_path"].as_str().unwrap().to_string(),
                    row["code_file_path"].as_str().unwrap().to_string(),
                    row["reason"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        rows.sort();
        rows
    }

    /// 不变量：仅修改文件内容（路径集合不变）的增量构建跳过 test_edges
    /// 重建，但边集必须与之后的全量重建完全一致。
    #[test]
    fn content_only_incremental_keeps_test_edges_consistent_with_full() {
        let (tmp, db, indexer) = setup(&[
            ("src/foo.py", "def foo():\n    return 1\n"),
            ("tests/foo_test.py", "def check_foo():\n    return 1\n"),
        ]);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        let initial = edges(&db);
        assert!(
            initial
                .iter()
                .any(|(t, c, _)| t == "tests/foo_test.py" && c == "src/foo.py"),
            "fixture must link tests/foo_test.py to src/foo.py, got {:?}",
            initial
        );

        // Content-only edits to both files: the batch adds/removes no paths,
        // so the rebuild is skipped — edges must survive unchanged.
        std::fs::write(
            project.join("src/foo.py"),
            "def foo():\n    return 2  # edited\n",
        )
        .unwrap();
        std::fs::write(
            project.join("tests/foo_test.py"),
            "def check_foo():\n    return 2  # edited\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            edges(&db),
            initial,
            "update-only incremental must leave test_edges identical"
        );

        // Cross-check against a from-scratch full rebuild.
        indexer.build_index(project, true).unwrap();
        assert_eq!(
            edges(&db),
            initial,
            "full rebuild must agree with the incrementally preserved edges"
        );
    }

    /// 不变量：新增 / 删除 test 或 source 文件的增量构建仍重建相关边，
    /// 边随路径集合变化正确出现与消失。
    #[test]
    fn added_and_removed_paths_update_test_edges_incrementally() {
        let (tmp, db, indexer) = setup(&[("src/foo.py", "def foo():\n    return 1\n")]);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(edges(&db).is_empty(), "no test files yet, no edges");

        // Add a test file: the edge must appear in the same incremental build.
        std::fs::create_dir_all(project.join("tests")).unwrap();
        std::fs::write(
            project.join("tests/foo_test.py"),
            "def check_foo():\n    return 1\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            edges(&db)
                .iter()
                .any(|(t, c, _)| t == "tests/foo_test.py" && c == "src/foo.py"),
            "adding a test file must create its edge, got {:?}",
            edges(&db)
        );

        // Add a second source file matched by a new test file in one batch.
        std::fs::write(project.join("src/bar.py"), "def bar():\n    return 1\n").unwrap();
        std::fs::write(
            project.join("tests/bar_test.py"),
            "def check_bar():\n    return 1\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            edges(&db)
                .iter()
                .any(|(t, c, _)| t == "tests/bar_test.py" && c == "src/bar.py"),
            "adding source+test in one batch must create the edge, got {:?}",
            edges(&db)
        );

        // Remove the test file: its edges must disappear.
        std::fs::remove_file(project.join("tests/foo_test.py")).unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            !edges(&db).iter().any(|(t, _, _)| t == "tests/foo_test.py"),
            "removing a test file must drop its edges, got {:?}",
            edges(&db)
        );

        // Remove a source file: edges pointing at it must disappear too.
        std::fs::remove_file(project.join("src/bar.py")).unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            !edges(&db).iter().any(|(_, c, _)| c == "src/bar.py"),
            "removing a source file must drop edges pointing at it, got {:?}",
            edges(&db)
        );
    }
}
