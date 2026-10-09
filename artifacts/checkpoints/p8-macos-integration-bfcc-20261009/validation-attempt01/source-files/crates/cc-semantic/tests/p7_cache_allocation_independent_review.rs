//! Independent public ArtifactCache allocation/compatibility/TOCTOU review.
use cc_semantic::{
    cache::{ArtifactCache, CacheRead},
    spec::{DocumentEncodingSpec, VectorSpace, MAX_DIMENSION, MAX_MODEL_ID_BYTES},
    types::{DocSpecDigest, InputDigest},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(target_os = "linux")]
use std::{
    path::Path,
    time::{Duration, Instant},
};

struct Allocator;
thread_local! {static ACTIVE:Cell<bool>=const{Cell::new(false)};static MAX:Cell<usize>=const{Cell::new(0)};static TOTAL:Cell<usize>=const{Cell::new(0)};}
fn count(n: usize) {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            MAX.with(|m| m.set(m.get().max(n)));
            TOTAL.with(|t| t.set(t.get() + n));
        }
    });
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            count(l.size())
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            count(l.size())
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let p = unsafe { System.realloc(p, l, n) };
        if !p.is_null() {
            count(n)
        }
        p
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn measured<T>(name: &str, f: impl FnOnce() -> T, bound: usize) -> T {
    MAX.with(|v| v.set(0));
    TOTAL.with(|v| v.set(0));
    ACTIVE.with(|v| v.set(true));
    struct Off;
    impl Drop for Off {
        fn drop(&mut self) {
            ACTIVE.with(|v| v.set(false));
        }
    }
    let off = Off;
    let result = f();
    drop(off);
    let max = MAX.with(Cell::get);
    let total = TOTAL.with(Cell::get);
    eprintln!("INDEPENDENT_ALLOC {{\"case\":\"{name}\",\"max_request_bytes\":{max},\"total_requested_bytes\":{total},\"max_request_bound\":{bound}}}");
    assert!(
        max <= bound,
        "untrusted object caused request {max} beyond admitted {bound}"
    );
    result
}
struct Fixture {
    root: PathBuf,
    cache: ArtifactCache,
    space: VectorSpace,
    input: InputDigest,
    spec: DocSpecDigest,
}
impl Fixture {
    fn new(dim: u32, model: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "independent-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let cache = ArtifactCache::open(&root, "independent".into()).unwrap();
        let space = VectorSpace::new(model, dim).unwrap();
        let input = InputDigest::of_input(b"synthetic allocation review").unwrap();
        let spec = DocumentEncodingSpec::new(space.clone(), None, 1024, "synthetic")
            .unwrap()
            .digest()
            .unwrap();
        let f = Self {
            root,
            cache,
            space,
            input,
            spec,
        };
        f.put();
        f
    }
    fn put(&self) {
        let vector: Vec<_> = (0..self.space.dimension())
            .map(|i| if i % 2 == 0 { -0.0 } else { 1.25 })
            .collect();
        self.cache
            .put(&self.space, &self.input, &self.spec, &vector, i64::MIN)
            .unwrap();
    }
    fn path(&self, suffix: &str) -> PathBuf {
        self.root
            .join("namespace-independent")
            .join(self.space.digest().unwrap().as_str())
            .join(self.input.as_str())
            .join(self.spec.as_str())
            .join(format!("{}.{suffix}", self.spec.as_str()))
    }
    fn get(&self) -> cc_model::CcResult<CacheRead> {
        self.cache.get(&self.space, &self.input, &self.spec)
    }
    fn meta(&self) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(self.path("meta.json")).unwrap()).unwrap()
    }
    fn write_meta(&self, meta: &serde_json::Value) {
        std::fs::write(self.path("meta.json"), serde_json::to_vec(meta).unwrap()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn corrupt(read: cc_model::CcResult<CacheRead>) {
    assert!(matches!(read.unwrap(), CacheRead::Corrupt(_)));
}

#[test]
fn legal_writer_extremes_and_exact_read_bound_preserve_f32_bits() {
    // Worst JSON expansion among legal 512 model bytes: six-byte escapes.
    for dim in [1, MAX_DIMENSION] {
        let f = Fixture::new(dim, &"\u{1}".repeat(MAX_MODEL_ID_BYTES));
        let bytes = std::fs::read(f.path("meta.json")).unwrap();
        assert!(bytes.len() < 4096);
        assert!(bytes.len() > 3072);
        let CacheRead::Hit(hit) = measured(
            "legal-extreme",
            || f.get(),
            (dim as usize * 4 + 1).max(4097),
        )
        .unwrap() else {
            panic!("legal canonical writer rejected")
        };
        assert_eq!(hit.data.len(), dim as usize);
        for (i, v) in hit.data.iter().enumerate() {
            assert_eq!(
                v.to_bits(),
                if i % 2 == 0 {
                    (-0.0_f32).to_bits()
                } else {
                    1.25_f32.to_bits()
                }
            );
        }
        let mut padded = bytes;
        padded.resize(4096, b' ');
        std::fs::write(f.path("meta.json"), &padded).unwrap();
        assert!(matches!(f.get().unwrap(), CacheRead::Hit(_)));
        padded.push(b' ');
        std::fs::write(f.path("meta.json"), &padded).unwrap();
        corrupt(measured("metadata-bound-plus-one", || f.get(), 4097));
    }
}

#[test]
fn forged_dimensions_overflow_sparse_lengths_and_truncation_remain_bounded() {
    let f = Fixture::new(4, "independent/dimensions");
    for claimed in [0_u64, 65537, u32::MAX as u64, u64::MAX] {
        f.put();
        let mut meta = f.meta();
        meta["dimension"] = claimed.into();
        f.write_meta(&meta);
        corrupt(measured("forged-meta-dimension", || f.get(), 4097));
    }
    // VectorSpace is sealed and not Deserialize: invalid dimensions cannot
    // enter public get. Prove rejection at the only public construction gate.
    for invalid in [0, 65537, u32::MAX] {
        assert!(measured(
            "invalid-requested-dimension",
            || VectorSpace::new("synthetic", invalid),
            4097
        )
        .is_err());
    }
    for suffix in ["bin", "meta.json"] {
        f.put();
        std::fs::OpenOptions::new()
            .write(true)
            .open(f.path(suffix))
            .unwrap()
            .set_len(8_u64 * 1024 * 1024 * 1024)
            .unwrap();
        corrupt(measured("eight-gib-sparse", || f.get(), 4097));
    }
    f.put();
    let original = std::fs::read(f.path("bin")).unwrap();
    for len in 0..=17 {
        std::fs::write(f.path("bin"), &original[..len.min(original.len())]).unwrap();
        if len == 17 {
            std::fs::OpenOptions::new()
                .append(true)
                .open(f.path("bin"))
                .unwrap()
                .write_all(&[0])
                .unwrap();
        }
        let bytes = std::fs::read(f.path("bin")).unwrap();
        let mut meta = f.meta();
        meta["checksum"] = blake3::hash(&bytes).to_hex().to_string().into();
        f.write_meta(&meta);
        let read = measured("self-checksummed-length", || f.get(), 4097);
        if len == 16 {
            assert!(matches!(read.unwrap(), CacheRead::Hit(_)));
        } else {
            corrupt(read);
        }
    }
}

#[test]
fn incomplete_corrupt_and_io_error_semantics_are_distinct() {
    let f = Fixture::new(2, "independent/semantics");
    for (missing, huge) in [("bin", "meta.json"), ("meta.json", "bin")] {
        f.put();
        std::fs::remove_file(f.path(missing)).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(f.path(huge))
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
        assert!(matches!(
            measured("incomplete-large-object", || f.get(), 4097).unwrap(),
            CacheRead::Miss
        ));
    }
    f.put();
    std::fs::remove_file(f.path("bin")).unwrap();
    std::fs::create_dir(f.path("bin")).unwrap();
    assert!(measured("payload-io-error", || f.get(), 4097).is_err());
    std::fs::remove_dir(f.path("bin")).unwrap();
    f.put();
    for value in [
        serde_json::json!({"dimension":u32::MAX}),
        serde_json::json!({"unexpected":"field"}),
    ] {
        f.write_meta(&value);
        corrupt(measured("invalid-sidecar", || f.get(), 4097));
    }
}

#[cfg(target_os = "linux")]
fn fifo(path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
    }
    let p = CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { mkfifo(p.as_ptr(), 0o600) }, 0);
}
#[cfg(target_os = "linux")]
fn wait_payload_open(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let observed = std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| std::fs::read_link(entry.path()).ok().as_deref() == Some(path));
        if observed {
            return;
        }
        assert!(Instant::now() < deadline, "get never opened payload");
        std::thread::yield_now();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn actual_get_growth_after_open_and_atomic_replacement_do_not_bypass_budget() {
    // Metadata FIFO creates a deterministic pause AFTER public get has opened
    // payload. No production helper/test hook is invoked or modified.
    for replace in [false, true] {
        let f = std::sync::Arc::new(Fixture::new(4, "independent/toctou"));
        let payload = f.path("bin");
        let metadata = f.path("meta.json");
        let original_meta = std::fs::read(&metadata).unwrap();
        std::fs::remove_file(&metadata).unwrap();
        fifo(&metadata);
        let inside = f.clone();
        let job =
            std::thread::spawn(move || measured("growth-or-replacement", || inside.get(), 4097));
        wait_payload_open(&payload);
        if replace {
            let old = payload.with_extension("old");
            std::fs::rename(&payload, &old).unwrap();
            std::fs::File::create(&payload)
                .unwrap()
                .set_len(8_u64 * 1024 * 1024 * 1024)
                .unwrap();
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&payload)
                .unwrap()
                .set_len(8_u64 * 1024 * 1024 * 1024)
                .unwrap();
        }
        let mut sender = std::fs::OpenOptions::new()
            .write(true)
            .open(&metadata)
            .unwrap();
        sender.write_all(&original_meta).unwrap();
        drop(sender);
        let outcome = job.join().unwrap();
        if replace {
            assert!(
                matches!(outcome.unwrap(), CacheRead::Hit(_)),
                "opened old inode remains the valid snapshot"
            );
        } else {
            corrupt(outcome);
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn metadata_stream_extra_probe_and_error_before_eof_are_bounded() {
    let f = std::sync::Arc::new(Fixture::new(2, "independent/meta-stream"));
    let path = f.path("meta.json");
    let mut body = std::fs::read(&path).unwrap();
    body.resize(4097, b' ');
    std::fs::remove_file(&path).unwrap();
    fifo(&path);
    let inside = f.clone();
    let job = std::thread::spawn(move || measured("growing-sidecar-stream", || inside.get(), 4097));
    let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    writer.write_all(&body).unwrap(); // Keep stream open: budget+1 must terminate without awaiting EOF.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !job.is_finished() {
        assert!(
            Instant::now() < deadline,
            "bounded reader waited for unbounded EOF"
        );
        std::thread::yield_now();
    }
    corrupt(job.join().unwrap());
    drop(writer);
}

#[cfg(target_os = "linux")]
#[test]
fn public_get_short_read_then_growing_payload_consumes_only_budget_plus_probe() {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn ioctl(
            fd: std::ffi::c_int,
            request: std::ffi::c_ulong,
            output: *mut std::ffi::c_int,
        ) -> std::ffi::c_int;
    }
    fn available(file: &std::fs::File) -> i32 {
        let mut bytes = 0;
        // Linux FIONREAD also observes unread pipe bytes through the writer fd.
        assert_eq!(unsafe { ioctl(file.as_raw_fd(), 0x541B, &mut bytes) }, 0);
        bytes
    }
    let f = std::sync::Arc::new(Fixture::new(4, "independent/short-payload"));
    let payload = f.path("bin");
    std::fs::remove_file(&payload).unwrap();
    fifo(&payload);
    let inside = f.clone();
    let job =
        std::thread::spawn(move || measured("short-then-growing-payload", || inside.get(), 4097));
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&payload)
        .unwrap();
    writer.write_all(&[0; 4]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while available(&writer) != 0 {
        assert!(
            Instant::now() < deadline,
            "public get did not consume its first short read"
        );
        std::thread::yield_now();
    }
    // Get is now inside its public payload read loop. Append fourteen bytes
    // and keep the writer open; only thirteen can be consumed by its 16+1 cap.
    writer.write_all(&[0; 14]).unwrap();
    while !job.is_finished() {
        assert!(
            Instant::now() < deadline,
            "growing read waited for EOF instead of its probe"
        );
        std::thread::yield_now();
    }
    let CacheRead::Corrupt(report) = job.join().unwrap().unwrap() else {
        panic!("oversized stream was served")
    };
    assert!(report.reason.contains("payload byte length exceeds"));
    assert_eq!(
        available(&writer),
        1,
        "exactly 17 of 18 bytes must have been consumed"
    );
}

#[test]
fn maximum_in_budget_json_diagnostics_remain_proportional_to_format_cap() {
    let f = Fixture::new(2, "independent/json-diagnostics");
    // The read buffer cap is not a cap on parser scratch/error formatting.
    // Long unknown-field names and oversized escaped model strings exercise
    // those downstream allocations independently of the file-length defense.
    let unknown = format!("{{\"{}\":0}}", "k".repeat(4089));
    assert_eq!(unknown.len(), 4095);
    std::fs::write(f.path("meta.json"), unknown).unwrap();
    corrupt(measured(
        "in-budget-long-unknown-field",
        || f.get(),
        4 * 4096,
    ));
    f.put();
    let mut meta = f.meta();
    meta["model_id"] = "x".repeat(3500).into();
    let bytes = serde_json::to_vec(&meta).unwrap();
    assert!(bytes.len() <= 4096);
    std::fs::write(f.path("meta.json"), bytes).unwrap();
    corrupt(measured(
        "in-budget-invalid-long-model",
        || f.get(),
        4 * 4096,
    ));
}
