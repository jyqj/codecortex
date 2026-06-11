//! JSX component synthesis pass: matches `<Component />` usage → component
//! definition (produces `RendersComponent` semantic edges).

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::DispatchSiteKind;
use cc_model::edge::{SemanticEdgeRecord, SemanticRelation};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;
use crate::synthesis_symbol_resolver::{ResolutionScope, SynthesisSymbolResolver};

use super::{synth_edge_id, PassContext, PassGate, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "jsx",
    gate: PassGate::Dispatch,
    owned_call_kinds: &[],
    owned_semantic_prefixes: &["synth:jsx:"],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let delta = compute_jsx_synthesis(ctx.db)?;
    if !delta.insert_semantic_edges.is_empty() {
        tracing::info!(
            edges = delta.insert_semantic_edges.len(),
            "JSX component synthesis complete"
        );
    }
    Ok(delta)
}

/// Match `<Component />` JSX usage sites to component definitions and produce
/// `RendersComponent` semantic edges.
pub(crate) fn compute_jsx_synthesis(db: &IndexDb) -> CcResult<EdgeDelta> {
    // 1. This pass replaces all synthesized RendersComponent edges
    //    (prefixed with "synth:jsx").
    let mut delta = EdgeDelta {
        delete_semantic_prefixes: vec!["synth:jsx:"],
        ..Default::default()
    };

    // 2. Load all JsxTag dispatch sites.
    let jsx_sites = db.load_dispatch_sites_by_kind(DispatchSiteKind::JsxTag.as_str())?;
    if jsx_sites.is_empty() {
        return Ok(delta);
    }

    // 3-4. Batch-resolve unique component names (only function/class/component
    //      kinds) into an in-memory resolver.
    let component_kinds: &[&str] = &["function", "class", "component", "hook"];
    let jsx_names: Vec<&str> = jsx_sites.iter().map(|s| s.key.as_str()).collect();
    let resolver = SynthesisSymbolResolver::prefetch(db, &jsx_names, component_kinds)?;

    // 5. For each JsxTag site, try to find the target component.
    let mut semantic_edges: Vec<SemanticEdgeRecord> = Vec::new();
    for site in &jsx_sites {
        let source_uid = match &site.enclosing_symbol_uid {
            Some(uid) => uid.clone(),
            None => continue,
        };

        // Prefer same-file match, then a truly unique global match.
        // Do not pick the first of several global candidates: JSX component
        // names collide frequently across feature folders.
        let target = resolver
            .resolve_lenient(site.key.as_str(), &site.file_path)
            .map(|(uid, scope)| match scope {
                ResolutionScope::SameFile => (uid, 0.82),
                ResolutionScope::UniqueGlobal => (uid, 0.75),
            });

        if let Some((target_uid, confidence)) = target {
            // Skip self-references.
            if target_uid.as_str() == source_uid.as_str() {
                continue;
            }
            semantic_edges.push(SemanticEdgeRecord {
                edge_id: synth_edge_id("jsx", &site.site_id, &target_uid),
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
    Ok(delta)
}
