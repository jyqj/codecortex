//! Central dirty-reload policy.
//!
//! When a content-unchanged file is reloaded for re-resolution
//! (`FileAction::DirtyResolveOnly`), its stored edge data is rebuilt into a
//! [`ParseOutcome`]. Phase 4a resolution only fills in targets whose UID is
//! `None`, so any stale cross-file UID left in place would be kept verbatim.
//!
//! Each edge/reference category must therefore declare what happens to its
//! stored resolution state on dirty reload. This module is the single
//! declaration point for that invariant; [`apply_dirty_reload_policy`]
//! mechanically enforces it.
//!
//! Compile-time enforcement covers both dimensions of the reload surface:
//! the semantic-edge arm matches [`SemanticRelation`] exhaustively, so adding
//! a relation kind forces the author to declare its policy here; and
//! [`parse_outcome_from_reloaded_edges`] destructures
//! [`FileEdgesForReresolve`] completely (no `..`), so adding a reload data
//! field fails to compile at this module until the author declares its
//! [`ReloadedEdgeCategory`] policy and routes the field through it.

use cc_db::index_db::FileEdgesForReresolve;
use cc_model::edge::{ResolutionKind, SemanticRelation};
use cc_model::parse::ParseOutcome;

/// What happens to a category's stored resolution state on dirty reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirtyReloadPolicy {
    /// Stored target UIDs may be stale (the target's defining file changed),
    /// so all resolved targets of this category — same-file and cross-file
    /// alike — are cleared unconditionally for phase 4a re-resolution.
    ClearResolvedTargets,
    /// Edges of this category are regenerated from scratch each indexing run
    /// (synthesis overwrites them), so stale stored UIDs are never consumed
    /// and the loaded values are kept untouched.
    RegeneratedEachRun,
    /// Stored values are file-local (or resolved in-memory during synthesis)
    /// and remain valid verbatim for a content-unchanged file.
    KeepAsIs,
}

/// Edge/reference categories carried through a dirty reload. One variant per
/// field of [`FileEdgesForReresolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadedEdgeCategory {
    Symbols,
    Imports,
    CallEdges,
    SymbolRefs,
    RouteEdges,
    DispatchSites,
    SemanticEdge(SemanticRelation),
}

/// Single declaration point: the dirty-reload policy for each category.
pub(crate) fn dirty_reload_policy(category: ReloadedEdgeCategory) -> DirtyReloadPolicy {
    use DirtyReloadPolicy::*;
    match category {
        // Symbols and imports carry no cross-file resolution state to clear.
        ReloadedEdgeCategory::Symbols => KeepAsIs,
        ReloadedEdgeCategory::Imports => KeepAsIs,
        // Resolver-resolved targets: clear so phase 4a re-resolves.
        ReloadedEdgeCategory::CallEdges => ClearResolvedTargets,
        ReloadedEdgeCategory::SymbolRefs => ClearResolvedTargets,
        ReloadedEdgeCategory::RouteEdges => ClearResolvedTargets,
        // Dispatch sites carry only same-file enclosing UIDs in the DB
        // (handler UIDs are resolved in-memory during synthesis), so they can
        // be reused as-is for content-unchanged files.
        ReloadedEdgeCategory::DispatchSites => KeepAsIs,
        ReloadedEdgeCategory::SemanticEdge(relation) => match relation {
            // Hierarchy relations are regenerated each run by synthesis.
            SemanticRelation::Defines
            | SemanticRelation::DefinesMethod
            | SemanticRelation::ContainsFile
            | SemanticRelation::ContainsModule => RegeneratedEachRun,
            // Resolver-resolved relations point at symbols in other files
            // whose UIDs may have changed.
            SemanticRelation::Inherits
            | SemanticRelation::Implements
            | SemanticRelation::Decorates
            | SemanticRelation::Throws
            | SemanticRelation::UsesType
            | SemanticRelation::RendersComponent
            | SemanticRelation::Injects => ClearResolvedTargets,
            // Unrecognized kinds from the DB: clear conservatively.
            SemanticRelation::Unknown => ClearResolvedTargets,
        },
    }
}

fn should_clear(category: ReloadedEdgeCategory) -> bool {
    matches!(
        dirty_reload_policy(category),
        DirtyReloadPolicy::ClearResolvedTargets
    )
}

/// Convert reloaded edge data into a [`ParseOutcome`], clearing resolution
/// state wherever the policy demands it.
///
/// The destructuring below is intentionally complete (no `..`): adding a
/// field to [`FileEdgesForReresolve`] fails to compile here until its
/// dirty-reload policy is declared as a [`ReloadedEdgeCategory`] and the
/// field is routed through it.
pub(crate) fn parse_outcome_from_reloaded_edges(edges: FileEdgesForReresolve) -> ParseOutcome {
    let FileEdgesForReresolve {
        symbols,
        imports,
        mut call_edges,
        mut symbol_refs,
        mut semantic_edges,
        dispatch_sites,
        mut route_edges,
    } = edges;

    // KeepAsIs categories pass through verbatim; the asserts pin each field
    // to its declared policy.
    debug_assert!(!should_clear(ReloadedEdgeCategory::Symbols));
    debug_assert!(!should_clear(ReloadedEdgeCategory::Imports));
    debug_assert!(!should_clear(ReloadedEdgeCategory::DispatchSites));

    if should_clear(ReloadedEdgeCategory::CallEdges) {
        for edge in &mut call_edges {
            edge.callee_symbol_uid = None;
            edge.resolution_kind = ResolutionKind::Unresolved;
        }
    }
    if should_clear(ReloadedEdgeCategory::SymbolRefs) {
        for sym_ref in &mut symbol_refs {
            sym_ref.target_symbol_uid = None;
            sym_ref.target_symbol_id = None;
            sym_ref.resolution_kind = ResolutionKind::Unresolved;
        }
    }
    if should_clear(ReloadedEdgeCategory::RouteEdges) {
        for route in &mut route_edges {
            route.handler_symbol_id = None;
            route.handler_symbol_uid = None;
            route.resolution_strategy = None;
            route.resolution_confidence = None;
        }
    }
    for edge in &mut semantic_edges {
        if should_clear(ReloadedEdgeCategory::SemanticEdge(edge.relation_kind)) {
            edge.target_symbol_uid = None;
        }
    }

    ParseOutcome {
        symbols,
        imports,
        call_edges,
        symbol_refs,
        semantic_edges,
        dispatch_sites,
        route_edges,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_resolved_categories_clear_resolved_targets() {
        for category in [
            ReloadedEdgeCategory::CallEdges,
            ReloadedEdgeCategory::SymbolRefs,
            ReloadedEdgeCategory::RouteEdges,
        ] {
            assert_eq!(
                dirty_reload_policy(category),
                DirtyReloadPolicy::ClearResolvedTargets,
                "{category:?} must clear its resolved targets"
            );
        }
    }

    #[test]
    fn local_state_categories_are_kept_as_is() {
        for category in [
            ReloadedEdgeCategory::Symbols,
            ReloadedEdgeCategory::Imports,
            ReloadedEdgeCategory::DispatchSites,
        ] {
            assert_eq!(
                dirty_reload_policy(category),
                DirtyReloadPolicy::KeepAsIs,
                "{category:?} carries no cross-file resolution state"
            );
        }
    }

    #[test]
    fn hierarchy_relations_are_regenerated_each_run() {
        for relation in [
            SemanticRelation::Defines,
            SemanticRelation::DefinesMethod,
            SemanticRelation::ContainsFile,
            SemanticRelation::ContainsModule,
        ] {
            assert_eq!(
                dirty_reload_policy(ReloadedEdgeCategory::SemanticEdge(relation)),
                DirtyReloadPolicy::RegeneratedEachRun,
                "{relation:?} is regenerated each run and must not be cleared"
            );
        }
    }

    #[test]
    fn resolver_resolved_relations_clear_resolved_targets() {
        for relation in [
            SemanticRelation::Inherits,
            SemanticRelation::Implements,
            SemanticRelation::Decorates,
            SemanticRelation::Throws,
            SemanticRelation::UsesType,
            SemanticRelation::RendersComponent,
            SemanticRelation::Injects,
        ] {
            assert_eq!(
                dirty_reload_policy(ReloadedEdgeCategory::SemanticEdge(relation)),
                DirtyReloadPolicy::ClearResolvedTargets,
                "{relation:?} is resolver-resolved and must be cleared"
            );
        }
    }

    #[test]
    fn unknown_relation_clears_conservatively() {
        assert_eq!(
            dirty_reload_policy(ReloadedEdgeCategory::SemanticEdge(
                SemanticRelation::Unknown
            )),
            DirtyReloadPolicy::ClearResolvedTargets,
        );
    }

    #[test]
    fn conversion_clears_per_policy_and_keeps_the_rest() {
        use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
        use cc_model::edge::{CallEdgeRecord, RouteEdgeRecord, SemanticEdgeRecord};
        use cc_model::symbol::SymbolRefRecord;
        use cc_model::ParserTier;

        let semantic_edge = |relation: SemanticRelation| SemanticEdgeRecord {
            edge_id: String::new(),
            file_path: String::new(),
            source_symbol: String::new(),
            source_symbol_uid: Some("uSrc".into()),
            target_symbol: String::new(),
            target_symbol_uid: Some("uTarget".into()),
            relation_kind: relation,
            line: 1,
            confidence: 1.0,
            parser_tier: ParserTier::Generic,
        };

        let edges = FileEdgesForReresolve {
            symbols: Vec::new(),
            imports: Vec::new(),
            call_edges: vec![CallEdgeRecord {
                callee_symbol_uid: Some("uCallee".into()),
                resolution_kind: ResolutionKind::Exact,
                ..Default::default()
            }],
            symbol_refs: vec![SymbolRefRecord {
                ref_id: String::new(),
                file_path: String::new(),
                symbol_name: String::new(),
                container: None,
                ref_kind: String::new(),
                line: 1,
                column: 0,
                target_symbol_id: Some("idRef".into()),
                target_file_path: None,
                target_symbol_uid: Some("uRef".into()),
                ref_name: None,
                scope_id: None,
                resolution_kind: ResolutionKind::Exact,
                resolution_confidence: 1.0,
                resolution_strategy: String::new(),
                ref_end_line: None,
                ref_end_col: None,
                parser_tier: ParserTier::Generic,
                parser_confidence: 1.0,
            }],
            route_edges: vec![RouteEdgeRecord {
                edge_id: String::new(),
                file_path: String::new(),
                route_path: String::new(),
                handler_name: None,
                method: None,
                line: 1,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: Some("idHandler".into()),
                handler_symbol_uid: Some("uHandler".into()),
                handler_expr: None,
                router_symbol_uid: None,
                framework: None,
                route_kind: None,
                confidence: 1.0,
                parser_tier: ParserTier::Generic,
                resolution_strategy: Some("route_dotted".into()),
                resolution_confidence: Some(0.85),
            }],
            semantic_edges: vec![
                semantic_edge(SemanticRelation::Inherits),
                semantic_edge(SemanticRelation::Defines),
            ],
            dispatch_sites: vec![DispatchSiteRecord {
                site_id: String::new(),
                file_path: String::new(),
                line: 1,
                col: 0,
                enclosing_symbol_uid: Some("uEnclosing".into()),
                receiver_expr: None,
                site_kind: DispatchSiteKind::EventOn,
                key: String::new(),
                handler_expr: None,
                handler_symbol_uid: None,
                confidence: 1.0,
            }],
        };

        let outcome = parse_outcome_from_reloaded_edges(edges);

        let call = &outcome.call_edges[0];
        assert_eq!(call.callee_symbol_uid, None);
        assert_eq!(call.resolution_kind, ResolutionKind::Unresolved);

        let sym_ref = &outcome.symbol_refs[0];
        assert_eq!(sym_ref.target_symbol_uid, None);
        assert_eq!(sym_ref.target_symbol_id, None);
        assert_eq!(sym_ref.resolution_kind, ResolutionKind::Unresolved);

        let route = &outcome.route_edges[0];
        assert_eq!(route.handler_symbol_id, None);
        assert_eq!(route.handler_symbol_uid, None);
        assert_eq!(route.resolution_strategy, None);
        assert_eq!(route.resolution_confidence, None);

        assert_eq!(
            outcome.semantic_edges[0].target_symbol_uid, None,
            "resolver-resolved semantic edge must be cleared"
        );
        assert_eq!(
            outcome.semantic_edges[1].target_symbol_uid.as_deref(),
            Some("uTarget"),
            "hierarchy semantic edge must be kept"
        );

        assert_eq!(
            outcome.dispatch_sites[0].enclosing_symbol_uid.as_deref(),
            Some("uEnclosing"),
            "dispatch sites are kept as-is"
        );
    }
}
