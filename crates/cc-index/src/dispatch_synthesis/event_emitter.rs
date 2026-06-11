//! Event-emitter synthesis pass: matches `emit(eventName, ...)` →
//! `on(eventName, handler)`.

use std::collections::HashMap;

use cc_db::index_db::IndexDb;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::{CcResult, ParserTier};

use crate::synthesis_pipeline::EdgeDelta;
use crate::synthesis_symbol_resolver::SynthesisSymbolResolver;

use super::{synth_edge_id, PassContext, PassGate, SynthesisConfig, SynthesisPassSpec};

pub(super) const SPEC: SynthesisPassSpec = SynthesisPassSpec {
    id: "event_emitter",
    gate: PassGate::Dispatch,
    owned_call_kinds: &["event_emitter"],
    owned_semantic_prefixes: &[],
    compute,
};

fn compute(ctx: &PassContext) -> CcResult<EdgeDelta> {
    let (delta, stats) = compute_event_emitter_synthesis(ctx.db, ctx.config)?;
    if stats.event_emitter_edges > 0 {
        tracing::info!(
            edges = stats.event_emitter_edges,
            skipped_generic = stats.skipped_generic,
            skipped_fanout = stats.skipped_fanout,
            "event emitter synthesis complete"
        );
    }
    Ok(delta)
}

/// Statistics returned by the event-emitter synthesis pass.
#[derive(Default)]
pub struct SynthesisStats {
    pub event_emitter_edges: usize,
    pub skipped_generic: usize,
    pub skipped_fanout: usize,
}

/// Compute event-emitter synthesis: match emit sites to on sites and produce
/// synthetic `CallEdgeRecord` entries.
pub(crate) fn compute_event_emitter_synthesis(
    db: &IndexDb,
    config: &SynthesisConfig,
) -> CcResult<(EdgeDelta, SynthesisStats)> {
    if !config.enabled {
        return Ok((EdgeDelta::default(), SynthesisStats::default()));
    }

    // 1. This pass replaces all synthetic event_emitter edges.
    let mut delta = EdgeDelta {
        delete_call_kinds: vec!["event_emitter"],
        ..Default::default()
    };

    // 2. Load all dispatch sites.
    let all_sites = db.reads().load_all_dispatch_sites()?;

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
    //     Look up handler_expr as a function/method name in the DB, preferring same
    //     file, then a truly unique global match. Avoid "first of a few" fallback:
    //     that produces plausible-looking but wrong synthetic call edges in common
    //     handler-name collisions.
    let handler_kinds: &[&str] = &["function", "method", "class", "hook", "component"];
    let lookup_names: Vec<&str> = on_sites
        .iter()
        .filter(|site| site.handler_symbol_uid.is_none())
        .filter_map(|site| site.handler_expr.as_deref())
        // Strip dotted prefix (e.g. "self.handle_ready" → "handle_ready")
        .map(|expr| expr.rsplit('.').next().unwrap_or(expr))
        .collect();
    // Lookup failure degrades to unresolved handlers (matching the previous
    // per-name `if let Ok` swallowing) instead of failing the pass.
    let handler_resolver =
        SynthesisSymbolResolver::prefetch(db, &lookup_names, handler_kinds).unwrap_or_default();
    let mut resolved_on_sites: Vec<DispatchSiteRecord> = Vec::new();
    for site in on_sites {
        let mut site = site.clone();
        if site.handler_symbol_uid.is_none() {
            if let Some(ref handler_name) = site.handler_expr {
                let lookup_name = handler_name.rsplit('.').next().unwrap_or(handler_name);
                site.handler_symbol_uid = handler_resolver
                    .resolve_strict(lookup_name, &site.file_path)
                    .map(|(uid, _)| uid);
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

    // 6. Collect all synthetic edges into the pass delta.
    let edge_count = synthetic_edges.len();
    delta.insert_call_edges = synthetic_edges;

    Ok((
        delta,
        SynthesisStats {
            event_emitter_edges: edge_count,
            skipped_generic,
            skipped_fanout,
        },
    ))
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
