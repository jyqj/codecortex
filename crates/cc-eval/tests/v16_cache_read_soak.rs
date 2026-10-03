//! Longer real read workloads make post-fix RSS observable without sleeping
//! while retaining a constructed allocation. PR48's original matrix is intact.
#![cfg(all(feature = "semantic", target_os = "linux"))]
use cc_eval::benchmark::sampler::process_snapshot;
use cc_semantic::cache::{ArtifactCache, CacheRead};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::InputDigest;
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Counter;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
fn add(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::SeqCst) + size;
    if ACTIVE.load(Ordering::SeqCst) {
        PEAK.fetch_max(live, Ordering::SeqCst);
    }
}
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            add(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(p, layout) };
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(p, layout, size) };
        if !p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
            add(size);
        }
        p
    }
}
#[global_allocator]
static COUNTER: Counter = Counter;
const READS: usize = 10_000;

#[test]
fn soak_worker() {
    let Ok(root) = std::env::var("V16_SOAK_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let suffix = std::env::var("V16_SOAK_SUFFIX").unwrap();
    let cache = ArtifactCache::open(root.join("cache"), "soak".into()).unwrap();
    let space = VectorSpace::new("synthetic/cache-read-soak", 128).unwrap();
    let input = InputDigest::of_input(b"synthetic bounded read").unwrap();
    let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "synthetic")
        .unwrap()
        .digest()
        .unwrap();
    cache.put(&space, &input, &spec, &[1.0; 128], 1000).unwrap();
    if !suffix.is_empty() {
        let file = cache
            .root()
            .join("namespace-soak")
            .join(space.digest().unwrap().as_str())
            .join(input.as_str())
            .join(spec.as_str())
            .join(format!("{}.{suffix}", spec.as_str()));
        std::fs::OpenOptions::new()
            .write(true)
            .open(file)
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
    }
    std::fs::write(root.join("ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("go").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    ACTIVE.store(true, Ordering::SeqCst);
    let started = Instant::now();
    for _ in 0..READS {
        match cache.get(&space, &input, &spec).unwrap() {
            CacheRead::Hit(v) if suffix.is_empty() => assert_eq!(v.data, [1.0; 128]),
            CacheRead::Corrupt(report) if !suffix.is_empty() => {
                assert!(report.reason.contains("budget"))
            }
            other => panic!("unexpected read {other:?}"),
        }
    }
    ACTIVE.store(false, Ordering::SeqCst);
    let peak = PEAK.load(Ordering::SeqCst);
    let delta = peak.saturating_sub(base);
    let result = json!({"suffix":suffix,"reads":READS,"elapsed_ms":started.elapsed().as_millis(),"rust_live_baseline_bytes":base,"rust_live_peak_bytes":peak,"rust_live_peak_delta_bytes":delta,"heap_envelope_bytes":262144});
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
    assert!(
        delta <= 262144,
        "bounded reader shifted a large allocation elsewhere"
    );
}

#[test]
#[ignore = "explicit real resource measurement requires V16_SOAK_OUTPUT"]
fn measured_bounded_cache_read_soak() {
    let output = std::env::var("V16_SOAK_OUTPUT").unwrap();
    let mut cases = Vec::<Value>::new();
    for suffix in ["bin", "meta.json", ""] {
        let root = tempfile::tempdir().unwrap();
        let log = std::fs::File::create(root.path().join("child.log")).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "soak_worker", "--nocapture"])
            .env("V16_SOAK_ROOT", root.path())
            .env("V16_SOAK_SUFFIX", suffix)
            .env("CODECORTEX_BENCH_PROCESS_PROBE", "1")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let (mut baseline, mut peak, mut samples, mut unavailable) = (None, 0, 0, 0);
        let mut last = Instant::now();
        let mut gap = 0;
        let status = loop {
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("soak child timed out");
            }
            if root.path().join("ready").exists() {
                if let Some(s) = process_snapshot(child.id()) {
                    samples += 1;
                    peak = peak.max(s.resident_bytes);
                    if baseline.is_none() {
                        baseline = Some(s.resident_bytes);
                        std::fs::write(root.path().join("go"), b"go").unwrap();
                        last = Instant::now();
                    } else {
                        gap = gap.max(last.elapsed().as_millis());
                        last = Instant::now();
                    }
                } else {
                    unavailable += 1;
                }
            }
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(
            status.success(),
            "{}",
            std::fs::read_to_string(root.path().join("child.log")).unwrap()
        );
        let mut result: Value =
            serde_json::from_slice(&std::fs::read(root.path().join("result.json")).unwrap())
                .unwrap();
        let base = baseline.unwrap();
        let delta = peak.saturating_sub(base);
        result["rss"] = json!({"pid":child.id(),"baseline_bytes":base,"sampled_peak_bytes":peak,"sampled_delta_bytes":delta,"samples":samples,"unavailable_samples":unavailable,"nominal_interval_ms":5,"max_gap_ms":gap,"envelope_bytes":8*1024*1024,"method":"PR19 process_snapshot Linux PID RSS"});
        eprintln!("V16_SOAK {result}");
        cases.push(result);
        std::fs::write(&output, serde_json::to_vec_pretty(&cases).unwrap()).unwrap();
        assert!(samples >= 2, "query-window RSS not observed");
        assert!(
            delta <= 8 * 1024 * 1024,
            "sampled soak RSS envelope exceeded"
        );
    }
}
