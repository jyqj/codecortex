//! Dispatch synthesis passes.
//!
//! 1. **Event-emitter** – matches `emit(eventName, ...)` → `on(eventName, handler)`.
//! 2. **JSX component** – matches `<Component />` usage → component definition
//!    (produces `RendersComponent` semantic edges).
//! 3. **State setter** – matches `setFoo(...)` / `this.setState(...)` → re-render
//!    (produces synthetic call edges).

use std::collections::{HashMap, HashSet};

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{
    CallEdgeRecord, DispatchKind, ResolutionKind, SemanticEdgeRecord, SemanticRelation,
};
use cc_model::{CcResult, ParserTier};

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

/// Statistics returned after a synthesis pass.
pub struct SynthesisStats {
    pub event_emitter_edges: usize,
    pub skipped_generic: usize,
    pub skipped_fanout: usize,
    pub jsx_edges: usize,
    pub setter_edges: usize,
}

/// Run event-emitter synthesis: match emit sites to on sites and produce
/// synthetic `CallEdgeRecord` entries.
pub fn run_event_emitter_synthesis(
    db: &IndexDb,
    config: &SynthesisConfig,
) -> CcResult<SynthesisStats> {
    if !config.enabled {
        return Ok(SynthesisStats {
            event_emitter_edges: 0,
            skipped_generic: 0,
            skipped_fanout: 0,
            jsx_edges: 0,
            setter_edges: 0,
        });
    }

    // 1. Delete all existing synthetic event_emitter edges.
    db.delete_synthetic_call_edges("event_emitter")?;

    // 2. Load all dispatch sites.
    let all_sites = db.load_all_dispatch_sites()?;

    // 3. Partition into emit/on.
    let mut emit_sites: Vec<&DispatchSiteRecord> = Vec::new();
    let mut on_sites: Vec<&DispatchSiteRecord> = Vec::new();
    for site in &all_sites {
        match site.site_kind {
            DispatchSiteKind::EventEmit => emit_sites.push(site),
            DispatchSiteKind::EventOn => on_sites.push(site),
            _ => {}
        }
    }

    // 3b. Resolve handler_symbol_uid for on-sites that have handler_expr but no uid.
    //     Look up handler_expr as a function/method name in the DB, preferring same file.
    let handler_kinds: &[&str] = &["function", "method", "class", "hook", "component"];
    let mut resolved_on_sites: Vec<DispatchSiteRecord> = Vec::new();
    for site in on_sites {
        let mut site = site.clone();
        if site.handler_symbol_uid.is_none() {
            if let Some(ref handler_name) = site.handler_expr {
                // Strip dotted prefix (e.g. "self.handle_ready" → "handle_ready")
                let lookup_name = handler_name.rsplit('.').next().unwrap_or(handler_name);
                if let Ok(matches) = db.find_symbols_by_name_and_kinds(lookup_name, handler_kinds) {
                    // Prefer same-file match, then a truly unique global match.
                    // Avoid "first of a few" fallback: that produces plausible-looking
                    // but wrong synthetic call edges in common handler-name collisions.
                    let target = if matches.len() == 1 {
                        matches[0].symbol_uid.clone()
                    } else {
                        matches
                            .iter()
                            .find(|s| s.file_path == site.file_path)
                            .and_then(|s| s.symbol_uid.clone())
                    };
                    site.handler_symbol_uid = target;
                }
            }
        }
        resolved_on_sites.push(site);
    }

    // 4. Build maps keyed by event name (the `key` field).
    let mut emit_map: HashMap<&str, Vec<&DispatchSiteRecord>> = HashMap::new();
    for site in &emit_sites {
        emit_map.entry(site.key.as_str()).or_default().push(site);
    }

    let mut on_map: HashMap<&str, Vec<&DispatchSiteRecord>> = HashMap::new();
    for site in &resolved_on_sites {
        on_map.entry(site.key.as_str()).or_default().push(site);
    }

    let mut synthetic_edges: Vec<CallEdgeRecord> = Vec::new();
    let mut skipped_generic: usize = 0;
    let mut skipped_fanout: usize = 0;

    // 5. For each event name that has emitters, try to match on-sites.
    for (event_name, emitters) in &emit_map {
        let matching_ons = match on_map.get(event_name) {
            Some(ons) => ons,
            None => continue,
        };

        let is_generic = config.generic_event_denylist.contains(*event_name);

        // Three-tier matching for each emitter:
        // 1. same receiver_expr + event_name;
        // 2. same file + event_name;
        // 3. global event_name (non-generic events only).
        //
        // Applying fanout after this narrowing avoids dropping useful
        // receiver-exact edges just because a generic event name has many
        // registrations elsewhere in the repo.
        for emit in emitters {
            let receiver_exact: Vec<&DispatchSiteRecord> = match &emit.receiver_expr {
                Some(recv) => matching_ons
                    .iter()
                    .copied()
                    .filter(|on| on.receiver_expr.as_deref() == Some(recv.as_str()))
                    .collect(),
                None => Vec::new(),
            };

            let same_file: Vec<&DispatchSiteRecord> = matching_ons
                .iter()
                .copied()
                .filter(|on| on.file_path == emit.file_path)
                .collect();

            let mut candidate_ons: Vec<&DispatchSiteRecord> = if !receiver_exact.is_empty() {
                receiver_exact
            } else if !same_file.is_empty() {
                same_file
            } else if is_generic {
                skipped_generic += 1;
                continue;
            } else {
                matching_ons.iter().copied().collect()
            };

            // Skip unresolved handlers before fanout accounting; unresolved
            // registrations should not suppress valid resolved candidates.
            candidate_ons.retain(|on| on.handler_symbol_uid.is_some());
            if candidate_ons.is_empty() {
                continue;
            }

            // Fanout cap: if too many narrowed on-sites remain, skip this emit
            // to avoid edge explosion.
            if candidate_ons.len() > config.event_fanout_cap {
                skipped_fanout += 1;
                continue;
            }

            for on in candidate_ons {
                let confidence = compute_confidence(emit, on);

                // For generic events, only allow receiver-exact or same-file matches.
                if is_generic && confidence < 0.65 {
                    continue;
                }

                synthetic_edges.push(make_synthetic_edge(emit, on, confidence));
            }
        }
    }

    // 6. Batch insert all synthetic edges.
    let edge_count = synthetic_edges.len();
    if !synthetic_edges.is_empty() {
        db.insert_synthetic_call_edges(&synthetic_edges)?;
    }

    Ok(SynthesisStats {
        event_emitter_edges: edge_count,
        skipped_generic,
        skipped_fanout,
        jsx_edges: 0,
        setter_edges: 0,
    })
}

/// Compute confidence based on the tier of match:
///   a. Same receiver_expr + event_name → 0.75
///   b. Same file + event_name → 0.65
///   c. Global event_name match → 0.50
fn compute_confidence(emit: &DispatchSiteRecord, on: &DispatchSiteRecord) -> f64 {
    // Tier A: receiver expression exact match
    if let (Some(ref emit_recv), Some(ref on_recv)) = (&emit.receiver_expr, &on.receiver_expr) {
        if emit_recv == on_recv {
            return 0.75;
        }
    }

    // Tier B: same file
    if emit.file_path == on.file_path {
        return 0.65;
    }

    // Tier C: global match
    0.50
}

/// Produce a synthetic `CallEdgeRecord` linking an emit site to an on handler.
fn make_synthetic_edge(
    emit: &DispatchSiteRecord,
    on: &DispatchSiteRecord,
    confidence: f64,
) -> CallEdgeRecord {
    CallEdgeRecord {
        edge_id: synth_edge_id("ee", &emit.site_id, &on.site_id),
        file_path: emit.file_path.clone(),
        caller_symbol: None,
        callee_symbol: on.handler_expr.clone().unwrap_or_default(),
        line: emit.line,
        start_col: emit.col,
        caller_symbol_uid: emit.enclosing_symbol_uid.clone(),
        // ONLY use handler_symbol_uid — do NOT fallback to enclosing.
        // If handler is unresolved, the caller should skip creating this edge.
        callee_symbol_uid: on.handler_symbol_uid.clone(),
        dispatch_kind: DispatchKind::EventEmitter,
        call_kind: "event_emitter".to_string(),
        resolution_kind: ResolutionKind::Heuristic,
        resolution_confidence: confidence,
        resolution_strategy: "event_name_match".to_string(),
        parser_tier: ParserTier::Heuristic,
        parser_confidence: confidence,
        synthesized_by: Some("event_emitter".to_string()),
        synthesis_key: Some(on.key.clone()),
        registered_file: Some(on.file_path.clone()),
        registered_line: Some(on.line),
        ..Default::default()
    }
}

// ── JSX component synthesis ────────────────────────────────────

/// Match `<Component />` JSX usage sites to component definitions and produce
/// `RendersComponent` semantic edges.
pub fn run_jsx_synthesis(db: &IndexDb) -> CcResult<usize> {
    // 1. Delete old synthesized RendersComponent edges (prefixed with "synth:jsx").
    db.delete_synthetic_semantic_edges("synth:jsx:")?;

    // 2. Load all JsxTag dispatch sites.
    let jsx_sites = db.load_dispatch_sites_by_kind(DispatchSiteKind::JsxTag.as_str())?;
    if jsx_sites.is_empty() {
        return Ok(0);
    }

    // 3. Collect unique component names to batch-query.
    let unique_names: HashSet<&str> = jsx_sites.iter().map(|s| s.key.as_str()).collect();

    // 4. Build name → symbol_uid map (only function/class/component kinds).
    let component_kinds: &[&str] = &["function", "class", "component", "hook"];
    let mut name_to_uid: HashMap<&str, Vec<(String, String)>> = HashMap::new();
    for name in &unique_names {
        let matches = db.find_symbols_by_name_and_kinds(name, component_kinds)?;
        let entries: Vec<(String, String)> = matches
            .into_iter()
            .filter_map(|s| s.symbol_uid.map(|uid| (uid, s.file_path)))
            .collect();
        if !entries.is_empty() {
            name_to_uid.insert(name, entries);
        }
    }

    // 5. For each JsxTag site, try to find the target component.
    let mut semantic_edges: Vec<SemanticEdgeRecord> = Vec::new();
    for site in &jsx_sites {
        let source_uid = match &site.enclosing_symbol_uid {
            Some(uid) => uid.clone(),
            None => continue,
        };

        let candidates = match name_to_uid.get(site.key.as_str()) {
            Some(c) => c,
            None => continue,
        };

        // Prefer same-file match, then a truly unique global match.
        // Do not pick the first of several global candidates: JSX component
        // names collide frequently across feature folders.
        let target =
            if let Some(candidate) = candidates.iter().find(|(_, fp)| *fp == site.file_path) {
                Some((candidate, 0.82))
            } else if candidates.len() == 1 {
                Some((&candidates[0], 0.75))
            } else {
                None
            };

        if let Some(((target_uid, _target_file), confidence)) = target {
            // Skip self-references.
            if target_uid.as_str() == source_uid.as_str() {
                continue;
            }
            semantic_edges.push(SemanticEdgeRecord {
                edge_id: synth_edge_id("jsx", &site.site_id, target_uid),
                file_path: site.file_path.clone(),
                source_symbol: String::new(),
                source_symbol_uid: Some(source_uid.clone()),
                target_symbol: site.key.clone(),
                target_symbol_uid: Some(target_uid.clone()),
                relation_kind: SemanticRelation::RendersComponent,
                line: site.line,
                confidence,
                parser_tier: ParserTier::Heuristic,
            });
        }
    }

    // 6. Batch insert.
    let count = semantic_edges.len();
    if !semantic_edges.is_empty() {
        db.insert_semantic_edges_batch(&semantic_edges)?;
    }

    Ok(count)
}

// ── State setter synthesis ────────────────────────────────────

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
pub fn run_state_setter_synthesis(db: &IndexDb) -> CcResult<usize> {
    // 1. Delete old synthetic state setter edges.
    db.delete_synthetic_call_edges("react_state_setter")?;

    // 2. Load StateSetterBinding sites — tells us which component owns which setter.
    let binding_sites =
        db.load_dispatch_sites_by_kind(DispatchSiteKind::StateSetterBinding.as_str())?;

    // 3. Load StateSetterCall sites — actual call sites.
    let call_sites = db.load_dispatch_sites_by_kind(DispatchSiteKind::StateSetterCall.as_str())?;

    if call_sites.is_empty() {
        return Ok(0);
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
        if let Ok(symbols) = db.file_symbols(file_path) {
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
            if let Ok(Some(render_uid)) = db.find_method_in_same_class(&caller_uid, "render") {
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

    // 5. Batch insert.
    let count = synthetic_edges.len();
    if !synthetic_edges.is_empty() {
        db.insert_synthetic_call_edges(&synthetic_edges)?;
    }

    Ok(count)
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
