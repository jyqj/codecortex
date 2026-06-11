//! Vue template synthesis pass: child components + event handlers.

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::DispatchSiteKind;
use cc_model::edge::{
    CallEdgeRecord, DispatchKind, ResolutionKind, SemanticEdgeRecord, SemanticRelation,
};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;
use crate::synthesis_symbol_resolver::{ResolutionScope, SynthesisSymbolResolver};

use super::{synth_edge_id, PassContext, PassGate, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "vue_template",
    gate: PassGate::Dispatch,
    owned_call_kinds: &["vue_event_handler"],
    owned_semantic_prefixes: &["synth:vue:"],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let delta = compute_vue_template_synthesis(ctx.db)?;
    let vue_edges = delta.insert_call_edges.len() + delta.insert_semantic_edges.len();
    if vue_edges > 0 {
        tracing::info!(edges = vue_edges, "Vue template synthesis complete");
    }
    Ok(delta)
}

/// Vue template synthesis: child components + event handlers.
///
/// - **VueChildComponent** dispatch sites (from `<ChildComponent />` in template)
///   are resolved to component definitions, producing `RendersComponent`
///   semantic edges (analogous to JSX synthesis).
/// - **VueEventHandler** dispatch sites (from `@click="handler"` in template)
///   are resolved to functions/methods in the same file, producing synthetic
///   call edges with `synthesized_by="vue_event_handler"`.
pub(crate) fn compute_vue_template_synthesis(db: &IndexDb) -> CcResult<EdgeDelta> {
    // 1. This pass replaces all synthetic Vue edges from previous runs.
    let mut delta = EdgeDelta {
        delete_call_kinds: vec!["vue_event_handler"],
        delete_semantic_prefixes: vec!["synth:vue:"],
        ..Default::default()
    };

    // 2. Load VueChildComponent dispatch sites.
    let child_sites =
        db.load_dispatch_sites_by_kind(DispatchSiteKind::VueChildComponent.as_str())?;

    // 3. Load VueEventHandler dispatch sites.
    let handler_sites =
        db.load_dispatch_sites_by_kind(DispatchSiteKind::VueEventHandler.as_str())?;

    if child_sites.is_empty() && handler_sites.is_empty() {
        return Ok(delta);
    }

    // ── Child component → RendersComponent semantic edges ──────
    if !child_sites.is_empty() {
        let component_kinds: &[&str] = &["function", "class", "component", "hook"];
        let child_names: Vec<&str> = child_sites.iter().map(|s| s.key.as_str()).collect();
        let resolver = SynthesisSymbolResolver::prefetch(db, &child_names, component_kinds)?;

        let mut semantic_edges: Vec<SemanticEdgeRecord> = Vec::new();
        for site in &child_sites {
            let source_uid = match &site.enclosing_symbol_uid {
                Some(uid) => uid.clone(),
                None => continue,
            };

            // Prefer same-file match, then unique global match.
            let target = resolver
                .resolve_lenient(site.key.as_str(), &site.file_path)
                .map(|(uid, scope)| match scope {
                    ResolutionScope::SameFile => (uid, 0.82),
                    ResolutionScope::UniqueGlobal => (uid, 0.75),
                });

            if let Some((target_uid, confidence)) = target {
                if target_uid.as_str() == source_uid.as_str() {
                    continue;
                }
                semantic_edges.push(SemanticEdgeRecord {
                    edge_id: synth_edge_id("vue", &site.site_id, &target_uid),
                    file_path: site.file_path.clone(),
                    source_symbol: String::new(),
                    source_symbol_uid: Some(source_uid.clone()),
                    target_symbol: site.key.clone(),
                    target_symbol_uid: Some(target_uid),
                    relation_kind: SemanticRelation::RendersComponent,
                    line: site.line,
                    confidence,
                    parser_tier: ParserTier::Heuristic,
                });
            }
        }

        delta.insert_semantic_edges = semantic_edges;
    }

    // ── Event handler → call edges ─────────────────────────────
    if !handler_sites.is_empty() {
        let handler_kinds: &[&str] = &["function", "method", "class", "hook", "component"];
        let handler_names: Vec<&str> = handler_sites
            .iter()
            .filter_map(|s| s.handler_expr.as_deref())
            .collect();
        let resolver = SynthesisSymbolResolver::prefetch(db, &handler_names, handler_kinds)?;
        let mut synthetic_edges: Vec<CallEdgeRecord> = Vec::new();

        for site in &handler_sites {
            let caller_uid = match &site.enclosing_symbol_uid {
                Some(uid) => uid.clone(),
                None => continue,
            };

            let handler_name = match &site.handler_expr {
                Some(name) => name.as_str(),
                None => continue,
            };

            // Resolve handler to a function/method in the same file first,
            // then fall back to unique global match.
            let target =
                resolver
                    .resolve_strict(handler_name, &site.file_path)
                    .map(|(uid, scope)| match scope {
                        ResolutionScope::SameFile => (uid, 0.80),
                        ResolutionScope::UniqueGlobal => (uid, 0.68),
                    });

            let (target_uid, confidence) = match target {
                Some(t) => t,
                None => continue,
            };

            // Skip self-reference.
            if target_uid == caller_uid {
                continue;
            }

            synthetic_edges.push(CallEdgeRecord {
                edge_id: synth_edge_id("vue", &site.site_id, &target_uid),
                file_path: site.file_path.clone(),
                caller_symbol: None,
                callee_symbol: handler_name.to_string(),
                line: site.line,
                start_col: site.col,
                caller_symbol_uid: Some(caller_uid),
                callee_symbol_uid: Some(target_uid),
                dispatch_kind: DispatchKind::EventEmitter,
                call_kind: "vue_event_handler".to_string(),
                resolution_kind: ResolutionKind::Heuristic,
                resolution_confidence: confidence,
                resolution_strategy: "vue_template_handler".to_string(),
                parser_tier: ParserTier::Heuristic,
                parser_confidence: confidence,
                synthesized_by: Some("vue_event_handler".to_string()),
                synthesis_key: Some(format!(
                    "{}:{}",
                    site.receiver_expr.as_deref().unwrap_or(""),
                    handler_name
                )),
                registered_file: Some(site.file_path.clone()),
                registered_line: Some(site.line),
                ..Default::default()
            });
        }

        delta.insert_call_edges = synthetic_edges;
    }

    Ok(delta)
}
