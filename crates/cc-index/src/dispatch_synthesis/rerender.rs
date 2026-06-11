//! React re-render chain synthesis pass: extends state setter synthesis by
//! linking re-rendering components to their JSX child components.

use std::collections::{HashMap, HashSet};

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;
use crate::synthesis_symbol_resolver::SynthesisSymbolResolver;

use super::{synth_edge_id, PassContext, PassGate, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "react_rerender",
    gate: PassGate::Dispatch,
    owned_call_kinds: &["react_rerender"],
    owned_semantic_prefixes: &[],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let delta = compute_react_rerender_chain_synthesis(ctx.db)?;
    if !delta.insert_call_edges.is_empty() {
        tracing::info!(
            edges = delta.insert_call_edges.len(),
            "React re-render chain synthesis complete"
        );
    }
    Ok(delta)
}

/// Enhance state setter synthesis with re-render chain:
/// When a setState call triggers a re-render, also link to child components
/// rendered via JSX in the render/return body.
///
/// Chain: setState caller → component render → child component renders
///
/// This runs after both state setter synthesis and JSX synthesis, and creates
/// additional `react_rerender` edges for the render→child component links
/// that form the cascade path.
pub(crate) fn compute_react_rerender_chain_synthesis(db: &IndexDb) -> CcResult<EdgeDelta> {
    // 1. This pass replaces all react_rerender synthetic edges.
    let mut delta = EdgeDelta {
        delete_call_kinds: vec!["react_rerender"],
        ..Default::default()
    };

    // 2. Load existing state setter edges to find components that have setState triggers.
    //    These were created by run_state_setter_synthesis and point caller → component.
    //    The callee_symbol_uid is the component that re-renders.
    let all_sites = db.reads().load_all_dispatch_sites()?;

    // Collect class components with setState calls.
    let class_setter_sites: Vec<&DispatchSiteRecord> = all_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::StateSetterCall && s.key == "setState")
        .collect();

    // Collect functional component setter bindings.
    let func_setter_bindings: Vec<&DispatchSiteRecord> = all_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::StateSetterBinding)
        .collect();

    // Collect JSX tag sites — these tell us which components are rendered in which parent.
    let jsx_sites: Vec<&DispatchSiteRecord> = all_sites
        .iter()
        .filter(|s| s.site_kind == DispatchSiteKind::JsxTag)
        .collect();

    if jsx_sites.is_empty() {
        return Ok(delta);
    }

    // 3. Build a map: component_uid → JSX children rendered by that component.
    let mut component_children: HashMap<&str, Vec<&DispatchSiteRecord>> = HashMap::new();
    for site in &jsx_sites {
        if let Some(ref uid) = site.enclosing_symbol_uid {
            component_children
                .entry(uid.as_str())
                .or_default()
                .push(site);
        }
    }

    // 4. For class components with setState, find the render method and its JSX children.
    let component_kinds: &[&str] = &["function", "class", "component", "hook"];
    let mut synthetic_edges: Vec<CallEdgeRecord> = Vec::new();

    // Collect all component UIDs that have state setters.
    let mut rerendering_components: HashSet<String> = HashSet::new();

    for site in &class_setter_sites {
        if let Some(ref caller_uid) = site.enclosing_symbol_uid {
            // Find the render method in the same class.
            if let Ok(Some(render_uid)) = db.reads().find_method_in_same_class(caller_uid, "render")
            {
                rerendering_components.insert(render_uid);
            }
        }
    }

    // For functional components, the component itself is the "render".
    for binding in &func_setter_bindings {
        if let Some(ref uid) = binding.enclosing_symbol_uid {
            rerendering_components.insert(uid.clone());
        }
    }

    // 5. For each re-rendering component, find JSX children and resolve them.
    //    Batch-prefetch every child tag name of re-rendering components first.
    let child_names: Vec<&str> = rerendering_components
        .iter()
        .filter_map(|uid| component_children.get(uid.as_str()))
        .flat_map(|children| children.iter().map(|site| site.key.as_str()))
        .collect();
    let resolver = SynthesisSymbolResolver::prefetch(db, &child_names, component_kinds)?;

    for component_uid in &rerendering_components {
        let children = match component_children.get(component_uid.as_str()) {
            Some(c) => c,
            None => continue,
        };

        for child_jsx in children {
            // Resolve the JSX tag name to a component symbol.
            // Prefer same-file, then unique global.
            let child_name = &child_jsx.key;
            let child_uid = match resolver.resolve_strict(child_name, &child_jsx.file_path) {
                Some((uid, _)) => uid,
                None => continue,
            };

            // Skip self-reference.
            if child_uid == *component_uid {
                continue;
            }

            synthetic_edges.push(CallEdgeRecord {
                edge_id: synth_edge_id("rr", component_uid, &child_uid),
                file_path: child_jsx.file_path.clone(),
                caller_symbol: None,
                callee_symbol: child_name.clone(),
                line: child_jsx.line,
                start_col: child_jsx.col,
                caller_symbol_uid: Some(component_uid.clone()),
                callee_symbol_uid: Some(child_uid),
                dispatch_kind: DispatchKind::ReactiveBinding,
                call_kind: "react_rerender".to_string(),
                resolution_kind: ResolutionKind::Heuristic,
                resolution_confidence: 0.60,
                resolution_strategy: "rerender_jsx_child".to_string(),
                parser_tier: ParserTier::Heuristic,
                parser_confidence: 0.60,
                synthesized_by: Some("react_rerender".to_string()),
                synthesis_key: Some(format!("{}.{}", component_uid, child_name)),
                registered_file: Some(child_jsx.file_path.clone()),
                registered_line: Some(child_jsx.line),
                ..Default::default()
            });
        }
    }

    delta.insert_call_edges = synthetic_edges;
    Ok(delta)
}
