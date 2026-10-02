//! Cache miss/corrupt degradation (P6-018): quarantine the bad record,
//! keep serving locally, and bound the re-embed spend.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-018: "缺失/损坏 → 隔离坏记录、语义 degraded、本地继续；补嵌受费用
//! 策略控制，不静默无界重费") and TASK-BRIEFS P6-018. The degradation matrix
//! this module completes (the P6-010/011/013/014/015/016 rows already exist —
//! see the delivery records; this module adds the quarantine move, the
//! re-embed cost gate and the visibility surface):
//!
//! | 消费点 | 缺失 (Miss) | 损坏 (Corrupt) |
//! |---|---|---|
//! | 检索 exact（P6-010） | 跳过候选，不报错 | 跳过候选，不报错 |
//! | 发布门 publish（P6-011） | read-back Miss → `ArtifactNotVerified` | read-back Corrupt → `ArtifactNotVerified` + reason |
//! | worker 队列（P6-013） | embed（首次付费，唯一付费方） | 先隔离（本模块）→ 补嵌受预算 → `put` 覆盖自愈 |
//! | 重建复用 reconcile（P6-014） | hand-back `pending` | 同 Miss（隔离由本模块门面落） |
//! | 恢复 recovery（P6-015） | 无 attempt 直写回 `pending` | 同 Miss（同上） |
//! | GC（P6-016） | 半文件超期回收 | 损坏对象超期回收；`quarantine/` 永不触及 |
//!
//! What a Corrupt read means here: the object's bytes failed the P6-008
//! verification chain, so the vector is unusable — but the cost that produced
//! it was real. [`quarantine_object`] therefore MOVES both halves into
//! `<root>/quarantine/` (P6-008's reserved convention,
//! `ArtifactCache::quarantine_dir`) instead of deleting: the sidecar meta and
//! a diagnostic report sidecar survive for post-mortem, the original address
//! reads `Miss` again, and GC structurally never touches the directory
//! (P6-016 sweeps only inside `namespace-<ns>/`). Re-embedding the input
//! re-`put`s a fresh object at the same address — the self-heal loop — and
//! that re-spend is exactly what [`DegradationLedger`] + [`BudgetedProvider`]
//! bound: a process-lifetime admission budget for re-embeds of quarantined
//! inputs, persisted-audited through the outbox `attempt_count`/`last_error`
//! (the P6-006 row counters). When the budget is exhausted the provider
//! refuses with a typed error, the queue's fenced retry dead-letters the task
//! to terminal `failed` carrying the reason — never a silent unbounded
//! re-fee loop.
//!
//! Degradation visibility ([`DegradationSnapshot`]): `degraded = true` once
//! any corrupt artifact was detected (or the budget ran out), with stable
//! reason strings. The composition root forwards the snapshot into
//! cc-server's `QueryServices::set_semantic_degradation`, which
//! `capability_status` surfaces as `semantic_state: "degraded"` +
//! `degraded_reason` — the "语义 degraded 可见" acceptance (V18). Reading the
//! ledger is side-effect free; nothing here polls, spawns, or waits (ADR
//! red line: no resident process).
//!
//! Module placement deviation (documented per the batch red line): the brief
//! assigns the quarantine landing to `cache.rs` and the degraded surfacing to
//! `crates/cc-server/src/capability_status.rs`. `cache.rs` is a sealed P6-008
//! deliverable ("008 不隔离，018 隔离" left the move here) and the batch red
//! line says "不改既有交付物（只调用/组合，缺口补齐走新增模块/方法）" — the
//! same reasoning that moved P6-016's sweep out of `cache.rs`. The quarantine
//! therefore lives here, composing the P6-008 primitives
//! (`ArtifactCache::quarantine_dir`, the documented layout) without modifying
//! them; the capability_status extension is a cc-server-side additive field
//! (P6-012 deviation-1 hand-over), wired through `QueryServices`' optional
//! slot — the default cc-server build neither depends on `cc-semantic` nor
//! reports any degradation.

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::ClaimedTask;
use cc_model::CcResult;

use crate::cache::{ArtifactCache, CorruptReport, NAMESPACE_DIR_PREFIX};
use crate::ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput};
use crate::spec::VectorSpace;
use crate::types::{DocSpecDigest, InputDigest};

/// Quarantine report sidecar format version.
pub const QUARANTINE_REPORT_FORMAT_VERSION: u32 = 1;

/// Sequence guard making quarantined file names unique even when the same
/// tuple is quarantined more than once over a process lifetime (a healed
/// object can rot again).
static QUARANTINE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Where a quarantined object's evidence now lives. All three files are
/// inside `<root>/quarantine/`; the `.meta.json` is the ORIGINAL sidecar
/// bytes (untouched evidence), the `.report.json` records why and whence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineRecord {
    pub bin_path: PathBuf,
    pub meta_path: PathBuf,
    pub report_path: PathBuf,
}

/// Diagnostic sidecar written next to the quarantined halves. Closed field
/// set, no secrets — the reason is a P6-008 `CorruptReport.reason`, the
/// addressing quadruple is already public layout data.
#[derive(Debug, Serialize)]
struct QuarantineReport<'a> {
    format_version: u32,
    quarantined_at: i64,
    reason: &'a str,
    original_bin_path: String,
    namespace: String,
    space_id: String,
    input_digest: String,
    spec_digest: String,
}

/// Move one detected-corrupt object into the cache quarantine, preserving
/// the sidecar meta (and adding a `.report.json`) for diagnosis.
///
/// Both halves are renamed out of the namespace tree, so the address
/// immediately reads [`crate::cache::CacheRead::Miss`] — the degrade-to-Miss
/// semantics of the brief. Files already gone (a concurrent GC sweep or a
/// double quarantine won the race) are tolerated: `Ok(None)` means "nothing
/// left to isolate", never an error. The quarantine directory is created on
/// first use — this module is its sanctioned creator (P6-008 only fixed the
/// convention).
pub fn quarantine_object(
    cache: &ArtifactCache,
    space: &VectorSpace,
    input: &InputDigest,
    spec: &DocSpecDigest,
    report: &CorruptReport,
    now_unix: i64,
) -> CcResult<Option<QuarantineRecord>> {
    space.validate()?;
    let space_digest = space.digest()?;
    let dir = cache
        .root()
        .join(format!("{NAMESPACE_DIR_PREFIX}{}", cache.namespace()))
        .join(space_digest.as_str())
        .join(input.as_str())
        .join(spec.as_str());
    let quarantine = cache.quarantine_dir();
    std::fs::create_dir_all(&quarantine)?;

    let seq = QUARANTINE_SEQ.fetch_add(1, Ordering::Relaxed);
    let stem = format!(
        "{}-{}-{}-q{}",
        space_digest.as_str(),
        input.as_str(),
        spec.as_str(),
        seq
    );
    let rec = QuarantineRecord {
        bin_path: quarantine.join(format!("{stem}.bin")),
        meta_path: quarantine.join(format!("{stem}.meta.json")),
        report_path: quarantine.join(format!("{stem}.report.json")),
    };

    let mut moved_any = false;
    for (from_name, to) in [
        (format!("{}.bin", spec.as_str()), &rec.bin_path),
        (format!("{}.meta.json", spec.as_str()), &rec.meta_path),
    ] {
        match std::fs::rename(dir.join(from_name), to) {
            Ok(()) => moved_any = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    if !moved_any {
        // Nothing was isolated (both halves already gone); do not leave a
        // report describing evidence that no longer exists.
        return Ok(None);
    }

    let report_doc = QuarantineReport {
        format_version: QUARANTINE_REPORT_FORMAT_VERSION,
        quarantined_at: now_unix,
        reason: &report.reason,
        original_bin_path: report.path.display().to_string(),
        namespace: cache.namespace().to_string(),
        space_id: space_digest.as_str().to_string(),
        input_digest: input.as_str().to_string(),
        spec_digest: spec.as_str().to_string(),
    };
    let mut file = std::fs::File::create(&rec.report_path)?;
    file.write_all(&serde_json::to_vec_pretty(&report_doc)?)?;
    file.sync_all()?;
    Ok(Some(rec))
}

/// Outcome of a re-embed admission check ([`DegradationLedger`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Not a re-embed (no corrupt record for this input): the first payment
    /// for the input, outside the re-fee budget by definition.
    FirstEmbed,
    /// A re-embed of a previously-corrupt input, admitted under budget.
    ReembedAdmitted,
    /// A re-embed refused: the process-lifetime budget is exhausted.
    BudgetExhausted,
}

/// Shared degradation ledger (P6-018 visibility + cost policy).
///
/// Cheap to clone (`Arc` inner), thread-safe, process-lifetime by design:
/// the re-embed budget deliberately does NOT survive a restart (a fresh
/// process may spend again; the outbox row counters remain the persistent
/// audit trail — the brief's 双轨). `note_corrupt` is called by the
/// quarantine path; `is_reembed`/`reembed_budget_available`/`record_reembed`
/// are consumed by [`BudgetedProvider`]; [`snapshot`][Self::snapshot] is the
/// read-only visibility surface.
#[derive(Debug, Clone, Default)]
pub struct DegradationLedger {
    inner: Arc<LedgerInner>,
}

#[derive(Debug, Default)]
struct LedgerInner {
    corrupt_inputs: Mutex<HashSet<String>>,
    corrupt_events: AtomicU64,
    quarantined_events: AtomicU64,
    reembeds_used: AtomicU64,
    max_reembeds: Option<u64>,
}

impl DegradationLedger {
    /// `max_reembeds`: process-lifetime cap on re-embeds of quarantined
    /// inputs; `None` = unbounded (still counted and visible).
    pub fn new(max_reembeds: Option<u64>) -> Self {
        Self {
            inner: Arc::new(LedgerInner {
                max_reembeds,
                ..LedgerInner::default()
            }),
        }
    }

    /// Record one corrupt-artifact event for `input` (called by
    /// [`quarantine_detected`], including the already-gone race case —
    /// a corrupt read happened, that alone degrades).
    pub fn note_corrupt(&self, input: &InputDigest) {
        self.inner
            .corrupt_inputs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(input.as_str().to_string());
        self.inner.corrupt_events.fetch_add(1, Ordering::AcqRel);
    }

    /// Record one completed quarantine move.
    pub fn note_quarantined(&self) {
        self.inner.quarantined_events.fetch_add(1, Ordering::AcqRel);
    }

    /// Whether embedding `input` is a re-embed of a previously-corrupt
    /// input (the budgeted class).
    pub fn is_reembed(&self, input: &InputDigest) -> bool {
        self.inner
            .corrupt_inputs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(input.as_str())
    }

    /// Whether the budget still admits a re-embed right now.
    pub fn reembed_budget_available(&self) -> bool {
        match self.inner.max_reembeds {
            None => true,
            Some(max) => self.inner.reembeds_used.load(Ordering::Acquire) < max,
        }
    }

    /// Count one admitted re-embed spend.
    pub fn record_reembed(&self) {
        self.inner.reembeds_used.fetch_add(1, Ordering::AcqRel);
    }

    /// Classify + account one document input ahead of a provider call.
    /// Accounting only happens on [`Admission::ReembedAdmitted`].
    pub fn admit(&self, input: &InputDigest) -> Admission {
        if !self.is_reembed(input) {
            return Admission::FirstEmbed;
        }
        if !self.reembed_budget_available() {
            return Admission::BudgetExhausted;
        }
        self.record_reembed();
        Admission::ReembedAdmitted
    }

    /// Read-only visibility snapshot (the degraded 口径: any corrupt event,
    /// or an exhausted budget, degrades the semantic capability; plain
    /// misses never do — they are the normal cold-cache state).
    pub fn snapshot(&self) -> DegradationSnapshot {
        let corrupt_events = self.inner.corrupt_events.load(Ordering::Acquire);
        let quarantined = self.inner.quarantined_events.load(Ordering::Acquire);
        let used = self.inner.reembeds_used.load(Ordering::Acquire);
        let budget_exhausted = match self.inner.max_reembeds {
            Some(max) => used >= max,
            None => false,
        };
        let mut reasons = Vec::new();
        if corrupt_events > 0 {
            reasons.push(format!(
                "corrupt cache artifacts detected: {corrupt_events} event(s), \
                 {quarantined} object(s) quarantined for diagnosis"
            ));
        }
        if budget_exhausted {
            reasons.push(format!(
                "re-embed budget exhausted: {used} paid re-embed(s) this process; \
                 further re-embeds of quarantined inputs are refused until \
                 restart or the budget is raised"
            ));
        }
        DegradationSnapshot {
            degraded: corrupt_events > 0 || budget_exhausted,
            corrupt_events,
            quarantined_objects: quarantined,
            reembeds_used: used,
            reembed_budget: self.inner.max_reembeds,
            budget_exhausted,
            degraded_reasons: reasons,
        }
    }
}

/// Read-only view of [`DegradationLedger`] for status surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DegradationSnapshot {
    pub degraded: bool,
    pub corrupt_events: u64,
    pub quarantined_objects: u64,
    pub reembeds_used: u64,
    pub reembed_budget: Option<u64>,
    pub budget_exhausted: bool,
    pub degraded_reasons: Vec<String>,
}

/// Provider decorator enforcing the P6-018 re-embed cost policy: document
/// batches are checked against the ledger BEFORE the inner provider runs;
/// a budget-exhausted re-embed is refused with a typed
/// [`ProviderError::InvalidInput`] whose text becomes the task's persisted
/// `last_error` through the queue's fenced retry — the dead-letter reason
/// required by "超限后任务终态 failed 带原因，绝不静默循环". The inner
/// provider is never invoked for a refused batch, so an exhausted budget
/// stops the spend, not just the accounting. Query embedding is delegated
/// unchanged (per-query cost, not a cache re-fee).
///
/// The inner provider is held as `&dyn` — the same shape
/// `EmbedHandler::new` takes — so any provider composes without a blanket
/// impl on the frozen port.
pub struct BudgetedProvider<'a> {
    inner: &'a dyn EmbeddingProvider,
    ledger: DegradationLedger,
}

impl<'a> BudgetedProvider<'a> {
    pub fn new(inner: &'a dyn EmbeddingProvider, ledger: DegradationLedger) -> Self {
        Self { inner, ledger }
    }

    /// Access the wrapped provider (tests read `FakeProvider::call_count`
    /// through a downcast-free shared reference of the concrete type).
    pub fn inner(&self) -> &dyn EmbeddingProvider {
        self.inner
    }
}

impl EmbeddingProvider for BudgetedProvider<'_> {
    fn space(&self) -> &VectorSpace {
        self.inner.space()
    }

    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        // Pass 1: classify without accounting (all-or-nothing per batch).
        let reembeds: Vec<&DocumentInput> = batch
            .iter()
            .filter(|d| self.ledger.is_reembed(&d.input_digest))
            .collect();
        if !reembeds.is_empty() {
            let snap = self.ledger.snapshot();
            let total_after = snap.reembeds_used + reembeds.len() as u64;
            let admitted = match snap.reembed_budget {
                // Unbounded budget: admit and account every re-embed.
                None => true,
                Some(max) => total_after <= max,
            };
            if !admitted {
                let cap = snap
                    .reembed_budget
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "unbounded".to_string());
                return Err(ProviderError::InvalidInput(format!(
                    "semantic re-embed budget exhausted: {}/{} paid re-embeds used \
                     this process; refusing re-embed of {} quarantined input(s)",
                    snap.reembeds_used,
                    cap,
                    reembeds.len()
                )));
            }
            for _ in &reembeds {
                self.ledger.record_reembed();
            }
        }
        self.inner.embed_documents(batch)
    }

    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.inner.embed_queries(batch)
    }
}

/// The P6-018 degrade step for one detected-corrupt object: quarantine the
/// evidence and record the degradation. Returns the quarantine record, or
/// `Ok(None)` when the object was already gone (the corrupt EVENT is still
/// recorded — a corrupt read happened, which is what degrades).
pub fn quarantine_detected(
    cache: &ArtifactCache,
    ledger: &DegradationLedger,
    space: &VectorSpace,
    input: &InputDigest,
    spec: &DocSpecDigest,
    report: &CorruptReport,
    now_unix: i64,
) -> CcResult<Option<QuarantineRecord>> {
    ledger.note_corrupt(input);
    let record = quarantine_object(cache, space, input, spec, report, now_unix)?;
    if record.is_some() {
        ledger.note_quarantined();
    }
    Ok(record)
}

/// Hand one CLAIMED task back to `pending` after a degrade, WITHOUT
/// consuming the attempt budget (`IndexDb::hand_back_semantic_task`, the
/// batch-3 closure ② primitive — a cache fault is not the task's fault).
/// `Ok(false)` is "lease lost": the task left `claimed` under this token
/// (expired/reclaimed, superseded, done) and the hand-back was a zero-write
/// no-op. The reason persists in `last_error` for audit.
pub fn requeue_after_degrade(db: &IndexDb, task: &ClaimedTask, reason: &str) -> CcResult<bool> {
    db.hand_back_semantic_task(task.task_id, task.token.as_str(), reason)
}

/// The claimed task's embedding-input address, rebuilt from the fenced task
/// row (the digest string was written by the enqueue path itself, so this
/// constructor is the same seal the crate uses internally). P7-014: lets the
/// composition-root drain run the corrupt-artifact pre-check against the
/// exact input the task is queued for.
pub fn task_input(task: &ClaimedTask) -> InputDigest {
    InputDigest::new(task.input_digest.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::fake::{FakeProvider, FakeProviderConfig};
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = SEQ.fetch_add(1, AtomicOrdering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "cc-semantic-degrade-unit-{tag}-{}-{n}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn space() -> VectorSpace {
        VectorSpace::new("fake/model-degrade", 2).expect("space")
    }

    fn spec_of(space: &VectorSpace) -> DocSpecDigest {
        crate::spec::DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
            .expect("doc spec")
            .digest()
            .expect("digest")
    }

    fn input_of(bytes: &[u8]) -> InputDigest {
        InputDigest::of_input(bytes).expect("input digest")
    }

    fn corrupt_report(dir: &std::path::Path) -> CorruptReport {
        CorruptReport {
            path: dir.to_path_buf(),
            reason: "payload checksum mismatch: meta a vs actual b".to_string(),
        }
    }

    #[test]
    fn quarantine_moves_both_halves_and_writes_a_diagnostic_report() {
        let dir = TempDir::new("quarantine");
        let space = space();
        let spec = spec_of(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), "ns-q".to_string()).expect("cache");
        let input = input_of(b"doc-a");
        cache
            .put(&space, &input, &spec, &[1.0, 2.0], 100)
            .expect("put");

        let rec = quarantine_object(&cache, &space, &input, &spec, &corrupt_report(&dir.0), 200)
            .expect("quarantine")
            .expect("record");

        // Address degrades to Miss; nothing left in the namespace tree.
        assert!(matches!(
            cache.get(&space, &input, &spec).expect("get"),
            crate::cache::CacheRead::Miss
        ));
        // Evidence preserved: original meta fields + report sidecar.
        let meta: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&rec.meta_path).expect("meta")).unwrap();
        assert_eq!(meta["dimension"], 2);
        assert_eq!(meta["model_id"], "fake/model-degrade");
        assert_eq!(meta["created_at"], 100);
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&rec.report_path).expect("report")).unwrap();
        assert_eq!(report["format_version"], 1);
        assert_eq!(report["quarantined_at"], 200);
        assert_eq!(
            report["reason"],
            "payload checksum mismatch: meta a vs actual b"
        );
        assert_eq!(report["namespace"], "ns-q");
        assert_eq!(report["input_digest"], input.as_str());
        // All evidence inside <root>/quarantine, names unique per tuple.
        assert!(rec.bin_path.starts_with(cache.quarantine_dir()));
        assert!(rec.bin_path.exists() && rec.meta_path.exists());
    }

    #[test]
    fn quarantine_is_none_when_the_object_is_already_gone() {
        let dir = TempDir::new("gone");
        let space = space();
        let spec = spec_of(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), "ns-g".to_string()).expect("cache");
        let input = input_of(b"doc-b");
        // Never written at all.
        assert!(
            quarantine_object(&cache, &space, &input, &spec, &corrupt_report(&dir.0), 1)
                .expect("quarantine")
                .is_none()
        );
        // Half-written: bin only → the pair move still isolates the bin.
        let d = cache
            .root()
            .join(format!("{NAMESPACE_DIR_PREFIX}ns-g"))
            .join(space.digest().unwrap().as_str())
            .join(input.as_str())
            .join(spec.as_str());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("{}.bin", spec.as_str())), [0u8; 8]).unwrap();
        assert!(
            quarantine_object(&cache, &space, &input, &spec, &corrupt_report(&dir.0), 1)
                .expect("quarantine")
                .is_some()
        );
    }

    #[test]
    fn ledger_tracks_corruption_budget_and_visibility() {
        let ledger = DegradationLedger::new(Some(2));
        // Fresh ledger: healthy, misses are not degradation.
        let snap = ledger.snapshot();
        assert!(!snap.degraded);
        let input = input_of(b"doc-c");
        assert_eq!(ledger.admit(&input), Admission::FirstEmbed);

        ledger.note_corrupt(&input);
        let snap = ledger.snapshot();
        assert!(snap.degraded);
        assert_eq!(snap.corrupt_events, 1);
        assert_eq!(snap.degraded_reasons.len(), 1);

        assert_eq!(ledger.admit(&input), Admission::ReembedAdmitted);
        assert_eq!(ledger.admit(&input), Admission::ReembedAdmitted);
        assert_eq!(ledger.admit(&input), Admission::BudgetExhausted);
        let snap = ledger.snapshot();
        assert!(snap.budget_exhausted);
        assert_eq!(snap.reembeds_used, 2);
        assert_eq!(snap.degraded_reasons.len(), 2);

        // Unbounded ledger never exhausts but stays visible.
        let open = DegradationLedger::new(None);
        open.note_corrupt(&input);
        assert_eq!(open.admit(&input), Admission::ReembedAdmitted);
        assert!(open.snapshot().degraded);
        assert!(!open.snapshot().budget_exhausted);
    }

    #[test]
    fn budgeted_provider_gates_reembeds_and_never_calls_through_when_exhausted() {
        let space = space();
        let fake = FakeProvider::new(FakeProviderConfig::new(space.clone()));
        let ledger = DegradationLedger::new(Some(1));
        let provider = BudgetedProvider::new(&fake, ledger.clone());

        let corrupt = input_of(b"doc-corrupt");
        let fresh = input_of(b"doc-fresh");
        ledger.note_corrupt(&corrupt);

        // 与真实 worker 一致：DocumentInput 直接携带任务解析出的 input_digest
        // （from_bytes 会按字节重算 digest，这里显式绑定）。
        let to_doc = |i: &InputDigest| DocumentInput {
            input_digest: i.clone(),
            bytes: i.as_str().as_bytes().to_vec(),
        };
        // First embed of a non-corrupt input: never budgeted.
        provider
            .embed_documents(&[to_doc(&fresh)])
            .expect("fresh embed");
        // First (admitted) re-embed of the corrupt input.
        provider
            .embed_documents(&[to_doc(&corrupt)])
            .expect("admitted re-embed");
        // Budget spent: refused WITHOUT calling through.
        let err = provider
            .embed_documents(&[to_doc(&corrupt)])
            .expect_err("budget refused");
        let ProviderError::InvalidInput(reason) = &err else {
            panic!("expected InvalidInput, got {err:?}");
        };
        assert!(reason.contains("re-embed budget exhausted"));
        // Mixed batch with one refused re-embed refuses whole batch.
        assert!(provider
            .embed_documents(&[to_doc(&fresh), to_doc(&corrupt)])
            .is_err());
        assert_eq!(fake.call_count(), 2);
        assert_eq!(ledger.snapshot().reembeds_used, 1);

        // Query embedding delegates regardless (call_count 计入双方法).
        let q = [QueryInput::from_bytes(b"q").expect("query")];
        provider.embed_queries(&q).expect("queries");
        assert_eq!(fake.call_count(), 3);
    }
}
