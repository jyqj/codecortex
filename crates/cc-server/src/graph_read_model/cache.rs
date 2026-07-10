//! Process-global, generation-keyed cache slots for graph read data.
//!
//! Caching stays in cc-server by design (see `docs/adr/0001`): cc-db owns the
//! persisted epoch vector and typed queries, the server owns cache identity
//! and invalidation policy.

use cc_db::index_db::IndexDb;
use cc_db::GraphReads;
use cc_model::CcResult;
use lru::LruCache;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use crate::graph_types::EdgeLite;

use super::projections::SemanticAdjPair;

/// Synthesized HTTP/async bridge edges keyed by caller UID.
pub(super) type BridgeEdgesByCaller = HashMap<String, Vec<EdgeLite>>;

/// Bridge edges plus the load-time truncation fact. When the bounded loads
/// behind `bridge_edges_by_caller` hit `CODECORTEX_BRIDGE_EDGE_LIMIT`, ANY
/// caller may be missing synthesized edges — consumers surface that as
/// `truncated_reason = "bridge_cap"` on their explain envelope.
#[derive(Default)]
pub(super) struct BridgeIndex {
    pub(super) by_caller: BridgeEdgesByCaller,
    pub(super) truncated: bool,
    /// Per-category counts of HTTP call edges that produced no bridge edge
    /// (`no_caller_uid` / `no_normalized_path` / `no_route_handler`). Surfaces
    /// as `synthesis_notes` on the explain envelope so a caller can see why
    /// some HTTP calls did not synthesize a bridge.
    pub(super) unmatched: BTreeMap<String, usize>,
}

pub(super) type SharedBridgeEdges = Arc<BridgeIndex>;

/// Process-global adjacency cache shared across all `GraphReadModel` instances.
///
/// The inner map is keyed by caller UID → outgoing edges.
pub(super) type SharedAdjacency = Arc<Mutex<HashMap<String, Vec<EdgeLite>>>>;

pub(super) type SharedSemanticAdj = Arc<Mutex<SemanticAdjPair>>;

pub(super) type SharedCalleeSet = Arc<HashSet<String>>;

/// How a cache slot's contents relate to the two persisted epochs. Declared
/// once at slot construction and consumed exhaustively by
/// [`generation_cached`] — call sites never normalize keys themselves.
#[derive(Debug, Clone, Copy)]
pub(super) enum EpochSensitivity {
    /// Contents derive only from committed index writes (semantic edges,
    /// imports, communities, dead-code caller sets, plain call adjacency).
    /// Evidence-only epoch bumps must not evict them.
    IndexOnly,
    /// Contents absorb runtime-evidence effects (bridge edges, whose
    /// confidence moves with evidence ingestion, and any adjacency that
    /// includes them). Any epoch bump evicts.
    IndexAndEvidence,
}

/// Per-project store of a slot: project identity -> the value computed for
/// that project's latest `GraphReadGeneration`.
type SlotStore<T> = OnceLock<Mutex<LruCache<u64, (GraphReadGeneration, Arc<T>)>>>;

/// One process-global generation-cache slot. The slot owns its
/// [`EpochSensitivity`], so the invalidation policy is part of the slot
/// declaration, not call-site discipline.
pub(super) struct GenerationSlot<T> {
    cell: SlotStore<T>,
    sensitivity: EpochSensitivity,
}

impl<T> GenerationSlot<T> {
    pub(super) const fn new(sensitivity: EpochSensitivity) -> Self {
        Self {
            cell: OnceLock::new(),
            sensitivity,
        }
    }

    /// The cache key for `generation` under this slot's declared sensitivity.
    /// This match is the only place epoch semantics are interpreted.
    fn key_for(&self, generation: &GraphReadGeneration) -> GraphReadGeneration {
        match self.sensitivity {
            EpochSensitivity::IndexOnly => generation.index_only(),
            EpochSensitivity::IndexAndEvidence => *generation,
        }
    }

    /// Drop this slot's entry for one project identity, if present.
    fn evict(&'static self, db_identity: u64) {
        let Some(cache) = self.cell.get() else {
            return;
        };
        if let Ok(mut guard) = cache.lock() {
            guard.pop(&db_identity);
        }
    }
}

/// Drop every process-global graph cache entry for one project identity.
///
/// Called when a `CodeIndex` closes its `IndexDb` handle (idle eviction or
/// project switch): `db_identity` is process-unique and never reused, so the
/// closed handle's entries can never be hit again — without this hook they
/// would linger as unreachable garbage until 16 other identities pushed them
/// out of each slot's LRU.
pub(crate) fn evict_project(db_identity: u64) {
    PLAIN_ADJ_CACHE.evict(db_identity);
    BRIDGED_ADJ_CACHE.evict(db_identity);
    BRIDGE_CACHE.evict(db_identity);
    SEMANTIC_CACHE.evict(db_identity);
    IMPORT_ADJ_CACHE.evict(db_identity);
    COMMUNITY_ADJ_CACHE.evict(db_identity);
    CALLEES_WITH_CALLERS_CACHE.evict(db_identity);
}

/// Per-project capacity for the process-global graph caches. Aligned with the
/// project-session LRU so a multi-project workload keeps each project's graph
/// hot instead of thrashing a single shared slot. Override with
/// CODECORTEX_GRAPH_CACHE_SIZE.
fn graph_cache_capacity() -> NonZeroUsize {
    std::env::var("CODECORTEX_GRAPH_CACHE_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .unwrap_or(NonZeroUsize::new(16).unwrap())
}

/// Process-global adjacency caches keyed by project identity (`db_identity`).
///
/// Each project keeps a single slot holding its latest `GraphReadGeneration`;
/// an incremental rebuild replaces the slot in place, while distinct projects
/// coexist up to `graph_cache_capacity()` so multi-project workloads do not
/// thrash.
///
/// Plain and bridge-including projections must NOT share a slot: `neighbors()`
/// lazily fills whichever map the model holds, so a shared slot would let the
/// first constructor decide whether bridge edges exist for every later caller
/// of the same generation (bridged traces would silently lose bridge edges, or
/// impact/dead-code would see synthesized ones).
///
/// The plain slot holds only `call_edges` content (its `http_bridges` map is
/// always empty), so it is index-only; the bridged slot absorbs bridge edges
/// whose confidence moves with evidence ingestion.
static PLAIN_ADJ_CACHE: GenerationSlot<Mutex<HashMap<String, Vec<EdgeLite>>>> =
    GenerationSlot::new(EpochSensitivity::IndexOnly);
static BRIDGED_ADJ_CACHE: GenerationSlot<Mutex<HashMap<String, Vec<EdgeLite>>>> =
    GenerationSlot::new(EpochSensitivity::IndexAndEvidence);

/// Process-global bridge edge cache, keyed by `db_identity`. Bridge edge
/// confidence moves with evidence ingestion, hence evidence-sensitive.
pub(super) static BRIDGE_CACHE: GenerationSlot<BridgeIndex> =
    GenerationSlot::new(EpochSensitivity::IndexAndEvidence);

/// Process-global semantic edge cache, keyed by `db_identity`.
static SEMANTIC_CACHE: GenerationSlot<Mutex<SemanticAdjPair>> =
    GenerationSlot::new(EpochSensitivity::IndexOnly);

pub(super) static IMPORT_ADJ_CACHE: GenerationSlot<HashMap<String, Vec<String>>> =
    GenerationSlot::new(EpochSensitivity::IndexOnly);

pub(super) static COMMUNITY_ADJ_CACHE: GenerationSlot<HashMap<String, Vec<String>>> =
    GenerationSlot::new(EpochSensitivity::IndexOnly);

/// Dead-code caller set cache (callee UIDs with at least one non-self caller).
pub(super) static CALLEES_WITH_CALLERS_CACHE: GenerationSlot<HashSet<String>> =
    GenerationSlot::new(EpochSensitivity::IndexOnly);

/// Cache/reuse discriminator for graph read data.
///
/// Derived from cc-db's persisted epoch vector: `index_epoch` advances on any
/// committed index-content write, `evidence_epoch` on runtime-evidence writes
/// (which also boost http_call_edges confidence, feeding the bridge edges).
/// `db_identity` is cc-db's process-unique, never-reused instance id, so a
/// dropped handle's identity can never alias a later project's (unlike an
/// `Arc::as_ptr` address, which the allocator may reuse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GraphReadGeneration {
    db_identity: u64,
    index_epoch: u64,
    evidence_epoch: u64,
    /// True when the persisted epoch vector could not be read. Such a key is
    /// untrustworthy — defaulting the epochs to 0 would alias a genuinely
    /// fresh database — so [`generation_cached`] bypasses both cache lookup
    /// and store for it.
    epoch_read_failed: bool,
}

impl GraphReadGeneration {
    pub(super) fn from_db(db: &Arc<IndexDb>) -> Self {
        let reads = GraphReads::new(db);
        // A failed epoch read must not silently key the caches as epoch 0:
        // mark the generation degraded instead, so this read model computes
        // without the shared caches and the next model retries the read.
        let (generation, epoch_read_failed) = match reads.generation() {
            Ok(generation) => (generation, false),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "graph generation read failed; bypassing shared graph caches"
                );
                (Default::default(), true)
            }
        };
        Self {
            db_identity: reads.instance_id(),
            index_epoch: generation.index_epoch,
            evidence_epoch: generation.evidence_epoch,
            epoch_read_failed,
        }
    }

    /// Key for caches whose contents do not depend on runtime evidence
    /// (semantic edges, imports, communities, dead-code caller sets): the
    /// evidence epoch is normalized away so evidence ingestion does not evict
    /// them. Bridge edges and call adjacency (which absorbs bridge edges in
    /// `neighbors()`) must keep the full generation.
    pub(crate) fn index_only(&self) -> Self {
        Self {
            evidence_epoch: 0,
            ..*self
        }
    }
}

/// Shared generation-cache ritual: look up `slot` by project identity, hit
/// only when the stored generation matches, otherwise compute outside the
/// lock and install the fresh value under the latest generation.
///
/// The key is normalized per the slot's declared [`EpochSensitivity`] before
/// any comparison, so call sites pass the current generation as-is.
///
/// A failed `compute` is NOT cached — the error is returned and the next call
/// retries, so per-request degradation at the call site never becomes sticky
/// for the whole generation. A generation whose epoch read failed bypasses
/// the cache entirely (compute, don't store): its key cannot be trusted not
/// to alias a real epoch-0 generation.
pub(super) fn generation_cached<T>(
    slot: &'static GenerationSlot<T>,
    generation: &GraphReadGeneration,
    compute: impl FnOnce() -> CcResult<Arc<T>>,
) -> CcResult<Arc<T>> {
    if generation.epoch_read_failed {
        return compute();
    }
    let generation = slot.key_for(generation);
    let cache = slot
        .cell
        .get_or_init(|| Mutex::new(LruCache::new(graph_cache_capacity())));
    if let Ok(mut guard) = cache.lock() {
        if let Some((stored_gen, value)) = guard.get(&generation.db_identity) {
            if *stored_gen == generation {
                return Ok(Arc::clone(value));
            }
        }
    }

    // Miss, or same project with a newer generation: compute outside the
    // lock, then replace this project's slot with the latest generation.
    let value = compute()?;
    if let Ok(mut guard) = cache.lock() {
        guard.put(generation.db_identity, (generation, Arc::clone(&value)));
    }
    Ok(value)
}

/// Get or create the process-global shared adjacency map for `gen`.
///
/// Keyed by project identity: a cache hit requires the same project, the same
/// generation, AND the same bridge dimension (`include_bridges` selects an
/// independent slot family — see the slot docs above for why sharing one slot
/// across both projections is incorrect).
///
/// Each slot's epoch sensitivity is declared on the slot itself (see the
/// statics above), so this only selects the slot family.
///
/// A miss installs a fresh empty adjacency under the latest generation for
/// this project's slot; `neighbors()` populates it lazily.
pub(super) fn cached_adjacency(
    gen: &GraphReadGeneration,
    include_bridges: bool,
) -> SharedAdjacency {
    let empty = || Ok(Arc::new(Mutex::new(HashMap::new())));
    let cached = if include_bridges {
        generation_cached(&BRIDGED_ADJ_CACHE, gen, empty)
    } else {
        generation_cached(&PLAIN_ADJ_CACHE, gen, empty)
    };
    cached.expect("empty adjacency construction is infallible")
}

/// Get or create the process-global shared semantic adjacency for `gen`.
///
/// Mirrors `cached_adjacency()`: keyed by project identity, a cache hit
/// requires both the same project and the same generation (normalized per
/// the slot's declared sensitivity).
pub(super) fn cached_semantic_adjacency(gen: &GraphReadGeneration) -> SharedSemanticAdj {
    generation_cached(&SEMANTIC_CACHE, gen, || {
        Ok(Arc::new(Mutex::new(SemanticAdjPair::default())))
    })
    .expect("empty semantic adjacency construction is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(index_epoch: u64, evidence_epoch: u64) -> GraphReadGeneration {
        GraphReadGeneration {
            db_identity: 42,
            index_epoch,
            evidence_epoch,
            epoch_read_failed: false,
        }
    }

    /// Invalidation matrix for the slot-declared sensitivities: an index bump
    /// evicts both kinds, an evidence-only bump evicts only evidence-sensitive
    /// slots. This pins `key_for`, the single place epoch semantics live.
    #[test]
    fn sensitivity_declares_which_epoch_bumps_evict() {
        static INDEX_ONLY: GenerationSlot<u64> = GenerationSlot::new(EpochSensitivity::IndexOnly);
        static FULL: GenerationSlot<u64> = GenerationSlot::new(EpochSensitivity::IndexAndEvidence);

        let base = generation(1, 1);
        let evidence_bumped = generation(1, 2);
        let index_bumped = generation(2, 1);

        let seed = |slot: &'static GenerationSlot<u64>, generation, value: u64| {
            generation_cached(slot, generation, || Ok(Arc::new(value))).unwrap()
        };

        assert_eq!(*seed(&INDEX_ONLY, &base, 10), 10);
        assert_eq!(*seed(&FULL, &base, 10), 10);

        // Evidence-only bump: index-only slot survives (compute not re-run),
        // evidence-sensitive slot recomputes.
        assert_eq!(*seed(&INDEX_ONLY, &evidence_bumped, 20), 10);
        assert_eq!(*seed(&FULL, &evidence_bumped, 20), 20);

        // Index bump: both recompute.
        assert_eq!(*seed(&INDEX_ONLY, &index_bumped, 30), 30);
        assert_eq!(*seed(&FULL, &index_bumped, 30), 30);
    }

    /// `evict` must drop exactly the given project identity's entry: after a
    /// `CodeIndex::close` the identity can never be hit again (identities are
    /// never reused), so keeping the entry would only hold memory hostage
    /// until 16 other projects pushed it out of the slot's LRU.
    #[test]
    fn evict_drops_only_the_given_identity() {
        static SLOT: GenerationSlot<u64> = GenerationSlot::new(EpochSensitivity::IndexOnly);

        let closing = generation(1, 1);
        let surviving = GraphReadGeneration {
            db_identity: 43,
            ..generation(1, 1)
        };
        let seed = |generation, value: u64| {
            generation_cached(&SLOT, generation, || Ok(Arc::new(value))).unwrap()
        };

        assert_eq!(*seed(&closing, 10), 10);
        assert_eq!(*seed(&surviving, 20), 20);

        SLOT.evict(closing.db_identity);

        // The evicted identity recomputes; the other project's entry survives.
        assert_eq!(*seed(&closing, 30), 30);
        assert_eq!(*seed(&surviving, 40), 20);
    }

    /// A generation whose epoch read failed must neither be served from the
    /// cache (its defaulted key would alias a real epoch) nor stored into it.
    #[test]
    fn degraded_generation_bypasses_cache_lookup_and_store() {
        static SLOT: GenerationSlot<u64> = GenerationSlot::new(EpochSensitivity::IndexOnly);

        let healthy = generation(1, 1);
        let degraded = GraphReadGeneration {
            epoch_read_failed: true,
            ..healthy
        };

        let seed = |generation, value: u64| {
            generation_cached(&SLOT, generation, || Ok(Arc::new(value))).unwrap()
        };

        assert_eq!(*seed(&healthy, 10), 10);
        // Same epochs, but degraded: must compute instead of serving 10 ...
        assert_eq!(*seed(&degraded, 20), 20);
        // ... and must not have overwritten the healthy entry.
        assert_eq!(*seed(&healthy, 30), 10);
    }
}
