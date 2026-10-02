//! Content-addressed artifact cache (P6-008).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`.
//! This is the single sanctioned exception to the single-authoritative-DB rule:
//! a *derivable, discardable, verifiable* store for paid embedding artifacts.
//! It is local files, not a service; it is never an authority for source or
//! manifest content; it stores no secrets — the payload is exactly the derived
//! vector bytes and the sidecar metadata is exactly the seven documented
//! addressing/provenance fields (see [`ObjectMeta`]); no credential, key or
//! token field exists, and no environment variable is read anywhere on the
//! put/get path.
//!
//! Addressing is the path (TASK-BRIEFS P6-008 layout): the tuple
//! `(namespace, space_id, input_digest, doc_spec_digest)` locates an object;
//! the blake3 checksum lives in the sidecar `.meta.json` and is verified
//! separately on every read (ADR: "namespace 与 input/spec/space/checksum 分开
//! 验证") — corruption is detected, never served.
//!
//! Namespace semantics (OPEN-QUESTIONS Q5, user decision 2026-10-02): a
//! **global cache root + project-identity namespace**, deliberately *not*
//! bound to `index_incarnation` — a rebuilt index (P6-014) resolves the same
//! namespace and reuses already-paid vectors. The project identity is a
//! caller-supplied string (`namespace_key`); today's only existing identity
//! source in the workspace is the canonical project path (the same source
//! `cc_model::config::project_cache_key` uses), supplied by the composition
//! root — no lookup order against `index.sqlite3` exists, so cache opening
//! never depends on DB state. If a project identity ever moves into
//! authoritative records, the resolution order becomes: open DB → read
//! identity → derive namespace → open cache; this module does not change.
//!
//! Durability scope: writes are atomic (temp file + fsync + rename, metadata
//! last as the completeness marker), which guarantees "atomic visibility on
//! the normal path". Crash-proofness is *not* claimed here (P6-015 owns real
//! kill/restart verification; the parent-directory fsync below is best-effort).
//!
//! Root resolution follows platform conventions ([`resolve_cache_root`]):
//! macOS `~/Library/Caches`, Linux `$XDG_CACHE_HOME|~/.cache`, both under
//! `codecortex/semantic`, with an explicit env override
//! ([`CACHE_ROOT_ENV`]) that doubles as the test seam. [`ArtifactCache::open`]
//! creates nothing (P6-002 acceptance: no empty cache directories); the
//! layout is created lazily by the first [`ArtifactCache::put`].

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use cc_model::identity::{bytes_hash, hash};
use cc_model::{CcError, CcResult};

use crate::admission::{plan_query_batches, InputBudget, OversizeReason, PlannedQueryInput};
use crate::error::SemanticError;
use crate::ports::{EmbeddingProvider, ProviderError, QueryInput};
use crate::spec::{QueryEncodingSpec, VectorSpace};
use crate::types::{
    ArtifactRef, DocSpecDigest, InputDigest, QueryDigest, QuerySpecDigest, SpaceDigest,
};

/// Sidecar/object format version. Bumping it is a breaking cache-format
/// change: old objects fail the `format_version` check and read as
/// [`CacheRead::Corrupt`], i.e. discardable derivates, never silent garbage.
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// Directory prefix of a project namespace under the cache root
/// (`<root>/namespace-<ns>/`, TASK-BRIEFS P6-008 layout).
pub const NAMESPACE_DIR_PREFIX: &str = "namespace-";

/// Quarantine directory convention (P6-018). Declared here so the layout is
/// fixed now; this module never creates it and never moves objects into it.
pub const QUARANTINE_DIR: &str = "quarantine";

/// Explicit cache-root override (first resolution priority). Exists so
/// deployment and tests can pin the root; the put/get path itself reads no
/// environment variables.
pub const CACHE_ROOT_ENV: &str = "CODECORTEX_SEMANTIC_CACHE_ROOT";

/// Domain tag of the namespace key digest — digest-domain separation per
/// `crates/cc-semantic/docs/ENCODING-SPACE.md` §3.
const NAMESPACE_DOMAIN: &str = "cc-semantic.cache-namespace.v1";

const MAX_NAMESPACE_BYTES: usize = 128;

/// `artifact_ref` scheme prefix. A ref is
/// `cas.v1:<namespace>:<space_id>:<input_digest>:<spec_digest>:<checksum>` —
/// the ADR addressing tuple including the payload checksum. The ref is what
/// P6-005's `semantic_manifest.artifact_ref` stores; the checksum component
/// lets a later round re-verify a ref without the file it points at.
const REF_SCHEME: &str = "cas.v1";

fn invalid(message: impl Into<String>) -> cc_model::CcError {
    SemanticError::InvalidInput(message.into()).into()
}

/// Project-identity namespace key: `blake3(NAMESPACE_DOMAIN, identity)` hex.
///
/// The identity string is supplied by the caller (composition root); the
/// digest keeps absolute paths out of `~/.cache` directory names and yields a
/// fixed, path-safe token. Stable per identity, distinct across identities.
pub fn namespace_key(project_identity: &str) -> CcResult<String> {
    if project_identity.trim().is_empty() {
        return Err(invalid(
            "project identity must be a non-empty string to derive a cache namespace",
        ));
    }
    hash(&(NAMESPACE_DOMAIN, project_identity))
}

fn validate_namespace(namespace: &str) -> CcResult<()> {
    let ok = !namespace.is_empty()
        && namespace.len() <= MAX_NAMESPACE_BYTES
        && namespace
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !namespace.starts_with('.')
        && !namespace.contains("..");
    if ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "cache namespace must be 1..={MAX_NAMESPACE_BYTES} chars of [A-Za-z0-9._-] without leading '.' or \"..\" substrings"
        )))
    }
}

/// Cache-root resolution priority (TASK-BRIEFS P6-008 risk item, Q5):
/// 1. [`CACHE_ROOT_ENV`] override (also the test/deployment seam);
/// 2. platform convention under `$HOME`: macOS `~/Library/Caches/codecortex/semantic`,
///    Linux `$XDG_CACHE_HOME|~/.cache/codecortex/semantic`.
///
/// `None` means "no default derivable" — callers must then pass an explicit
/// root to [`ArtifactCache::open`]. Resolves only; creates nothing.
pub fn resolve_cache_root() -> Option<PathBuf> {
    resolve_cache_root_with(
        |var| {
            std::env::var(var)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        },
        cfg!(target_os = "macos"),
    )
}

/// Injectable form of [`resolve_cache_root`]: `lookup` supplies environment
/// values, `macos_layout` selects the platform branch (`cfg!(target_os =
/// "macos")` in production) so every branch is testable on any host.
pub fn resolve_cache_root_with(
    lookup: impl Fn(&str) -> Option<String>,
    macos_layout: bool,
) -> Option<PathBuf> {
    if let Some(root) = lookup(CACHE_ROOT_ENV)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Some(PathBuf::from(root));
    }
    let home = lookup("HOME")?;
    if macos_layout {
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Caches")
                .join("codecortex")
                .join("semantic"),
        )
    } else {
        let base = lookup("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".cache"));
        Some(base.join("codecortex").join("semantic"))
    }
}

/// Sidecar metadata. The field set is closed and documented; it is the
/// no-secrets contract of this module — nothing beyond these seven provenance
/// fields may ever be persisted here. Serialization is canonical by struct
/// field order (C03; no maps involved).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectMeta {
    format_version: u32,
    dimension: u32,
    model_id: String,
    /// blake3 hex of the `.bin` payload; verified on every read.
    checksum: String,
    /// Unix seconds, supplied by the caller (clock discipline, P6-007 style).
    created_at: i64,
    input_digest: String,
    space_id: String,
    spec_digest: String,
}

/// A verified cache hit: the payload bytes decoded, dimension-checked and
/// checksum-proven.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedVector {
    pub artifact_ref: ArtifactRef,
    pub dimension: u32,
    pub data: Vec<f32>,
}

/// Outcome of [`ArtifactCache::get`]. `Corrupt` carries a diagnostic report;
/// P6-008 does not quarantine or delete on detection (P6-018 owns the
/// quarantine move) — callers degrade via [`ArtifactCache::discard`].
#[derive(Debug, Clone, PartialEq)]
pub enum CacheRead {
    Hit(ValidatedVector),
    Miss,
    Corrupt(CorruptReport),
}

/// Diagnostic for a detected-corrupt object (ADR: "损坏可检测且只降级不污染").
#[derive(Debug, Clone, PartialEq)]
pub struct CorruptReport {
    pub path: PathBuf,
    pub reason: String,
}

/// The artifact cache: `<root>/namespace-<ns>/<space_id>/<input>/<spec>.{bin,meta.json}`.
///
/// Open is side-effect free; [`ArtifactCache::put`] creates the layout
/// lazily. All methods are thread-safe (content-addressed ids make concurrent
/// writes converge: unique temp names + atomic rename, identical payload).
pub struct ArtifactCache {
    root: PathBuf,
    namespace: String,
}

impl ArtifactCache {
    /// Open a namespace without creating anything on disk (P6-002 acceptance
    /// "不生成空缓存目录", carried into P6-008). Fails on a namespace that is
    /// not a safe single path component.
    pub fn open(root: impl Into<PathBuf>, namespace: String) -> CcResult<Self> {
        validate_namespace(&namespace)?;
        Ok(Self {
            root: root.into(),
            namespace,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Quarantine directory convention (P6-018). Reported, never created by
    /// this module.
    pub fn quarantine_dir(&self) -> PathBuf {
        self.root.join(QUARANTINE_DIR)
    }

    /// Read a validated vector for `(space, input, spec)`.
    ///
    /// Verification chain: sidecar JSON parse → format version → addressing
    /// triple matches the requested tuple → blake3 payload checksum → payload
    /// length vs `dimension` → dimension/model vs the frozen [`VectorSpace`] →
    /// finite f32 decode. Any failure past the file-existence gate is
    /// [`CacheRead::Corrupt`], never an error and never a partial payload.
    pub fn get(
        &self,
        space: &VectorSpace,
        input: &InputDigest,
        spec: &DocSpecDigest,
    ) -> CcResult<CacheRead> {
        space.validate()?;
        let space_digest = space.digest()?;
        let dir = self.object_dir(&space_digest, input, spec);
        let bin_path = dir.join(format!("{}.bin", spec.as_str()));
        let meta_path = dir.join(format!("{}.meta.json", spec.as_str()));

        let (payload, meta_raw) = match (std::fs::read(&bin_path), std::fs::read(&meta_path)) {
            (Ok(p), Ok(m)) => (p, m),
            (Err(e), _) | (_, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CacheRead::Miss)
            }
            (Err(e), _) | (_, Err(e)) => return Err(e.into()),
        };

        let corrupt = |reason: String| {
            CacheRead::Corrupt(CorruptReport {
                path: bin_path.clone(),
                reason,
            })
        };

        let meta: ObjectMeta = match serde_json::from_slice(&meta_raw) {
            Ok(meta) => meta,
            Err(e) => return Ok(corrupt(format!("meta json is unreadable: {e}"))),
        };
        if meta.format_version != CACHE_FORMAT_VERSION {
            return Ok(corrupt(format!(
                "meta format_version {} is not the supported {}",
                meta.format_version, CACHE_FORMAT_VERSION
            )));
        }
        if meta.space_id != space_digest.as_str()
            || meta.input_digest != input.as_str()
            || meta.spec_digest != spec.as_str()
        {
            return Ok(corrupt(
                "meta addressing triple does not match the addressed location".to_string(),
            ));
        }
        let checksum = bytes_hash(&payload);
        if meta.checksum != checksum {
            return Ok(corrupt(format!(
                "payload checksum mismatch: meta {} vs actual {checksum}",
                meta.checksum
            )));
        }
        if payload.len() != meta.dimension as usize * 4 {
            return Ok(corrupt(format!(
                "payload byte length {} does not match meta dimension {}",
                payload.len(),
                meta.dimension
            )));
        }
        if meta.dimension != space.dimension() || meta.model_id != space.model_id() {
            return Ok(corrupt(
                "meta dimension/model_id do not match the requested frozen space".to_string(),
            ));
        }
        let mut data = Vec::with_capacity(payload.len() / 4);
        for chunk in payload.as_chunks::<4>().0 {
            data.push(f32::from_le_bytes(*chunk));
        }
        if data.iter().any(|v| !v.is_finite()) {
            return Ok(corrupt("payload contains non-finite values".to_string()));
        }
        Ok(CacheRead::Hit(ValidatedVector {
            artifact_ref: self.reference(&space_digest, input, spec, &checksum),
            dimension: meta.dimension,
            data,
        }))
    }

    /// Persist a validated vector (payload `.bin` little-endian f32, sidecar
    /// `.meta.json` written last as the completeness marker) and return its
    /// [`ArtifactRef`] — the sanctioned public constructor of that type
    /// (P6-003 留白兑现). `now_unix` is caller-supplied clock discipline.
    ///
    /// Idempotent: identical content under the same tuple converges to the
    /// same ref; concurrent writers overwrite atomically. This is the
    /// "artifact durable first" side of the ADR durability order
    /// (artifact-before-manifest).
    pub fn put(
        &self,
        space: &VectorSpace,
        input: &InputDigest,
        spec: &DocSpecDigest,
        vector: &[f32],
        now_unix: i64,
    ) -> CcResult<ArtifactRef> {
        space.validate()?;
        let space_digest = space.digest()?;
        if vector.len() != space.dimension() as usize {
            return Err(invalid(format!(
                "vector length {} does not match space dimension {}",
                vector.len(),
                space.dimension()
            )));
        }
        if vector.iter().any(|v| !v.is_finite()) {
            return Err(invalid("vector contains NaN or infinite components"));
        }

        let payload: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
        let checksum = bytes_hash(&payload);
        let meta = ObjectMeta {
            format_version: CACHE_FORMAT_VERSION,
            dimension: space.dimension(),
            model_id: space.model_id().to_string(),
            checksum: checksum.clone(),
            created_at: now_unix,
            input_digest: input.as_str().to_string(),
            space_id: space_digest.as_str().to_string(),
            spec_digest: spec.as_str().to_string(),
        };

        let dir = self.object_dir(&space_digest, input, spec);
        std::fs::create_dir_all(&dir)?;
        atomic_write(&dir.join(format!("{}.bin", spec.as_str())), &payload)?;
        atomic_write(
            &dir.join(format!("{}.meta.json", spec.as_str())),
            &serde_json::to_vec(&meta)?,
        )?;
        Ok(self.reference(&space_digest, input, spec, &checksum))
    }

    /// Discardable semantics: remove both halves of an object so subsequent
    /// reads are [`CacheRead::Miss`]. Returns whether anything was removed.
    /// Used by callers degrading from [`CacheRead::Corrupt`]; the quarantine
    /// *move* (preserve-for-diagnostics) is P6-018 and deliberately not here.
    pub fn discard(
        &self,
        space: &VectorSpace,
        input: &InputDigest,
        spec: &DocSpecDigest,
    ) -> CcResult<bool> {
        space.validate()?;
        let space_digest = space.digest()?;
        let dir = self.object_dir(&space_digest, input, spec);
        let mut removed = false;
        for suffix in [".bin", ".meta.json"] {
            match std::fs::remove_file(dir.join(format!("{}{suffix}", spec.as_str()))) {
                Ok(()) => removed = true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(removed)
    }

    fn object_dir(
        &self,
        space: &SpaceDigest,
        input: &InputDigest,
        spec: &DocSpecDigest,
    ) -> PathBuf {
        self.root
            .join(format!("{NAMESPACE_DIR_PREFIX}{}", self.namespace))
            .join(space.as_str())
            .join(input.as_str())
            .join(spec.as_str())
    }

    fn reference(
        &self,
        space: &SpaceDigest,
        input: &InputDigest,
        spec: &DocSpecDigest,
        checksum: &str,
    ) -> ArtifactRef {
        ArtifactRef::new(format!(
            "{REF_SCHEME}:{}:{}:{}:{}:{checksum}",
            self.namespace,
            space.as_str(),
            input.as_str(),
            spec.as_str()
        ))
    }
}

/// Atomic file write: unique temp name in the target directory, full write,
/// fsync, rename over the target (POSIX atomic replace; the supported
/// deployment targets are macOS/Linux), best-effort parent-dir fsync.
fn atomic_write(target: &Path, payload: &[u8]) -> CcResult<()> {
    let parent = target
        .parent()
        .ok_or_else(|| invalid("cache target must live inside a directory"))?;
    let temp = parent.join(format!(
        "{}.tmp-{}-{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("object"),
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed),
    ));
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(payload)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, target)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

// ── Bounded query-vector cache and the query encoding path (P7-009) ───────
//
// Query-side analog of the document artifact cache, one deliberate
// divergence documented on [`QueryCacheKey`]. The brief (P7-009 interface
// draft) fixes the shape: an in-process bounded LRU with DUAL hard bounds
// (entry count AND total payload bytes — "有界硬要求"), keyed by
// `(namespace, QuerySpecDigest, QueryDigest)`. The document artifact cache
// (P6-008, above) is untouched: documents never route through this cache, so
// a query-side spec/instruction change can never re-embed a document (the
// P6-003 three-way digest split).

/// Cache key of ONE encoded query vector.
///
/// Q5 ruling (this round; replaces the brief's conservative default of
/// keying on `semantic_epoch`): the key is **namespace + QuerySpecDigest +
/// QueryDigest**, deliberately WITHOUT `semantic_epoch`:
///
/// - A query vector is a pure function of (query encoding spec, query text).
///   A reconcile/backfill that advances the semantic epoch changes the
///   VISIBLE DOCUMENT SET; it does not change how query text encodes. Keying
///   on the epoch would flush every valid entry on every backfill (brief
///   risk ①) with zero correctness gain.
/// - The C12 dense row ("semantic epoch / vector-space / query encoding
///   spec") governs the dense RECALL RESULT cache, whose candidates are
///   document data; this module caches no document-derived content, so that
///   row does not reach here. Consumer-side freshness stays with the
///   dense-recall path (manifest × artifact-cache re-read per query +
///   generation verification per C12's first-version rule).
/// - The spec digest still gives total invalidation where it matters:
///   instruction/tokenizer/`max_tokens`/space changes (space INCLUDED — it
///   is a spec field, so cross-space reuse is structurally unreachable) and
///   any [`crate::spec::ENCODING_SPEC_VERSION`] bump mint new keys.
///
/// Escape hatch: this cache is in-process and discardable; if the ruling is
/// ever overturned, adding a `semantic_epoch` field here invalidates all
/// entries with no migration burden.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueryCacheKey {
    /// Project namespace ([`namespace_key`] output; the same isolation root
    /// as the document artifact cache — cross-project reuse is impossible).
    pub namespace: String,
    /// Digest of the frozen [`QueryEncodingSpec`] (space + instruction +
    /// max_tokens + tokenizer): an instruction change invalidates here,
    /// never on the document path.
    pub spec: QuerySpecDigest,
    /// Digest of the exact query text bytes (the [`QueryInput`] digest
    /// binding). Raw query text is structurally absent from the key.
    pub query: QueryDigest,
}

impl QueryCacheKey {
    /// Sanctioned constructor: the namespace must be a safe key (the
    /// [`namespace_key`] output always is), the spec digest is minted from
    /// the validated frozen spec, and the query digest comes from the
    /// digest-bound [`QueryInput`].
    pub fn new(namespace: &str, spec: &QueryEncodingSpec, query: &QueryInput) -> CcResult<Self> {
        validate_namespace(namespace)?;
        Ok(Self {
            namespace: namespace.to_string(),
            spec: spec.digest()?,
            query: query.digest.clone(),
        })
    }
}

/// One encoded query vector. Validated before anything is cached (intrinsic
/// sanity by [`QueryVectorCache::put`], space match by [`encode_queries`]) —
/// an error vector can never reach this cache, the same gate the document
/// path enforces at [`ArtifactCache::put`].
#[derive(Debug, Clone, PartialEq)]
pub struct QueryVector {
    pub dimension: u32,
    pub data: Vec<f32>,
}

impl QueryVector {
    /// Space-independent sanity: declared dimension in range, length match,
    /// finite, non-zero.
    fn validate_intrinsic(&self) -> CcResult<()> {
        if self.dimension == 0 {
            return Err(invalid("query vector dimension must be at least 1"));
        }
        if self.data.len() != self.dimension as usize {
            return Err(invalid(format!(
                "query vector length {} does not match its declared dimension {}",
                self.data.len(),
                self.dimension
            )));
        }
        if self.data.iter().any(|v| !v.is_finite()) {
            return Err(invalid("query vector contains NaN or infinite components"));
        }
        if self.data.iter().all(|v| *v == 0.0) {
            return Err(invalid("query vector is the all-zero vector"));
        }
        Ok(())
    }

    /// [`Self::validate_intrinsic`] plus the match against the frozen space
    /// the encoding claims to embed into.
    fn validate_for(&self, space: &VectorSpace) -> CcResult<()> {
        self.validate_intrinsic()?;
        if self.dimension != space.dimension() {
            return Err(invalid(format!(
                "query vector dimension {} does not match the frozen space dimension {}",
                self.dimension,
                space.dimension()
            )));
        }
        Ok(())
    }

    fn payload_bytes(&self) -> usize {
        self.data.len() * 4
    }
}

#[derive(Debug)]
struct QueryLru {
    map: HashMap<QueryCacheKey, QueryVector>,
    order: VecDeque<QueryCacheKey>,
    total_bytes: usize,
}

/// Bounded in-process LRU cache of encoded query vectors (P7-009).
///
/// Dual hard bounds: at most `max_entries` entries and at most `max_bytes`
/// payload bytes, whichever bites first; `max_entries == 0` disables storing
/// entirely (every get is a miss). An entry above the byte budget is never
/// stored (not an error: the encoder still returns the vector; only the
/// cache skips it). Thread-safe. Contains NOTHING derived from raw query
/// text — keys are digests, values are vectors.
#[derive(Debug)]
pub struct QueryVectorCache {
    max_entries: usize,
    max_bytes: usize,
    inner: Mutex<QueryLru>,
}

impl QueryVectorCache {
    /// Construct with the two hard bounds (composition-root assembled; the
    /// absolute values are a deployment decision, P7-009 brief risk ② —
    /// deliberately no `semantic.*` config key this round, P7-008 precedent).
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            inner: Mutex::new(QueryLru {
                map: HashMap::new(),
                order: VecDeque::new(),
                total_bytes: 0,
            }),
        }
    }

    /// Cached vector for `key`, refreshing its recency.
    pub fn get(&self, key: &QueryCacheKey) -> Option<QueryVector> {
        let mut lru = self.inner.lock().expect("query vector cache lock");
        let vector = lru.map.get(key).cloned()?;
        if let Some(position) = lru.order.iter().position(|k| k == key) {
            lru.order.remove(position);
        }
        lru.order.push_back(key.clone());
        Some(vector)
    }

    /// Validate and store `vector` under `key`, evicting least-recently-used
    /// entries until both bounds hold again (the entry just inserted is
    /// never the eviction victim). Storing is skipped — `Ok(())`, not an
    /// error — when storing is disabled (`max_entries == 0`) or the entry
    /// alone exceeds the byte budget.
    pub fn put(&self, key: QueryCacheKey, vector: QueryVector) -> CcResult<()> {
        vector.validate_intrinsic()?;
        let bytes = vector.payload_bytes();
        if self.max_entries == 0 || bytes > self.max_bytes {
            return Ok(());
        }
        let mut lru = self.inner.lock().expect("query vector cache lock");
        if let Some(old) = lru.map.insert(key.clone(), vector) {
            lru.total_bytes -= old.payload_bytes();
            if let Some(position) = lru.order.iter().position(|k| *k == key) {
                lru.order.remove(position);
            }
        }
        lru.total_bytes += bytes;
        lru.order.push_back(key.clone());
        // Evict from the front while either bound is violated. The guard
        // keeps the just-inserted entry (always at the back) as the last
        // survivor; it can never violate the bounds on its own (checked
        // above).
        while (lru.map.len() > self.max_entries || lru.total_bytes > self.max_bytes)
            && lru.map.len() > 1
        {
            if let Some(evicted) = lru.order.pop_front() {
                if let Some(old) = lru.map.remove(&evicted) {
                    lru.total_bytes -= old.payload_bytes();
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("query vector cache lock")
            .map
            .len()
    }

    /// Whether nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total cached payload bytes (the byte bound's current consumption).
    pub fn total_payload_bytes(&self) -> usize {
        self.inner
            .lock()
            .expect("query vector cache lock")
            .total_bytes
    }
}

/// One query encode outcome, in input order (P7-009 query encoding path).
#[derive(Debug, Clone, PartialEq)]
pub enum QueryEncodeOutcome<K> {
    /// Encoded (from the cache or freshly) and cached under the Q5 key.
    Encoded { key: K, vector: QueryVector },
    /// Refused by admission before any provider contact; explicit, never
    /// silent (query-path dual of the document-path skip ledger).
    Skipped { key: K, reason: OversizeReason },
}

/// Mirrors `queue.rs::provider_reason` (private there; this mapping adds the
/// query-path error typing on top).
fn provider_failure_reason(err: &ProviderError) -> String {
    match err {
        ProviderError::RateLimited { retry_after } => {
            format!("provider rate limited, retry after {retry_after:?}")
        }
        ProviderError::ServerError => "provider server error".into(),
        ProviderError::AuthError => "provider auth error".into(),
        ProviderError::Timeout => "provider timeout".into(),
        ProviderError::Cancelled => "provider call cancelled".into(),
        ProviderError::InvalidInput(why) => format!("provider rejected the input: {why}"),
    }
}

fn query_provider_failure(err: ProviderError) -> CcError {
    match err {
        // Cancellation and timeout have dedicated CcError variants with the
        // right retryability semantics for a query-path caller.
        ProviderError::Cancelled => CcError::QueryCancelled,
        ProviderError::Timeout => CcError::QueryTimedOut,
        other => CcError::Other(format!(
            "query embedding provider call failed: {}",
            provider_failure_reason(&other)
        )),
    }
}

/// The full query encoding path (P7-009): query text → admission's query
/// planner ([`QueryInput`] digest binding, P7-003 query-side口径) → cache
/// lookup under the Q5 key → provider encoding of the MISSES only → output
/// validation → cache put → vectors in input order.
///
/// Batch discipline: queries are cut by [`plan_query_batches`] against
/// `budget` (`spec.tokenizer()` must be the declared estimator —
/// [`crate::admission::tokenizer_gate`]); within a planned batch only the
/// misses are re-batched to the provider, so a call never exceeds its
/// planned batch. One failed batch fails the whole call (`Err`; nothing from
/// that batch is cached — P7-004 whole-batch rejection); earlier successful
/// batches keep their cached entries. Raw query text reaches only the
/// provider port — never keys, errors, or cache state.
pub fn encode_queries<K: Clone>(
    provider: &dyn EmbeddingProvider,
    cache: &QueryVectorCache,
    namespace: &str,
    spec: &QueryEncodingSpec,
    budget: &InputBudget,
    queries: &[(K, Vec<u8>)],
) -> CcResult<Vec<QueryEncodeOutcome<K>>> {
    validate_namespace(namespace)?;
    let spec_digest = spec.digest()?;
    let space = spec.space().clone();
    let plan = plan_query_batches(queries, budget, spec.tokenizer())?;

    let mut outcomes: Vec<QueryEncodeOutcome<K>> = Vec::with_capacity(plan.len());
    for planned in plan {
        match planned {
            PlannedQueryInput::Skipped { key, reason } => {
                outcomes.push(QueryEncodeOutcome::Skipped { key, reason });
            }
            PlannedQueryInput::Batch(batch) => {
                // One Q5 key per planned input, in batch order.
                let keys: Vec<QueryCacheKey> = batch
                    .items
                    .iter()
                    .map(|(_, input)| QueryCacheKey {
                        namespace: namespace.to_string(),
                        spec: spec_digest.clone(),
                        query: input.digest.clone(),
                    })
                    .collect();
                let mut slots: Vec<Option<QueryVector>> = vec![None; batch.items.len()];
                let mut misses: Vec<usize> = Vec::new();
                for (index, key) in keys.iter().enumerate() {
                    match cache.get(key) {
                        Some(vector) => slots[index] = Some(vector),
                        None => misses.push(index),
                    }
                }
                // Re-batch only the misses — a provider call never exceeds
                // its planned batch.
                if !misses.is_empty() {
                    let miss_inputs: Vec<QueryInput> = misses
                        .iter()
                        .map(|&index| batch.items[index].1.clone())
                        .collect();
                    let vectors = provider
                        .embed_queries(&miss_inputs)
                        .map_err(query_provider_failure)?;
                    if vectors.len() != miss_inputs.len() {
                        return Err(invalid(format!(
                            "provider returned {} vectors for {} query inputs",
                            vectors.len(),
                            miss_inputs.len()
                        )));
                    }
                    // 错误向量不缓存: validated BEFORE the put; a bad vector
                    // fails the whole call (P7-004 whole-batch rejection).
                    for (index, data) in misses.into_iter().zip(vectors) {
                        let vector = QueryVector {
                            dimension: space.dimension(),
                            data,
                        };
                        vector.validate_for(&space)?;
                        cache.put(keys[index].clone(), vector.clone())?;
                        slots[index] = Some(vector);
                    }
                }
                for ((key, _), vector) in batch.items.into_iter().zip(slots) {
                    let vector = vector.expect("every planned slot is filled");
                    outcomes.push(QueryEncodeOutcome::Encoded { key, vector });
                }
            }
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod decoding_boundary_tests {
    use super::*;
    use crate::spec::DocumentEncodingSpec;

    #[test]
    fn little_endian_bits_and_partial_payload_rejection_are_preserved() {
        let root = std::env::temp_dir().join(format!(
            "cc-cache-chunk-boundary-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = ArtifactCache::open(&root, "chunk-boundary".into()).unwrap();
        let space = VectorSpace::new("fake/chunk-boundary", 3).unwrap();
        let input = InputDigest::of_input(b"chunk boundary input").unwrap();
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "fake-tokenizer")
            .unwrap()
            .digest()
            .unwrap();
        // Signed zero, smallest positive subnormal and largest finite f32:
        // numeric equality alone would miss a changed signed-zero encoding.
        let bits = [0x8000_0000, 0x0000_0001, 0x7f7f_ffff];
        let data = bits.map(f32::from_bits);
        cache.put(&space, &input, &spec, &data, 1000).unwrap();
        let CacheRead::Hit(hit) = cache.get(&space, &input, &spec).unwrap() else {
            panic!("valid payload was not a hit");
        };
        assert_eq!(
            hit.data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            bits
        );
        let dir = cache.object_dir(&space.digest().unwrap(), &input, &spec);
        let bin = dir.join(format!("{}.bin", spec.as_str()));
        let meta = dir.join(format!("{}.meta.json", spec.as_str()));
        let original = std::fs::read(&bin).unwrap();
        let mut metadata: ObjectMeta =
            serde_json::from_slice(&std::fs::read(&meta).unwrap()).unwrap();
        // Checksum-valid truncations/extensions reach the length gate rather
        // than being rejected earlier by the checksum gate. Cover all three
        // possible partial-word remainders and aligned/empty wrong lengths.
        for length in [0, 8, 9, 10, 11, 13, 14, 15] {
            let mut payload = original.clone();
            payload.resize(length, 0xa5);
            metadata.checksum = bytes_hash(&payload);
            std::fs::write(&bin, payload).unwrap();
            std::fs::write(&meta, serde_json::to_vec(&metadata).unwrap()).unwrap();
            let CacheRead::Corrupt(report) = cache.get(&space, &input, &spec).unwrap() else {
                panic!("invalid payload length {length} was not rejected");
            };
            assert!(report.reason.contains("payload byte length"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
