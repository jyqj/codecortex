//! Interface/abstract method dispatch synthesis pass: when a call targets an
//! interface method, synthesize edges to implementor methods.

use std::collections::{HashMap, HashSet};

use cc_db::index_db::IndexDb;
use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;

use super::{synth_edge_id, PassContext, PassGate, SynthesisConfig, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "interface_dispatch",
    gate: PassGate::Interface,
    owned_call_kinds: &["interface_dispatch"],
    owned_semantic_prefixes: &[],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let delta = compute_interface_dispatch_synthesis(ctx.db, ctx.config, ctx.prior_deltas)?;
    if !delta.insert_call_edges.is_empty() {
        tracing::info!(
            edges = delta.insert_call_edges.len(),
            "Interface dispatch synthesis complete"
        );
    }
    Ok(delta)
}

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
pub(crate) fn compute_interface_dispatch_synthesis(
    db: &IndexDb,
    config: &SynthesisConfig,
    prior_deltas: &[EdgeDelta],
) -> CcResult<EdgeDelta> {
    if !config.enabled {
        return Ok(EdgeDelta::default());
    }

    // 1. This pass replaces all interface_dispatch synthetic edges.
    let mut delta = EdgeDelta {
        delete_call_kinds: vec!["interface_dispatch"],
        ..Default::default()
    };

    // 2. Load all call edges where both UIDs are set. Committed rows of every
    //    `synthesized_by` kind regenerated this round are excluded (their
    //    replacement lives in `prior_deltas`, overlaid below) — including this
    //    pass's own previous edges. This reproduces the transaction-local view
    //    the passes had when they ran inside one unit of work.
    let mut excluded_kinds: Vec<&str> = vec!["interface_dispatch"];
    for prior in prior_deltas {
        excluded_kinds.extend(prior.delete_call_kinds.iter().copied());
    }
    let mut call_edge_rows = db
        .symbol_graph_reads()
        .dispatch_call_edges_excluding_synthesized(&excluded_kinds)?;
    // Overlay the call edges synthesized earlier in this round.
    for prior in prior_deltas {
        for edge in &prior.insert_call_edges {
            if let (Some(caller_uid), Some(callee_uid)) =
                (&edge.caller_symbol_uid, &edge.callee_symbol_uid)
            {
                call_edge_rows.push(cc_db::index_db::DispatchCallEdgeRow {
                    edge_id: edge.edge_id.clone(),
                    caller_uid: caller_uid.clone(),
                    callee_uid: callee_uid.clone(),
                    file_path: edge.file_path.clone(),
                    line: edge.line,
                });
            }
        }
    }
    if call_edge_rows.is_empty() {
        return Ok(delta);
    }

    // 3a. Load all symbols to build:
    //     - uid → (container, kind, name)
    //     - a set of interface/trait UIDs
    let symbol_rows = db.symbol_graph_reads().symbol_dispatch_rows()?;

    // Map: symbol_uid → (container_uid_or_name, kind, name)
    // The `container` column stores the container name (not UID), so we need an
    // extra step to resolve container name → container UID.
    let mut uid_to_info: HashMap<String, (Option<String>, String, String)> = HashMap::new();
    // Map: (name, kind) for quick lookup — name → uid for containers
    let mut name_to_container_uid: HashMap<String, Vec<String>> = HashMap::new();

    for row in symbol_rows {
        if row.symbol_uid.is_empty() {
            continue;
        }

        // Track all non-method symbols by name → UID for container resolution
        if row.kind != "method" {
            name_to_container_uid
                .entry(row.name.clone())
                .or_default()
                .push(row.symbol_uid.clone());
        }

        uid_to_info.insert(row.symbol_uid, (row.container, row.kind, row.name));
    }

    // 3b. Identify interface/trait UIDs.
    let interface_uids: HashSet<String> = uid_to_info
        .iter()
        .filter(|(_, (_, kind, _))| kind == "interface" || kind == "trait")
        .map(|(uid, _)| uid.clone())
        .collect();

    if interface_uids.is_empty() {
        return Ok(delta);
    }

    // 3c. Load implements edges: source = implementor, target = interface.
    let implements_rows = db
        .edge_reads()
        .semantic_uid_pairs_by_relation("implements")?;

    // Map: interface_uid → [implementor_uid, ...]
    let mut interface_to_implementors: HashMap<String, Vec<String>> = HashMap::new();
    for (impl_uid, iface_uid) in implements_rows {
        interface_to_implementors
            .entry(iface_uid)
            .or_default()
            .push(impl_uid);
    }

    if interface_to_implementors.is_empty() {
        return Ok(delta);
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
        let caller_uid = &row.caller_uid;
        let callee_uid = &row.callee_uid;
        let file_path = &row.file_path;
        let line = row.line;

        if caller_uid.is_empty() || callee_uid.is_empty() || row.edge_id.is_empty() {
            continue;
        }

        // Check if callee is a method on an interface/trait.
        let iface_uid = match method_uid_to_iface_uid.get(callee_uid) {
            Some(uid) => uid,
            None => continue,
        };

        // Get the method name of the callee.
        let method_name = match uid_to_info.get(callee_uid) {
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
                if impl_method_uid == callee_uid {
                    continue;
                }

                synthetic_edges.push(CallEdgeRecord {
                    edge_id: synth_edge_id("id", caller_uid, impl_method_uid),
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

    delta.insert_call_edges = synthetic_edges;
    Ok(delta)
}
