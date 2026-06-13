//! Dispatch synthesis passes.
//!
//! 1. **Event-emitter** – matches `emit(eventName, ...)` → `on(eventName, handler)`.
//! 2. **JSX component** – matches `<Component />` usage → component definition
//!    (produces `RendersComponent` semantic edges).
//! 3. **State setter** – matches `setFoo(...)` / `this.setState(...)` → re-render
//!    (produces synthetic call edges).
//! 4. **Field-backed observer** – detects registrar/dispatcher method pairs within
//!    the same class (e.g. `on`/`emit`, `subscribe`/`notify`) and creates edges
//!    from dispatcher to registrar targets.
//! 5. **React re-render chain** – extends state setter synthesis by linking
//!    re-rendering components to their JSX child components.
//!
//! Every pass is compute-only: it reads committed index state via the read
//! pool and returns an [`EdgeDelta`] (the synthesized-edge kinds it replaces
//! plus the edges it produces). Passes never take the write lock — ordering
//! and atomic application live in [`crate::synthesis_pipeline`].
//!
//! Each pass is declared once as a [`SynthesisPassSpec`] in its own submodule:
//! its id, the synthetic edge kinds/prefixes it owns, and its compute entry
//! point. [`registry`] lists the specs in execution order; the pipeline drives
//! the round from it, and the disable-cleanup path in `phase_postprocess`
//! derives its deletion set from the owned declarations instead of repeating
//! the kind strings.

use std::collections::HashSet;

use cc_db::index_db::IndexDb;
use cc_model::CcResult;

use crate::synthesis_pipeline::EdgeDelta;

mod event_emitter;
mod field_observer;
mod interface_dispatch;
mod jsx;
mod rerender;
mod state_setter;
mod vue;

/// Configuration knobs for dispatch synthesis.
pub struct SynthesisConfig {
    pub enabled: bool,
    /// Maximum narrowed on-sites for a single emit site before we skip it.
    pub event_fanout_cap: usize,
    /// Event names that are too generic to match globally (only matched if
    /// receiver_expr or same-file evidence exists).
    pub generic_event_denylist: HashSet<String>,
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            event_fanout_cap: 6,
            generic_event_denylist: [
                "data",
                "error",
                "close",
                "end",
                "message",
                "change",
                "connect",
                "disconnect",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

// ── Declarative pass seam ─────────────────────────────────────

/// Which `phase_postprocess` signature gate enables a pass for a round.
pub(crate) enum PassGate {
    /// Gated on `dispatch_changed` (dispatch sites / symbols changed).
    Dispatch,
    /// Gated on `interface_changed` (call edges / implements edges changed).
    Interface,
}

/// Read-only inputs for one synthesis pass compute.
///
/// `prior_deltas` holds the deltas of the passes that already ran in this
/// round, in registry order. The cross-pass overlay covers CALL edges only:
/// a pass that consumes committed semantic edges (interface dispatch reads
/// `implements` rows) will not see semantic edges synthesized earlier in the
/// same round. Passes today synthesize semantic edges solely with the
/// `RendersComponent` relation, which no pass consumes — so committed-state
/// semantic reads are round-equivalent. If a future pass ever synthesizes
/// `implements` semantic edges, the overlay must be extended to semantic
/// edges first.
pub(crate) struct PassContext<'a> {
    pub(crate) db: &'a IndexDb,
    pub(crate) config: &'a SynthesisConfig,
    pub(crate) prior_deltas: &'a [EdgeDelta],
}

/// Single declaration point for one synthesis pass: its identity, the
/// synthetic edge kinds it owns, and its compute entry point.
///
/// A pass's [`EdgeDelta`] may only delete call kinds / semantic prefixes
/// listed in its owned sets (enforced by a `debug_assert` in the pipeline);
/// the disable-cleanup path deletes exactly the union of the owned sets.
pub(crate) struct SynthesisPassSpec {
    /// Stable pass identifier (logs, assertions, tests).
    pub(crate) id: &'static str,
    /// Signature gate that enables this pass for a round.
    pub(crate) gate: PassGate,
    /// `synthesized_by` kinds of call edges this pass owns and replaces.
    pub(crate) owned_call_kinds: &'static [&'static str],
    /// `edge_id` prefixes of semantic edges this pass owns and replaces.
    pub(crate) owned_semantic_prefixes: &'static [&'static str],
    /// Compute the pass delta against committed state plus the prior-delta
    /// overlay in [`PassContext`]. Never takes the write lock.
    pub(crate) compute: fn(&PassContext) -> CcResult<EdgeDelta>,
}

/// All synthesis passes in execution order. Interface dispatch runs last so
/// its prior-delta overlay sees every dispatch-pass call edge of the round.
pub(crate) fn registry() -> &'static [SynthesisPassSpec] {
    const REGISTRY: &[SynthesisPassSpec] = &[
        event_emitter::SPEC,
        jsx::SPEC,
        state_setter::SPEC,
        field_observer::SPEC,
        rerender::SPEC,
        vue::SPEC,
        interface_dispatch::SPEC,
    ];
    REGISTRY
}

// ── Shared helpers ────────────────────────────────────────────

/// Deterministic edge id via blake3 hash.
///
/// The output format is `synth:{kind}:{hash}`, e.g. `synth:ee:abc123`,
/// `synth:jsx:def456`, `synth:ss:789abc`.  This ensures deletion by prefix
/// (`synth:jsx:`) only removes the intended synthesis pass's edges.
///
/// Uses source and target identifiers (typically `site_id` or `symbol_uid`)
/// which are already unique per dispatch site, avoiding collisions when
/// multiple calls to the same setter appear on the same line.
fn synth_edge_id(kind: &str, source_id: &str, target_id: &str) -> String {
    use blake3::Hasher;
    let mut h = Hasher::new();
    h.update(b"synth:");
    h.update(kind.as_bytes());
    h.update(b":");
    h.update(source_id.as_bytes());
    h.update(b":");
    h.update(target_id.as_bytes());
    format!("synth:{}:{}", kind, &h.finalize().to_hex()[..16])
}

#[cfg(test)]
mod tests {
    use super::event_emitter::compute_event_emitter_synthesis;
    use super::field_observer::compute_field_observer_synthesis;
    use super::interface_dispatch::compute_interface_dispatch_synthesis;
    use super::jsx::compute_jsx_synthesis;
    use super::rerender::compute_react_rerender_chain_synthesis;
    use super::state_setter::compute_state_setter_synthesis;
    use super::vue::compute_vue_template_synthesis;
    use super::*;
    use crate::synthesis_pipeline::{
        apply_synthesis_round, compute_synthesis_round, SynthesisRound,
    };
    use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
    use tempfile::TempDir;

    fn setup_test_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test_index.sqlite3");
        let db = IndexDb::open(&db_path).unwrap().0;
        (tmp, db)
    }

    /// Compute-then-apply for a single pass delta (test convenience).
    fn apply_one(db: &IndexDb, delta: EdgeDelta) {
        apply_synthesis_round(
            db,
            &SynthesisRound {
                deltas: vec![delta],
            },
        )
        .unwrap();
    }

    /// Insert a symbol into the DB for resolution during synthesis.
    fn insert_symbol(db: &IndexDb, file_path: &str, name: &str, kind: &str, uid: &str) {
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
             VALUES(?1, 'vue', 'abc', 0.0, 100, '2025-01-01')",
            rusqlite::params![file_path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, start_col, end_col, \
             parser_tier, parser_confidence, symbol_uid, is_default_export) \
             VALUES(?1, ?2, ?3, ?4, 1, 50, 0, 0, 'heuristic', 0.8, ?5, 0)",
            rusqlite::params![
                format!("sym:{}:{}", file_path, name),
                file_path,
                name,
                kind,
                uid,
            ],
        )
        .unwrap();
    }

    #[test]
    fn vue_child_component_produces_renders_component_edge() {
        let (_tmp, db) = setup_test_db();

        let component_uid = "uid:App:component";
        let child_uid = "uid:ChildComponent:component";

        // Insert the parent component symbol and child component symbol.
        insert_symbol(&db, "src/App.vue", "App", "component", component_uid);
        insert_symbol(
            &db,
            "src/ChildComponent.vue",
            "ChildComponent",
            "component",
            child_uid,
        );

        // Insert a VueChildComponent dispatch site.
        let sites = vec![DispatchSiteRecord {
            site_id: "ds_vue_child:App.vue:5:4".to_string(),
            file_path: "src/App.vue".to_string(),
            line: 5,
            col: 4,
            enclosing_symbol_uid: Some(component_uid.to_string()),
            receiver_expr: None,
            site_kind: DispatchSiteKind::VueChildComponent,
            key: "ChildComponent".to_string(),
            handler_expr: None,
            handler_symbol_uid: None,
            confidence: 0.78,
        }];
        db.writes()
            .replace_dispatch_sites("src/App.vue", &sites)
            .unwrap();

        // Run synthesis.
        let delta = compute_vue_template_synthesis(&db).unwrap();
        let count = delta.insert_call_edges.len() + delta.insert_semantic_edges.len();
        apply_one(&db, delta);
        assert_eq!(count, 1, "should produce 1 RendersComponent edge");

        // Verify the semantic edge.
        let edges = db
            .reads()
            .query_semantic_edges(
                Some(component_uid),
                Some(child_uid),
                Some("renders_component"),
            )
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].edge_id.starts_with("synth:vue:"));
        assert_eq!(edges[0].target_symbol.as_str(), "ChildComponent");
    }

    #[test]
    fn vue_event_handler_produces_call_edge() {
        let (_tmp, db) = setup_test_db();

        let component_uid = "uid:MyForm:component";
        let handler_uid = "uid:MyForm:submitForm:function";

        // Insert component and handler symbols in the same file.
        insert_symbol(&db, "src/MyForm.vue", "MyForm", "component", component_uid);
        insert_symbol(&db, "src/MyForm.vue", "submitForm", "function", handler_uid);

        // Insert a VueEventHandler dispatch site.
        let sites = vec![DispatchSiteRecord {
            site_id: "ds_vue_evt:MyForm.vue:10:20".to_string(),
            file_path: "src/MyForm.vue".to_string(),
            line: 10,
            col: 20,
            enclosing_symbol_uid: Some(component_uid.to_string()),
            receiver_expr: Some("submit".to_string()),
            site_kind: DispatchSiteKind::VueEventHandler,
            key: "submitForm".to_string(),
            handler_expr: Some("submitForm".to_string()),
            handler_symbol_uid: None,
            confidence: 0.78,
        }];
        db.writes()
            .replace_dispatch_sites("src/MyForm.vue", &sites)
            .unwrap();

        // Run synthesis.
        let delta = compute_vue_template_synthesis(&db).unwrap();
        let count = delta.insert_call_edges.len() + delta.insert_semantic_edges.len();
        apply_one(&db, delta);
        assert_eq!(count, 1, "should produce 1 call edge");

        // Verify the synthetic call edge via SQL.
        let conn = db.reads().read_conn().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, caller_symbol_uid, callee_symbol_uid, synthesized_by, call_kind \
                 FROM call_edges WHERE synthesized_by = 'vue_event_handler'",
            )
            .unwrap();
        let edges: Vec<(String, String, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(edges.len(), 1);
        let (edge_id, caller_uid, callee_uid, synthesized_by, call_kind) = &edges[0];
        assert!(edge_id.starts_with("synth:vue:"));
        assert_eq!(caller_uid.as_str(), component_uid);
        assert_eq!(callee_uid.as_str(), handler_uid);
        assert_eq!(synthesized_by.as_str(), "vue_event_handler");
        assert_eq!(call_kind.as_str(), "vue_event_handler");
    }

    #[test]
    fn vue_synthesis_no_dispatch_sites_returns_zero() {
        let (_tmp, db) = setup_test_db();
        let delta = compute_vue_template_synthesis(&db).unwrap();
        assert!(delta.insert_call_edges.is_empty());
        assert!(delta.insert_semantic_edges.is_empty());
    }

    // ── Interface dispatch synthesis tests ─────────────────────

    /// Insert a symbol with a container (for method symbols belonging to a class/interface).
    fn insert_symbol_with_container(
        db: &IndexDb,
        file_path: &str,
        name: &str,
        kind: &str,
        uid: &str,
        container: &str,
    ) {
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
             VALUES(?1, 'ts', 'abc', 0.0, 100, '2025-01-01')",
            rusqlite::params![file_path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols(symbol_id, file_path, name, kind, container, start_line, end_line, start_col, end_col, \
             parser_tier, parser_confidence, symbol_uid, is_default_export) \
             VALUES(?1, ?2, ?3, ?4, ?5, 1, 50, 0, 0, 'heuristic', 0.8, ?6, 0)",
            rusqlite::params![
                format!("sym:{}:{}:{}", file_path, container, name),
                file_path,
                name,
                kind,
                container,
                uid,
            ],
        )
        .unwrap();
    }

    /// Insert a semantic edge (e.g. implements).
    fn insert_semantic_edge(
        db: &IndexDb,
        edge_id: &str,
        file_path: &str,
        source_uid: &str,
        target_uid: &str,
        relation_kind: &str,
    ) {
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT INTO semantic_edges(edge_id, file_path, source_symbol, source_symbol_uid, \
             target_symbol, target_symbol_uid, relation_kind, line, confidence, parser_tier) \
             VALUES(?1, ?2, '', ?3, '', ?4, ?5, 1, 0.9, 'heuristic')",
            rusqlite::params![edge_id, file_path, source_uid, target_uid, relation_kind],
        )
        .unwrap();
    }

    /// Insert a call edge.
    fn insert_call_edge(
        db: &IndexDb,
        edge_id: &str,
        file_path: &str,
        caller_uid: &str,
        callee_uid: &str,
        callee_symbol: &str,
    ) {
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT OR REPLACE INTO call_edges(edge_id, file_path, callee_symbol, line, start_col, end_col, \
             caller_symbol_uid, callee_symbol_uid, dispatch_kind, call_kind, \
             resolution_kind, resolution_confidence, resolution_strategy, \
             is_optional_chain, is_awaited, is_constructor, parser_tier, parser_confidence) \
             VALUES(?1, ?2, ?3, 10, 0, 0, ?4, ?5, 'direct', 'method_call', \
             'exact', 0.9, 'test', 0, 0, 0, 'heuristic', 0.9)",
            rusqlite::params![edge_id, file_path, callee_symbol, caller_uid, callee_uid],
        )
        .unwrap();
    }

    #[test]
    fn interface_dispatch_one_implementor_produces_synthetic_edge() {
        let (_tmp, db) = setup_test_db();
        let config = SynthesisConfig::default();

        // Setup: interface IService with method `execute`,
        // class ServiceImpl implements IService with method `execute`,
        // and a caller that calls IService.execute.

        let iface_uid = "uid:IService";
        let impl_uid = "uid:ServiceImpl";
        let iface_method_uid = "uid:IService:execute";
        let impl_method_uid = "uid:ServiceImpl:execute";
        let caller_uid = "uid:Caller:run";

        // Insert interface symbol.
        insert_symbol(&db, "src/service.ts", "IService", "interface", iface_uid);
        // Insert interface method.
        insert_symbol_with_container(
            &db,
            "src/service.ts",
            "execute",
            "method",
            iface_method_uid,
            "IService",
        );
        // Insert implementor class.
        insert_symbol(&db, "src/impl.ts", "ServiceImpl", "class", impl_uid);
        // Insert implementor method.
        insert_symbol_with_container(
            &db,
            "src/impl.ts",
            "execute",
            "method",
            impl_method_uid,
            "ServiceImpl",
        );
        // Insert caller.
        insert_symbol(&db, "src/caller.ts", "run", "function", caller_uid);

        // Insert "implements" semantic edge: ServiceImpl implements IService.
        insert_semantic_edge(
            &db,
            "se:impl:1",
            "src/impl.ts",
            impl_uid,
            iface_uid,
            "implements",
        );

        // Insert call edge: caller → IService.execute.
        insert_call_edge(
            &db,
            "ce:1",
            "src/caller.ts",
            caller_uid,
            iface_method_uid,
            "execute",
        );

        // Run interface dispatch synthesis (no prior in-round deltas).
        let delta = compute_interface_dispatch_synthesis(&db, &config, &[]).unwrap();
        let count = delta.insert_call_edges.len();
        apply_one(&db, delta);
        assert_eq!(count, 1, "should produce 1 synthetic edge");

        // Verify the synthetic edge.
        let conn = db.reads().read_conn().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, caller_symbol_uid, callee_symbol_uid, synthesized_by, dispatch_kind \
                 FROM call_edges WHERE synthesized_by = 'interface_dispatch'",
            )
            .unwrap();
        let edges: Vec<(String, String, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(edges.len(), 1);
        let (edge_id, caller, callee, synth_by, dispatch) = &edges[0];
        assert!(edge_id.starts_with("synth:id:"));
        assert_eq!(caller.as_str(), caller_uid);
        assert_eq!(callee.as_str(), impl_method_uid);
        assert_eq!(synth_by.as_str(), "interface_dispatch");
        assert_eq!(dispatch.as_str(), "virtual_dispatch");
    }

    #[test]
    fn interface_dispatch_fanout_cap_skips_high_fanout() {
        let (_tmp, db) = setup_test_db();
        let config = SynthesisConfig {
            event_fanout_cap: 2, // Low cap for testing
            ..SynthesisConfig::default()
        };

        let iface_uid = "uid:IHandler";
        let iface_method_uid = "uid:IHandler:handle";
        let caller_uid = "uid:Caller:dispatch";

        // Insert interface.
        insert_symbol(&db, "src/handler.ts", "IHandler", "interface", iface_uid);
        insert_symbol_with_container(
            &db,
            "src/handler.ts",
            "handle",
            "method",
            iface_method_uid,
            "IHandler",
        );
        insert_symbol(&db, "src/caller.ts", "dispatch", "function", caller_uid);

        // Insert 3 implementors (exceeds fanout cap of 2).
        for i in 0..3 {
            let impl_name = format!("Handler{}", i);
            let impl_uid = format!("uid:{}", impl_name);
            let impl_method_uid = format!("uid:{}:handle", impl_name);

            insert_symbol(&db, "src/handlers.ts", &impl_name, "class", &impl_uid);
            insert_symbol_with_container(
                &db,
                "src/handlers.ts",
                "handle",
                "method",
                &impl_method_uid,
                &impl_name,
            );
            insert_semantic_edge(
                &db,
                &format!("se:impl:{}", i),
                "src/handlers.ts",
                &impl_uid,
                iface_uid,
                "implements",
            );
        }

        // Insert call edge.
        insert_call_edge(
            &db,
            "ce:fanout",
            "src/caller.ts",
            caller_uid,
            iface_method_uid,
            "handle",
        );

        // Run synthesis — should skip due to fanout > 2.
        let delta = compute_interface_dispatch_synthesis(&db, &config, &[]).unwrap();
        let count = delta.insert_call_edges.len();
        apply_one(&db, delta);
        assert_eq!(count, 0, "should skip due to fanout cap");

        // Verify no synthetic edges were created.
        let conn = db.reads().read_conn().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM call_edges WHERE synthesized_by = 'interface_dispatch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    // ── Unit-of-work atomicity and equivalence tests ────────────

    fn make_site(
        site_id: &str,
        file_path: &str,
        line: u32,
        site_kind: DispatchSiteKind,
        key: &str,
        enclosing_symbol_uid: Option<&str>,
        handler_expr: Option<&str>,
    ) -> DispatchSiteRecord {
        DispatchSiteRecord {
            site_id: site_id.to_string(),
            file_path: file_path.to_string(),
            line,
            col: 1,
            enclosing_symbol_uid: enclosing_symbol_uid.map(|s| s.to_string()),
            receiver_expr: None,
            site_kind,
            key: key.to_string(),
            handler_expr: handler_expr.map(|s| s.to_string()),
            handler_symbol_uid: None,
            confidence: 0.78,
        }
    }

    /// Fixture exercising every synthesis pass: event emitter, JSX, state
    /// setter, field observer, React re-render, Vue template, and interface
    /// dispatch all produce at least one edge.
    fn seed_multi_pass_fixture(db: &IndexDb) {
        // Event emitter: emit("userSaved") in emitSave → on("userSaved", handleSaved).
        insert_symbol(db, "src/events.ts", "emitSave", "function", "uid:emitSave");
        insert_symbol(
            db,
            "src/events.ts",
            "handleSaved",
            "function",
            "uid:handleSaved",
        );
        db.writes()
            .replace_dispatch_sites(
                "src/events.ts",
                &[
                    make_site(
                        "ds:emit:1",
                        "src/events.ts",
                        5,
                        DispatchSiteKind::EventEmit,
                        "userSaved",
                        Some("uid:emitSave"),
                        None,
                    ),
                    make_site(
                        "ds:on:1",
                        "src/events.ts",
                        20,
                        DispatchSiteKind::EventOn,
                        "userSaved",
                        None,
                        Some("handleSaved"),
                    ),
                ],
            )
            .unwrap();

        // JSX + state setter + re-render: App renders <Child/> and owns setOpen.
        insert_symbol(db, "src/App.tsx", "App", "component", "uid:App");
        insert_symbol(db, "src/Child.tsx", "Child", "component", "uid:Child");
        db.writes()
            .replace_dispatch_sites(
                "src/App.tsx",
                &[
                    make_site(
                        "ds:jsx:1",
                        "src/App.tsx",
                        8,
                        DispatchSiteKind::JsxTag,
                        "Child",
                        Some("uid:App"),
                        None,
                    ),
                    make_site(
                        "ds:ssb:1",
                        "src/App.tsx",
                        3,
                        DispatchSiteKind::StateSetterBinding,
                        "setOpen",
                        Some("uid:App"),
                        None,
                    ),
                    make_site(
                        "ds:ssc:1",
                        "src/App.tsx",
                        12,
                        DispatchSiteKind::StateSetterCall,
                        "setOpen",
                        Some("uid:App"),
                        None,
                    ),
                ],
            )
            .unwrap();

        // Field observer (name heuristic): class Bus with on()/emit() methods.
        insert_symbol(db, "src/bus.ts", "Bus", "class", "uid:Bus");
        insert_symbol_with_container(db, "src/bus.ts", "on", "method", "uid:Bus:on", "Bus");
        insert_symbol_with_container(db, "src/bus.ts", "emit", "method", "uid:Bus:emit", "Bus");

        // Vue template: <VChild/> plus @submit="submitForm".
        insert_symbol(db, "src/MyForm.vue", "MyForm", "component", "uid:MyForm");
        insert_symbol(
            db,
            "src/MyForm.vue",
            "submitForm",
            "function",
            "uid:submitForm",
        );
        insert_symbol(db, "src/VChild.vue", "VChild", "component", "uid:VChild");
        db.writes()
            .replace_dispatch_sites(
                "src/MyForm.vue",
                &[
                    make_site(
                        "ds:vuec:1",
                        "src/MyForm.vue",
                        4,
                        DispatchSiteKind::VueChildComponent,
                        "VChild",
                        Some("uid:MyForm"),
                        None,
                    ),
                    make_site(
                        "ds:vueh:1",
                        "src/MyForm.vue",
                        9,
                        DispatchSiteKind::VueEventHandler,
                        "submitForm",
                        Some("uid:MyForm"),
                        Some("submitForm"),
                    ),
                ],
            )
            .unwrap();

        // Interface dispatch: caller → IService.execute, ServiceImpl implements.
        insert_symbol(
            db,
            "src/service.ts",
            "IService",
            "interface",
            "uid:IService",
        );
        insert_symbol_with_container(
            db,
            "src/service.ts",
            "execute",
            "method",
            "uid:IService:execute",
            "IService",
        );
        insert_symbol(db, "src/impl.ts", "ServiceImpl", "class", "uid:ServiceImpl");
        insert_symbol_with_container(
            db,
            "src/impl.ts",
            "execute",
            "method",
            "uid:ServiceImpl:execute",
            "ServiceImpl",
        );
        insert_symbol(db, "src/caller.ts", "run", "function", "uid:Caller:run");
        insert_semantic_edge(
            db,
            "se:impl:fixture",
            "src/impl.ts",
            "uid:ServiceImpl",
            "uid:IService",
            "implements",
        );
        insert_call_edge(
            db,
            "ce:fixture",
            "src/caller.ts",
            "uid:Caller:run",
            "uid:IService:execute",
            "execute",
        );

        // Cross-pass dependency chain: the ONLY call edge targeting
        // IListener.handleEvent is the one synthesized by the event emitter
        // pass (emit("ping") → on("ping", handleEvent), where handleEvent
        // resolves to the interface method declared in the same file as the
        // on-site). The interface dispatch pass can therefore produce the
        // pingEmitter → ConcreteListener.handleEvent edge only if it observes
        // the event-emitter edge synthesized earlier in the same round — via
        // the in-memory prior-delta overlay in a single-round run, or via the
        // committed row in a per-pass-applied run.
        insert_symbol(
            db,
            "src/listener.ts",
            "IListener",
            "interface",
            "uid:IListener",
        );
        insert_symbol_with_container(
            db,
            "src/listener.ts",
            "handleEvent",
            "method",
            "uid:IListener:handleEvent",
            "IListener",
        );
        insert_symbol(
            db,
            "src/concrete.ts",
            "ConcreteListener",
            "class",
            "uid:ConcreteListener",
        );
        insert_symbol_with_container(
            db,
            "src/concrete.ts",
            "handleEvent",
            "method",
            "uid:ConcreteListener:handleEvent",
            "ConcreteListener",
        );
        insert_semantic_edge(
            db,
            "se:impl:listener",
            "src/concrete.ts",
            "uid:ConcreteListener",
            "uid:IListener",
            "implements",
        );
        insert_symbol(
            db,
            "src/ping.ts",
            "pingEmitter",
            "function",
            "uid:pingEmitter",
        );
        db.writes()
            .replace_dispatch_sites(
                "src/ping.ts",
                &[make_site(
                    "ds:emit:ping",
                    "src/ping.ts",
                    7,
                    DispatchSiteKind::EventEmit,
                    "ping",
                    Some("uid:pingEmitter"),
                    None,
                )],
            )
            .unwrap();
        db.writes()
            .replace_dispatch_sites(
                "src/listener.ts",
                &[make_site(
                    "ds:on:ping",
                    "src/listener.ts",
                    15,
                    DispatchSiteKind::EventOn,
                    "ping",
                    None,
                    Some("handleEvent"),
                )],
            )
            .unwrap();
    }

    /// Deterministic snapshot of all synthetic edges (call + semantic).
    fn synthetic_snapshot(db: &IndexDb) -> Vec<String> {
        let mut rows: Vec<String> = db
            .reads()
            .query_json(
                "SELECT edge_id, caller_symbol_uid, callee_symbol_uid, call_kind, synthesized_by \
                 FROM call_edges WHERE synthesized_by IS NOT NULL",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.to_string())
            .collect();
        rows.extend(
            db.reads()
                .query_json(
                    "SELECT edge_id, source_symbol_uid, target_symbol_uid, relation_kind \
                 FROM semantic_edges WHERE edge_id LIKE 'synth:%'",
                    &[],
                )
                .unwrap()
                .iter()
                .map(|row| row.to_string()),
        );
        rows.sort();
        rows
    }

    #[test]
    fn single_round_matches_per_pass_applied_synthesis() {
        let config = SynthesisConfig::default();

        // Reference run: each pass computed against committed state and
        // applied in its own round (so later passes read the committed rows
        // of earlier passes).
        let (_tmp_a, db_a) = setup_test_db();
        seed_multi_pass_fixture(&db_a);
        {
            let (delta, _) = compute_event_emitter_synthesis(&db_a, &config).unwrap();
            apply_one(&db_a, delta);
            apply_one(&db_a, compute_jsx_synthesis(&db_a).unwrap());
            apply_one(&db_a, compute_state_setter_synthesis(&db_a).unwrap());
            apply_one(
                &db_a,
                compute_field_observer_synthesis(&db_a, &config).unwrap(),
            );
            apply_one(
                &db_a,
                compute_react_rerender_chain_synthesis(&db_a).unwrap(),
            );
            apply_one(&db_a, compute_vue_template_synthesis(&db_a).unwrap());
            apply_one(
                &db_a,
                compute_interface_dispatch_synthesis(&db_a, &config, &[]).unwrap(),
            );
        }

        // Pipeline run: one compute round (interface dispatch sees earlier
        // passes only through the in-memory prior-delta overlay), one apply.
        let (_tmp_b, db_b) = setup_test_db();
        seed_multi_pass_fixture(&db_b);
        let round = compute_synthesis_round(&db_b, &config, true, true).unwrap();
        apply_synthesis_round(&db_b, &round).unwrap();

        let reference = synthetic_snapshot(&db_a);
        let pipeline = synthetic_snapshot(&db_b);
        assert_eq!(
            reference, pipeline,
            "a single compute round must produce the same synthetic edge set \
             as per-pass applied rounds"
        );

        // The fixture must exercise every pass, otherwise equality is vacuous.
        let joined = reference.join("\n");
        for marker in [
            "event_emitter",
            "react_state_setter",
            "field_observer",
            "react_rerender",
            "vue_event_handler",
            "interface_dispatch",
            "synth:jsx:",
            "synth:vue:",
        ] {
            assert!(
                joined.contains(marker),
                "fixture must produce edges for {marker}, got:\n{joined}"
            );
        }

        // Cross-pass visibility pin: this interface_dispatch edge is derived
        // exclusively from the event-emitter edge synthesized earlier in the
        // SAME round (no real call edge targets IListener.handleEvent). If
        // the interface pass ever stopped overlaying the prior in-round
        // deltas onto its committed-state read, this edge would silently
        // disappear.
        let chained = db_b
            .reads()
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges \
                 WHERE synthesized_by = 'interface_dispatch' \
                   AND caller_symbol_uid = 'uid:pingEmitter' \
                   AND callee_symbol_uid = 'uid:ConcreteListener:handleEvent'",
                &[],
            )
            .unwrap();
        assert_eq!(
            chained[0]["cnt"].as_i64(),
            Some(1),
            "interface dispatch must observe the event-emitter edge written \
             earlier in the same unit of work"
        );
    }

    /// Pin the `(dispatch_changed=false, interface_changed=true)` gate: an
    /// interface-only round must still see the COMMITTED call edges of the
    /// dispatch kinds (they are not regenerated this round, so they must not
    /// be excluded from its committed-state read). If the exclusion list ever
    /// wrongly covered the dispatch kinds unconditionally, the chained edge
    /// below would silently disappear when interface dispatch is recomputed
    /// alone.
    #[test]
    fn interface_only_round_reads_committed_dispatch_kind_edges() {
        let (_tmp, db) = setup_test_db();
        seed_multi_pass_fixture(&db);
        let config = SynthesisConfig::default();

        let chained_edge_count = |db: &IndexDb| {
            db.reads()
                .query_json(
                    "SELECT COUNT(*) AS cnt FROM call_edges \
                 WHERE synthesized_by = 'interface_dispatch' \
                   AND caller_symbol_uid = 'uid:pingEmitter' \
                   AND callee_symbol_uid = 'uid:ConcreteListener:handleEvent'",
                    &[],
                )
                .unwrap()[0]["cnt"]
                .as_i64()
        };

        // Round 1: full synthesis commits the event-emitter edge and the
        // interface edge chained from it.
        let round = compute_synthesis_round(&db, &config, true, true).unwrap();
        apply_synthesis_round(&db, &round).unwrap();
        assert_eq!(chained_edge_count(&db), Some(1));

        // Round 2: interface-only. Exactly one delta (the dispatch passes are
        // gated off), and the chained edge survives recomputation because the
        // committed event_emitter edge is visible to its read.
        let round = compute_synthesis_round(&db, &config, false, true).unwrap();
        assert_eq!(
            round.deltas.len(),
            1,
            "interface-only round must produce exactly the interface delta"
        );
        apply_synthesis_round(&db, &round).unwrap();
        assert_eq!(
            chained_edge_count(&db),
            Some(1),
            "interface-only recomputation must observe committed dispatch-kind \
             edges instead of excluding them"
        );
    }

    #[test]
    fn mid_compute_failure_leaves_database_and_write_lock_untouched() {
        let (_tmp, db) = setup_test_db();
        seed_multi_pass_fixture(&db);
        let config = SynthesisConfig::default();
        let generation_before = db.reads().generation().unwrap();

        let result: CcResult<()> = (|| {
            // Compute is write-free: the first passes produce in-memory
            // deltas only.
            let (delta, _) = compute_event_emitter_synthesis(&db, &config)?;
            let produced = !delta.insert_call_edges.is_empty();
            let jsx = compute_jsx_synthesis(&db)?;
            let produced = produced || !jsx.insert_semantic_edges.is_empty();
            assert!(produced, "fixture must make early passes produce edges");
            // Simulate a later pass failing: the error propagates before
            // apply ever runs (as in phase_postprocess).
            Err(cc_model::CcError::Database(
                "injected pass failure".to_string(),
            ))
        })();
        assert!(result.is_err());

        // Nothing was applied: compute never touches the database, so there
        // is no partial write to roll back and the write mutex was never
        // taken (a poisoned-lock failure mode no longer exists for compute).
        let call_rows = db
            .reads()
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges WHERE synthesized_by IS NOT NULL",
                &[],
            )
            .unwrap();
        assert_eq!(call_rows[0]["cnt"].as_i64(), Some(0));
        let semantic_rows = db
            .reads()
            .query_json(
                "SELECT COUNT(*) AS cnt FROM semantic_edges WHERE edge_id LIKE 'synth:%'",
                &[],
            )
            .unwrap();
        assert_eq!(semantic_rows[0]["cnt"].as_i64(), Some(0));

        // Signatures never advanced: phase_postprocess persists them only
        // after every pass (and the commit) succeeded, so the next run
        // re-executes synthesis.
        assert!(db
            .reads()
            .get_metadata("last_dispatch_sig")
            .unwrap()
            .is_none());
        assert!(db
            .reads()
            .get_metadata("last_interface_sig")
            .unwrap()
            .is_none());

        // The aborted unit of work did not bump the index epoch.
        let generation_after = db.reads().generation().unwrap();
        assert_eq!(generation_after.index_epoch, generation_before.index_epoch);
    }

    // ── Registry / owned-set declaration tests ──────────────────

    /// Pin the registry order and the owned edge-kind declarations. The
    /// pipeline drives the round from this order, and the disable-cleanup
    /// path in `phase_postprocess` derives its deletion set from these
    /// declarations — so this is the single lock on which synthetic edge
    /// kinds exist and in which order the passes run.
    #[test]
    fn registry_pins_pass_order_and_owned_edge_kinds() {
        let specs = registry();
        let ids: Vec<&str> = specs.iter().map(|spec| spec.id).collect();
        assert_eq!(
            ids,
            [
                "event_emitter",
                "jsx",
                "state_setter",
                "field_observer",
                "react_rerender",
                "vue_template",
                "interface_dispatch",
            ]
        );

        let owned_call_kinds: Vec<&str> = specs
            .iter()
            .flat_map(|spec| spec.owned_call_kinds.iter().copied())
            .collect();
        assert_eq!(
            owned_call_kinds,
            [
                "event_emitter",
                "react_state_setter",
                "field_observer",
                "react_rerender",
                "vue_event_handler",
                "interface_dispatch",
            ]
        );

        let owned_semantic_prefixes: Vec<&str> = specs
            .iter()
            .flat_map(|spec| spec.owned_semantic_prefixes.iter().copied())
            .collect();
        assert_eq!(owned_semantic_prefixes, ["synth:jsx:", "synth:vue:"]);

        // Interface dispatch must run last (its prior-delta overlay must see
        // every dispatch-pass call edge) and is the only interface-gated pass.
        let interface_gated: Vec<&str> = specs
            .iter()
            .filter(|spec| matches!(spec.gate, PassGate::Interface))
            .map(|spec| spec.id)
            .collect();
        assert_eq!(interface_gated, ["interface_dispatch"]);
        assert_eq!(specs.last().map(|spec| spec.id), Some("interface_dispatch"));
    }

    /// The disable-cleanup deletion set (the union of all declared owned
    /// kinds/prefixes) must cover every edge any pass actually synthesizes:
    /// deleting by the declared sets after a full round leaves no synthetic
    /// edge behind.
    #[test]
    fn declared_owned_sets_cover_every_synthesized_edge() {
        let (_tmp, db) = setup_test_db();
        seed_multi_pass_fixture(&db);
        let config = SynthesisConfig::default();

        let round = compute_synthesis_round(&db, &config, true, true).unwrap();
        apply_synthesis_round(&db, &round).unwrap();
        assert!(
            !synthetic_snapshot(&db).is_empty(),
            "fixture must synthesize edges"
        );

        for spec in registry() {
            for kind in spec.owned_call_kinds {
                db.writes().delete_synthetic_call_edges(kind).unwrap();
            }
            for prefix in spec.owned_semantic_prefixes {
                db.writes().delete_synthetic_semantic_edges(prefix).unwrap();
            }
        }
        assert!(
            synthetic_snapshot(&db).is_empty(),
            "owned kind/prefix declarations must cover every synthesized edge"
        );
    }
}
