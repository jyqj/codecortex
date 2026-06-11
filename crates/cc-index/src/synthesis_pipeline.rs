//! Synthesis pipeline: compute/apply separation for dispatch synthesis.
//!
//! The pass order lives in the declarative pass registry
//! ([`crate::dispatch_synthesis::registry`]); this module drives a round from
//! it. Each pass is compute-only: it reads committed index state through the
//! read pool and returns an [`EdgeDelta`] — it never touches the write
//! connection. The single cross-pass dependency (interface dispatch consumes
//! call edges synthesized earlier in the same round) is threaded explicitly
//! through [`PassContext::prior_deltas`]: each pass receives the deltas of
//! the passes before it and may overlay their in-memory call edges onto its
//! committed-state read. The overlay covers CALL edges only — see the
//! [`PassContext`] documentation for the boundary and its rationale.
//!
//! A pass may only delete the call kinds / semantic prefixes it declared as
//! owned in its `SynthesisPassSpec`; the round loop `debug_assert`s this,
//! so a pass that grows a new synthetic edge kind without declaring it fails
//! loudly in tests instead of silently desynchronizing the disable-cleanup
//! deletion set derived from the registry.
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

use crate::dispatch_synthesis::{registry, PassContext, PassGate, SynthesisConfig};

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

/// Run the synthesis passes (gated like `phase_postprocess`) in registry
/// order and collect their deltas. Pure compute: no write lock, no
/// transaction.
pub(crate) fn compute_synthesis_round(
    db: &IndexDb,
    config: &SynthesisConfig,
    dispatch_changed: bool,
    interface_changed: bool,
) -> CcResult<SynthesisRound> {
    let mut deltas: Vec<EdgeDelta> = Vec::new();

    for spec in registry() {
        let gate_open = match spec.gate {
            PassGate::Dispatch => dispatch_changed,
            PassGate::Interface => interface_changed,
        };
        if !gate_open {
            continue;
        }

        let delta = (spec.compute)(&PassContext {
            db,
            config,
            prior_deltas: &deltas,
        })?;

        debug_assert!(
            delta
                .delete_call_kinds
                .iter()
                .all(|kind| spec.owned_call_kinds.contains(kind)),
            "pass `{}` deletes call kinds outside its declared owned set",
            spec.id
        );
        debug_assert!(
            delta
                .delete_semantic_prefixes
                .iter()
                .all(|prefix| spec.owned_semantic_prefixes.contains(prefix)),
            "pass `{}` deletes semantic prefixes outside its declared owned set",
            spec.id
        );

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
