//! Filtered exact vector backend (P6-010) — the small-scale oracle backend.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (constraint row P6-010: "过滤先于 exact top-k、bounded batch、稳定 ties、
//! 空间隔离；删除/异空间向量不可返回") and C09
//! (`02-CONTRACTS.md`: "向量 scope 筛选应发生在 top-k 之前").
//!
//! ## Scope and limits (must be stated, brief risk item)
//!
//! This is a brute-force exact scan over the published manifest: every batch
//! costs one cache read plus one `O(dim)` dot product per candidate, so the
//! whole query is `O(N·dim)` over the visible set. That is the point — an
//! oracle small corpora can afford — and it is *not* a production backend for
//! 100k-scale document sets. ANN structures are the V22 optional enhancement
//! track (`06-VALIDATION.md`), never a silent property of this module; callers
//! must not read exact-search latency numbers as an ANN baseline.
//!
//! ## Combination semantics (filtered exact, per brief)
//!
//! **过滤先于 top-k** ("filter before top-k"): the [`HardScope`] intersection
//! (C09: repo namespace + caller `file_paths`/`path_prefix`/`languages`) is
//! applied to every candidate row *before* it is scored; the top-k selection
//! therefore runs over the filtered stream only. `Some(empty)` scopes short
//! -circuit to an empty result without loading a single candidate (C09:
//! "Some(empty) 表示空集合，永不退化全仓"). Because exact scoring evaluates
//! every filtered candidate, filter-before-top-k and filter-after-top-k
//! coincide *for the result set* here; the ordering is still load-bearing for
//! cost and for the ANN successor, where filtering after top-k would silently
//! under-report.
//!
//! ## Invariants (each mapped to an oracle test)
//!
//! 1. **空间隔离（装载层）**: the candidate scan filters `space_id` (cc-db
//!    [`cc_db::semantic_manifest_reads`]) and this module additionally rejects
//!    any row whose `artifact_ref` does not carry the requested
//!    [`SpaceDigest`] — a foreign-space vector is structurally unloadable, not
//!    merely low-ranked.
//! 2. **删除不可返回**: candidates come only from `semantic_manifest`; a
//!    deleted document has no manifest row (same-transaction revoke or FK
//!    cascade), so it has no candidate and is unreachable by construction.
//! 3. **bounded batch**: candidates stream in keyset batches of at most
//!    `ExactSearch::batch_rows`; peak memory is `O(batch_rows·dim + k)` (one
//!    batch of vectors plus the size-k selection heap).
//! 4. **稳定 ties**: scores accumulate in `f64` with a fixed operand order;
//!    the total order is `(score desc, doc_key asc)` via
//!    [`f64::total_cmp`] — independent of thread and batch order (C10:
//!    "固定 tie-break；结果顺序不是线程完成顺序").
//! 5. **metric 按 spec**: [`DistanceMetric`] is a frozen closed enum whose
//!    only admitted variant is `Cosine` (spec v1); the dispatch below is an
//!    exhaustive `match`, so admitting a new metric under a spec-version bump
//!    is a compile error here until its branch is written. L2 / inner product
//!    are therefore *not* implemented — "Cosine/L2/内积按 spec" resolves to
//!    Cosine-only at the current frozen spec.
//!
//! ## Degradation (ADR: "损坏可检测且只降级不污染")
//!
//! A candidate row with an unparseable/reforeign `artifact_ref`, a missing
//! cache object (`Miss`) or a detected-corrupt object (`Corrupt`) is *skipped*;
//! it is never an error and never quarantined here (the quarantine *move* is
//! P6-018). [`search_controlled_with_coverage`] also reports these gaps among
//! the requested space's scope-passing rows, so a caller can distinguish a
//! complete result from a scan that lost eligible artifacts. The legacy Vec
//! APIs keep their result-only contract. A zero-magnitude document or query
//! vector has no cosine and is skipped likewise — NaN/Inf scores can never
//! be produced (C10: "NaN/Inf/... 拒收").

use cc_db::semantic_manifest_reads::{SemanticManifestReads, SemanticManifestRow};
use cc_model::query::QueryControl;
use cc_model::retrieval::HardScope;
use cc_model::{CcError, CcResult, Language};

use crate::cache::{ArtifactCache, CacheRead};
use crate::spec::VectorSpace;
use crate::types::{DocSpecDigest, InputDigest, SpaceDigest};

/// Candidate stream port of the filtered exact backend (new port in a new
/// module per the freeze-surface rule; the frozen [`crate::ports`] surface is
/// untouched).
///
/// The contract is keyset pagination: each call returns the rows with
/// `doc_key > after_doc_key` in ascending `doc_key` order, at most
/// `batch_rows` of them. [`SemanticManifestReads`] is the sanctioned
/// production source (adapter below); tests substitute an in-memory source.
pub trait ManifestCandidates {
    fn next_batch(
        &self,
        after_doc_key: &str,
        batch_rows: usize,
    ) -> CcResult<Vec<SemanticManifestRow>>;
}

/// Production adapter: a [`SemanticManifestReads`] handle pinned to one query
/// space. Space isolation lives in the SQL `WHERE space_id = ?`; this wrapper
/// is the seam that pins the space so a caller cannot scan the wrong space by
/// accident.
pub struct SpaceScopedManifestReads<'a> {
    reads: &'a SemanticManifestReads<'a>,
    space_id: String,
}

/// Bind a manifest read handle to one space, yielding the sanctioned
/// [`ManifestCandidates`] production source for [`search`].
pub fn space_manifest_reads<'a>(
    reads: &'a SemanticManifestReads<'a>,
    space: &SpaceDigest,
) -> SpaceScopedManifestReads<'a> {
    SpaceScopedManifestReads {
        reads,
        space_id: space.as_str().to_string(),
    }
}

impl ManifestCandidates for SpaceScopedManifestReads<'_> {
    fn next_batch(
        &self,
        after_doc_key: &str,
        batch_rows: usize,
    ) -> CcResult<Vec<SemanticManifestRow>> {
        self.reads
            .scan_space(&self.space_id, after_doc_key, batch_rows)
    }
}

/// One scored candidate of the exact result: the published `doc_key` and its
/// query similarity under the space's distance metric.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredDoc {
    pub doc_key: String,
    pub score: f64,
}

/// Top-k plus bounded, per-scan artifact availability diagnostics. Counts are
/// per eligible manifest row, not distinct cache objects. Foreign-space rows
/// and hard-scope exclusions contribute nothing; a bad reference on an
/// eligible row is counted without opening any cache object. No row list or
/// repair work is retained. These counters do not assert publication coverage
/// or active-space status, which remain the caller's manifest fence.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExactSearchResult {
    pub scored: Vec<ScoredDoc>,
    pub missing_artifacts: usize,
    pub corrupt_artifacts: usize,
    pub rejected_artifact_refs: usize,
}

impl ExactSearchResult {
    pub fn has_unavailable_artifacts(&self) -> bool {
        self.missing_artifacts != 0
            || self.corrupt_artifacts != 0
            || self.rejected_artifact_refs != 0
    }
}

/// Parameters of one filtered exact query (brief interface draft; `space`
/// carries the full frozen [`VectorSpace`] rather than a bare
/// [`SpaceDigest`] because the artifact cache verifies model/dimension against
/// the space on every read — the digest is derived internally).
pub struct ExactSearch<'a> {
    pub space: &'a VectorSpace,
    pub query: &'a [f32],
    /// C09 hard scope intersection applied before top-k.
    pub filter: &'a HardScope,
    pub k: usize,
    /// Candidate load batch upper bound (the bounded-memory knob).
    pub batch_rows: usize,
}

/// Filtered exact top-k: stream the published space through the cache, score
/// every scope-passing candidate, return the top `k` under
/// `(score desc, doc_key asc)`.
pub fn search(
    cache: &ArtifactCache,
    manifest: &dyn ManifestCandidates,
    q: ExactSearch<'_>,
) -> CcResult<Vec<ScoredDoc>> {
    search_checked(cache, manifest, q, || Ok(())).map(|result| result.scored)
}

/// Cooperative variant of [`search`] with the same scoring and ordering.
/// Checks cancellation/deadline before work, around each batch read and
/// candidate score, and before returning the final top-k. An interrupted
/// scan returns an error, never its accumulated partial top-k as success.
///
/// This synchronous function does not preempt a SQL read, cache I/O, or one
/// cosine computation already in progress. Async callers must run it on a
/// bounded blocking executor and retain admission until the worker exits;
/// putting it inside an async future does not make blocking work preemptible.
pub fn search_controlled(
    cache: &ArtifactCache,
    manifest: &dyn ManifestCandidates,
    q: ExactSearch<'_>,
    control: &QueryControl,
) -> CcResult<Vec<ScoredDoc>> {
    search_controlled_with_coverage(cache, manifest, q, control).map(|result| result.scored)
}

/// Cooperative exact scan with the same top-k and checkpoints as
/// [`search_controlled`], retaining artifact gaps even outside the winning
/// top-k. An empty hard domain or zero limit reports no artifact gaps. An
/// interrupted or failed scan still returns an error, never a successful
/// partial report.
pub fn search_controlled_with_coverage(
    cache: &ArtifactCache,
    manifest: &dyn ManifestCandidates,
    q: ExactSearch<'_>,
    control: &QueryControl,
) -> CcResult<ExactSearchResult> {
    search_checked(cache, manifest, q, || control.check())
}

fn search_checked(
    cache: &ArtifactCache,
    manifest: &dyn ManifestCandidates,
    q: ExactSearch<'_>,
    mut checkpoint: impl FnMut() -> CcResult<()>,
) -> CcResult<ExactSearchResult> {
    checkpoint()?;
    q.space.validate()?;
    if q.batch_rows == 0 {
        return Err(CcError::InvalidParams(
            "exact search requires batch_rows >= 1".into(),
        ));
    }
    if q.query.len() != q.space.dimension() as usize {
        return Err(CcError::InvalidParams(format!(
            "query length {} does not match space dimension {}",
            q.query.len(),
            q.space.dimension()
        )));
    }
    // C09: Some(empty) is an empty set — never degrade to a full-repo scan.
    // (k == 0 is the trivial empty result.)
    if q.k == 0 || q.filter.is_empty() {
        return Ok(ExactSearchResult::default());
    }

    let space_digest = q.space.digest()?;
    let mut cursor = String::new();
    let mut top = TopK::new(q.k);
    let mut result = ExactSearchResult::default();
    loop {
        checkpoint()?;
        let batch = manifest.next_batch(&cursor, q.batch_rows)?;
        checkpoint()?;
        if batch.is_empty() {
            break;
        }
        for row in &batch {
            checkpoint()?;
            cursor = row.doc_key.clone();
            match score_candidate(cache, q.space, &space_digest, q.query, q.filter, row)? {
                CandidateScore::Scored(score) => top.offer(row.doc_key.clone(), score),
                CandidateScore::Skipped => {}
                CandidateScore::MissingArtifact => {
                    result.missing_artifacts = result.missing_artifacts.saturating_add(1);
                }
                CandidateScore::CorruptArtifact => {
                    result.corrupt_artifacts = result.corrupt_artifacts.saturating_add(1);
                }
                CandidateScore::RejectedArtifactRef => {
                    result.rejected_artifact_refs = result.rejected_artifact_refs.saturating_add(1);
                }
            }
            checkpoint()?;
        }
    }
    result.scored = top.finish();
    checkpoint()?;
    Ok(result)
}

enum CandidateScore {
    Scored(f64),
    Skipped,
    MissingArtifact,
    CorruptArtifact,
    RejectedArtifactRef,
}

/// Scope gate → address validation → cache load → metric scoring. Only rows
/// admitted by both space and scope can report an unavailable artifact.
/// Actual I/O errors continue to propagate rather than masquerading as gaps.
fn score_candidate(
    cache: &ArtifactCache,
    space: &VectorSpace,
    space_digest: &SpaceDigest,
    query: &[f32],
    filter: &HardScope,
    row: &SemanticManifestRow,
) -> CcResult<CandidateScore> {
    // Defense in depth: the scan already filtered by space id; a row that
    // still disagrees (stale join, wrong source) never reaches scoring.
    if row.space_id != space_digest.as_str() {
        return Ok(CandidateScore::Skipped);
    }
    if !row_passes_scope(filter, row) {
        return Ok(CandidateScore::Skipped);
    }
    // The cache addressing tuple must match the row before any byte is read:
    // a ref from another namespace, another space, or another input is not
    // this row's vector.
    let address = match parse_artifact_ref(&row.artifact_ref) {
        Some(address) => address,
        None => return Ok(CandidateScore::RejectedArtifactRef),
    };
    if address.namespace != cache.namespace()
        || address.space_id != space_digest.as_str()
        || address.input_digest != row.input_digest
    {
        return Ok(CandidateScore::RejectedArtifactRef);
    }
    let input = InputDigest::new(row.input_digest.clone());
    let spec = DocSpecDigest::new(address.spec_digest);
    match cache.get(space, &input, &spec)? {
        CacheRead::Hit(vector) => {
            // Frozen-spec metric dispatch. Exhaustive on purpose: a new
            // `DistanceMetric` variant (spec-version bump) fails to compile
            // here until its scoring branch is admitted explicitly.
            let score = match space.distance() {
                crate::spec::DistanceMetric::Cosine => cosine_f64(query, &vector.data),
            };
            Ok(score.map_or(CandidateScore::Skipped, CandidateScore::Scored))
        }
        // Missing or detected-corrupt artifact: degrade (skip), never pollute.
        CacheRead::Miss => Ok(CandidateScore::MissingArtifact),
        CacheRead::Corrupt(_) => Ok(CandidateScore::CorruptArtifact),
    }
}

/// C09 hard-scope intersection for one row. Reuses the exact
/// [`HardScope::passes`] semantics; the only addition is the
/// "language unknown" case (`files` row absent → matches no language filter).
fn row_passes_scope(filter: &HardScope, row: &SemanticManifestRow) -> bool {
    let language = row.language.as_deref().map(Language::from_name);
    match language {
        Some(language) => filter.passes(&row.file_path, language),
        // Keep `HardScope::passes` as the single intersection authority: with
        // no language, the languages dimension must be unconstrained and the
        // remaining dimensions are evaluated by `passes` itself.
        None => filter.languages.is_none() && filter.passes(&row.file_path, Language::Unknown),
    }
}

/// Cosine similarity with `f64` accumulation in fixed operand order
/// (`dot`, then squared norms, sequential fold; one division).
/// `None` when either side has zero magnitude — cosine is undefined there and
/// a NaN must never surface (C10).
fn cosine_f64(a: &[f32], b: &[f32]) -> Option<f64> {
    debug_assert_eq!(a.len(), b.len(), "cosine operands must share dimension");
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return None;
    }
    Some(dot / (norm_a.sqrt() * norm_b.sqrt()))
}

/// Components of the P6-008 artifact reference
/// `cas.v1:<namespace>:<space_id>:<input_digest>:<spec_digest>:<checksum>`.
///
/// Parsed here (the format is owned by the frozen [`crate::cache`] module,
/// which this round must not modify); the checksum component is not validated
/// on this path — the cache read verifies the payload blake3 against the
/// sidecar itself.
struct ArtifactRefAddress {
    namespace: String,
    space_id: String,
    input_digest: String,
    spec_digest: String,
}

fn parse_artifact_ref(reference: &str) -> Option<ArtifactRefAddress> {
    let rest = reference.strip_prefix("cas.v1:")?;
    let mut parts = rest.split(':');
    let address = ArtifactRefAddress {
        namespace: parts.next()?.to_string(),
        space_id: parts.next()?.to_string(),
        input_digest: parts.next()?.to_string(),
        spec_digest: parts.next()?.to_string(),
    };
    // Exactly five components; a checksum must be present (may be empty only
    // if a producer wrote a malformed ref, which we then reject).
    if parts.next()?.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(address)
}

/// Bounded top-k selection under the total order
/// `(score desc, doc_key asc)` — a min-heap of the current best `k`
/// desirabilities, so the popped element is always the weakest survivor.
///
/// Because the order is total (score compared with [`f64::total_cmp`], ties
/// broken by `doc_key`), the surviving set *and its order* do not depend on
/// candidate arrival order (C10: "结果顺序不是线程完成顺序").
struct TopK {
    k: usize,
    heap: std::collections::BinaryHeap<std::cmp::Reverse<Candidate>>,
}

#[derive(Debug, PartialEq)]
struct Candidate {
    score: f64,
    doc_key: String,
}

// Desirability ordering: greater score compares Greater; on equal score the
// lexicographically *smaller* doc_key compares Greater (the more desirable
// survivor under `(score desc, doc_key asc)`).
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.doc_key.cmp(&self.doc_key))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Candidate {}

impl TopK {
    fn new(k: usize) -> Self {
        Self {
            k,
            heap: std::collections::BinaryHeap::new(),
        }
    }

    fn offer(&mut self, doc_key: String, score: f64) {
        self.heap
            .push(std::cmp::Reverse(Candidate { score, doc_key }));
        if self.heap.len() > self.k {
            self.heap.pop(); // drops the least desirable survivor
        }
    }

    fn finish(self) -> Vec<ScoredDoc> {
        let mut best: Vec<Candidate> = self.heap.into_iter().map(|reverse| reverse.0).collect();
        // Descending desirability = (score desc, doc_key asc). `sort` is
        // stable, but the order is total so stability is not load-bearing.
        best.sort_by(|a, b| b.cmp(a));
        best.into_iter()
            .map(|c| ScoredDoc {
                doc_key: c.doc_key,
                score: c.score,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{DocumentInput as DocInput, EmbeddingProvider, QueryInput};
    use crate::providers::fake::{FakeProvider, FakeProviderConfig};
    use crate::spec::DocumentEncodingSpec;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ── Test fixtures ────────────────────────────────────────────────────

    fn space(dim: u32) -> VectorSpace {
        VectorSpace::new("fake/model-exact", dim).expect("valid space")
    }

    fn scope() -> HardScope {
        HardScope {
            path_prefix: None,
            languages: None,
            file_paths: None,
        }
    }

    /// In-memory keyset candidate source with the production contract
    /// (ascending `doc_key`, `> after`, at most `batch_rows`), recording the
    /// requested batch sizes for the bounded-batch assertions.
    struct MemSource {
        rows: Vec<SemanticManifestRow>,
        batches: RefCell<Vec<usize>>,
    }

    impl MemSource {
        fn new(rows: Vec<SemanticManifestRow>) -> Self {
            let mut rows = rows;
            rows.sort_by(|a, b| a.doc_key.cmp(&b.doc_key));
            Self {
                rows,
                batches: RefCell::new(Vec::new()),
            }
        }
    }

    impl ManifestCandidates for MemSource {
        fn next_batch(&self, after: &str, batch_rows: usize) -> CcResult<Vec<SemanticManifestRow>> {
            self.batches.borrow_mut().push(batch_rows);
            Ok(self
                .rows
                .iter()
                .filter(|r| r.doc_key.as_str() > after)
                .take(batch_rows)
                .cloned()
                .collect())
        }
    }

    fn row(
        doc_key: &str,
        file_path: &str,
        language: &str,
        artifact_ref: &str,
        space_id: &str,
    ) -> SemanticManifestRow {
        SemanticManifestRow {
            doc_key: doc_key.into(),
            doc_version: "v1".into(),
            file_path: file_path.into(),
            input_digest: doc_key.into(),
            space_id: space_id.into(),
            artifact_ref: artifact_ref.into(),
            language: Some(language.into()),
        }
    }

    struct TempCache {
        root: std::path::PathBuf,
    }

    impl TempCache {
        fn open(tag: &str) -> (Self, ArtifactCache) {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "cc-semantic-p6010-{tag}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::SeqCst)
            ));
            let cache = ArtifactCache::open(&root, format!("ns-{tag}")).expect("open cache");
            (Self { root }, cache)
        }
    }

    impl Drop for TempCache {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Deterministic pseudo-random unit-ish vectors (LCG; the exact values do
    /// not matter, only that the corpus is fixed and non-degenerate).
    struct Lcg(u64);

    impl Lcg {
        fn vector(&mut self, dim: usize) -> Vec<f32> {
            let mut vector: Vec<f32> = (0..dim)
                .map(|_| {
                    self.0 = self
                        .0
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    (((self.0 >> 33) as f64 / u32::MAX as f64) * 2.0 - 1.0) as f32
                })
                .collect();
            if vector.iter().all(|&v| v == 0.0) {
                vector[0] = 1.0; // degenerate guard, unreachable in practice
            }
            vector
        }
    }

    /// Independent naive reference (deliberately formulated differently from
    /// the implementation: normalize both vectors first, then dot).
    fn reference_cosine(a: &[f32], b: &[f32]) -> f64 {
        let norm = |v: &[f32]| -> Vec<f64> {
            let length = v
                .iter()
                .fold(0.0_f64, |acc, x| acc + (*x as f64) * (*x as f64))
                .sqrt();
            v.iter().map(|x| *x as f64 / length).collect()
        };
        let (na, nb) = (norm(a), norm(b));
        na.iter()
            .zip(nb.iter())
            .fold(0.0_f64, |acc, (x, y)| acc + x * y)
    }

    fn reference_top_k(
        query: &[f32],
        vectors: &[(String, Vec<f32>)],
        k: usize,
    ) -> Vec<(String, f64)> {
        let mut scored: Vec<(String, f64)> = vectors
            .iter()
            .map(|(key, v)| (key.clone(), reference_cosine(query, v)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored.truncate(k);
        scored
    }

    fn assert_docs_eq(actual: &[ScoredDoc], expected: &[(String, f64)]) {
        assert_eq!(actual.len(), expected.len());
        for (got, (key, score)) in actual.iter().zip(expected) {
            assert_eq!(got.doc_key, *key);
            assert!(
                (got.score - score).abs() < 1e-12,
                "score for {key}: got {} expected {score}",
                got.score
            );
        }
    }

    fn run(
        cache: &ArtifactCache,
        rows: Vec<SemanticManifestRow>,
        query: &[f32],
        space: &VectorSpace,
        filter: &HardScope,
        k: usize,
        batch_rows: usize,
    ) -> CcResult<Vec<ScoredDoc>> {
        search(
            cache,
            &MemSource::new(rows),
            ExactSearch {
                space,
                query,
                filter,
                k,
                batch_rows,
            },
        )
    }

    struct InterruptedSource {
        inner: MemSource,
        control: QueryControl,
        cancel_on_call: Option<usize>,
        delay: std::time::Duration,
    }

    impl ManifestCandidates for InterruptedSource {
        fn next_batch(&self, after: &str, batch_rows: usize) -> CcResult<Vec<SemanticManifestRow>> {
            let rows = self.inner.next_batch(after, batch_rows)?;
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            if self.cancel_on_call == Some(self.inner.batches.borrow().len()) {
                self.control.cancel();
            }
            Ok(rows)
        }
    }

    fn controlled_fixture(
        tag: &str,
    ) -> (
        TempCache,
        ArtifactCache,
        VectorSpace,
        Vec<SemanticManifestRow>,
    ) {
        let (temp, cache) = TempCache::open(tag);
        let space = space(2);
        let digest = space.digest().unwrap();
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .unwrap()
            .digest()
            .unwrap();
        let rows = ["a", "b", "c"]
            .into_iter()
            .map(|key| {
                let input = InputDigest::new(key.to_string());
                let reference = cache
                    .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
                    .unwrap();
                row(
                    key,
                    &format!("src/{key}.rs"),
                    "rust",
                    reference.as_str(),
                    digest.as_str(),
                )
            })
            .collect();
        (temp, cache, space, rows)
    }

    #[test]
    fn read_coverage_counts_eligible_gaps_and_resets_for_excluded_rows() {
        let (temp, cache, space, mut rows) = controlled_fixture("read-coverage");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .unwrap()
            .digest()
            .unwrap();
        assert!(cache
            .discard(&space, &InputDigest::new("b"), &spec)
            .unwrap());
        let corrupt_path = temp
            .root
            .join(format!("namespace-{}", cache.namespace()))
            .join(space.digest().unwrap().as_str())
            .join("c")
            .join(spec.as_str())
            .join(format!("{}.bin", spec.as_str()));
        std::fs::write(corrupt_path, [0u8; 8]).unwrap();
        let mut foreign_row = rows[1].clone();
        foreign_row.doc_key = "d".into();
        foreign_row.space_id = "different-space".into();
        let mut excluded_row = rows[1].clone();
        excluded_row.doc_key = "e".into();
        excluded_row.file_path = "outside/e.rs".into();
        let mut malformed_ref = rows[1].clone();
        malformed_ref.doc_key = "f".into();
        malformed_ref.file_path = "src/f.rs".into();
        malformed_ref.artifact_ref = "invalid-reference".into();
        let mut foreign_ref = rows[0].clone();
        foreign_ref.doc_key = "g".into();
        foreign_ref.file_path = "src/g.rs".into();
        foreign_ref.artifact_ref = foreign_ref
            .artifact_ref
            .replace(space.digest().unwrap().as_str(), "different-space");
        rows.extend([foreign_row, excluded_row, malformed_ref, foreign_ref]);
        let filter = HardScope {
            path_prefix: Some("src/".into()),
            ..scope()
        };
        let expected = ExactSearchResult {
            scored: vec![ScoredDoc {
                doc_key: "a".into(),
                score: 1.0,
            }],
            missing_artifacts: 1,
            corrupt_artifacts: 1,
            rejected_artifact_refs: 2,
        };
        for batch_rows in [1, 2, 8] {
            let result = search_controlled_with_coverage(
                &cache,
                &MemSource::new(rows.clone()),
                ExactSearch {
                    space: &space,
                    query: &[1.0, 0.0],
                    filter: &filter,
                    k: 1,
                    batch_rows,
                },
                &QueryControl::new(std::time::Duration::from_secs(5)).unwrap(),
            )
            .unwrap();
            assert_eq!(result, expected);
            assert!(result.has_unavailable_artifacts());
        }
        assert_eq!(
            run(&cache, rows.clone(), &[1.0, 0.0], &space, &filter, 1, 2).unwrap(),
            expected.scored,
            "the legacy Vec API keeps identical scores and ordering"
        );
        // The same bad rows must not taint another request's narrower scope.
        let healthy = HardScope {
            file_paths: Some(vec!["src/a.rs".into()]),
            ..scope()
        };
        for (filter, limit, expected_count) in [
            (healthy, 1, 1),
            (
                HardScope {
                    file_paths: Some(vec![]),
                    ..scope()
                },
                1,
                0,
            ),
            (scope(), 0, 0),
        ] {
            let result = search_controlled_with_coverage(
                &cache,
                &MemSource::new(rows.clone()),
                ExactSearch {
                    space: &space,
                    query: &[1.0, 0.0],
                    filter: &filter,
                    k: limit,
                    batch_rows: 2,
                },
                &QueryControl::new(std::time::Duration::from_secs(5)).unwrap(),
            )
            .unwrap();
            assert!(!result.has_unavailable_artifacts());
            assert_eq!(result.scored.len(), expected_count);
        }
    }

    #[test]
    fn controlled_scan_rejects_cancelled_and_expired_requests_before_loading() {
        let (_temp, cache, space, rows) = controlled_fixture("control-entry");
        for expired in [false, true] {
            let control = QueryControl::new(if expired {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_secs(30)
            })
            .unwrap();
            if !expired {
                control.cancel();
            }
            let source = MemSource::new(rows.clone());
            let result = search_controlled(
                &cache,
                &source,
                ExactSearch {
                    space: &space,
                    query: &[1.0, 0.0],
                    filter: &scope(),
                    k: 1,
                    batch_rows: 1,
                },
                &control,
            );
            assert!(if expired {
                matches!(result, Err(CcError::QueryTimedOut))
            } else {
                matches!(result, Err(CcError::QueryCancelled))
            });
            assert!(source.batches.borrow().is_empty());
        }
    }

    #[test]
    fn controlled_scan_discards_accumulated_top_k_on_cancellation() {
        let (_temp, cache, space, rows) = controlled_fixture("control-cancel");
        let control = QueryControl::new(std::time::Duration::from_secs(30)).unwrap();
        let source = InterruptedSource {
            inner: MemSource::new(rows),
            control: control.clone(),
            cancel_on_call: Some(2),
            delay: std::time::Duration::ZERO,
        };
        // Batch one scores a valid published vector and fills the size-one heap.
        // Cancellation while loading batch two must discard it, not return success.
        let result = search_controlled(
            &cache,
            &source,
            ExactSearch {
                space: &space,
                query: &[1.0, 0.0],
                filter: &scope(),
                k: 1,
                batch_rows: 1,
            },
            &control,
        );
        assert!(matches!(result, Err(CcError::QueryCancelled)));
        assert_eq!(source.inner.batches.borrow().len(), 2);
    }

    #[test]
    fn controlled_scan_observes_deadline_after_an_uninterruptible_batch_read() {
        let (_temp, cache, space, rows) = controlled_fixture("control-deadline");
        let control = QueryControl::new(std::time::Duration::from_millis(100)).unwrap();
        let source = InterruptedSource {
            inner: MemSource::new(rows),
            control: control.clone(),
            cancel_on_call: None,
            delay: std::time::Duration::from_millis(120),
        };
        let result = search_controlled(
            &cache,
            &source,
            ExactSearch {
                space: &space,
                query: &[1.0, 0.0],
                filter: &scope(),
                k: 1,
                batch_rows: 1,
            },
            &control,
        );
        assert!(matches!(result, Err(CcError::QueryTimedOut)));
        // One synchronous read may finish late; no further batch is admitted.
        assert_eq!(source.inner.batches.borrow().len(), 1);
    }

    #[test]
    fn controlled_scan_preserves_legacy_filter_top_k_and_score_bits() {
        let (_temp, cache, space, rows) = controlled_fixture("control-parity");
        let filter = HardScope {
            file_paths: Some(vec!["src/b.rs".into(), "src/c.rs".into()]),
            languages: Some(vec![Language::Rust]),
            ..Default::default()
        };
        let expected = run(&cache, rows.clone(), &[1.0, 0.0], &space, &filter, 1, 2).unwrap();
        for batch_rows in [1, 2, 8] {
            let control = QueryControl::new(std::time::Duration::from_secs(30)).unwrap();
            let actual = search_controlled(
                &cache,
                &MemSource::new(rows.clone()),
                ExactSearch {
                    space: &space,
                    query: &[1.0, 0.0],
                    filter: &filter,
                    k: 1,
                    batch_rows,
                },
                &control,
            )
            .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual[0].doc_key, "b");
            assert_eq!(actual[0].score.to_bits(), expected[0].score.to_bits());
        }
    }

    // ── 1. kNN correctness vs the naive reference (hand-computed gold) ───

    #[test]
    fn hand_computed_cosine_gold_and_ordering() {
        // Hand-computed, dim ≤ 4, no fake-provider numerics (brief invariant 5):
        // query q = (1, 0, 0); docs a=(1,0,0) cos=1, b=(1,1,0) cos=1/√2,
        // c=(0,1,0) cos=0, d=(-1,0,0) cos=-1.
        let (_temp, cache) = TempCache::open("gold");
        let space = space(3);
        let digest = space.digest().expect("space digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let docs: Vec<(&str, Vec<f32>)> = vec![
            ("a", vec![1.0, 0.0, 0.0]),
            ("b", vec![1.0, 1.0, 0.0]),
            ("c", vec![0.0, 1.0, 0.0]),
            ("d", vec![-1.0, 0.0, 0.0]),
        ];
        let mut rows = Vec::new();
        for (key, vector) in &docs {
            let input = InputDigest::new((*key).to_string());
            let reference = cache
                .put(&space, &input, &spec, vector, 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let query = [1.0_f32, 0.0, 0.0];
        let result = run(&cache, rows, &query, &space, &scope(), 4, 2).expect("search");
        let inv_sqrt2 = 1.0_f64 / 2.0_f64.sqrt();
        assert_docs_eq(
            &result,
            &[
                ("a".to_string(), 1.0),
                ("b".to_string(), inv_sqrt2),
                ("c".to_string(), 0.0),
                ("d".to_string(), -1.0),
            ],
        );
    }

    #[test]
    fn knn_matches_naive_reference_on_deterministic_corpus() {
        let (_temp, cache) = TempCache::open("oracle");
        let space = space(8);
        let digest = space.digest().expect("space digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let mut rng = Lcg(42);
        let corpus: Vec<(String, Vec<f32>)> = (0..40)
            .map(|i| (format!("doc-{i:03}"), rng.vector(8)))
            .collect();
        let mut rows = Vec::new();
        for (key, vector) in &corpus {
            let input = InputDigest::new(key.clone());
            let reference = cache
                .put(&space, &input, &spec, vector, 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let mut query_rng = Lcg(7);
        let query = query_rng.vector(8);

        let expected = reference_top_k(&query, &corpus, 7);
        let actual = run(&cache, rows, &query, &space, &scope(), 7, 3).expect("search");
        assert_docs_eq(&actual, &expected);
    }

    #[test]
    fn result_is_independent_of_batch_splitting() {
        let (_temp, cache) = TempCache::open("batches");
        let space = space(8);
        let digest = space.digest().expect("space digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let mut rng = Lcg(99);
        let corpus: Vec<(String, Vec<f32>)> = (0..17)
            .map(|i| (format!("doc-{i:02}"), rng.vector(8)))
            .collect();
        let mut rows = Vec::new();
        for (key, vector) in &corpus {
            let input = InputDigest::new(key.clone());
            let reference = cache
                .put(&space, &input, &spec, vector, 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let mut query_rng = Lcg(5);
        let query = query_rng.vector(8);

        let baseline = run(&cache, rows.clone(), &query, &space, &scope(), 5, 4).expect("baseline");
        for batch_rows in [1_usize, 2, 16, 17, 100] {
            let other = run(
                &cache,
                rows.clone(),
                &query,
                &space,
                &scope(),
                5,
                batch_rows,
            )
            .expect("batched run");
            assert_eq!(
                baseline, other,
                "batch_rows={batch_rows} changed the result"
            );
        }
    }

    // ── 2. Metric dispatch per the frozen spec ───────────────────────────

    #[test]
    fn metric_dispatch_is_cosine_per_the_frozen_spec() {
        // spec v1 admits exactly one metric; the dispatch match in
        // `score_candidate` is exhaustive, so L2/inner product cannot exist
        // until the spec is bumped and the branch is written.
        assert_eq!(space(4).distance(), crate::spec::DistanceMetric::Cosine);
        let (_temp, cache) = TempCache::open("metric");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        // Orthogonal vectors: cosine must be exactly 0.0 — an L2 or inner
        // -product backend would rank these differently (nonzero distance /
        // zero dot), so a wrong metric dispatch would flip this result.
        let input = InputDigest::new("ortho".to_string());
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
            .expect("put");
        let rows = vec![row(
            "ortho",
            "ortho.rs",
            "rust",
            reference.as_str(),
            digest.as_str(),
        )];
        let result = run(&cache, rows, &[0.0, 1.0], &space, &scope(), 1, 1).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].score, 0.0);
    }

    // ── 3. Filter combination semantics (filter before top-k, C09) ───────

    #[test]
    fn filter_none_admits_every_candidate() {
        let (_temp, cache) = TempCache::open("scope-none");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let mut rows = Vec::new();
        for key in ["a", "b", "c"] {
            let input = InputDigest::new(key.to_string());
            let reference = cache
                .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
                .expect("put");
            rows.push(row(
                key,
                &format!("src/{key}.py"),
                "python",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let result = run(&cache, rows, &[1.0, 0.0], &space, &scope(), 10, 2).expect("search");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn some_empty_scope_short_circuits_without_loading_candidates() {
        // C09: Some(empty) is an empty set and must never degrade to a
        // full-repo scan — zero batches are requested at all.
        let source = MemSource::new(vec![row(
            "a",
            "a.rs",
            "rust",
            "cas.v1:ns:sp:in:spec:ck",
            "sp",
        )]);
        let (_temp, cache) = TempCache::open("scope-empty");
        let space = space(2);
        let empty_paths = HardScope {
            path_prefix: None,
            languages: None,
            file_paths: Some(Vec::new()),
        };
        let result = search(
            &cache,
            &source,
            ExactSearch {
                space: &space,
                query: &[1.0, 0.0],
                filter: &empty_paths,
                k: 10,
                batch_rows: 4,
            },
        )
        .expect("search");
        assert!(result.is_empty());
        assert!(source.batches.borrow().is_empty(), "no batch may be loaded");
    }

    #[test]
    fn prefix_file_paths_and_languages_intersect_before_top_k() {
        let (_temp, cache) = TempCache::open("scope-intersect");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let corpus = [
            ("a", "src/a.rs", "rust"),
            ("b", "src/b.py", "python"),
            ("c", "lib/c.rs", "rust"),
            ("d", "src/d.rs", "rust"),
        ];
        let mut rows = Vec::new();
        for (key, path, language) in corpus {
            let input = InputDigest::new(key.to_string());
            let reference = cache
                .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
                .expect("put");
            rows.push(row(
                key,
                path,
                language,
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let query = [1.0_f32, 0.0];

        // prefix only: src/ → a, b, d.
        let prefix = HardScope {
            path_prefix: Some("src".into()),
            languages: None,
            file_paths: None,
        };
        let got = run(&cache, rows.clone(), &query, &space, &prefix, 10, 2).expect("prefix");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["a", "b", "d"]
        );

        // prefix ∩ languages(rust) → a, d.
        let prefix_rust = HardScope {
            path_prefix: Some("src".into()),
            languages: Some(vec![Language::Rust]),
            file_paths: None,
        };
        let got =
            run(&cache, rows.clone(), &query, &space, &prefix_rust, 10, 2).expect("prefix+lang");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["a", "d"]
        );

        // prefix ∩ languages ∩ file_paths([b, d]) → d only (b fails language).
        let full = HardScope {
            path_prefix: Some("src".into()),
            languages: Some(vec![Language::Rust]),
            file_paths: Some(vec!["src/b.py".into(), "src/d.rs".into()]),
        };
        let got = run(&cache, rows, &query, &space, &full, 10, 2).expect("full intersect");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["d"]
        );
    }

    #[test]
    fn missing_language_row_matches_no_language_filter() {
        // `files` row absent → language None → a languages-filtered query must
        // exclude it (never "matches all"), an unfiltered query may admit it.
        let (_temp, cache) = TempCache::open("scope-lang-none");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let mut orphan = row(
            "x",
            "x.rs",
            "rust",
            "cas.v1:ns:sp:in:spec:ck",
            digest.as_str(),
        );
        // points at a real object so it would be scorable if admitted
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let input = InputDigest::new("x".to_string());
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
            .expect("put");
        orphan.artifact_ref = reference.as_str().to_string();

        let languages = HardScope {
            path_prefix: None,
            languages: Some(vec![Language::Rust]),
            file_paths: None,
        };
        let mut no_language = orphan.clone();
        no_language.language = None;
        let got = run(
            &cache,
            vec![no_language.clone()],
            &[1.0, 0.0],
            &space,
            &languages,
            10,
            1,
        )
        .expect("filtered");
        assert!(got.is_empty(), "None language must fail a languages filter");
        let got = run(
            &cache,
            vec![no_language],
            &[1.0, 0.0],
            &space,
            &scope(),
            10,
            1,
        )
        .expect("unfiltered");
        assert_eq!(got.len(), 1);
    }

    // ── 4. Boundary cases ────────────────────────────────────────────────

    #[test]
    fn empty_and_single_element_boundaries() {
        let (_temp, cache) = TempCache::open("edges");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");

        // Empty candidate set: no batches with content, empty result.
        let got = run(&cache, Vec::new(), &[1.0, 0.0], &space, &scope(), 5, 3).expect("empty");
        assert!(got.is_empty());

        // Single element, k larger than the corpus. (3,4)/(4,3) are exact in
        // f32, so the hand value 24/25 = 0.96 holds to the last f64 bit.
        let input = InputDigest::new("only".to_string());
        let reference = cache
            .put(&space, &input, &spec, &[3.0, 4.0], 1_000)
            .expect("put");
        let rows = vec![row(
            "only",
            "only.rs",
            "rust",
            reference.as_str(),
            digest.as_str(),
        )];
        let got = run(&cache, rows, &[4.0, 3.0], &space, &scope(), 5, 3).expect("single");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].score, 0.96, "cos((4,3),(3,4)) = 24/25 exactly");

        // All candidates filtered out: empty result, not an error.
        let everything = HardScope {
            path_prefix: None,
            languages: None,
            file_paths: Some(vec!["other.rs".into()]),
        };
        let input = InputDigest::new("only".to_string());
        let rows = vec![row(
            "only",
            "only.rs",
            "rust",
            cache
                .put(&space, &input, &spec, &[3.0, 4.0], 1_000)
                .expect("put")
                .as_str(),
            digest.as_str(),
        )];
        let got = run(&cache, rows, &[4.0, 3.0], &space, &everything, 5, 3).expect("all filtered");
        assert!(got.is_empty());
    }

    #[test]
    fn degenerate_inputs_are_rejected_or_yield_empty() {
        let (_temp, cache) = TempCache::open("degenerate");
        let space = space(2);

        // k = 0 → empty; batch_rows = 0 → typed error; dimension mismatch →
        // typed error.
        let got = run(&cache, Vec::new(), &[1.0, 0.0], &space, &scope(), 0, 1).expect("k=0");
        assert!(got.is_empty());
        let err = run(&cache, Vec::new(), &[1.0, 0.0], &space, &scope(), 1, 0).unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        let err = run(&cache, Vec::new(), &[1.0, 0.0, 0.0], &space, &scope(), 1, 1).unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));

        // Zero query vector: cosine undefined everywhere → empty, no NaN.
        let got = run(&cache, Vec::new(), &[0.0, 0.0], &space, &scope(), 1, 1).expect("zero query");
        assert!(got.is_empty());
        // ... and scores are finite even with candidates present.
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let input = InputDigest::new("d".to_string());
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
            .expect("put");
        let rows = vec![row(
            "d",
            "d.rs",
            "rust",
            reference.as_str(),
            space.digest().expect("d").as_str(),
        )];
        let got =
            run(&cache, rows, &[0.0, 0.0], &space, &scope(), 1, 1).expect("zero query w/ docs");
        assert!(got.is_empty());

        // Zero document vector among valid ones: the zero doc is skipped.
        let zero_input = InputDigest::new("zero-doc".to_string());
        let zero_ref = cache
            .put(&space, &zero_input, &spec, &[0.0, 0.0], 1_000)
            .expect("put zero");
        let mut rows = vec![row(
            "zero-doc",
            "zero.rs",
            "rust",
            zero_ref.as_str(),
            space.digest().expect("d").as_str(),
        )];
        let input = InputDigest::new("d".to_string());
        rows.push(row(
            "d",
            "d.rs",
            "rust",
            cache
                .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
                .expect("put")
                .as_str(),
            space.digest().expect("d").as_str(),
        ));
        let got = run(&cache, rows, &[1.0, 0.0], &space, &scope(), 5, 1).expect("zero doc");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["d"],
            "zero-magnitude document must be skipped, never NaN-scored"
        );
    }

    // ── 5. Determinism and tie-break ─────────────────────────────────────

    #[test]
    fn equal_scores_tie_break_on_doc_key_ascending() {
        let (_temp, cache) = TempCache::open("ties");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        // Three identical vectors → identical scores; the total order must
        // rank them purely by doc_key ascending, for every k.
        let mut rows = Vec::new();
        for key in ["zz", "aa", "mm"] {
            let input = InputDigest::new(key.to_string());
            let reference = cache
                .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        for k in [1_usize, 2, 3] {
            let got = run(&cache, rows.clone(), &[1.0, 0.0], &space, &scope(), k, 1).expect("ties");
            let keys: Vec<_> = got.iter().map(|d| d.doc_key.as_str()).collect();
            assert_eq!(
                keys,
                ["aa", "mm", "zz"][..k.min(3)],
                "k={k} must take the lexicographically first docs"
            );
            assert!(got.windows(2).all(|w| w[0].score == w[1].score));
        }
        // Reversed insertion order must not change the outcome.
        rows.reverse();
        let got = run(&cache, rows, &[1.0, 0.0], &space, &scope(), 3, 1).expect("ties reversed");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["aa", "mm", "zz"]
        );
    }

    #[test]
    fn repeated_runs_are_bitwise_deterministic() {
        let (_temp, cache) = TempCache::open("determinism");
        let space = space(8);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let mut rng = Lcg(1234);
        let corpus: Vec<(String, Vec<f32>)> = (0..23)
            .map(|i| (format!("doc-{i:02}"), rng.vector(8)))
            .collect();
        let mut rows = Vec::new();
        for (key, vector) in &corpus {
            let input = InputDigest::new(key.clone());
            let reference = cache
                .put(&space, &input, &spec, vector, 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let mut query_rng = Lcg(77);
        let query = query_rng.vector(8);
        let a = run(&cache, rows.clone(), &query, &space, &scope(), 6, 5).expect("run a");
        let b = run(&cache, rows, &query, &space, &scope(), 6, 5).expect("run b");
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.doc_key, y.doc_key);
            assert_eq!(x.score.to_bits(), y.score.to_bits(), "bitwise determinism");
        }
    }

    // ── 6. Bounded batch ─────────────────────────────────────────────────

    #[test]
    fn batch_loads_never_exceed_the_bounded_batch_knob() {
        let (_temp, cache) = TempCache::open("bounded");
        let space = space(4);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let mut rng = Lcg(31337);
        let corpus: Vec<(String, Vec<f32>)> = (0..11)
            .map(|i| (format!("doc-{i:02}"), rng.vector(4)))
            .collect();
        let mut rows = Vec::new();
        for (key, vector) in &corpus {
            let input = InputDigest::new(key.clone());
            let reference = cache
                .put(&space, &input, &spec, vector, 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        let source = MemSource::new(rows);
        let mut query_rng = Lcg(9);
        let query = query_rng.vector(4);
        let got = search(
            &cache,
            &source,
            ExactSearch {
                space: &space,
                query: &query,
                filter: &scope(),
                k: 3,
                batch_rows: 4,
            },
        )
        .expect("search");
        assert_eq!(got.len(), 3);
        let batches = source.batches.borrow();
        assert!(
            batches.iter().all(|&b| b == 4),
            "every request ≤ batch_rows"
        );
        // 11 rows / batch 4 → keyset walks 4,4,3 then stops on the empty batch
        // (the MemSource records the request, so 4 requests total).
        assert_eq!(
            batches.len(),
            4,
            "one request per keyset step incl. terminator"
        );
    }

    // ── 7. Space isolation, deletion, and degradation ────────────────────

    #[test]
    fn foreign_space_and_foreign_ref_rows_never_score() {
        let (_temp, cache) = TempCache::open("isolation");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let other_space = VectorSpace::new("fake/model-other", 2).expect("valid space");
        let other_digest = other_space.digest().expect("other digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");

        let input = InputDigest::new("mine".to_string());
        let mine = cache
            .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
            .expect("put");
        let rows = vec![
            // Row published under a different space id — the load layer (SQL
            // WHERE space_id) excludes it; the ref check here is defense in
            // depth.
            row(
                "foreign-space",
                "f.rs",
                "rust",
                mine.as_str(),
                other_digest.as_str(),
            ),
            // Correct space id but the ref carries a foreign space digest.
            row(
                "foreign-ref",
                "g.rs",
                "rust",
                &format!("cas.v1:ns-digest:{}:in:spec:ck", other_digest.as_str()),
                digest.as_str(),
            ),
            // Unparseable ref.
            row("bad-ref", "h.rs", "rust", "not-a-ref", digest.as_str()),
            // Ref input digest disagrees with the row's input digest.
            row(
                "mismatched-input",
                "i.rs",
                "rust",
                &format!(
                    "cas.v1:ns-digest:{}:other-input:{}:ck",
                    digest.as_str(),
                    spec.as_str()
                ),
                digest.as_str(),
            ),
            // The one legitimate row.
            row("mine", "m.rs", "rust", mine.as_str(), digest.as_str()),
        ];
        let got = run(&cache, rows, &[1.0, 0.0], &space, &scope(), 5, 2).expect("search");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["mine"],
            "only the same-space, self-consistent row may score"
        );
    }

    #[test]
    fn deleted_or_corrupt_cache_objects_degrade_without_failing() {
        let (temp, cache) = TempCache::open("degrade");
        let space = space(2);
        let digest = space.digest().expect("digest");
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "t")
            .expect("spec")
            .digest()
            .expect("spec digest");
        let mut rows = Vec::new();
        for key in ["gone", "corrupt", "alive"] {
            let input = InputDigest::new(key.to_string());
            let reference = cache
                .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
                .expect("put");
            rows.push(row(
                key,
                format!("{key}.rs").as_str(),
                "rust",
                reference.as_str(),
                digest.as_str(),
            ));
        }
        // "gone": discarded after publish (cache Miss → skipped).
        let gone_input = InputDigest::new("gone".to_string());
        assert!(cache.discard(&space, &gone_input, &spec).expect("discard"));
        // "corrupt": payload tampered after publish (checksum → Corrupt →
        // skipped, never served and never an error).
        let corrupt_input = InputDigest::new("corrupt".to_string());
        let corrupt_path = temp
            .root
            .join(format!("namespace-ns-degrade/{}", digest.as_str()))
            .join(corrupt_input.as_str())
            .join(spec.as_str())
            .join(format!("{}.bin", spec.as_str()));
        std::fs::write(&corrupt_path, [0u8, 0, 0, 0, 0, 0, 0, 0]).expect("tamper");

        let got = run(&cache, rows, &[1.0, 0.0], &space, &scope(), 5, 1).expect("search");
        assert_eq!(
            got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
            ["alive"],
            "missing/corrupt artifacts degrade to skip, only healthy rows score"
        );
    }

    // ── 8. FakeProvider integration loop (embed → cache → search) ────────

    #[test]
    fn fake_provider_embed_cache_search_hits_its_own_documents() {
        let (_temp, cache) = TempCache::open("provider-loop");
        let space = space(8);
        let provider = FakeProvider::new(FakeProviderConfig::new(space.clone()));
        let doc_spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
            .expect("doc spec");
        let spec_digest = doc_spec.digest().expect("doc spec digest");

        let texts = ["alpha payload", "beta payload", "gamma payload"];
        let inputs: Vec<_> = texts
            .iter()
            .map(|t| DocInput::from_bytes(t.as_bytes()).expect("doc input"))
            .collect();
        let vectors = provider.embed_documents(&inputs).expect("embed");
        let mut corpus: Vec<(String, Vec<f32>)> = Vec::new();
        let mut rows = Vec::new();
        for (input, vector) in inputs.iter().zip(&vectors) {
            let reference = cache
                .put(&space, &input.input_digest, &spec_digest, vector, 1_000)
                .expect("put");
            let key = format!(
                "doc-{}",
                input.input_digest.as_str().get(..8).unwrap_or("x")
            );
            corpus.push((key.clone(), vector.to_vec()));
            let mut candidate = row(
                &key,
                &format!("{key}.rs"),
                "rust",
                reference.as_str(),
                space.digest().expect("digest").as_str(),
            );
            // The manifest row must carry the same embedded-input digest the
            // artifact was published under (the cache address check reads it).
            candidate.input_digest = input.input_digest.as_str().to_string();
            rows.push(candidate);
        }

        let query_bytes = b"gamma payload";
        let query_input = QueryInput::from_bytes(query_bytes).expect("query input");
        let query_vector = &provider.embed_queries(&[query_input]).expect("embed query")[0];

        let expected = reference_top_k(query_vector, &corpus, 3);
        let got = run(&cache, rows, query_vector, &space, &scope(), 3, 2).expect("search");
        assert_docs_eq(&got, &expected);
        // The embedded query itself is never a document: doc/query paths are
        // domain separated by the provider and only document rows were loaded.
        assert!(got
            .iter()
            .all(|d| corpus.iter().any(|(key, _)| *key == d.doc_key)));
    }
}
