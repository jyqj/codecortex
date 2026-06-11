//! Synthesis pipeline: compute/apply separation for dispatch synthesis.
//!
//! This module owns the pass order and the cross-pass data flow of dispatch
//! synthesis. Each pass is a compute-only function (`dispatch_synthesis::
//! compute_*`) that reads committed index state through the read pool and
//! returns an [`EdgeDelta`] — it never touches the write connection. The
//! single cross-pass dependency (interface dispatch consumes call edges
//! synthesized earlier in the same round) is threaded explicitly: the
//! interface pass receives the prior deltas and overlays their in-memory
//! call edges onto its committed-state read, excluding the committed rows of
//! every `synthesized_by` kind regenerated this round.
//!
//! Boundary: the overlay covers CALL edges only. Passes today synthesize
//! semantic edges solely with the `RendersComponent` relation, which the
//! interface pass (reading `implements` rows) never consumes — so its
//! committed-state semantic read is round-equivalent. If a future pass ever
//! synthesizes `implements` semantic edges, the interface pass will not see
//! them in-round until the overlay is extended to semantic edges.
//!
//! [`apply_synthesis_round`] then applies all deltas in pass order inside one
//! [`UnitOfWork`]: the write lock and the `IMMEDIATE` transaction cover only
//! this batch write, not the pass computation. A panic or error during
//! compute leaves the database and the write mutex untouched.
//!
//! Concurrency note: compute reads the committed snapshot; a writer in
//! another process could commit between compute and apply, in which case the
//! applied edges reflect the pre-write snapshot. The synthesis signature gate
//! in `phase_postprocess` self-heals this on the next run (the persisted
//! signature will not match the new inputs). In-process, indexing runs are
//! already serialized by the engine, so compute/apply see a stable snapshot.

use cc_db::index_db::IndexDb;
use cc_model::edge::{CallEdgeRecord, SemanticEdgeRecord};
use cc_model::CcResult;

use crate::dispatch_synthesis::{
    compute_event_emitter_synthesis, compute_field_observer_synthesis,
    compute_interface_dispatch_synthesis, compute_jsx_synthesis,
    compute_react_rerender_chain_synthesis, compute_state_setter_synthesis,
    compute_vue_template_synthesis, SynthesisConfig,
};

/// The write set of one synthesis pass: which previously-synthesized edges it
/// replaces, and the edges it produces.
#[derive(Default)]
pub(crate) struct EdgeDelta {
    /// `synthesized_by` kinds whose call edges this pass replaces.
    pub(crate) delete_call_kinds: Vec<&'static str>,
    /// `edge_id` prefixes whose semantic edges this pass replaces.
    pub(crate) delete_semantic_prefixes: Vec<&'static str>,
    pub(crate) insert_call_edges: Vec<CallEdgeRecord>,
    pub(crate) insert_semantic_edges: Vec<SemanticEdgeRecord>,
}

/// All deltas of one synthesis round, in pass order.
pub(crate) struct SynthesisRound {
    pub(crate) deltas: Vec<EdgeDelta>,
}

/// Run the synthesis passes (gated like `phase_postprocess`) and collect
/// their deltas. Pure compute: no write lock, no transaction.
pub(crate) fn compute_synthesis_round(
    db: &IndexDb,
    config: &SynthesisConfig,
    dispatch_changed: bool,
    interface_changed: bool,
) -> CcResult<SynthesisRound> {
    let mut deltas: Vec<EdgeDelta> = Vec::new();

    if dispatch_changed {
        let (delta, stats) = compute_event_emitter_synthesis(db, config)?;
        if stats.event_emitter_edges > 0 {
            tracing::info!(
                edges = stats.event_emitter_edges,
                skipped_generic = stats.skipped_generic,
                skipped_fanout = stats.skipped_fanout,
                "event emitter synthesis complete"
            );
        }
        deltas.push(delta);

        let delta = compute_jsx_synthesis(db)?;
        if !delta.insert_semantic_edges.is_empty() {
            tracing::info!(
                edges = delta.insert_semantic_edges.len(),
                "JSX component synthesis complete"
            );
        }
        deltas.push(delta);

        let delta = compute_state_setter_synthesis(db)?;
        if !delta.insert_call_edges.is_empty() {
            tracing::info!(
                edges = delta.insert_call_edges.len(),
                "state setter synthesis complete"
            );
        }
        deltas.push(delta);

        let delta = compute_field_observer_synthesis(db, config)?;
        if !delta.insert_call_edges.is_empty() {
            tracing::info!(
                edges = delta.insert_call_edges.len(),
                "field observer synthesis complete"
            );
        }
        deltas.push(delta);

        let delta = compute_react_rerender_chain_synthesis(db)?;
        if !delta.insert_call_edges.is_empty() {
            tracing::info!(
                edges = delta.insert_call_edges.len(),
                "React re-render chain synthesis complete"
            );
        }
        deltas.push(delta);

        let delta = compute_vue_template_synthesis(db)?;
        let vue_edges = delta.insert_call_edges.len() + delta.insert_semantic_edges.len();
        if vue_edges > 0 {
            tracing::info!(edges = vue_edges, "Vue template synthesis complete");
        }
        deltas.push(delta);
    }

    if interface_changed {
        let delta = compute_interface_dispatch_synthesis(db, config, &deltas)?;
        if !delta.insert_call_edges.is_empty() {
            tracing::info!(
                edges = delta.insert_call_edges.len(),
                "Interface dispatch synthesis complete"
            );
        }
        deltas.push(delta);
    }

    Ok(SynthesisRound { deltas })
}

/// Apply a synthesis round atomically. The write lock is held only for this
/// batch write (deletes then inserts, in pass order), not for the compute.
pub(crate) fn apply_synthesis_round(db: &IndexDb, round: &SynthesisRound) -> CcResult<()> {
    if round.deltas.is_empty() {
        return Ok(());
    }
    let uow = db.begin_unit_of_work()?;
    for delta in &round.deltas {
        for kind in &delta.delete_call_kinds {
            uow.delete_synthetic_call_edges(kind)?;
        }
        for prefix in &delta.delete_semantic_prefixes {
            uow.delete_synthetic_semantic_edges(prefix)?;
        }
        if !delta.insert_semantic_edges.is_empty() {
            uow.insert_semantic_edges_batch(&delta.insert_semantic_edges)?;
        }
        if !delta.insert_call_edges.is_empty() {
            uow.insert_synthetic_call_edges(&delta.insert_call_edges)?;
        }
    }
    uow.commit()
}
