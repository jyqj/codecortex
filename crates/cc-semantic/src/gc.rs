//! Artifact-cache garbage collection and publish coordination (P6-016).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-016: "共享同步点；无『manifest 引用刚被 GC 删除产物』竞态；孤儿最终
//! 可回收"; lines 60-62: artifact-before-manifest, no cross-store atomicity)
//! and `artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
//! P6-016 (mark set / synchronization point / sweep design, quoted inline
//! below). Deletion criterion (conservative per the red line "不确定即保留"):
//! an object is deletable only when it is
//!
//! 1. **older than the shortest retention period** (`GcConfig::
//!    min_retention_secs` against the object's `created_at` — refreshed by
//!    every `Publisher` re-put, P6-008 `put`). This freshness grace protects
//!    newly observed objects, but a timestamp captured during collection is
//!    not an atomic freshness check at the later unlink;
//! 2. **not referenced by any manifest row** — exact `artifact_ref` match when
//!    the checksum is reconstructable from the sidecar, conservative
//!    `(space_id, input_digest)` match otherwise (all spaces count, including
//!    revoked ones — P6-017 rollback reuse depends on it);
//! 3. **not held by a live task** — no `pending`/`claimed` outbox row embeds
//!    from its input digest (the brief's "进行中任务的 `input_digest` 派生目标
//!    （活跃 lease 行）"; `pending` is added conservatively: a queued task may
//!    reuse the cached vector, and deleting it would trade a paid artifact for
//!    a re-embed).
//!
//! Synchronization point (brief: "GC 候选收集与删除决定之间，取一次 DB 短事务
//! 快照"): [`sweep_batch`] marks the whole batch inside ONE short read
//! snapshot on the cc-db side (`IndexDb::semantic_gc_mark`). That snapshot
//! checks the addresses captured by collection. No shared filesystem lock
//! spans collection, mark, and unlink: replacement/re-put of those paths can
//! race with the captured metadata or mark. The deletion-error accounting
//! below does not establish atomicity with publication or close that race.
//!
//! Module placement deviation (documented per the batch red line): the brief
//! assigns the sweep to `cache.rs` and the sync point to `publish.rs`, but
//! both are sealed deliverables of P6-008/P6-011 and the batch red line says
//! "不改既有交付物（只调用/组合）" — the same reasoning that moved P6-011's
//! SQL primitive out of `semantic_outbox.rs`. The sweep therefore lives here,
//! calling the P6-008 primitives (`ArtifactCache`) and the cc-db read-only
//! mark (`cc_db::semantic_gc_reads`) without modifying either. The snapshot
//! transaction itself lives in cc-db because `cc-semantic` has no production
//! `rusqlite` dependency (ADR row P6-002: "只依赖 cc-model/cc-db").
//!
//! Boundedness: every pass processes at most `batch_entries` entries
//! (keyset cursor = the last visited `(space_id, input_digest, file_name)`
//! position), deletes at most that many, and returns the resume position —
//! an explicit driver loops until `exhausted`. No resident process, timer, or
//! daemon exists anywhere here; whether and when to run a pass is the
//! composition root's decision (same discipline as `crate::queue`/`recovery`).
//!
//! Sweep physics (brief: "unlink 候选文件 + 删除空目录"): deletion is unlink of
//! the `.bin`/`.meta.json` pair — the filesystem twin of the P6-008
//! `discard()` semantics (both halves gone, subsequent reads are `Miss`).
//! Empty input/space directories are pruned bottom-up; the namespace
//! directory itself is never removed, and nothing outside
//! `<root>/namespace-<ns>/` (the P6-018 `quarantine/` included) is ever
//! touched. Temp files (`*.tmp-<pid>-<seq>` from P6-008 `atomic_write`) and
//! half-written objects (`.bin` without `.meta.json` — the sidecar is the
//! completeness marker — or vice versa) are swept under the same freshness
//! grace; the DB checks apply to halves because a manifest row could still
//! publish from their address. The full pass is DB-read-only and moves no
//! epoch (brief: "全程无 DB 写……不刷 epoch").

use std::path::{Path, PathBuf};

use serde::Deserialize;

use cc_db::index_db::IndexDb;
use cc_db::semantic_gc_reads::GcMarkProbe;
use cc_model::CcResult;

use crate::cache::{ArtifactCache, NAMESPACE_DIR_PREFIX};

/// `artifact_ref` scheme prefix (format documentation lives with
/// `crate::cache`; duplicated here because that constant is module-private to
/// the sealed P6-008 deliverable).
const REF_SCHEME: &str = "cas.v1";

/// Suffixes of the two halves of one cache object (P6-008 layout).
const BIN_SUFFIX: &str = ".bin";
const META_SUFFIX: &str = ".meta.json";

/// Marker of a P6-008 `atomic_write` temp leftover
/// (`<name>.tmp-<pid>-<seq>`).
const TEMP_MARKER: &str = ".tmp-";

/// Conservative default of the fresh-timestamp grace: one hour. Must exceed
/// the worst-case latency between a publisher's `put` (which refreshes
/// `created_at`) and its manifest CAS; a publish latency beyond this would
/// need a larger configured retention, never a smaller one.
pub const DEFAULT_MIN_RETENTION_SECS: i64 = 3_600;

/// Tunables of one GC pass. `now_unix` is the caller's clock (P6-007
/// discipline: no clock reads inside the sweep); `min_retention_secs` is the
/// fresh-timestamp grace; `batch_entries` bounds the pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcConfig {
    pub min_retention_secs: i64,
    pub batch_entries: usize,
    pub now_unix: i64,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            min_retention_secs: DEFAULT_MIN_RETENTION_SECS,
            batch_entries: 256,
            now_unix: 0,
        }
    }
}

/// Keyset cursor: the last visited leaf position, compared component-wise in
/// deterministic sorted order (`space_id` → `input_digest` → `spec` directory
/// → `file_name`; the P6-008 layout is
/// `<ns>/<space>/<input>/<spec>/<spec>.{bin,meta.json}`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GcPosition {
    pub space_id: String,
    pub input_digest: String,
    pub leaf_dir: String,
    pub file_name: String,
}

/// One discovered cache-leaf candidate. The DB mark decides Objects and
/// Halves; Temps are pure filesystem leftovers.
#[derive(Debug, Clone, PartialEq)]
pub enum GcEntryKind {
    /// A complete object: sidecar parsed (possibly unsuccessfully —
    /// `created_at`/`checksum` then `None`, and the decision falls back to
    /// mtime freshness and the conservative pair mark).
    Object {
        bin: PathBuf,
        meta: PathBuf,
        spec: String,
        created_at: Option<i64>,
        checksum: Option<String>,
    },
    /// One half of an object (`.bin` without `.meta.json`, or vice versa) —
    /// reads as `Miss` (P6-008), deletable garbage once old and unmarked.
    Half { path: PathBuf },
    /// An `atomic_write` temp leftover.
    Temp { path: PathBuf },
}

/// A candidate with its address (parsed from the directory layout) and its
/// resume position.
#[derive(Debug, Clone, PartialEq)]
pub struct GcEntry {
    pub space_id: String,
    pub input_digest: String,
    pub position: GcPosition,
    pub kind: GcEntryKind,
}

/// Result of one bounded candidate collection (no clock, no DB, no writes).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CollectedBatch {
    pub entries: Vec<GcEntry>,
    /// True when the whole namespace tree has been visited in this pass.
    pub exhausted: bool,
}

/// Counters returned by one successful sweep. `kept_*` are protection
/// outcomes; `deleted_*` count entries for which at least one path was
/// actually unlinked. Already absent paths do not count as deletions. An
/// object counts once after both unlink attempts succeed (including an
/// already absent half). `pruned_dirs` counts removed empty directories.
/// An error returns no counters and may follow earlier successful unlinks;
/// it must not be interpreted as zero work or a rolled-back filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GcCounters {
    pub kept_fresh: usize,
    pub kept_referenced: usize,
    pub kept_live_task: usize,
    pub deleted_objects: usize,
    pub deleted_halves: usize,
    pub deleted_temps: usize,
    pub pruned_dirs: usize,
}

fn invalid(message: impl Into<String>) -> cc_model::CcError {
    crate::error::SemanticError::InvalidInput(message.into()).into()
}

/// Minimum sidecar surface the sweep needs. P6-008's full seven-field meta is
/// validated on every `get`; the sweep only reads two fields and deliberately
/// does not re-run the verification chain (a corrupt object is still just a
/// candidate — the mark + freshness decide it, and sweeping it IS the
/// P6-008-`discard`-equivalent cleanup of detected corruption).
#[derive(Debug, Deserialize)]
struct GcMetaPeek {
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    checksum: Option<String>,
}

/// Collect at most `batch_entries` candidates from the namespace tree,
/// starting strictly after `after` (deterministic sorted keyset order).
/// Filesystem read-only; creates nothing; a not-yet-existing namespace
/// directory is simply exhausted.
pub fn collect_candidates(
    cache: &ArtifactCache,
    after: Option<&GcPosition>,
    batch_entries: usize,
) -> CcResult<CollectedBatch> {
    if batch_entries == 0 {
        return Err(invalid(
            "gc candidate collection requires batch_entries >= 1",
        ));
    }
    let ns_dir = cache
        .root()
        .join(format!("{NAMESPACE_DIR_PREFIX}{}", cache.namespace()));

    let mut batch = CollectedBatch::default();
    let mut exhausted = true;

    'spaces: for space in sorted_dirs(&ns_dir) {
        if let Some(a) = after {
            if space.0 < a.space_id {
                continue;
            }
        }
        for input in sorted_dirs(&space.1) {
            if let Some(a) = after {
                if space.0 == a.space_id && input.0 < a.input_digest {
                    continue;
                }
            }
            for leaf in sorted_dirs(&input.1) {
                if let Some(a) = after {
                    if space.0 == a.space_id && input.0 == a.input_digest && leaf.0 < a.leaf_dir {
                        continue;
                    }
                }
                for file in sorted_files(&leaf.1) {
                    if let Some(a) = after {
                        if space.0 == a.space_id
                            && input.0 == a.input_digest
                            && leaf.0 == a.leaf_dir
                            && file <= a.file_name
                        {
                            continue;
                        }
                    }
                    let position = GcPosition {
                        space_id: space.0.clone(),
                        input_digest: input.0.clone(),
                        leaf_dir: leaf.0.clone(),
                        file_name: file.clone(),
                    };
                    let path = leaf.1.join(&file);
                    if let Some(kind) = classify(&path, &file, &leaf.1) {
                        batch.entries.push(GcEntry {
                            space_id: space.0.clone(),
                            input_digest: input.0.clone(),
                            position,
                            kind,
                        });
                        if batch.entries.len() >= batch_entries {
                            exhausted = false;
                            break 'spaces;
                        }
                    }
                }
            }
        }
    }
    batch.exhausted = exhausted;
    Ok(batch)
}

/// (sorted name, full path) pairs of a directory's subdirectories.
fn sorted_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            e.path().is_dir().then_some((name, e.path()))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn sorted_files(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let e = e.ok()?;
            e.path()
                .is_file()
                .then(|| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    out.sort();
    out
}

/// Classify one leaf file of an input directory.
fn classify(path: &Path, file_name: &str, input_dir: &Path) -> Option<GcEntryKind> {
    if file_name.contains(TEMP_MARKER) {
        return Some(GcEntryKind::Temp {
            path: path.to_path_buf(),
        });
    }
    if let Some(base) = file_name.strip_suffix(META_SUFFIX) {
        let bin = input_dir.join(format!("{base}{BIN_SUFFIX}"));
        if bin.exists() {
            let (created_at, checksum) = peek_meta(path);
            return Some(GcEntryKind::Object {
                bin,
                meta: path.to_path_buf(),
                spec: base.to_string(),
                created_at,
                checksum,
            });
        }
        return Some(GcEntryKind::Half {
            path: path.to_path_buf(),
        });
    }
    if file_name.ends_with(BIN_SUFFIX) {
        let base = file_name.strip_suffix(BIN_SUFFIX).unwrap_or(file_name);
        if !input_dir.join(format!("{base}{META_SUFFIX}")).exists() {
            return Some(GcEntryKind::Half {
                path: path.to_path_buf(),
            });
        }
        // The paired `.meta.json` entry classifies this object; skip here.
        return None;
    }
    // Unknown leaf name: not produced by P6-008; conservative — leave alone.
    None
}

fn peek_meta(meta_path: &Path) -> (Option<i64>, Option<String>) {
    match std::fs::read(meta_path) {
        Ok(raw) => match serde_json::from_slice::<GcMetaPeek>(&raw) {
            Ok(peek) => (peek.created_at, peek.checksum),
            Err(_) => (None, None),
        },
        Err(_) => (None, None),
    }
}

/// The protection decision for one non-temp entry against the batch's mark
/// snapshot. `marks` runs in the same order as the probes that
/// [`sweep_batch`] built — objects and halves only, in entry order.
fn fresh_timestamp(entry: &GcEntry, cfg: &GcConfig) -> bool {
    let ts = match &entry.kind {
        GcEntryKind::Object {
            bin,
            meta,
            created_at,
            ..
        } => created_at
            .or_else(|| mtime_secs(bin))
            .or_else(|| mtime_secs(meta)),
        GcEntryKind::Half { path } | GcEntryKind::Temp { path } => mtime_secs(path),
    };
    match ts {
        None => true, // un-dateable: conservative — treat as fresh
        Some(t) => cfg.now_unix - t < cfg.min_retention_secs,
    }
}

fn mtime_secs(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(secs)
}

/// Sweep one collected batch: freshness filter → ONE cc-db read snapshot
/// marking the whole batch (the synchronization point) → unlink the
/// survivors of no protection → prune emptied directories. Zero DB writes,
/// zero epoch movement.
pub fn sweep_batch(
    db: &IndexDb,
    cache: &ArtifactCache,
    cfg: &GcConfig,
    batch: &CollectedBatch,
) -> CcResult<GcCounters> {
    let mut counters = GcCounters::default();

    // Pass 1: freshness (filesystem clock data only).
    let marked: Vec<bool> = batch
        .entries
        .iter()
        .map(|entry| fresh_timestamp(entry, cfg))
        .collect();

    // Pass 2: build probes for the non-fresh objects/halves and mark them
    // under one short read snapshot (the synchronization point). Temps need
    // no DB mark: a temp name is never a publishable address.
    let probe_indices: Vec<usize> = batch
        .entries
        .iter()
        .enumerate()
        .filter(|(i, entry)| !marked[*i] && !matches!(entry.kind, GcEntryKind::Temp { .. }))
        .map(|(i, _)| i)
        .collect();
    let probes: Vec<GcMarkProbe> = probe_indices
        .iter()
        .map(|&i| probe_of(cache, &batch.entries[i]).expect("non-temp entry has a probe"))
        .collect();
    let marks = db.semantic_gc_mark(&probes)?;
    if marks.len() != probes.len() {
        return Err(invalid(
            "gc mark snapshot returned a different number of verdicts than probes",
        ));
    }
    let mut marks_aligned: Vec<Option<cc_db::semantic_gc_reads::GcMark>> =
        vec![None; batch.entries.len()];
    for (idx, mark) in probe_indices.into_iter().zip(marks) {
        marks_aligned[idx] = Some(mark);
    }

    // Pass 3: decide and unlink; collect leaf directories touched by
    // deletions.
    let mut prunable: Vec<PathBuf> = Vec::new(); // leaf (spec) dirs of deletions
    use cc_db::semantic_gc_reads::GcMark;
    for (index, (entry, fresh)) in batch.entries.iter().zip(&marked).enumerate() {
        if let GcEntryKind::Temp { path } = &entry.kind {
            if *fresh {
                counters.kept_fresh += 1;
            } else if unlink_if_present(path)? {
                counters.deleted_temps += 1;
            }
            continue;
        }
        if *fresh {
            counters.kept_fresh += 1;
            continue;
        }
        match marks_aligned[index] {
            Some(GcMark::ManifestRef) | Some(GcMark::ManifestPair) => {
                counters.kept_referenced += 1;
            }
            Some(GcMark::LiveTask) => counters.kept_live_task += 1,
            Some(GcMark::Unreferenced) | None => match &entry.kind {
                GcEntryKind::Object { bin, meta, .. } => {
                    let removed_bin = unlink_if_present(bin)?;
                    let removed_meta = unlink_if_present(meta)?;
                    if removed_bin || removed_meta {
                        counters.deleted_objects += 1;
                    }
                    prunable.push(entry_dir(entry));
                }
                GcEntryKind::Half { path } => {
                    if unlink_if_present(path)? {
                        counters.deleted_halves += 1;
                    }
                    prunable.push(entry_dir(entry));
                }
                GcEntryKind::Temp { .. } => unreachable!("temps handled above"),
            },
        }
    }

    // Pass 4: prune emptied directories bottom-up (spec leaf dir → input dir
    // → space dir). remove_dir only succeeds on empty dirs, so fresh halves,
    // temps or surviving objects hold their parents — conservative by
    // construction. The namespace dir itself is never removed.
    for leaf in &prunable {
        let mut depth = 0;
        let mut dir = Some(leaf.as_path());
        while let Some(current) = dir {
            if depth >= 3 {
                break; // leaf → input → space; never above the space dir
            }
            match std::fs::remove_dir(current) {
                Ok(()) => counters.pruned_dirs += 1,
                Err(_) => break, // non-empty (or already gone): stop climbing
            }
            depth += 1;
            dir = current.parent();
        }
    }
    Ok(counters)
}

fn probe_of(cache: &ArtifactCache, entry: &GcEntry) -> Option<GcMarkProbe> {
    match &entry.kind {
        GcEntryKind::Object { spec, checksum, .. } => {
            let artifact_ref = checksum.as_ref().map(|checksum| {
                format!(
                    "{REF_SCHEME}:{}:{}:{}:{}:{checksum}",
                    cache.namespace(),
                    entry.space_id,
                    entry.input_digest,
                    spec
                )
            });
            Some(GcMarkProbe {
                artifact_ref,
                space_id: entry.space_id.clone(),
                input_digest: entry.input_digest.clone(),
            })
        }
        GcEntryKind::Half { .. } => Some(GcMarkProbe {
            artifact_ref: None,
            space_id: entry.space_id.clone(),
            input_digest: entry.input_digest.clone(),
        }),
        GcEntryKind::Temp { .. } => None,
    }
}

fn entry_dir(entry: &GcEntry) -> PathBuf {
    match &entry.kind {
        GcEntryKind::Object { bin, .. } | GcEntryKind::Half { path: bin } => {
            bin.parent().map(Path::to_path_buf).unwrap_or_default()
        }
        GcEntryKind::Temp { path } => path.parent().map(Path::to_path_buf).unwrap_or_default(),
    }
}

fn unlink_if_present(path: &Path) -> CcResult<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// One explicit GC pass = collect (bounded) + sweep (one snapshot). Returns
/// the counters, the resume position (`None` once exhausted), and the
/// exhausted flag. The caller loops with the returned position to converge a
/// backlog larger than the batch — "有界批次推进……超上限存量分轮收敛".
/// An I/O error may follow successful unlinks and returns no new cursor.
/// After clearing its cause, retry from the previous `after` position so
/// collection and the DB mark are renewed for any surviving files.
pub fn run_gc_pass(
    db: &IndexDb,
    cache: &ArtifactCache,
    cfg: &GcConfig,
    after: Option<&GcPosition>,
) -> CcResult<(GcCounters, Option<GcPosition>, bool)> {
    let batch = collect_candidates(cache, after, cfg.batch_entries)?;
    let counters = sweep_batch(db, cache, cfg, &batch)?;
    // The resume position exists only while the tree has more to visit.
    let resume = (!batch.exhausted)
        .then(|| batch.entries.last().map(|e| e.position.clone()))
        .flatten();
    Ok((counters, resume, batch.exhausted))
}
