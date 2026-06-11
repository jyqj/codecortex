//! Field-backed observer synthesis pass: detects registrar/dispatcher method
//! pairs within the same class (e.g. `on`/`emit`, `subscribe`/`notify`) and
//! creates edges from dispatcher to registrar targets.

use std::collections::{HashMap, HashSet};

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;

use super::{synth_edge_id, PassContext, PassGate, SynthesisConfig, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "field_observer",
    gate: PassGate::Dispatch,
    owned_call_kinds: &["field_observer"],
    owned_semantic_prefixes: &[],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let delta = compute_field_observer_synthesis(ctx.db, ctx.config)?;
    if !delta.insert_call_edges.is_empty() {
        tracing::info!(
            edges = delta.insert_call_edges.len(),
            "field observer synthesis complete"
        );
    }
    Ok(delta)
}

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
pub(crate) fn compute_field_observer_synthesis(
    db: &IndexDb,
    config: &SynthesisConfig,
) -> CcResult<EdgeDelta> {
    if !config.enabled {
        return Ok(EdgeDelta::default());
    }

    // 1. This pass replaces all field_observer synthetic edges.
    let mut delta = EdgeDelta {
        delete_call_kinds: vec!["field_observer"],
        ..Default::default()
    };

    // 2. Collect all registrar and dispatcher method names to query.
    //    We query for classes that contain methods matching either pattern.
    //    Since we can't do prefix matching in SQL efficiently, we query all
    //    methods and filter in Rust.

    // First, find all classes that have CallbackStore or CallbackInvoke dispatch sites.
    let store_sites = db
        .reads()
        .load_dispatch_sites_by_kind(DispatchSiteKind::CallbackStore.as_str())?;
    let invoke_sites = db
        .reads()
        .load_dispatch_sites_by_kind(DispatchSiteKind::CallbackInvoke.as_str())?;

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
    let registrar_classes = db
        .reads()
        .find_classes_with_method_names(&registrar_query_names)?;
    let dispatcher_classes = db
        .reads()
        .find_classes_with_method_names(&dispatcher_query_names)?;

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
    let methods_by_container = db.reads().find_methods_by_containers(&container_list)?;

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

    delta.insert_call_edges = synthetic_edges;
    Ok(delta)
}
