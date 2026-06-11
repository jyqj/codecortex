//! State setter synthesis pass: matches `setFoo(...)` / `this.setState(...)`
//! → re-render (produces synthetic call edges).

use std::collections::{HashMap, HashSet};

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::DispatchSiteKind;
use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;

use super::{synth_edge_id, PassContext, PassGate, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "state_setter",
    gate: PassGate::Dispatch,
    owned_call_kinds: &["react_state_setter"],
    owned_semantic_prefixes: &[],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let delta = compute_state_setter_synthesis(ctx.db)?;
    if !delta.insert_call_edges.is_empty() {
        tracing::info!(
            edges = delta.insert_call_edges.len(),
            "state setter synthesis complete"
        );
    }
    Ok(delta)
}

#[derive(Debug, Clone)]
struct StateSetterBindingTarget {
    component_uid: String,
    start_line: u32,
    end_line: u32,
}

impl StateSetterBindingTarget {
    fn contains_line(&self, line: u32) -> bool {
        self.start_line <= line && line <= self.end_line
    }

    fn line_span(&self) -> u32 {
        self.end_line.saturating_sub(self.start_line)
    }
}

/// Match `setFoo(...)` and `this.setState(...)` dispatch sites to component
/// re-render relationships, producing synthetic call edges.
///
/// Logic:
/// - `StateSetterBinding` sites record where `const [x, setX] = useState(...)` is declared.
/// - `StateSetterCall` sites record where `setX(...)` or `this.setState(...)` is actually called.
/// - For each `StateSetterCall`, find the matching `StateSetterBinding` in the same file
///   to determine which component owns the state. Create an edge from the caller to the component.
/// - For `this.setState`, find the class's render method (existing logic).
/// - If no binding is found for a setter call, skip (could be a non-React set* function).
pub(crate) fn compute_state_setter_synthesis(db: &IndexDb) -> CcResult<EdgeDelta> {
    // 1. This pass replaces all synthetic state setter edges.
    let mut delta = EdgeDelta {
        delete_call_kinds: vec!["react_state_setter"],
        ..Default::default()
    };

    // 2. Load StateSetterBinding sites — tells us which component owns which setter.
    let binding_sites = db
        .reads()
        .load_dispatch_sites_by_kind(DispatchSiteKind::StateSetterBinding.as_str())?;

    // 3. Load StateSetterCall sites — actual call sites.
    let call_sites = db
        .reads()
        .load_dispatch_sites_by_kind(DispatchSiteKind::StateSetterCall.as_str())?;

    if call_sites.is_empty() {
        return Ok(delta);
    }

    // 4. Build a map: (file_path, setter_name) → candidate owner components.
    //    We store all bindings so that when multiple components in the same file
    //    declare setters with the same name (e.g. two `setOpen`), we can prefer:
    //    1. exact same enclosing component UID;
    //    2. the narrowest component range containing the setter call line;
    //    3. a unique candidate only if there is no ambiguity.
    let binding_files: HashSet<&str> = binding_sites
        .iter()
        .map(|site| site.file_path.as_str())
        .collect();
    let mut component_ranges: HashMap<String, (u32, u32)> = HashMap::new();
    for file_path in binding_files {
        if let Ok(symbols) = db.reads().file_symbols(file_path) {
            for sym in symbols {
                if let Some(uid) = sym.symbol_uid {
                    component_ranges.insert(uid, (sym.start_line, sym.end_line));
                }
            }
        }
    }

    let mut binding_map: HashMap<(&str, &str), Vec<StateSetterBindingTarget>> = HashMap::new();
    for binding in &binding_sites {
        if let Some(ref uid) = binding.enclosing_symbol_uid {
            let (start_line, end_line) = component_ranges
                .get(uid)
                .copied()
                .unwrap_or((binding.line, binding.line));
            let end_line = end_line.max(start_line);
            binding_map
                .entry((binding.file_path.as_str(), binding.key.as_str()))
                .or_default()
                .push(StateSetterBindingTarget {
                    component_uid: uid.clone(),
                    start_line,
                    end_line,
                });
        }
    }

    let mut synthetic_edges: Vec<CallEdgeRecord> = Vec::new();

    for site in &call_sites {
        let caller_uid = match &site.enclosing_symbol_uid {
            Some(uid) => uid.clone(),
            None => continue,
        };

        if site.key == "setState" {
            // Class component: caller method → render method in same class.
            if let Ok(Some(render_uid)) =
                db.reads().find_method_in_same_class(&caller_uid, "render")
            {
                synthetic_edges.push(CallEdgeRecord {
                    edge_id: synth_edge_id("ss", &site.site_id, &render_uid),
                    file_path: site.file_path.clone(),
                    caller_symbol: None,
                    callee_symbol: "render".to_string(),
                    line: site.line,
                    start_col: site.col,
                    caller_symbol_uid: Some(caller_uid.clone()),
                    callee_symbol_uid: Some(render_uid.clone()),
                    dispatch_kind: DispatchKind::ReactiveBinding,
                    call_kind: "state_setter".to_string(),
                    resolution_kind: ResolutionKind::Heuristic,
                    resolution_confidence: 0.75,
                    resolution_strategy: "class_setState_render".to_string(),
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: 0.75,
                    synthesized_by: Some("react_state_setter".to_string()),
                    synthesis_key: Some(site.key.clone()),
                    registered_file: Some(site.file_path.clone()),
                    registered_line: Some(site.line),
                    ..Default::default()
                });
            }
        } else {
            // Functional component: find the binding to determine which component owns the setter.
            //
            // Calls often happen inside nested handlers (`const onClick = () => setOpen(true)`),
            // so the call site's enclosing_symbol_uid may be the nested handler, not the
            // component function.  In that case use the component source range from the symbol
            // table.  If multiple candidates remain and none contains the call line, skip instead
            // of falling back to an arbitrary first match.
            let candidates = binding_map.get(&(site.file_path.as_str(), site.key.as_str()));
            let component = match candidates {
                Some(targets) => {
                    if let Some(target) = targets
                        .iter()
                        .find(|target| target.component_uid.as_str() == caller_uid.as_str())
                    {
                        Some((
                            target.component_uid.clone(),
                            0.70,
                            "functional_useState_rerender_direct",
                        ))
                    } else if let Some(target) = targets
                        .iter()
                        .filter(|target| target.contains_line(site.line))
                        .min_by_key(|target| target.line_span())
                    {
                        Some((
                            target.component_uid.clone(),
                            0.68,
                            "functional_useState_rerender_range",
                        ))
                    } else if targets.len() == 1 {
                        targets.first().map(|target| {
                            (
                                target.component_uid.clone(),
                                0.62,
                                "functional_useState_rerender_unique",
                            )
                        })
                    } else {
                        None
                    }
                }
                None => None,
            };
            let (component_uid, confidence, strategy) = match component {
                Some(component) => component,
                None => continue, // No binding found — not a React setter, skip.
            };

            synthetic_edges.push(CallEdgeRecord {
                edge_id: synth_edge_id("ss", &site.site_id, &component_uid),
                file_path: site.file_path.clone(),
                caller_symbol: None,
                callee_symbol: site.key.clone(),
                line: site.line,
                start_col: site.col,
                caller_symbol_uid: Some(caller_uid.clone()),
                callee_symbol_uid: Some(component_uid),
                dispatch_kind: DispatchKind::ReactiveBinding,
                call_kind: "state_setter".to_string(),
                resolution_kind: ResolutionKind::Heuristic,
                resolution_confidence: confidence,
                resolution_strategy: strategy.to_string(),
                parser_tier: ParserTier::Heuristic,
                parser_confidence: confidence,
                synthesized_by: Some("react_state_setter".to_string()),
                synthesis_key: Some(site.key.clone()),
                registered_file: Some(site.file_path.clone()),
                registered_line: Some(site.line),
                ..Default::default()
            });
        }
    }

    delta.insert_call_edges = synthetic_edges;
    Ok(delta)
}
