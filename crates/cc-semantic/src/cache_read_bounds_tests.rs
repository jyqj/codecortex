//! Dedicated cache-read-budget regressions. No provider or private files.
use super::*;
use crate::spec::{DocumentEncodingSpec, MAX_DIMENSION, MAX_MODEL_ID_BYTES};
use std::io::{Cursor, ErrorKind};

struct Fixture {
    root: PathBuf,
    cache: ArtifactCache,
    space: VectorSpace,
    input: InputDigest,
    spec: DocSpecDigest,
}

impl Fixture {
    fn new(dimension: u32, model: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cc-cache-read-bounds-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = ArtifactCache::open(&root, "read-bounds".into()).unwrap();
        let space = VectorSpace::new(model, dimension).unwrap();
        let input = InputDigest::of_input(b"synthetic fixture").unwrap();
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "synthetic")
            .unwrap()
            .digest()
            .unwrap();
        let fixture = Self {
            root,
            cache,
            space,
            input,
            spec,
        };
        fixture.put();
        fixture
    }
    fn put(&self) {
        self.cache
            .put(
                &self.space,
                &self.input,
                &self.spec,
                &vec![1.0; self.space.dimension() as usize],
                i64::MIN,
            )
            .unwrap();
    }
    fn path(&self, suffix: &str) -> PathBuf {
        self.cache
            .object_dir(&self.space.digest().unwrap(), &self.input, &self.spec)
            .join(format!("{}.{suffix}", self.spec.as_str()))
    }
    fn get(&self) -> CacheRead {
        self.cache
            .get(&self.space, &self.input, &self.spec)
            .unwrap()
    }
    fn assert_corrupt(&self) {
        assert!(matches!(self.get(), CacheRead::Corrupt(_)));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn valid_maximum_dimension_and_worst_json_escaped_model_round_trip() {
    let model = "\u{1}".repeat(MAX_MODEL_ID_BYTES);
    let f = Fixture::new(MAX_DIMENSION, &model);
    let metadata = std::fs::read(f.path("meta.json")).unwrap();
    assert!(metadata.len() > 3000);
    assert!(metadata.len() <= MAX_ARTIFACT_METADATA_BYTES);
    let CacheRead::Hit(hit) = f.get() else {
        panic!("maximum legal object rejected")
    };
    assert_eq!(hit.dimension, MAX_DIMENSION);
    assert_eq!(hit.data, vec![1.0; MAX_DIMENSION as usize]);
}

#[test]
fn metadata_budget_boundary_and_one_extra_byte() {
    let f = Fixture::new(128, "synthetic/boundary");
    let mut metadata = std::fs::read(f.path("meta.json")).unwrap();
    metadata.resize(MAX_ARTIFACT_METADATA_BYTES, b' ');
    std::fs::write(f.path("meta.json"), &metadata).unwrap();
    assert!(matches!(f.get(), CacheRead::Hit(_)));
    metadata.push(b' ');
    std::fs::write(f.path("meta.json"), &metadata).unwrap();
    f.assert_corrupt();
}

#[test]
fn payload_extra_byte_and_every_truncation_are_corrupt() {
    let f = Fixture::new(4, "synthetic/lengths");
    let original = std::fs::read(f.path("bin")).unwrap();
    for len in 0..original.len() {
        std::fs::write(f.path("bin"), &original[..len]).unwrap();
        f.assert_corrupt();
    }
    let mut extended = original.clone();
    extended.push(0);
    std::fs::write(f.path("bin"), &extended).unwrap();
    f.assert_corrupt();
    std::fs::write(f.path("bin"), original).unwrap();
    assert!(matches!(f.get(), CacheRead::Hit(_)));
}

#[test]
fn enormous_sparse_files_and_untrusted_dimension_are_corrupt() {
    let f = Fixture::new(128, "synthetic/sparse");
    for suffix in ["bin", "meta.json"] {
        f.put();
        std::fs::OpenOptions::new()
            .write(true)
            .open(f.path(suffix))
            .unwrap()
            .set_len(8 * 1024 * 1024 * 1024)
            .unwrap();
        f.assert_corrupt();
    }
    f.put();
    let mut meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.path("meta.json")).unwrap()).unwrap();
    meta["dimension"] = u32::MAX.into();
    std::fs::write(f.path("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
    f.assert_corrupt();
}

#[test]
fn incomplete_objects_remain_misses_even_if_the_other_file_is_oversized() {
    let f = Fixture::new(128, "synthetic/missing");
    for (missing, oversized) in [("bin", "meta.json"), ("meta.json", "bin")] {
        f.put();
        std::fs::remove_file(f.path(missing)).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(f.path(oversized))
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
        assert!(matches!(f.get(), CacheRead::Miss));
    }
}

#[test]
fn bounded_reader_never_waits_for_unbounded_stream_eof() {
    struct Endless {
        read_bytes: usize,
    }
    impl Read for Endless {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = out.len().min(7);
            out[..n].fill(1);
            self.read_bytes += n;
            Ok(n)
        }
    }
    for limit in [
        0,
        512,
        MAX_ARTIFACT_METADATA_BYTES,
        MAX_DIMENSION as usize * 4,
    ] {
        let mut reader = Endless { read_bytes: 0 };
        assert!(read_artifact_bounded(&mut reader, limit).unwrap().is_none());
        assert_eq!(reader.read_bytes, limit + 1);
    }
}

#[test]
fn short_reads_interruption_and_io_errors_keep_their_meaning() {
    struct Short {
        input: Cursor<Vec<u8>>,
        interrupt: bool,
    }
    impl Read for Short {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.interrupt {
                self.interrupt = false;
                return Err(ErrorKind::Interrupted.into());
            }
            let n = out.len().min(3);
            self.input.read(&mut out[..n])
        }
    }
    let mut input = Short {
        input: Cursor::new(vec![7; 32]),
        interrupt: true,
    };
    let got = read_artifact_bounded(&mut input, 32).unwrap().unwrap();
    assert_eq!(got, vec![7; 32]);
    assert_eq!(got.capacity(), 33);
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(ErrorKind::PermissionDenied.into())
        }
    }
    assert_eq!(
        read_artifact_bounded(&mut Failed, 32).unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
}

#[test]
fn file_growth_after_first_read_is_bounded_without_a_stat_assumption() {
    struct Growing {
        read: std::fs::File,
        write: std::fs::File,
        grow: bool,
        consumed: usize,
    }
    impl Read for Growing {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = out.len().min(4);
            let got = self.read.read(&mut out[..n])?;
            if self.grow {
                self.grow = false;
                self.write.set_len(64 * 1024 * 1024)?;
            }
            self.consumed += got;
            Ok(got)
        }
    }
    let f = Fixture::new(4, "synthetic/growth");
    let path = f.path("bin");
    assert_eq!(path.metadata().unwrap().len(), 16);
    let mut reader = Growing {
        read: std::fs::File::open(&path).unwrap(),
        write: std::fs::OpenOptions::new().write(true).open(&path).unwrap(),
        grow: true,
        consumed: 0,
    };
    assert!(read_artifact_bounded(&mut reader, 16).unwrap().is_none());
    assert_eq!(reader.consumed, 17);
    assert_eq!(path.metadata().unwrap().len(), 64 * 1024 * 1024);
}
