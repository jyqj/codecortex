//! Measured V16 exact-scan resource submatrix, not full P7-015/V20 acceptance.
#![cfg(all(feature = "semantic", target_os = "linux"))]

use cc_db::index_migrate::migrate_index_db;
use cc_db::semantic_manifest_reads::{SemanticManifestReads, SemanticManifestRow};
use cc_eval::benchmark::sampler::process_snapshot;
use cc_model::retrieval::HardScope;
use cc_semantic::cache::ArtifactCache;
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::InputDigest;
use cc_semantic::vector::exact::{search, space_manifest_reads, ExactSearch, ManifestCandidates};
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// Tracks requested live Rust allocation bytes, not allocator arenas or SQLite
// C allocations. Parent PID sampler covers those as resident pages instead.
struct MeasuredAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static MEASURE: AtomicBool = AtomicBool::new(false);

fn allocated(n: usize) {
    let live = LIVE.fetch_add(n, Ordering::SeqCst) + n;
    if MEASURE.load(Ordering::SeqCst) {
        PEAK.fetch_max(live, Ordering::SeqCst);
    }
}

unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, size) };
        if !p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
            allocated(size);
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;
const DIM: usize = 128;
const BATCH: usize = 64;
const K: usize = 10;
const REPEATS: usize = 2;
const MIB: usize = 1024 * 1024;

struct Observed<'a> {
    source: &'a dyn ManifestCandidates,
    rows: Cell<usize>,
    max_batch: Cell<usize>,
    calls: Cell<usize>,
}
impl ManifestCandidates for Observed<'_> {
    fn next_batch(
        &self,
        after: &str,
        batch: usize,
    ) -> cc_model::CcResult<Vec<SemanticManifestRow>> {
        let rows = self.source.next_batch(after, batch)?;
        self.rows.set(self.rows.get() + rows.len());
        self.max_batch.set(self.max_batch.get().max(rows.len()));
        self.calls.set(self.calls.get() + 1);
        Ok(rows)
    }
}

fn cache_files(root: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let (f, b) = cache_files(&path);
            files += f;
            bytes += b;
        } else {
            files += 1;
            bytes += path.metadata().unwrap().len();
        }
    }
    (files, bytes)
}

fn sqlite_counter(db: &rusqlite::Connection, op: i32) -> i32 {
    let (mut current, mut highwater) = (0, 0);
    // This connection is owned by this worker, no other thread can close or
    // operate on its handle. db_status reads counters without resetting them.
    let status = unsafe {
        rusqlite::ffi::sqlite3_db_status(db.handle(), op, &mut current, &mut highwater, 0)
    };
    assert_eq!(status, rusqlite::ffi::SQLITE_OK);
    current
}

fn seed(root: &Path, n: usize, corrupt: &str) {
    let mut db = rusqlite::Connection::open(root.join("index.db")).unwrap();
    migrate_index_db(&db).unwrap();
    db.execute_batch("PRAGMA cache_size=-2048; PRAGMA mmap_size=0; PRAGMA foreign_keys=ON;")
        .unwrap();
    let cache = ArtifactCache::open(root.join("cache"), "v16-resource".into()).unwrap();
    let space = VectorSpace::new("synthetic/v16-memory", DIM as u32).unwrap();
    let digest = space.digest().unwrap();
    let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "synthetic")
        .unwrap()
        .digest()
        .unwrap();
    let tx = db.transaction().unwrap();
    for i in 0..n {
        let key = format!("doc-{i:08}");
        let input = InputDigest::of_input(key.as_bytes()).unwrap();
        // Different addresses and real cache files. Identical vectors make
        // top-k deterministically known without storing an O(N) oracle.
        let reference = cache
            .put(&space, &input, &spec, &vec![1.0; DIM], 1000)
            .unwrap();
        let path = format!("src/{key}.rs");
        tx.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','hash',1.0,1,'2026-01-01')", [&path]).unwrap();
        tx.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES(?1,?2,'rust',0,1,2,'body')", rusqlite::params![key,path]).unwrap();
        tx.execute("INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,'v1',?2,?1,NULL,'{}','{}')", rusqlite::params![key,path]).unwrap();
        tx.execute("INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,space_id,artifact_ref,published_at,published_incarnation) VALUES(?1,'v1',?2,'enc',?3,?4,?5,'2026-01-01','inc')", rusqlite::params![key,path,input.as_str(),digest.as_str(),reference.as_str()]).unwrap();
    }
    tx.commit().unwrap();
    if !corrupt.is_empty() {
        fn inflate(root: &Path, suffix: &str) -> bool {
            for entry in std::fs::read_dir(root).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if inflate(&path, suffix) {
                        return true;
                    }
                } else if path.to_string_lossy().ends_with(suffix) {
                    // Sparse extension has zero seed allocation; fs::read
                    // must nevertheless allocate/read 64 MiB on the query.
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(path)
                        .unwrap()
                        .set_len((64 * MIB) as u64)
                        .unwrap();
                    return true;
                }
            }
            false
        }
        assert!(inflate(&root.join("cache"), corrupt));
    }
}

#[test]
fn resource_worker() {
    let Ok(root) = std::env::var("V16_RESOURCE_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let n: usize = std::env::var("V16_RESOURCE_N").unwrap().parse().unwrap();
    let concurrency: usize = std::env::var("V16_RESOURCE_C").unwrap().parse().unwrap();
    let corrupt = std::env::var("V16_RESOURCE_CORRUPT").unwrap_or_default();
    seed(root, n, &corrupt);
    let corrupt = corrupt.as_str();
    let disk_before = cache_files(&root.join("cache"));
    std::fs::write(root.join("ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("go").exists() {
        assert!(
            Instant::now() < deadline,
            "parent did not start measurement"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    MEASURE.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let barrier = std::sync::Barrier::new(concurrency);
    let observations = std::thread::scope(|s| {
        let workers: Vec<_> = (0..concurrency).map(|_| {
            let barrier = &barrier;
            s.spawn(move || {
                let db = rusqlite::Connection::open(root.join("index.db")).unwrap();
                db.execute_batch("PRAGMA cache_size=-2048; PRAGMA mmap_size=0; PRAGMA query_only=ON;").unwrap();
                let configured_cache_kib: i64 = db.query_row("PRAGMA cache_size", [], |r| r.get(0)).unwrap();
                let configured_mmap: i64 = db.query_row("PRAGMA mmap_size", [], |r| r.get(0)).unwrap();
                let cache = ArtifactCache::open(root.join("cache"), "v16-resource".into()).unwrap();
                let space = VectorSpace::new("synthetic/v16-memory", DIM as u32).unwrap();
                let digest = space.digest().unwrap();
                let reads = SemanticManifestReads::on(&db);
                let source = space_manifest_reads(&reads, &digest);
                let observed = Observed { source: &source, rows: Cell::new(0), max_batch: Cell::new(0), calls: Cell::new(0) };
                let query = vec![1.0; DIM];
                let filter = HardScope { path_prefix: None, languages: None, file_paths: None };
                barrier.wait();
                let mut result_count = 0;
                for _ in 0..REPEATS {
                    let got = search(&cache, &observed, ExactSearch { space: &space, query: &query, filter: &filter, k: K, batch_rows: if corrupt.is_empty() { BATCH } else { 1 } }).unwrap();
                    if corrupt.is_empty() {
                        assert_eq!(got.len(), K);
                        for (i, doc) in got.iter().enumerate() {
                            assert_eq!(doc.doc_key, format!("doc-{i:08}"));
                            assert!((doc.score - 1.0).abs() < 1e-12);
                        }
                    } else { assert!(got.is_empty(), "corrupt vector must be skipped"); }
                    result_count += got.len();
                }
                json!({"rows_scanned":observed.rows.get(),"max_batch_observed":observed.max_batch.get(),"scan_calls":observed.calls.get(),"result_count":result_count,"sqlite_cache_size_kib":configured_cache_kib,"sqlite_mmap_bytes":configured_mmap,
                    "sqlite_cache_used_bytes_after_scan":sqlite_counter(&db, rusqlite::ffi::SQLITE_DBSTATUS_CACHE_USED),
                    "sqlite_cache_hits":sqlite_counter(&db, rusqlite::ffi::SQLITE_DBSTATUS_CACHE_HIT),
                    "sqlite_cache_misses":sqlite_counter(&db, rusqlite::ffi::SQLITE_DBSTATUS_CACHE_MISS)})
            })
        }).collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    MEASURE.store(false, Ordering::SeqCst);
    let peak = PEAK.load(Ordering::SeqCst);
    let elapsed = started.elapsed();
    let heap_delta = peak.saturating_sub(base);
    // Fixed envelope independent of N; conservative harness/connection slack.
    let heap_envelope = 4 * MIB + concurrency * (BATCH * 2048 + K * 128 + DIM * 8 + MIB);
    let disk_after = cache_files(&root.join("cache"));
    assert_eq!(
        disk_before, disk_after,
        "read-only scan changed artifact cache"
    );
    let result = json!({"n":n,"concurrency":concurrency,"dimension":DIM,"k":K,"batch_rows":if corrupt.is_empty(){BATCH}else{1},"repeats":REPEATS,"corrupt_suffix":corrupt,"rust_live_baseline_bytes":base,"rust_live_peak_bytes":peak,"rust_live_peak_delta_bytes":heap_delta,"rust_heap_envelope_bytes":heap_envelope,"heap_within_envelope":heap_delta<=heap_envelope,"elapsed_ms":elapsed.as_millis(),"cache_files_before":disk_before.0,"cache_disk_bytes_before":disk_before.1,"cache_files_after":disk_after.0,"cache_disk_bytes_after":disk_after.1,"workers":observations});
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "explicit resource run requires V16_RESOURCE_OUTPUT; no claim from default CI"]
fn measured_exact_resource_submatrix() {
    let output = std::env::var("V16_RESOURCE_OUTPUT")
        .expect("set V16_RESOURCE_OUTPUT to an evidence JSON path");
    let mut cells = Vec::<Value>::new();
    // Negative cache cases run first so a defect gets a receipt even if a
    // later normal-scale cell exceeds its resource envelope.
    for (n, c, corrupt) in [
        (1, 1, ".bin"),
        (1, 1, ".meta.json"),
        (1000, 1, ""),
        (1000, 4, ""),
        (8000, 1, ""),
        (8000, 4, ""),
        (16000, 1, ""),
        (16000, 4, ""),
    ] {
        let root = tempfile::tempdir().unwrap();
        let log = std::fs::File::create(root.path().join("child.log")).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "resource_worker", "--nocapture"])
            .env("V16_RESOURCE_ROOT", root.path())
            .env("V16_RESOURCE_N", n.to_string())
            .env("V16_RESOURCE_C", c.to_string())
            .env("V16_RESOURCE_CORRUPT", corrupt)
            .env("CODECORTEX_BENCH_PROCESS_PROBE", "1")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut baseline = None;
        let mut peak = 0;
        let mut samples = 0;
        let mut unavailable = 0;
        let mut kernel = String::new();
        let mut method = String::new();
        let mut last_sample = Instant::now();
        let mut max_gap_ms = 0;
        let status = loop {
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("case deadline exceeded");
            }
            if root.path().join("ready").exists() {
                match process_snapshot(child.id()) {
                    Some(s) => {
                        samples += 1;
                        peak = peak.max(s.resident_bytes);
                        kernel = s.kernel_release;
                        method = s.resident_method.into();
                        if baseline.is_none() {
                            baseline = Some(s.resident_bytes);
                            std::fs::write(root.path().join("go"), b"go").unwrap();
                            last_sample = Instant::now();
                        } else {
                            max_gap_ms = max_gap_ms.max(last_sample.elapsed().as_millis());
                            last_sample = Instant::now();
                        }
                    }
                    None => unavailable += 1,
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
        let base = baseline.expect("live child RSS baseline unavailable");
        let delta = peak.saturating_sub(base);
        let rss_envelope = (32 * MIB + c * 4 * MIB) as u64;
        result["rss"] = json!({"pid":child.id(),"baseline_bytes":base,"sampled_peak_bytes":peak,"sampled_peak_delta_bytes":delta,"envelope_bytes":rss_envelope,"within_envelope":delta<=rss_envelope,"samples":samples,"unavailable_samples":unavailable,"nominal_interval_ms":5,"max_observed_gap_ms":max_gap_ms,"method":method,"kernel_release":kernel});
        eprintln!("V16_CELL {result}");
        cells.push(result);
        std::fs::write(&output,serde_json::to_vec_pretty(&json!({"target_source_sha":"b973de5c542db2ba46ddb0798b03d67627e42275","platform":std::env::consts::OS,"arch":std::env::consts::ARCH,"cells":cells})).unwrap()).unwrap();
        if corrupt.is_empty() {
            assert!(
                cells.last().unwrap()["heap_within_envelope"]
                    .as_bool()
                    .unwrap(),
                "normal scan Rust heap envelope exceeded"
            );
            assert!(
                delta <= rss_envelope,
                "normal scan sampled RSS envelope exceeded"
            );
        }
    }
}
