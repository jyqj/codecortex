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
//! Compile-time enforcement is limited to the semantic-relation dimension:
//! the semantic-edge arm matches [`SemanticRelation`] exhaustively, so adding
//! a relation kind forces the author to declare its policy here. Adding a
//! whole new reload category (a new edge field on `FileEdgesForReresolve` /
//! [`ParseOutcome`]) is NOT caught by the compiler — the new field would
//! silently bypass this policy until a [`ReloadedEdgeCategory`] variant and a
//! matching step in [`apply_dirty_reload_policy`] are added by hand.
//! Likewise, `symbols` and `imports` are written back by the reload but are
//! intentionally not modelled as categories: they carry no cross-file
//! resolution state to clear (implicitly `KeepAsIs`).

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

/// Edge/reference categories carried through a dirty reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadedEdgeCategory {
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

/// Apply the dirty-reload policy to a [`ParseOutcome`] rebuilt from stored
/// edge data, clearing resolution state wherever the policy demands it.
pub(crate) fn apply_dirty_reload_policy(outcome: &mut ParseOutcome) {
    if should_clear(ReloadedEdgeCategory::CallEdges) {
        for edge in &mut outcome.call_edges {
            edge.callee_symbol_uid = None;
            edge.resolution_kind = ResolutionKind::Unresolved;
        }
    }
    if should_clear(ReloadedEdgeCategory::SymbolRefs) {
        for sym_ref in &mut outcome.symbol_refs {
            sym_ref.target_symbol_uid = None;
            sym_ref.target_symbol_id = None;
            sym_ref.resolution_kind = ResolutionKind::Unresolved;
        }
    }
    if should_clear(ReloadedEdgeCategory::RouteEdges) {
        for route in &mut outcome.route_edges {
            route.handler_symbol_id = None;
            route.handler_symbol_uid = None;
        }
    }
    for edge in &mut outcome.semantic_edges {
        if should_clear(ReloadedEdgeCategory::SemanticEdge(edge.relation_kind)) {
            edge.target_symbol_uid = None;
        }
    }
    // DispatchSites: KeepAsIs — nothing to do.
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
    fn dispatch_sites_are_kept_as_is() {
        assert_eq!(
            dirty_reload_policy(ReloadedEdgeCategory::DispatchSites),
            DirtyReloadPolicy::KeepAsIs,
        );
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
    fn apply_clears_per_policy_and_keeps_the_rest() {
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

        let mut outcome = ParseOutcome {
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
            ..Default::default()
        };

        apply_dirty_reload_policy(&mut outcome);

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
