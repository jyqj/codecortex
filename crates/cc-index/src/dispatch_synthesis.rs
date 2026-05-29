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
    pub field_observer_edges: usize,
    pub react_rerender_edges: usize,
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
            field_observer_edges: 0,
            react_rerender_edges: 0,
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
                matching_ons.to_vec()
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
        field_observer_edges: 0,
        react_rerender_edges: 0,
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

// ── Field-backed observer synthesis ──────────────────────────

/// Well-known registrar method name prefixes.
const REGISTRAR_PREFIXES: &[&str] = &[
    "on",
    "add",
    "subscribe",
    "register",
    "attach",
    "listen",
    "bind",
];

/// Well-known dispatcher method name prefixes.
const DISPATCHER_PREFIXES: &[&str] = &[
    "emit",
    "fire",
    "dispatch",
    "trigger",
    "notify",
    "publish",
    "broadcast",
    "send",
];

/// Returns true if `name` starts with any of the given prefixes (case-insensitive check
/// on the prefix, with the character after the prefix being uppercase or end-of-string
/// to avoid false positives like "addition" matching "add").
fn matches_method_prefix(name: &str, prefixes: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            let rest = &name[prefix.len()..];
            // Accept exact match ("on") or camelCase boundary ("onClick")
            if rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_uppercase() || c == '_') {
                return true;
            }
        }
    }
    false
}

/// Detect field-backed observer patterns: classes that have both registrar methods
/// (on/add/subscribe/...) and dispatcher methods (emit/fire/dispatch/...) are
/// likely observer/event-bus implementations. We create synthetic edges from
/// each dispatcher method to every registrar method's handler targets within
/// the same class.
///
/// The approach:
/// 1. Find all classes that have methods matching registrar OR dispatcher name patterns.
/// 2. For each such class, partition its methods into registrars and dispatchers.
/// 3. If a class has at least one registrar AND one dispatcher, pair them.
/// 4. For each dispatcher→registrar pair, create a synthetic call edge.
///
/// This complements the event-emitter synthesis (which matches by event name string)
/// by catching cases where the event bus pattern is used but event names aren't
/// statically detectable.
pub fn run_field_observer_synthesis(db: &IndexDb, config: &SynthesisConfig) -> CcResult<usize> {
    if !config.enabled {
        return Ok(0);
    }

    // 1. Delete old field_observer synthetic edges.
    db.delete_synthetic_call_edges("field_observer")?;

    // 2. Collect all registrar and dispatcher method names to query.
    //    We query for classes that contain methods matching either pattern.
    //    Since we can't do prefix matching in SQL efficiently, we query all
    //    methods and filter in Rust.

    // First, find all classes that have CallbackStore or CallbackInvoke dispatch sites.
    let store_sites = db.load_dispatch_sites_by_kind(DispatchSiteKind::CallbackStore.as_str())?;
    let invoke_sites = db.load_dispatch_sites_by_kind(DispatchSiteKind::CallbackInvoke.as_str())?;

    // Build a map of (class_uid → store_sites) and (class_uid → invoke_sites).
    let mut class_stores: HashMap<String, Vec<&DispatchSiteRecord>> = HashMap::new();
    let mut class_invokes: HashMap<String, Vec<&DispatchSiteRecord>> = HashMap::new();
    for site in &store_sites {
        if let Some(ref uid) = site.enclosing_symbol_uid {
            class_stores.entry(uid.clone()).or_default().push(site);
        }
    }
    for site in &invoke_sites {
        if let Some(ref uid) = site.enclosing_symbol_uid {
            class_invokes.entry(uid.clone()).or_default().push(site);
        }
    }

    let mut synthetic_edges: Vec<CallEdgeRecord> = Vec::new();

    // Strategy A: Use CallbackStore/CallbackInvoke dispatch sites (if available).
    // These are emitted by the parser when it detects `this.field.push(callback)` (store)
    // and `this.field.forEach(cb => cb())` (invoke) patterns.
    let store_class_uids: HashSet<&str> = class_stores.keys().map(|s| s.as_str()).collect();
    for (invoke_uid, invokes) in &class_invokes {
        if !store_class_uids.contains(invoke_uid.as_str()) {
            continue;
        }
        let stores = match class_stores.get(invoke_uid) {
            Some(s) => s,
            None => continue,
        };

        // Match by field name (the `key` field in dispatch sites).
        let mut store_by_key: HashMap<&str, Vec<&&DispatchSiteRecord>> = HashMap::new();
        for store in stores {
            store_by_key
                .entry(store.key.as_str())
                .or_default()
                .push(store);
        }

        for invoke in invokes {
            let matching_stores = match store_by_key.get(invoke.key.as_str()) {
                Some(s) => s,
                None => continue,
            };

            // Fanout cap.
            if matching_stores.len() > config.event_fanout_cap {
                continue;
            }

            for store in matching_stores {
                let handler_uid = match &store.handler_symbol_uid {
                    Some(uid) => uid.clone(),
                    None => continue,
                };

                synthetic_edges.push(CallEdgeRecord {
                    edge_id: synth_edge_id("fo", &invoke.site_id, &store.site_id),
                    file_path: invoke.file_path.clone(),
                    caller_symbol: None,
                    callee_symbol: store.handler_expr.clone().unwrap_or_default(),
                    line: invoke.line,
                    start_col: invoke.col,
                    caller_symbol_uid: invoke.enclosing_symbol_uid.clone(),
                    callee_symbol_uid: Some(handler_uid),
                    dispatch_kind: DispatchKind::FieldObserver,
                    call_kind: "field_observer".to_string(),
                    resolution_kind: ResolutionKind::Heuristic,
                    resolution_confidence: 0.65,
                    resolution_strategy: "callback_store_invoke_field".to_string(),
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: 0.65,
                    synthesized_by: Some("field_observer".to_string()),
                    synthesis_key: Some(format!("{}.{}", invoke_uid, invoke.key)),
                    registered_file: Some(store.file_path.clone()),
                    registered_line: Some(store.line),
                    ..Default::default()
                });
            }
        }
    }

    // Strategy B: Name-based heuristic — scan for classes with registrar + dispatcher
    // method names even without explicit CallbackStore/CallbackInvoke dispatch sites.
    // This catches patterns like:
    //   class EventBus { on(...) { ... }  emit(...) { ... } }
    // where the parser didn't emit specific dispatch site records.

    // Collect all candidate registrar/dispatcher method names for querying.
    let registrar_query_names: Vec<&str> = REGISTRAR_PREFIXES.to_vec();
    let dispatcher_query_names: Vec<&str> = DISPATCHER_PREFIXES.to_vec();

    // Find classes that have methods with registrar names.
    let registrar_classes = db.find_classes_with_method_names(&registrar_query_names)?;
    let dispatcher_classes = db.find_classes_with_method_names(&dispatcher_query_names)?;

    // Intersect: classes that have both registrar and dispatcher methods.
    let registrar_containers: HashSet<(&str, &str)> = registrar_classes
        .iter()
        .map(|(c, f)| (c.as_str(), f.as_str()))
        .collect();
    let candidate_containers: Vec<(&str, &str)> = dispatcher_classes
        .iter()
        .filter_map(|(c, f)| {
            if registrar_containers.contains(&(c.as_str(), f.as_str())) {
                Some((c.as_str(), f.as_str()))
            } else {
                None
            }
        })
        .collect();

    // Deduplicate — we only need unique containers.
    let unique_containers: HashSet<&str> = candidate_containers.iter().map(|(c, _)| *c).collect();

    // Track edges we've already created (from Strategy A) to avoid duplicates.
    let existing_edge_ids: HashSet<String> =
        synthetic_edges.iter().map(|e| e.edge_id.clone()).collect();

    // Fetch methods for all candidate containers in one batched query instead
    // of a per-container round-trip.
    let container_list: Vec<&str> = unique_containers.iter().copied().collect();
    let methods_by_container = db.find_methods_by_containers(&container_list)?;

    for container in unique_containers {
        let methods = match methods_by_container.get(container) {
            Some(m) if !m.is_empty() => m,
            _ => continue,
        };

        let mut registrars: Vec<(&str, &str, &str, u32)> = Vec::new(); // (uid, name, file, line)
        let mut dispatchers: Vec<(&str, &str, &str, u32)> = Vec::new();
        for (uid, name, file_path, line) in methods {
            if matches_method_prefix(name, REGISTRAR_PREFIXES) {
                registrars.push((uid.as_str(), name.as_str(), file_path.as_str(), *line));
            }
            if matches_method_prefix(name, DISPATCHER_PREFIXES) {
                dispatchers.push((uid.as_str(), name.as_str(), file_path.as_str(), *line));
            }
        }

        if registrars.is_empty() || dispatchers.is_empty() {
            continue;
        }

        // Fanout cap per dispatcher.
        if registrars.len() > config.event_fanout_cap {
            continue;
        }

        for (disp_uid, _disp_name, disp_file, disp_line) in &dispatchers {
            for (reg_uid, reg_name, reg_file, reg_line) in &registrars {
                // Skip self-reference.
                if disp_uid == reg_uid {
                    continue;
                }

                let edge_id = synth_edge_id("fo", disp_uid, reg_uid);
                if existing_edge_ids.contains(&edge_id) {
                    continue;
                }

                synthetic_edges.push(CallEdgeRecord {
                    edge_id,
                    file_path: disp_file.to_string(),
                    caller_symbol: None,
                    callee_symbol: reg_name.to_string(),
                    line: *disp_line,
                    start_col: 0,
                    caller_symbol_uid: Some(disp_uid.to_string()),
                    callee_symbol_uid: Some(reg_uid.to_string()),
                    dispatch_kind: DispatchKind::FieldObserver,
                    call_kind: "field_observer".to_string(),
                    resolution_kind: ResolutionKind::Heuristic,
                    resolution_confidence: 0.55,
                    resolution_strategy: "name_pattern_registrar_dispatcher".to_string(),
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: 0.55,
                    synthesized_by: Some("field_observer".to_string()),
                    synthesis_key: Some(format!("{}.{}", container, reg_name)),
                    registered_file: Some(reg_file.to_string()),
                    registered_line: Some(*reg_line),
                    ..Default::default()
                });
            }
        }
    }

    // 3. Batch insert.
    let count = synthetic_edges.len();
    if !synthetic_edges.is_empty() {
        db.insert_synthetic_call_edges(&synthetic_edges)?;
    }

    Ok(count)
}

// ── React re-render chain synthesis ─────────────────────────

/// Enhance state setter synthesis with re-render chain:
/// When a setState call triggers a re-render, also link to child components
/// rendered via JSX in the render/return body.
///
/// Chain: setState caller → component render → child component renders
///
/// This runs after both state setter synthesis and JSX synthesis, and creates
/// additional `react_rerender` edges for the render→child component links
/// that form the cascade path.
pub fn run_react_rerender_chain_synthesis(db: &IndexDb) -> CcResult<usize> {
    // 1. Delete old react_rerender synthetic edges.
    db.delete_synthetic_call_edges("react_rerender")?;

    // 2. Load existing state setter edges to find components that have setState triggers.
    //    These were created by run_state_setter_synthesis and point caller → component.
    //    The callee_symbol_uid is the component that re-renders.
    let all_sites = db.load_all_dispatch_sites()?;

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
        return Ok(0);
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
            if let Ok(Some(render_uid)) = db.find_method_in_same_class(caller_uid, "render") {
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
    for component_uid in &rerendering_components {
        let children = match component_children.get(component_uid.as_str()) {
            Some(c) => c,
            None => continue,
        };

        for child_jsx in children {
            // Resolve the JSX tag name to a component symbol.
            let child_name = &child_jsx.key;
            let matches = db.find_symbols_by_name_and_kinds(child_name, component_kinds)?;

            // Same resolution logic as JSX synthesis: prefer same-file, then unique global.
            let target =
                if let Some(m) = matches.iter().find(|s| s.file_path == child_jsx.file_path) {
                    m.symbol_uid.clone()
                } else if matches.len() == 1 {
                    matches[0].symbol_uid.clone()
                } else {
                    None
                };

            let child_uid = match target {
                Some(uid) => uid,
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

    // 6. Batch insert.
    let count = synthetic_edges.len();
    if !synthetic_edges.is_empty() {
        db.insert_synthetic_call_edges(&synthetic_edges)?;
    }

    Ok(count)
}

// ── Vue template synthesis ─────────────────────────────────

/// Vue template synthesis: child components + event handlers.
///
/// - **VueChildComponent** dispatch sites (from `<ChildComponent />` in template)
///   are resolved to component definitions, producing `RendersComponent`
///   semantic edges (analogous to JSX synthesis).
/// - **VueEventHandler** dispatch sites (from `@click="handler"` in template)
///   are resolved to functions/methods in the same file, producing synthetic
///   call edges with `synthesized_by="vue_event_handler"`.
pub fn run_vue_template_synthesis(db: &IndexDb) -> CcResult<usize> {
    // 1. Delete old synthetic edges from previous runs.
    db.delete_synthetic_semantic_edges("synth:vue:")?;
    db.delete_synthetic_call_edges("vue_event_handler")?;

    // 2. Load VueChildComponent dispatch sites.
    let child_sites =
        db.load_dispatch_sites_by_kind(DispatchSiteKind::VueChildComponent.as_str())?;

    // 3. Load VueEventHandler dispatch sites.
    let handler_sites =
        db.load_dispatch_sites_by_kind(DispatchSiteKind::VueEventHandler.as_str())?;

    if child_sites.is_empty() && handler_sites.is_empty() {
        return Ok(0);
    }

    let mut total = 0usize;

    // ── Child component → RendersComponent semantic edges ──────
    if !child_sites.is_empty() {
        let unique_names: HashSet<&str> = child_sites.iter().map(|s| s.key.as_str()).collect();
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

        let mut semantic_edges: Vec<SemanticEdgeRecord> = Vec::new();
        for site in &child_sites {
            let source_uid = match &site.enclosing_symbol_uid {
                Some(uid) => uid.clone(),
                None => continue,
            };

            let candidates = match name_to_uid.get(site.key.as_str()) {
                Some(c) => c,
                None => continue,
            };

            // Prefer same-file match, then unique global match.
            let target =
                if let Some(candidate) = candidates.iter().find(|(_, fp)| *fp == site.file_path) {
                    Some((candidate, 0.82))
                } else if candidates.len() == 1 {
                    Some((&candidates[0], 0.75))
                } else {
                    None
                };

            if let Some(((target_uid, _), confidence)) = target {
                if target_uid.as_str() == source_uid.as_str() {
                    continue;
                }
                semantic_edges.push(SemanticEdgeRecord {
                    edge_id: synth_edge_id("vue", &site.site_id, target_uid),
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

        let count = semantic_edges.len();
        if !semantic_edges.is_empty() {
            db.insert_semantic_edges_batch(&semantic_edges)?;
        }
        total += count;
    }

    // ── Event handler → call edges ─────────────────────────────
    if !handler_sites.is_empty() {
        let handler_kinds: &[&str] = &["function", "method", "class", "hook", "component"];
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
            let matches = db.find_symbols_by_name_and_kinds(handler_name, handler_kinds)?;
            let target = if let Some(m) = matches.iter().find(|s| s.file_path == site.file_path) {
                m.symbol_uid.clone().map(|uid| (uid, 0.80))
            } else if matches.len() == 1 {
                matches[0].symbol_uid.clone().map(|uid| (uid, 0.68))
            } else {
                None
            };

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

        let count = synthetic_edges.len();
        if !synthetic_edges.is_empty() {
            db.insert_synthetic_call_edges(&synthetic_edges)?;
        }
        total += count;
    }

    Ok(total)
}

// ── Interface/abstract method dispatch synthesis ──────────────

/// Interface/abstract method dispatch synthesis.
/// When a call targets an interface method, synthesize edges to implementor methods.
///
/// Algorithm:
/// 1. Delete old `interface_dispatch` synthetic edges.
/// 2. Load all call edges where both caller and callee UIDs are set.
/// 3. Build lookup structures:
///    - symbol_uid → (container_uid, kind, name) for methods
///    - symbol_uid → kind for all symbols (to identify interfaces/traits)
///    - interface_uid → [implementor_uid, ...] from `implements` semantic edges
///    - (class_uid, method_name) → method_uid for quick lookup
/// 4. For each call edge targeting an interface method, find implementor methods
///    with the same name and create synthetic edges.
pub fn run_interface_dispatch_synthesis(db: &IndexDb, config: &SynthesisConfig) -> CcResult<usize> {
    if !config.enabled {
        return Ok(0);
    }

    // 1. Delete old interface_dispatch synthetic edges.
    db.delete_synthetic_call_edges("interface_dispatch")?;

    // 2. Load all call edges where callee_symbol_uid is set.
    let call_edge_rows = db.query_json(
        "SELECT edge_id, caller_symbol_uid, callee_symbol_uid, callee_symbol, file_path, line \
         FROM call_edges \
         WHERE callee_symbol_uid IS NOT NULL AND caller_symbol_uid IS NOT NULL",
        &[],
    )?;
    if call_edge_rows.is_empty() {
        return Ok(0);
    }

    // 3a. Load all symbols to build:
    //     - uid → (container, kind, name)
    //     - a set of interface/trait UIDs
    let symbol_rows = db.query_json(
        "SELECT symbol_uid, name, kind, container \
         FROM symbols \
         WHERE symbol_uid IS NOT NULL",
        &[],
    )?;

    // Map: symbol_uid → (container_uid_or_name, kind, name)
    // The `container` column stores the container name (not UID), so we need an
    // extra step to resolve container name → container UID.
    let mut uid_to_info: HashMap<String, (Option<String>, String, String)> = HashMap::new();
    // Map: (name, kind) for quick lookup — name → uid for containers
    let mut name_to_container_uid: HashMap<String, Vec<String>> = HashMap::new();

    for row in &symbol_rows {
        let uid = row["symbol_uid"].as_str().unwrap_or_default().to_string();
        let name = row["name"].as_str().unwrap_or_default().to_string();
        let kind = row["kind"].as_str().unwrap_or_default().to_string();
        let container = row["container"].as_str().map(|s| s.to_string());

        if uid.is_empty() {
            continue;
        }

        // Track all non-method symbols by name → UID for container resolution
        if kind != "method" {
            name_to_container_uid
                .entry(name.clone())
                .or_default()
                .push(uid.clone());
        }

        uid_to_info.insert(uid, (container, kind, name));
    }

    // 3b. Identify interface/trait UIDs.
    let interface_uids: HashSet<String> = uid_to_info
        .iter()
        .filter(|(_, (_, kind, _))| kind == "interface" || kind == "trait")
        .map(|(uid, _)| uid.clone())
        .collect();

    if interface_uids.is_empty() {
        return Ok(0);
    }

    // 3c. Load implements edges: source = implementor, target = interface.
    let implements_rows = db.query_json(
        "SELECT source_symbol_uid, target_symbol_uid \
         FROM semantic_edges \
         WHERE relation_kind = 'implements' \
           AND source_symbol_uid IS NOT NULL \
           AND target_symbol_uid IS NOT NULL",
        &[],
    )?;

    // Map: interface_uid → [implementor_uid, ...]
    let mut interface_to_implementors: HashMap<String, Vec<String>> = HashMap::new();
    for row in &implements_rows {
        let impl_uid = row["source_symbol_uid"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let iface_uid = row["target_symbol_uid"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if !impl_uid.is_empty() && !iface_uid.is_empty() {
            interface_to_implementors
                .entry(iface_uid)
                .or_default()
                .push(impl_uid);
        }
    }

    if interface_to_implementors.is_empty() {
        return Ok(0);
    }

    // 3d. Build (container_name, method_name) → [method_uid, ...] for implementor method lookup.
    //     Also build container_uid → container_name reverse map.
    let mut container_method_map: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut uid_to_container_name: HashMap<String, String> = HashMap::new();
    for (uid, (container, kind, name)) in &uid_to_info {
        if kind == "method" {
            if let Some(ref cname) = container {
                container_method_map
                    .entry((cname.clone(), name.clone()))
                    .or_default()
                    .push(uid.clone());
            }
        }
        // For classes/interfaces/traits, map UID → name
        if kind == "class" || kind == "interface" || kind == "trait" || kind == "struct" {
            uid_to_container_name.insert(uid.clone(), name.clone());
        }
    }

    // 3e. Build a mapping from callee UID to its container UID (if that container is an interface).
    //     A method's container is stored by name, so we need to resolve container_name → UID.
    //     Since container names can collide, we restrict to known interface UIDs.
    let mut method_uid_to_iface_uid: HashMap<String, String> = HashMap::new();
    for (uid, (container, kind, _name)) in &uid_to_info {
        if kind != "method" {
            continue;
        }
        if let Some(cname) = container {
            // Find a container UID that is an interface/trait
            if let Some(candidates) = name_to_container_uid.get(cname.as_str()) {
                for candidate_uid in candidates {
                    if interface_uids.contains(candidate_uid) {
                        method_uid_to_iface_uid.insert(uid.clone(), candidate_uid.clone());
                        break;
                    }
                }
            }
        }
    }

    // 4. For each call edge, check if callee is an interface method → synthesize dispatch.
    let mut synthetic_edges: Vec<CallEdgeRecord> = Vec::new();

    for row in &call_edge_rows {
        let edge_id = row["edge_id"].as_str().unwrap_or_default();
        let caller_uid = row["caller_symbol_uid"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let callee_uid = row["callee_symbol_uid"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let file_path = row["file_path"].as_str().unwrap_or_default().to_string();
        let line = row["line"].as_i64().unwrap_or(0) as u32;

        if caller_uid.is_empty() || callee_uid.is_empty() || edge_id.is_empty() {
            continue;
        }

        // Check if callee is a method on an interface/trait.
        let iface_uid = match method_uid_to_iface_uid.get(&callee_uid) {
            Some(uid) => uid,
            None => continue,
        };

        // Get the method name of the callee.
        let method_name = match uid_to_info.get(&callee_uid) {
            Some((_, _, name)) => name.clone(),
            None => continue,
        };

        // Find all implementors of this interface.
        let implementors = match interface_to_implementors.get(iface_uid) {
            Some(impls) => impls,
            None => continue,
        };

        // Fanout cap: skip if too many implementors.
        if implementors.len() > config.event_fanout_cap {
            continue;
        }

        // For each implementor, find the method with the same name.
        for impl_uid in implementors {
            // Get the implementor class name.
            let impl_name = match uid_to_container_name.get(impl_uid) {
                Some(name) => name.clone(),
                None => continue,
            };

            // Look up (implementor_name, method_name) → method UID.
            let impl_method_uids =
                match container_method_map.get(&(impl_name.clone(), method_name.clone())) {
                    Some(uids) => uids,
                    None => continue,
                };

            for impl_method_uid in impl_method_uids {
                // Skip self-reference.
                if *impl_method_uid == callee_uid {
                    continue;
                }

                synthetic_edges.push(CallEdgeRecord {
                    edge_id: synth_edge_id("id", &caller_uid, impl_method_uid),
                    file_path: file_path.clone(),
                    caller_symbol: None,
                    callee_symbol: method_name.clone(),
                    line,
                    start_col: 0,
                    caller_symbol_uid: Some(caller_uid.clone()),
                    callee_symbol_uid: Some(impl_method_uid.clone()),
                    dispatch_kind: DispatchKind::VirtualDispatch,
                    call_kind: "interface_dispatch".to_string(),
                    resolution_kind: ResolutionKind::Heuristic,
                    resolution_confidence: 0.60,
                    resolution_strategy: "interface_method_dispatch".to_string(),
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: 0.60,
                    synthesized_by: Some("interface_dispatch".to_string()),
                    synthesis_key: Some(format!("{}::{}", iface_uid, method_name)),
                    registered_file: None,
                    registered_line: None,
                    ..Default::default()
                });
            }
        }
    }

    // 5. Deduplicate by edge_id (same caller→impl_method pair from multiple call sites).
    let mut seen: HashSet<String> = HashSet::new();
    synthetic_edges.retain(|e| seen.insert(e.edge_id.clone()));

    // 6. Batch insert.
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test_index.sqlite3");
        let db = IndexDb::open(&db_path).unwrap().0;
        (tmp, db)
    }

    /// Insert a symbol into the DB for resolution during synthesis.
    fn insert_symbol(db: &IndexDb, file_path: &str, name: &str, kind: &str, uid: &str) {
        let conn = db.read_conn().unwrap();
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
        db.replace_dispatch_sites("src/App.vue", &sites).unwrap();

        // Run synthesis.
        let count = run_vue_template_synthesis(&db).unwrap();
        assert_eq!(count, 1, "should produce 1 RendersComponent edge");

        // Verify the semantic edge.
        let edges = db
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
        db.replace_dispatch_sites("src/MyForm.vue", &sites).unwrap();

        // Run synthesis.
        let count = run_vue_template_synthesis(&db).unwrap();
        assert_eq!(count, 1, "should produce 1 call edge");

        // Verify the synthetic call edge via SQL.
        let conn = db.read_conn().unwrap();
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
        let count = run_vue_template_synthesis(&db).unwrap();
        assert_eq!(count, 0);
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
        let conn = db.read_conn().unwrap();
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
        let conn = db.read_conn().unwrap();
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
        let conn = db.read_conn().unwrap();
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

        // Run interface dispatch synthesis.
        let count = run_interface_dispatch_synthesis(&db, &config).unwrap();
        assert_eq!(count, 1, "should produce 1 synthetic edge");

        // Verify the synthetic edge.
        let conn = db.read_conn().unwrap();
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
        let count = run_interface_dispatch_synthesis(&db, &config).unwrap();
        assert_eq!(count, 0, "should skip due to fanout cap");

        // Verify no synthetic edges were created.
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM call_edges WHERE synthesized_by = 'interface_dispatch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
