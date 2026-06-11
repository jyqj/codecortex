//! Declared epoch invariants: which cache-invalidation clock each writable
//! table bumps.
//!
//! cc-db persists two monotonic clocks in the metadata KV table (see
//! [`crate::index_db::IndexGeneration`]): `index_epoch` for index content and
//! `evidence_epoch` for runtime evidence. Consumers key cache slots on one or
//! both clocks, so a write that bumps the wrong clock either leaks stale
//! results (missed bump) or destroys cache locality (spurious bump).
//!
//! This module is the single declaration of the table → clock mapping. It
//! does not change runtime behavior: the write methods keep their explicit
//! `bump_index_epoch_on` / `bump_evidence_epoch_on` calls, and the audit
//! tests below verify those calls against this declaration. When adding a
//! table or a write method, extend [`EPOCH_RULES`] and the audit test.
//!
//! Derived FTS mirrors (`symbols_fts`, `chunks_fts`, ...) are maintained in
//! the same transactions as their source tables and follow the source
//! table's clock; they are not listed separately.

/// The two cache-invalidation clocks persisted in the metadata KV table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochClock {
    /// `index_epoch`: bumped by writes to index content. Invalidates caches
    /// derived purely from parsed/post-processed code structure.
    Index,
    /// `evidence_epoch`: bumped by runtime-evidence ingestion. Index-only
    /// cache slots deliberately ignore it so evidence writes never evict them.
    Evidence,
}

/// Table → clock declaration, with a one-line reason per entry.
///
/// Exception worth calling out: `http_call_edges` content is written by index
/// batches (Index), but `boost_http_edge_confidence` mutates only the
/// `confidence` column during evidence ingestion and bumps Evidence — an
/// evidence-driven confidence nudge must not invalidate IndexOnly cache
/// slots (bridges/adjacency caches that consume the boost are keyed on
/// `evidence_epoch`). That per-method exception is encoded in
/// [`boost_http_edge_confidence_clock`].
pub const EPOCH_RULES: &[(&str, EpochClock, &str)] = &[
    // ── File-batch content: written by replace_files_batch /
    //    write_incremental_batch / remove_files_batch ────────────────
    (
        "files",
        EpochClock::Index,
        "parsed file metadata is index content",
    ),
    (
        "symbols",
        EpochClock::Index,
        "symbol table is index content",
    ),
    (
        "imports",
        EpochClock::Index,
        "import edges are index content",
    ),
    (
        "call_edges",
        EpochClock::Index,
        "call graph is index content (incl. synthetic edges)",
    ),
    (
        "symbol_refs",
        EpochClock::Index,
        "reference index is index content",
    ),
    (
        "semantic_edges",
        EpochClock::Index,
        "semantic relations are index content",
    ),
    (
        "dispatch_sites",
        EpochClock::Index,
        "dispatch sites feed synthesis, an index concern",
    ),
    (
        "data_flow_edges",
        EpochClock::Index,
        "data-flow edges are index content",
    ),
    (
        "literal_index",
        EpochClock::Index,
        "literal index is index content",
    ),
    (
        "chunks",
        EpochClock::Index,
        "retrieval chunks mirror file content",
    ),
    // ── Post-process artifacts ──────────────────────────────────────
    (
        "routes",
        EpochClock::Index,
        "route nodes are parsed/post-processed index content",
    ),
    (
        "http_call_edges",
        EpochClock::Index,
        "edge rows are index content; confidence boost is the documented Evidence exception",
    ),
    (
        "test_edges",
        EpochClock::Index,
        "test linkage derives from indexed files",
    ),
    (
        "co_change_edges",
        EpochClock::Index,
        "co-change mining output consumed as index content",
    ),
    (
        "communities",
        EpochClock::Index,
        "community detection output over the index graph",
    ),
    (
        "frameworks",
        EpochClock::Index,
        "framework detection derives from indexed files",
    ),
    (
        "infra_nodes",
        EpochClock::Index,
        "infra graph is parsed from indexed config files",
    ),
    (
        "infra_edges",
        EpochClock::Index,
        "infra graph is parsed from indexed config files",
    ),
    // ── Authored content stored alongside the index ─────────────────
    (
        "adr",
        EpochClock::Index,
        "ADRs surface in context output keyed on index_epoch",
    ),
    // ── Runtime evidence ─────────────────────────────────────────────
    (
        "runtime_evidence",
        EpochClock::Evidence,
        "observations arrive continuously; must not evict index-only caches",
    ),
];

/// Clock for a content write to `table`, per [`EPOCH_RULES`]. `None` for
/// unknown tables and for `metadata` (which stores the clocks themselves).
pub fn epoch_clock_for_table(table: &str) -> Option<EpochClock> {
    EPOCH_RULES
        .iter()
        .find(|(name, _, _)| *name == table)
        .map(|(_, clock, _)| *clock)
}

/// The one declared per-method exception: `boost_http_edge_confidence`
/// mutates `http_call_edges.confidence` but bumps the Evidence clock.
pub fn boost_http_edge_confidence_clock() -> EpochClock {
    EpochClock::Evidence
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{boost_http_edge_confidence_clock, epoch_clock_for_table, EpochClock, EPOCH_RULES};
    use crate::index_db::{FileWriteUnit, IndexDb, IndexGeneration};

    fn setup() -> (IndexDb, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("epoch_rules.db")).unwrap().0;
        (db, tmp)
    }

    fn file_unit(rel_path: &str) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: rel_path.to_string(),
            language: cc_model::Language::Rust,
            content_hash: format!("hash-{rel_path}"),
            mtime: 1.0,
            size: 1,
            outcome: cc_model::ParseOutcome::default(),
        }
    }

    /// Run `write`, then assert exactly the declared clock advanced.
    fn assert_bumps(db: &IndexDb, declared: EpochClock, label: &str, write: impl FnOnce(&IndexDb)) {
        let before = db.generation().unwrap();
        write(db);
        let after = db.generation().unwrap();
        match declared {
            EpochClock::Index => {
                assert!(
                    after.index_epoch > before.index_epoch,
                    "{label}: declared Index but index_epoch did not advance"
                );
                assert_eq!(
                    after.evidence_epoch, before.evidence_epoch,
                    "{label}: declared Index but evidence_epoch moved"
                );
            }
            EpochClock::Evidence => {
                assert!(
                    after.evidence_epoch > before.evidence_epoch,
                    "{label}: declared Evidence but evidence_epoch did not advance"
                );
                assert_eq!(
                    after.index_epoch, before.index_epoch,
                    "{label}: declared Evidence but index_epoch moved"
                );
            }
        }
    }

    /// Audit: every declared table has a representative write method whose
    /// observed epoch bump matches the declaration. Tables marked as covered
    /// by the file batch are exercised once through `replace_files_batch`.
    #[test]
    fn declared_clock_matches_observed_bump_for_every_table() {
        let (db, _tmp) = setup();

        // Tables whose content writes flow through the file batch writers.
        const FILE_BATCH_TABLES: &[&str] = &[
            "files",
            "symbols",
            "imports",
            "symbol_refs",
            "data_flow_edges",
            "literal_index",
            "chunks",
            "http_call_edges",
        ];

        for (table, declared, _reason) in EPOCH_RULES {
            let declared = *declared;
            match *table {
                t if FILE_BATCH_TABLES.contains(&t) => {
                    assert_bumps(&db, declared, t, |db| {
                        db.replace_files_batch(&[file_unit(&format!("src/{t}.rs"))])
                            .unwrap();
                    });
                }
                "call_edges" => assert_bumps(&db, declared, "call_edges", |db| {
                    db.delete_synthetic_call_edges("event_emitter").unwrap();
                }),
                "semantic_edges" => assert_bumps(&db, declared, "semantic_edges", |db| {
                    db.remove_semantic_edges_by_file("src/none.rs").unwrap();
                }),
                "dispatch_sites" => assert_bumps(&db, declared, "dispatch_sites", |db| {
                    db.replace_dispatch_sites("src/none.rs", &[]).unwrap();
                }),
                "routes" => assert_bumps(&db, declared, "routes", |db| {
                    db.insert_route_nodes_batch(&[cc_model::edge::RouteNodeRecord {
                        route_id: "route:1".to_string(),
                        file_path: "src/files.rs".to_string(),
                        route_path: "/x".to_string(),
                        method: None,
                        handler_symbol_uid: None,
                        handler_name: None,
                        framework: None,
                        line: 1,
                        end_line: None,
                        normalized_path: None,
                        confidence: 0.9,
                        parser_tier: cc_model::ParserTier::TreeSitter,
                    }])
                    .unwrap();
                }),
                "test_edges" => assert_bumps(&db, declared, "test_edges", |db| {
                    db.rebuild_test_edges_for_files(&["src/files.rs".to_string()])
                        .unwrap();
                }),
                "co_change_edges" => assert_bumps(&db, declared, "co_change_edges", |db| {
                    db.insert_co_change_edges_batch(&[cc_model::edge::CoChangeEdgeRecord {
                        edge_id: "cc:1".to_string(),
                        file_a: "src/files.rs".to_string(),
                        file_b: "src/symbols.rs".to_string(),
                        co_change_count: 1,
                        total_commits_a: 1,
                        total_commits_b: 1,
                        confidence: 0.5,
                    }])
                    .unwrap();
                }),
                "communities" => assert_bumps(&db, declared, "communities", |db| {
                    db.assign_all_symbols_to_community(0).unwrap();
                }),
                "frameworks" => assert_bumps(&db, declared, "frameworks", |db| {
                    db.replace_repo_frameworks(&[]).unwrap();
                }),
                "infra_nodes" | "infra_edges" => assert_bumps(&db, declared, table, |db| {
                    db.replace_infra_data(&[], &[]).unwrap();
                }),
                "adr" => assert_bumps(&db, declared, "adr", |db| {
                    db.adr_upsert("adr-1", "t", "accepted", "c", "d", "2024-01-01T00:00:00Z")
                        .unwrap();
                }),
                "runtime_evidence" => assert_bumps(&db, declared, "runtime_evidence", |db| {
                    db.upsert_runtime_evidence(
                        "ev1",
                        "svc",
                        Some("GET"),
                        "/x",
                        Some("200"),
                        "2024-01-01T00:00:00Z",
                    )
                    .unwrap();
                }),
                other => panic!("EPOCH_RULES entry '{other}' has no representative writer in the audit test — add one"),
            }
        }
    }

    /// The remaining evidence write methods all bump the Evidence clock only.
    #[test]
    fn evidence_method_family_bumps_evidence_only() {
        let (db, _tmp) = setup();
        let declared = epoch_clock_for_table("runtime_evidence").unwrap();

        assert_bumps(&db, declared, "link_evidence_to_edge", |db| {
            db.link_evidence_to_edge("ev1", "edge1").unwrap();
        });
        assert_bumps(&db, declared, "update_evidence_p95", |db| {
            db.update_evidence_p95("ev1", 12.5).unwrap();
        });
        assert_bumps(&db, declared, "update_evidence_route_id", |db| {
            db.update_evidence_route_id("ev1", "route:1").unwrap();
        });
    }

    /// The documented exception: boosting http edge confidence during
    /// evidence ingestion bumps Evidence, not Index.
    #[test]
    fn boost_http_edge_confidence_bumps_evidence_clock() {
        let (db, _tmp) = setup();
        assert_eq!(boost_http_edge_confidence_clock(), EpochClock::Evidence);
        assert_bumps(
            &db,
            boost_http_edge_confidence_clock(),
            "boost_http_edge_confidence",
            |db| {
                db.boost_http_edge_confidence("missing-edge", 0.1).unwrap();
            },
        );
    }

    /// A committed unit of work bumps index_epoch exactly once, regardless of
    /// how many writes it batches; evidence_epoch is untouched.
    #[test]
    fn unit_of_work_commit_bumps_index_exactly_once() {
        let (db, _tmp) = setup();
        let before = db.generation().unwrap();

        let uow = db.begin_unit_of_work().unwrap();
        uow.delete_synthetic_call_edges("event_emitter").unwrap();
        uow.delete_synthetic_semantic_edges("synth:").unwrap();
        uow.commit().unwrap();

        let after = db.generation().unwrap();
        assert_eq!(
            after,
            IndexGeneration {
                index_epoch: before.index_epoch + 1,
                evidence_epoch: before.evidence_epoch,
            }
        );
    }

    #[test]
    fn lookup_covers_known_tables_and_rejects_metadata() {
        assert_eq!(epoch_clock_for_table("symbols"), Some(EpochClock::Index));
        assert_eq!(
            epoch_clock_for_table("runtime_evidence"),
            Some(EpochClock::Evidence)
        );
        assert_eq!(epoch_clock_for_table("metadata"), None);
        assert_eq!(epoch_clock_for_table("no_such_table"), None);
    }
}
