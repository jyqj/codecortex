//! Skip-signature seam for postprocess/analysis passes.
//!
//! Every pass that runs after the index write follows the same shape: decide
//! whether its inputs changed since the last successful run, execute when
//! they did, and persist a marker so the next build can skip. This module
//! owns that shape once — [`PassGate`] is the decision/record contract and
//! [`run_gated_passes`] is the orchestration loop — so individual passes only
//! declare *what* their input signature is, never re-implement the
//! compare/record plumbing.
//!
//! Gate adapters:
//! - [`DbSignatureGate`] — u64 signature computed from committed DB state
//!   (dispatch synthesis, interface dispatch, community detection).
//! - [`FileSignatureGate`] — u64 signature computed from the filesystem
//!   (infra pass: candidate paths + mtime + size).
//! - [`StringCacheGate`] — opaque string cache key (git co-change: HEAD sha).
//!   A missing/empty key means the source is unavailable and the pass must
//!   run unconditionally — never skip permanently on a read failure.
//! - [`Unconditional`] — passes with no skip condition (ADR indexing).
//! - [`PairGate`] — couples two gates whose passes execute as one round
//!   (dispatch + interface gates feed a single synthesis round).
//!
//! Signature gates compare lazily: metadata is read first, and the signature
//! is computed only when an actual comparison (or a record) needs it, at most
//! once per gate per build — the cached value is shared between `should_run`
//! and `record_run`. Hash comparison can never skip the computation itself,
//! but the cache removes same-build recomputation and the forced (full
//! rebuild) path skips the metadata round-trip entirely.
//!
//! Signature gates also persist an algorithm version next to the signature.
//! Signatures recorded before the version key existed were produced by
//! algorithm "1", so a missing version key reads as "1"; bumping a gate's
//! version therefore forces exactly one recompute on the next build.
//!
//! Relation to `crate::dispatch_synthesis::SynthesisPassSpec`: the synthesis
//! sub-passes keep their declarative registry and its per-pass `PassGate`
//! *enum* (Dispatch/Interface) — that enum routes the two signature decisions
//! to sub-passes within one round and is not a second signature layer. The
//! two `DbSignatureGate`s here are the single source of those decisions;
//! [`PairGate`] only pairs them so the round runs when either input changed,
//! preserving the prior `dispatch_changed || interface_changed` reach
//! condition.

use std::cell::Cell;

use cc_db::index_db::IndexDb;
use cc_model::CcResult;

/// Algorithm version implied by signatures recorded before the version key
/// existed.
const LEGACY_ALGORITHM_VERSION: &str = "1";

/// Outcome of a gate check: whether the pass must run, plus a reason for logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GateDecision {
    pub(crate) run: bool,
    pub(crate) reason: &'static str,
}

impl GateDecision {
    pub(crate) fn run(reason: &'static str) -> Self {
        Self { run: true, reason }
    }

    pub(crate) fn skip(reason: &'static str) -> Self {
        Self { run: false, reason }
    }
}

/// Decision/record contract for one gated pass.
///
/// `should_run` must be free of side effects on the index; `record_run` is
/// called only after the pass completed successfully, so a mid-pass failure
/// never records a marker for work that did not finish.
pub(crate) trait PassGate {
    fn id(&self) -> &'static str;
    fn should_run(&self) -> CcResult<GateDecision>;
    fn record_run(&self) -> CcResult<()>;
}

/// Shared compare/record plumbing for u64-signature gates: a signature
/// metadata key plus its algorithm-version key.
struct SignatureStore<'a> {
    db: &'a IndexDb,
    sig_key: &'static str,
    algo_key: &'static str,
    algo_version: &'static str,
}

impl SignatureStore<'_> {
    /// Metadata-first comparison. The signature closure is invoked only when
    /// the recorded state is comparable (matching algorithm version and a
    /// present signature value).
    fn decide(&self, signature: &dyn Fn() -> CcResult<u64>) -> CcResult<GateDecision> {
        let recorded_algo = self
            .db
            .get_metadata(self.algo_key)?
            .unwrap_or_else(|| LEGACY_ALGORITHM_VERSION.to_string());
        if recorded_algo != self.algo_version {
            return Ok(GateDecision::run("signature algorithm changed"));
        }
        let Some(recorded) = self.db.get_metadata(self.sig_key)? else {
            return Ok(GateDecision::run("no recorded signature"));
        };
        // An unparseable recorded value never matches, matching the previous
        // `parse::<u64>().ok()` comparison semantics.
        if recorded.parse::<u64>().ok() == Some(signature()?) {
            Ok(GateDecision::skip("signature unchanged"))
        } else {
            Ok(GateDecision::run("signature changed"))
        }
    }

    fn record(&self, signature: u64) -> CcResult<()> {
        self.db.set_metadata(self.sig_key, &signature.to_string())?;
        self.db.set_metadata(self.algo_key, self.algo_version)
    }
}

/// Signature gate over committed DB state. `compute` queries the read pool;
/// its result is cached for the lifetime of the gate (one build).
pub(crate) struct DbSignatureGate<'a, F: Fn() -> CcResult<u64>> {
    id: &'static str,
    store: SignatureStore<'a>,
    compute: F,
    cached: Cell<Option<u64>>,
    /// When set (full rebuild), `should_run` answers without touching
    /// metadata or computing the signature; the signature is then computed
    /// once at record time.
    forced: Option<&'static str>,
}

impl<'a, F: Fn() -> CcResult<u64>> DbSignatureGate<'a, F> {
    pub(crate) fn new(
        id: &'static str,
        db: &'a IndexDb,
        sig_key: &'static str,
        algo_key: &'static str,
        algo_version: &'static str,
        forced: Option<&'static str>,
        compute: F,
    ) -> Self {
        Self {
            id,
            store: SignatureStore {
                db,
                sig_key,
                algo_key,
                algo_version,
            },
            compute,
            cached: Cell::new(None),
            forced,
        }
    }

    fn signature(&self) -> CcResult<u64> {
        if let Some(sig) = self.cached.get() {
            return Ok(sig);
        }
        let sig = (self.compute)()?;
        self.cached.set(Some(sig));
        Ok(sig)
    }
}

impl<F: Fn() -> CcResult<u64>> PassGate for DbSignatureGate<'_, F> {
    fn id(&self) -> &'static str {
        self.id
    }

    fn should_run(&self) -> CcResult<GateDecision> {
        if let Some(reason) = self.forced {
            return Ok(GateDecision::run(reason));
        }
        self.store.decide(&|| self.signature())
    }

    fn record_run(&self) -> CcResult<()> {
        self.store.record(self.signature()?)
    }
}

/// Signature gate over filesystem state (infallible compute: a stat walk).
/// Same compare/record semantics as [`DbSignatureGate`].
pub(crate) struct FileSignatureGate<'a, F: Fn() -> u64> {
    id: &'static str,
    store: SignatureStore<'a>,
    compute: F,
    cached: Cell<Option<u64>>,
}

impl<'a, F: Fn() -> u64> FileSignatureGate<'a, F> {
    pub(crate) fn new(
        id: &'static str,
        db: &'a IndexDb,
        sig_key: &'static str,
        algo_key: &'static str,
        algo_version: &'static str,
        compute: F,
    ) -> Self {
        Self {
            id,
            store: SignatureStore {
                db,
                sig_key,
                algo_key,
                algo_version,
            },
            compute,
            cached: Cell::new(None),
        }
    }

    fn signature(&self) -> u64 {
        if let Some(sig) = self.cached.get() {
            return sig;
        }
        let sig = (self.compute)();
        self.cached.set(Some(sig));
        sig
    }
}

impl<F: Fn() -> u64> PassGate for FileSignatureGate<'_, F> {
    fn id(&self) -> &'static str {
        self.id
    }

    fn should_run(&self) -> CcResult<GateDecision> {
        self.store.decide(&|| Ok(self.signature()))
    }

    fn record_run(&self) -> CcResult<()> {
        self.store.record(self.signature())
    }
}

/// Cache-key gate over an opaque string (e.g. the git HEAD sha). `None` or an
/// empty key means the source is unavailable: the pass runs unconditionally
/// and nothing is recorded, so a transient read failure never poisons the
/// skip cache.
pub(crate) struct StringCacheGate<'a, F: Fn() -> Option<String>> {
    id: &'static str,
    db: &'a IndexDb,
    key: &'static str,
    compute: F,
    cached: std::cell::RefCell<Option<Option<String>>>,
}

impl<'a, F: Fn() -> Option<String>> StringCacheGate<'a, F> {
    pub(crate) fn new(id: &'static str, db: &'a IndexDb, key: &'static str, compute: F) -> Self {
        Self {
            id,
            db,
            key,
            compute,
            cached: std::cell::RefCell::new(None),
        }
    }

    /// Current cache key, computed at most once per gate lifetime. Empty
    /// strings normalize to `None` (unavailable).
    fn current(&self) -> Option<String> {
        let mut cached = self.cached.borrow_mut();
        cached
            .get_or_insert_with(|| (self.compute)().filter(|value| !value.is_empty()))
            .clone()
    }
}

impl<F: Fn() -> Option<String>> PassGate for StringCacheGate<'_, F> {
    fn id(&self) -> &'static str {
        self.id
    }

    fn should_run(&self) -> CcResult<GateDecision> {
        let Some(current) = self.current() else {
            return Ok(GateDecision::run("cache key unavailable"));
        };
        if self.db.get_metadata(self.key)?.as_deref() == Some(current.as_str()) {
            Ok(GateDecision::skip("cache key unchanged"))
        } else {
            Ok(GateDecision::run("cache key changed"))
        }
    }

    fn record_run(&self) -> CcResult<()> {
        // Unavailable key: skip the record so the next build runs again.
        if let Some(current) = self.current() {
            self.db.set_metadata(self.key, &current)?;
        }
        Ok(())
    }
}

/// Gate for passes with no skip condition.
pub(crate) struct Unconditional {
    id: &'static str,
}

impl Unconditional {
    pub(crate) fn new(id: &'static str) -> Self {
        Self { id }
    }
}

impl PassGate for Unconditional {
    fn id(&self) -> &'static str {
        self.id
    }

    fn should_run(&self) -> CcResult<GateDecision> {
        Ok(GateDecision::run("unconditional"))
    }

    fn record_run(&self) -> CcResult<()> {
        Ok(())
    }
}

/// Couples two gates whose passes execute as a single round: the round runs
/// when either gate's input changed, and both gates record afterwards (the
/// round's output covers both input groups). The individual decisions stay
/// observable so the pass can route work per input group.
pub(crate) struct PairGate<'a> {
    id: &'static str,
    first: &'a dyn PassGate,
    second: &'a dyn PassGate,
    decisions: Cell<Option<(bool, bool)>>,
}

impl<'a> PairGate<'a> {
    pub(crate) fn new(id: &'static str, first: &'a dyn PassGate, second: &'a dyn PassGate) -> Self {
        Self {
            id,
            first,
            second,
            decisions: Cell::new(None),
        }
    }

    pub(crate) fn first_changed(&self) -> bool {
        self.decisions.get().map(|d| d.0).unwrap_or(false)
    }

    pub(crate) fn second_changed(&self) -> bool {
        self.decisions.get().map(|d| d.1).unwrap_or(false)
    }
}

impl PassGate for PairGate<'_> {
    fn id(&self) -> &'static str {
        self.id
    }

    fn should_run(&self) -> CcResult<GateDecision> {
        let first = self.first.should_run()?;
        let second = self.second.should_run()?;
        self.decisions.set(Some((first.run, second.run)));
        Ok(match (first.run, second.run) {
            (true, true) => GateDecision::run("both signatures changed"),
            (true, false) => GateDecision::run("first signature changed"),
            (false, true) => GateDecision::run("second signature changed"),
            (false, false) => GateDecision::skip("signatures unchanged"),
        })
    }

    fn record_run(&self) -> CcResult<()> {
        self.first.record_run()?;
        self.second.record_run()
    }
}

/// When a completed pass records its gate marker.
///
/// `Immediate` records right after the pass body returns (co-change, infra,
/// community). `Deferred` postpones the record until every pass in the batch
/// completed — the synthesis round uses this so a later community failure
/// leaves no signature recorded for the build, exactly as before the seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordTiming {
    Immediate,
    Deferred,
}

/// One pass wired to its gate. `run` returns whether the pass completed and
/// its marker may be recorded — `Ok(false)` means the pass degraded
/// gracefully (e.g. git unavailable) and must run again next build.
pub(crate) struct GatedPass<'a> {
    pub(crate) gate: &'a dyn PassGate,
    pub(crate) timing: RecordTiming,
    pub(crate) run: &'a dyn Fn() -> CcResult<bool>,
}

/// Drive a batch of gated passes in declaration order:
/// `gate.should_run → pass → gate.record_run` (record timing per pass).
/// Deferred records execute after all passes completed, in declaration order.
pub(crate) fn run_gated_passes(passes: &[GatedPass]) -> CcResult<()> {
    let mut deferred: Vec<&dyn PassGate> = Vec::new();
    for pass in passes {
        let decision = pass.gate.should_run()?;
        tracing::debug!(
            pass = pass.gate.id(),
            run = decision.run,
            reason = decision.reason,
            "pass gate decision"
        );
        if !decision.run {
            continue;
        }
        if !(pass.run)()? {
            continue;
        }
        match pass.timing {
            RecordTiming::Immediate => pass.gate.record_run()?,
            RecordTiming::Deferred => deferred.push(pass.gate),
        }
    }
    for gate in deferred {
        gate.record_run()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn open_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("gate.db")).unwrap().0;
        (tmp, db)
    }

    /// Counting compute closure: returns a fixed signature and counts calls.
    fn counted_compute(value: u64, calls: &Cell<usize>) -> impl Fn() -> CcResult<u64> + '_ {
        move || {
            calls.set(calls.get() + 1);
            Ok(value)
        }
    }

    #[test]
    fn db_signature_gate_runs_without_recorded_signature_and_skips_after_record() {
        let (_tmp, db) = open_db();
        let calls = Cell::new(0usize);

        let gate = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "1",
            None,
            counted_compute(42, &calls),
        );
        let decision = gate.should_run().unwrap();
        assert!(decision.run, "missing signature must run");
        assert_eq!(decision.reason, "no recorded signature");
        assert_eq!(
            calls.get(),
            0,
            "no comparison needed, signature must not be computed for the decision"
        );

        gate.record_run().unwrap();
        assert_eq!(db.get_metadata("test_sig").unwrap().as_deref(), Some("42"));
        assert_eq!(
            db.get_metadata("test_sig_algo").unwrap().as_deref(),
            Some("1")
        );

        // Fresh gate (next build), same inputs: must skip.
        let calls2 = Cell::new(0usize);
        let gate2 = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "1",
            None,
            counted_compute(42, &calls2),
        );
        let decision = gate2.should_run().unwrap();
        assert!(!decision.run, "unchanged signature must skip");
        assert_eq!(decision.reason, "signature unchanged");
        assert_eq!(calls2.get(), 1, "comparison computes exactly once");
    }

    #[test]
    fn db_signature_gate_runs_when_signature_changed() {
        let (_tmp, db) = open_db();
        db.set_metadata("test_sig", "41").unwrap();

        let calls = Cell::new(0usize);
        let gate = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "1",
            None,
            counted_compute(42, &calls),
        );
        let decision = gate.should_run().unwrap();
        assert!(decision.run);
        assert_eq!(decision.reason, "signature changed");

        // record_run reuses the cached signature from should_run.
        gate.record_run().unwrap();
        assert_eq!(calls.get(), 1, "same-build signature computes at most once");
        assert_eq!(db.get_metadata("test_sig").unwrap().as_deref(), Some("42"));
    }

    #[test]
    fn db_signature_gate_missing_algo_key_reads_as_version_one() {
        let (_tmp, db) = open_db();
        // Signature recorded by a build that predates the algorithm key.
        db.set_metadata("test_sig", "42").unwrap();

        let calls = Cell::new(0usize);
        let gate = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "1",
            None,
            counted_compute(42, &calls),
        );
        assert!(
            !gate.should_run().unwrap().run,
            "missing algo key must be treated as version 1 (compatible)"
        );
    }

    #[test]
    fn db_signature_gate_algorithm_version_mismatch_forces_run() {
        let (_tmp, db) = open_db();
        db.set_metadata("test_sig", "42").unwrap();
        db.set_metadata("test_sig_algo", "1").unwrap();

        let calls = Cell::new(0usize);
        let gate = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "2",
            None,
            counted_compute(42, &calls),
        );
        let decision = gate.should_run().unwrap();
        assert!(
            decision.run,
            "equal signature with older algorithm must run"
        );
        assert_eq!(decision.reason, "signature algorithm changed");
        assert_eq!(calls.get(), 0, "no comparison: signature not computed");

        gate.record_run().unwrap();
        assert_eq!(
            db.get_metadata("test_sig_algo").unwrap().as_deref(),
            Some("2"),
            "record must persist the new algorithm version"
        );
        // After the record, the v2 gate skips.
        let gate2 = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "2",
            None,
            counted_compute(42, &calls),
        );
        assert!(!gate2.should_run().unwrap().run);
    }

    #[test]
    fn db_signature_gate_forced_skips_metadata_and_compute_until_record() {
        let (_tmp, db) = open_db();
        // Recorded state matches — a non-forced gate would skip.
        db.set_metadata("test_sig", "42").unwrap();

        let calls = Cell::new(0usize);
        let gate = DbSignatureGate::new(
            "p",
            &db,
            "test_sig",
            "test_sig_algo",
            "1",
            Some("full rebuild"),
            counted_compute(42, &calls),
        );
        let decision = gate.should_run().unwrap();
        assert!(decision.run, "forced gate always runs");
        assert_eq!(decision.reason, "full rebuild");
        assert_eq!(calls.get(), 0, "forced decision must not compute");

        gate.record_run().unwrap();
        assert_eq!(calls.get(), 1, "record computes the signature once");
    }

    #[test]
    fn file_signature_gate_compare_and_record() {
        let (_tmp, db) = open_db();
        let calls = Cell::new(0usize);
        let compute = || {
            calls.set(calls.get() + 1);
            7u64
        };

        let gate = FileSignatureGate::new("infra", &db, "fs_sig", "fs_sig_algo", "1", &compute);
        assert!(gate.should_run().unwrap().run, "missing signature runs");
        gate.record_run().unwrap();
        assert_eq!(calls.get(), 1, "decision + record share one computation");
        assert_eq!(db.get_metadata("fs_sig").unwrap().as_deref(), Some("7"));

        let gate2 = FileSignatureGate::new("infra", &db, "fs_sig", "fs_sig_algo", "1", &compute);
        assert!(
            !gate2.should_run().unwrap().run,
            "unchanged stat walk skips"
        );

        db.set_metadata("fs_sig", "8").unwrap();
        let gate3 = FileSignatureGate::new("infra", &db, "fs_sig", "fs_sig_algo", "1", &compute);
        assert!(gate3.should_run().unwrap().run, "changed signature runs");
    }

    #[test]
    fn string_cache_gate_unavailable_key_runs_and_never_records() {
        let (_tmp, db) = open_db();

        let gate = StringCacheGate::new("cochange", &db, "head_key", || None);
        let decision = gate.should_run().unwrap();
        assert!(decision.run);
        assert_eq!(decision.reason, "cache key unavailable");
        gate.record_run().unwrap();
        assert_eq!(
            db.get_metadata("head_key").unwrap(),
            None,
            "unavailable key must not be recorded"
        );

        // Empty string normalizes to unavailable.
        let gate = StringCacheGate::new("cochange", &db, "head_key", || Some(String::new()));
        assert!(gate.should_run().unwrap().run);
        gate.record_run().unwrap();
        assert_eq!(db.get_metadata("head_key").unwrap(), None);
    }

    #[test]
    fn string_cache_gate_skips_on_match_and_runs_on_change() {
        let (_tmp, db) = open_db();
        let head = RefCell::new("abc".to_string());
        let compute = || Some(head.borrow().clone());

        let gate = StringCacheGate::new("cochange", &db, "head_key", &compute);
        assert!(gate.should_run().unwrap().run, "no recorded key runs");
        gate.record_run().unwrap();
        assert_eq!(db.get_metadata("head_key").unwrap().as_deref(), Some("abc"));

        let gate2 = StringCacheGate::new("cochange", &db, "head_key", &compute);
        let decision = gate2.should_run().unwrap();
        assert!(!decision.run, "matching key skips");
        assert_eq!(decision.reason, "cache key unchanged");

        *head.borrow_mut() = "def".to_string();
        let gate3 = StringCacheGate::new("cochange", &db, "head_key", &compute);
        assert!(gate3.should_run().unwrap().run, "advanced key runs");
    }

    #[test]
    fn unconditional_gate_always_runs() {
        let gate = Unconditional::new("adr");
        assert!(gate.should_run().unwrap().run);
        gate.record_run().unwrap();
        assert!(gate.should_run().unwrap().run);
    }

    #[test]
    fn pair_gate_exposes_individual_decisions_and_records_both() {
        let (_tmp, db) = open_db();
        // First gate changed (recorded 1, computes 2); second unchanged.
        db.set_metadata("sig_a", "1").unwrap();
        db.set_metadata("sig_b", "5").unwrap();
        let calls = Cell::new(0usize);

        let gate_a = DbSignatureGate::new(
            "a",
            &db,
            "sig_a",
            "sig_a_algo",
            "1",
            None,
            counted_compute(2, &calls),
        );
        let gate_b = DbSignatureGate::new(
            "b",
            &db,
            "sig_b",
            "sig_b_algo",
            "1",
            None,
            counted_compute(5, &calls),
        );
        let pair = PairGate::new("round", &gate_a, &gate_b);

        let decision = pair.should_run().unwrap();
        assert!(decision.run, "round runs when either input changed");
        assert!(pair.first_changed());
        assert!(!pair.second_changed());

        pair.record_run().unwrap();
        assert_eq!(
            db.get_metadata("sig_a").unwrap().as_deref(),
            Some("2"),
            "changed gate records its new signature"
        );
        assert_eq!(
            db.get_metadata("sig_b").unwrap().as_deref(),
            Some("5"),
            "unchanged gate re-records the same value (no-op write)"
        );
    }

    #[test]
    fn pair_gate_skips_when_both_unchanged() {
        let (_tmp, db) = open_db();
        db.set_metadata("sig_a", "2").unwrap();
        db.set_metadata("sig_b", "5").unwrap();
        let calls = Cell::new(0usize);

        let gate_a = DbSignatureGate::new(
            "a",
            &db,
            "sig_a",
            "sig_a_algo",
            "1",
            None,
            counted_compute(2, &calls),
        );
        let gate_b = DbSignatureGate::new(
            "b",
            &db,
            "sig_b",
            "sig_b_algo",
            "1",
            None,
            counted_compute(5, &calls),
        );
        let pair = PairGate::new("round", &gate_a, &gate_b);

        let decision = pair.should_run().unwrap();
        assert!(!decision.run);
        assert!(!pair.first_changed());
        assert!(!pair.second_changed());
    }

    /// Recording gate for orchestration tests: configurable decision, logs
    /// record calls into a shared journal.
    struct ProbeGate<'a> {
        id: &'static str,
        run: bool,
        journal: &'a RefCell<Vec<String>>,
    }

    impl PassGate for ProbeGate<'_> {
        fn id(&self) -> &'static str {
            self.id
        }

        fn should_run(&self) -> CcResult<GateDecision> {
            Ok(if self.run {
                GateDecision::run("probe")
            } else {
                GateDecision::skip("probe")
            })
        }

        fn record_run(&self) -> CcResult<()> {
            self.journal
                .borrow_mut()
                .push(format!("record:{}", self.id));
            Ok(())
        }
    }

    #[test]
    fn run_gated_passes_orders_immediate_and_deferred_records() {
        let journal = RefCell::new(Vec::new());
        let deferred_gate = ProbeGate {
            id: "synthesis",
            run: true,
            journal: &journal,
        };
        let immediate_gate = ProbeGate {
            id: "community",
            run: true,
            journal: &journal,
        };
        let skipped_gate = ProbeGate {
            id: "skipped",
            run: false,
            journal: &journal,
        };

        let run_synthesis = || -> CcResult<bool> {
            journal.borrow_mut().push("run:synthesis".to_string());
            Ok(true)
        };
        let run_community = || -> CcResult<bool> {
            journal.borrow_mut().push("run:community".to_string());
            Ok(true)
        };
        let run_skipped = || -> CcResult<bool> {
            journal.borrow_mut().push("run:skipped".to_string());
            Ok(true)
        };

        run_gated_passes(&[
            GatedPass {
                gate: &deferred_gate,
                timing: RecordTiming::Deferred,
                run: &run_synthesis,
            },
            GatedPass {
                gate: &skipped_gate,
                timing: RecordTiming::Immediate,
                run: &run_skipped,
            },
            GatedPass {
                gate: &immediate_gate,
                timing: RecordTiming::Immediate,
                run: &run_community,
            },
        ])
        .unwrap();

        assert_eq!(
            *journal.borrow(),
            vec![
                "run:synthesis",
                "run:community",
                "record:community",
                "record:synthesis",
            ],
            "deferred records run after all passes, skipped passes never run"
        );
    }

    #[test]
    fn run_gated_passes_skips_record_when_pass_did_not_complete() {
        let journal = RefCell::new(Vec::new());
        let gate = ProbeGate {
            id: "cochange",
            run: true,
            journal: &journal,
        };
        let degraded = || -> CcResult<bool> {
            journal.borrow_mut().push("run:cochange".to_string());
            Ok(false)
        };

        run_gated_passes(&[GatedPass {
            gate: &gate,
            timing: RecordTiming::Immediate,
            run: &degraded,
        }])
        .unwrap();

        assert_eq!(
            *journal.borrow(),
            vec!["run:cochange"],
            "Ok(false) from the pass must not record the gate marker"
        );
    }
}
