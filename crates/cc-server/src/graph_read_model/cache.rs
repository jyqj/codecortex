//! Process-global, generation-keyed cache slots for graph read data.
//!
//! Caching stays in cc-server by design (see `docs/adr/0001`): cc-db owns the
//! persisted epoch vector and typed queries, the server owns cache identity
//! and invalidation policy.

use cc_db::index_db::IndexDb;
use cc_model::CcResult;
use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use crate::graph_types::EdgeLite;

use super::projections::SemanticAdjPair;

/// Synthesized HTTP/async bridge edges keyed by caller UID.
pub(super) type BridgeEdgesByCaller = HashMap<String, Vec<EdgeLite>>;
pub(super) type SharedBridgeEdges = Arc<BridgeEdgesByCaller>;

/// Process-global adjacency cache shared across all `GraphReadModel` instances.
///
/// The inner map is keyed by caller UID → outgoing edges.
pub(super) type SharedAdjacency = Arc<Mutex<HashMap<String, Vec<EdgeLite>>>>;

pub(super) type SharedSemanticAdj = Arc<Mutex<SemanticAdjPair>>;

pub(super) type SharedCalleeSet = Arc<HashSet<String>>;

/// One process-global generation-cache slot: project identity -> the value
/// computed for that project's latest `GraphReadGeneration`.
pub(super) type GenerationSlot<T> = OnceLock<Mutex<LruCache<u64, (GraphReadGeneration, Arc<T>)>>>;

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
static PLAIN_ADJ_CACHE: GenerationSlot<Mutex<HashMap<String, Vec<EdgeLite>>>> = OnceLock::new();
static BRIDGED_ADJ_CACHE: GenerationSlot<Mutex<HashMap<String, Vec<EdgeLite>>>> = OnceLock::new();

/// Process-global bridge edge cache, keyed by `db_identity`.
pub(super) static BRIDGE_CACHE: GenerationSlot<BridgeEdgesByCaller> = OnceLock::new();

/// Process-global semantic edge cache, keyed by `db_identity`.
static SEMANTIC_CACHE: GenerationSlot<Mutex<SemanticAdjPair>> = OnceLock::new();

pub(super) static IMPORT_ADJ_CACHE: GenerationSlot<HashMap<String, Vec<String>>> = OnceLock::new();

pub(super) static COMMUNITY_ADJ_CACHE: GenerationSlot<HashMap<String, Vec<String>>> =
    OnceLock::new();

/// Dead-code caller set cache (callee UIDs with at least one non-self caller).
pub(super) static CALLEES_WITH_CALLERS_CACHE: GenerationSlot<HashSet<String>> = OnceLock::new();

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
}

impl GraphReadGeneration {
    pub(super) fn from_db(db: &Arc<IndexDb>) -> Self {
        let generation = db.generation().unwrap_or_default();
        Self {
            db_identity: db.instance_id(),
            index_epoch: generation.index_epoch,
            evidence_epoch: generation.evidence_epoch,
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
/// A failed `compute` is NOT cached — the error is returned and the next call
/// retries, so per-request degradation at the call site never becomes sticky
/// for the whole generation.
pub(super) fn generation_cached<T>(
    slot: &'static GenerationSlot<T>,
    generation: &GraphReadGeneration,
    compute: impl FnOnce() -> CcResult<Arc<T>>,
) -> CcResult<Arc<T>> {
    let cache = slot.get_or_init(|| Mutex::new(LruCache::new(graph_cache_capacity())));
    if let Ok(mut guard) = cache.lock() {
        if let Some((stored_gen, value)) = guard.get(&generation.db_identity) {
            if stored_gen == generation {
                return Ok(Arc::clone(value));
            }
        }
    }

    // Miss, or same project with a newer generation: compute outside the
    // lock, then replace this project's slot with the latest generation.
    let value = compute()?;
    if let Ok(mut guard) = cache.lock() {
        guard.put(generation.db_identity, (*generation, Arc::clone(&value)));
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
/// The bridged slot keeps the full generation: its content absorbs bridge
/// edges whose confidence moves with evidence ingestion. The plain slot holds
/// only `call_edges` content (its `http_bridges` map is always empty), so it
/// is keyed evidence-free — like the semantic/import/community caches — and
/// survives evidence-only epoch bumps.
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
        generation_cached(&PLAIN_ADJ_CACHE, &gen.index_only(), empty)
    };
    cached.expect("empty adjacency construction is infallible")
}

/// Get or create the process-global shared semantic adjacency for `gen`.
///
/// Mirrors `cached_adjacency()`: keyed by project identity, a cache hit
/// requires both the same project and the same generation. Semantic edges
/// never depend on runtime evidence, so the evidence epoch is excluded.
pub(super) fn cached_semantic_adjacency(gen: &GraphReadGeneration) -> SharedSemanticAdj {
    generation_cached(&SEMANTIC_CACHE, &gen.index_only(), || {
        Ok(Arc::new(Mutex::new(SemanticAdjPair::default())))
    })
    .expect("empty semantic adjacency construction is infallible")
}
