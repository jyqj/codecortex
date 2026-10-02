//! P6-008 integration tests: content-addressed artifact cache.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (derivable / discardable / verifiable / cross-project isolation / no secrets)
//! and `artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
//! P6-008 (layout, checksum verified separately from the path, read-time
//! corruption detection, atomic tmp+rename writes, no empty dirs at open).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cc_semantic::cache::{
    resolve_cache_root, resolve_cache_root_with, ArtifactCache, CacheRead, CACHE_ROOT_ENV,
};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{ArtifactRef, InputDigest};

static UNIQUE: AtomicU64 = AtomicU64::new(0);

fn unique_tag(label: &str) -> String {
    let n = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{label}-{nanos}-{n}")
}

fn tmp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cc-semantic-p6008-{}", unique_tag(label)))
}

fn space(model: &str, dim: u32) -> VectorSpace {
    VectorSpace::new(model, dim).expect("valid space")
}

fn doc_spec(sp: &VectorSpace) -> DocumentEncodingSpec {
    DocumentEncodingSpec::new(sp.clone(), None, 8_192, "fake-tokenizer").expect("valid doc spec")
}

fn input(text: &str) -> InputDigest {
    InputDigest::of_input(text.as_bytes()).expect("valid input bytes")
}

fn vector(dim: u32, seed: f32) -> Vec<f32> {
    (0..dim).map(|i| seed + i as f32 * 0.25).collect()
}

fn put(
    cache: &ArtifactCache,
    sp: &VectorSpace,
    spec: &DocumentEncodingSpec,
    text: &str,
    data: &[f32],
) -> ArtifactRef {
    cache
        .put(
            sp,
            &input(text),
            &spec.digest().expect("spec digest"),
            data,
            1_000,
        )
        .expect("put")
}

fn get(
    cache: &ArtifactCache,
    sp: &VectorSpace,
    spec: &DocumentEncodingSpec,
    text: &str,
) -> CacheRead {
    cache
        .get(sp, &input(text), &spec.digest().expect("spec digest"))
        .expect("get")
}

fn assert_hit(read: CacheRead) -> cc_semantic::cache::ValidatedVector {
    match read {
        CacheRead::Hit(v) => v,
        other => panic!("expected Hit, got {other:?}"),
    }
}

// ── layout / laziness ────────────────────────────────────────────────────

#[test]
fn open_creates_nothing_and_put_lays_out_the_brief_layout() {
    let root = tmp_root("layout");
    let cache = ArtifactCache::open(&root, "nskey".to_string()).expect("open");
    // P6-002 acceptance carried forward: open() never touches the filesystem.
    assert!(!root.exists());

    let sp = space("fake/model-a", 8);
    let spec = doc_spec(&sp);
    put(&cache, &sp, &spec, "hello", &vector(8, 1.0));

    let space_id = sp.digest().expect("space digest");
    let input_id = input("hello");
    let spec_id = spec.digest().expect("spec digest");
    let obj = root
        .join("namespace-nskey")
        .join(space_id.as_str())
        .join(input_id.as_str())
        .join(spec_id.as_str());
    assert!(obj.join(format!("{}.bin", spec_id.as_str())).is_file());
    assert!(obj
        .join(format!("{}.meta.json", spec_id.as_str()))
        .is_file());
}

// ── read/write roundtrip + same-input reuse (acceptance 1, cross-clone) ──

#[test]
fn read_write_roundtrip_is_lossless() {
    let root = tmp_root("roundtrip");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    let data = vector(4, -2.5);
    let reference = put(&cache, &sp, &spec, "doc", &data);

    let hit = assert_hit(get(&cache, &sp, &spec, "doc"));
    assert_eq!(hit.dimension, 4);
    assert_eq!(hit.data, data);
    assert_eq!(hit.artifact_ref, reference);
}

#[test]
fn same_input_hits_across_independent_cache_instances() {
    // Cross-clone sharing semantics (Q5): two independent handles over the
    // same root + namespace behave as two clones of one project — the second
    // one pays nothing for identical (space, input, spec).
    let root = tmp_root("share");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    let first = ArtifactCache::open(&root, "proj".to_string()).expect("open a");
    let second = ArtifactCache::open(&root, "proj".to_string()).expect("open b");

    assert!(matches!(get(&second, &sp, &spec, "same"), CacheRead::Miss));
    let reference = put(&first, &sp, &spec, "same", &vector(4, 1.0));
    let hit = assert_hit(get(&second, &sp, &spec, "same"));
    assert_eq!(hit.artifact_ref, reference);

    // Different input bytes under the same tuple prefix: Miss, not a hit.
    assert!(matches!(get(&second, &sp, &spec, "other"), CacheRead::Miss));
}

#[test]
fn namespaces_are_isolated() {
    let root = tmp_root("isolated");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    let a = ArtifactCache::open(&root, "project-a".to_string()).expect("open a");
    let b = ArtifactCache::open(&root, "project-b".to_string()).expect("open b");

    put(&a, &sp, &spec, "doc", &vector(4, 1.0));
    assert!(matches!(get(&a, &sp, &spec, "doc"), CacheRead::Hit(_)));
    assert!(matches!(get(&b, &sp, &spec, "doc"), CacheRead::Miss));
}

#[test]
fn different_spaces_never_cross_hit() {
    // V16 first layer continued at the storage layer: same input bytes, same
    // spec shape, different model → different space → no hit.
    let root = tmp_root("spaces");
    let sp_a = space("fake/model-a", 4);
    let sp_b = space("fake/model-b", 4);
    let spec_a = doc_spec(&sp_a);
    let spec_b = doc_spec(&sp_b);
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");

    put(&cache, &sp_a, &spec_a, "doc", &vector(4, 1.0));
    assert!(matches!(
        get(&cache, &sp_b, &spec_b, "doc"),
        CacheRead::Miss
    ));
}

#[test]
fn spec_change_routes_to_a_new_object() {
    // DocSpecDigest is part of the addressing tuple: a document-encoding
    // change never silently serves old encodings.
    let root = tmp_root("specs");
    let sp = space("fake/model-a", 4);
    let spec_v1 = doc_spec(&sp);
    let spec_v2 = DocumentEncodingSpec::new(
        sp.clone(),
        Some("instruction".into()),
        8_192,
        "fake-tokenizer",
    )
    .expect("valid doc spec v2");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");

    let r1 = put(&cache, &sp, &spec_v1, "doc", &vector(4, 1.0));
    assert!(matches!(get(&cache, &sp, &spec_v2, "doc"), CacheRead::Miss));
    let r2 = put(&cache, &sp, &spec_v2, "doc", &vector(4, 2.0));
    assert_ne!(r1, r2);
    assert_eq!(
        assert_hit(get(&cache, &sp, &spec_v1, "doc")).artifact_ref,
        r1
    );
}

// ── corruption detection + discardable semantics ─────────────────────────

#[test]
fn corrupt_payload_is_detected_and_discard_degrades_to_miss() {
    let root = tmp_root("corrupt-bin");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    put(&cache, &sp, &spec, "doc", &vector(4, 1.0));

    let spec_id = spec.digest().expect("spec digest");
    let input_id = input("doc");
    let bin_path = root
        .join("namespace-ns")
        .join(sp.digest().expect("space digest").as_str())
        .join(input_id.as_str())
        .join(spec_id.as_str())
        .join(format!("{}.bin", spec_id.as_str()));

    // Flip one payload byte.
    let mut bytes = std::fs::read(&bin_path).expect("read bin");
    bytes[0] ^= 0xff;
    std::fs::write(&bin_path, &bytes).expect("write bin");

    match get(&cache, &sp, &spec, "doc") {
        CacheRead::Corrupt(report) => {
            assert_eq!(report.path, bin_path);
            assert!(report.reason.contains("checksum"), "{}", report.reason);
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
    // Detection is non-destructive at 008 (quarantine move is P6-018) …
    assert!(matches!(
        get(&cache, &sp, &spec, "doc"),
        CacheRead::Corrupt(_)
    ));
    // … but the object is discardable: explicit discard → subsequent reads Miss.
    assert!(cache.discard(&sp, &input_id, &spec_id).expect("discard"));
    assert!(matches!(get(&cache, &sp, &spec, "doc"), CacheRead::Miss));
    // Discarding an absent object is a no-op reporting false.
    assert!(!cache.discard(&sp, &input_id, &spec_id).expect("discard 2"));
}

#[test]
fn meta_tampering_is_detected_as_corrupt() {
    let root = tmp_root("corrupt-meta");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    put(&cache, &sp, &spec, "doc", &vector(4, 1.0));

    let spec_id = spec.digest().expect("spec digest");
    let input_id = input("doc");
    let obj_dir = root
        .join("namespace-ns")
        .join(sp.digest().expect("space digest").as_str())
        .join(input_id.as_str())
        .join(spec_id.as_str());
    let meta_path = obj_dir.join(format!("{}.meta.json", spec_id.as_str()));

    // (a) payload truncated to a whole number of floats but meta still claims
    // dimension 4: the checksum is recomputed over the truncated bytes so the
    // length-vs-dimension branch is what fires.
    let bin_path = obj_dir.join(format!("{}.bin", spec_id.as_str()));
    let bytes = std::fs::read(&bin_path).expect("read bin");
    let truncated = &bytes[..bytes.len() - 4];
    std::fs::write(&bin_path, truncated).expect("truncate bin");
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).expect("read meta"))
            .expect("parse meta");
    meta["checksum"] = serde_json::Value::String(cc_model::identity::bytes_hash(truncated));
    std::fs::write(&meta_path, serde_json::to_vec(&meta).expect("meta")).expect("write meta");
    match get(&cache, &sp, &spec, "doc") {
        CacheRead::Corrupt(report) => {
            assert!(report.reason.contains("dimension"), "{}", report.reason)
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
    put(&cache, &sp, &spec, "doc", &vector(4, 1.0));

    // (b) addressing field rewritten to another tuple
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).expect("read meta"))
            .expect("parse meta");
    meta["space_id"] = serde_json::Value::String("0".repeat(64));
    std::fs::write(&meta_path, serde_json::to_vec(&meta).expect("meta")).expect("write meta");
    match get(&cache, &sp, &spec, "doc") {
        CacheRead::Corrupt(report) => {
            assert!(report.reason.contains("addressing"), "{}", report.reason)
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }

    // (c) unknown format version
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).expect("read meta"))
            .expect("parse meta");
    meta["format_version"] = serde_json::Value::from(99);
    std::fs::write(&meta_path, serde_json::to_vec(&meta).expect("meta")).expect("write meta");
    match get(&cache, &sp, &spec, "doc") {
        CacheRead::Corrupt(report) => assert!(
            report.reason.contains("format_version"),
            "{}",
            report.reason
        ),
        other => panic!("expected Corrupt, got {other:?}"),
    }

    // (d) unreadable meta json
    std::fs::write(&meta_path, b"{not json").expect("write meta");
    match get(&cache, &sp, &spec, "doc") {
        CacheRead::Corrupt(report) => {
            assert!(report.reason.contains("meta json"), "{}", report.reason)
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }

    // (e) meta present but payload missing (or vice versa) is a Miss — an
    // incomplete write is indistinguishable from "never written".
    std::fs::remove_file(&bin_path).expect("remove bin");
    std::fs::write(&meta_path, b"{}").expect("write meta");
    assert!(matches!(get(&cache, &sp, &spec, "doc"), CacheRead::Miss));
}

// ── atomic writes under concurrency ─────────────────────────────────────

#[test]
fn concurrent_puts_of_the_same_key_race_safely() {
    let root = tmp_root("race");
    let root_for_threads = root.clone();
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    let reference = put(&cache, &sp, &spec, "doc", &vector(4, 1.0));

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let root = root_for_threads.clone();
            let sp = space("fake/model-a", 4);
            let spec = doc_spec(&sp);
            std::thread::spawn(move || {
                let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
                cache
                    .put(
                        &sp,
                        &InputDigest::of_input(b"doc").expect("digest"),
                        &spec.digest().expect("spec digest"),
                        &vector(4, 1.0),
                        1_000 + i,
                    )
                    .expect("put")
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().expect("thread"), reference);
    }
    let hit = assert_hit(get(&cache, &sp, &spec, "doc"));
    assert_eq!(hit.data, vector(4, 1.0));
}

#[test]
fn rewrite_of_existing_object_is_idempotent_and_stable() {
    let root = tmp_root("idempotent");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    let r1 = put(&cache, &sp, &spec, "doc", &vector(4, 1.0));
    let r2 = put(&cache, &sp, &spec, "doc", &vector(4, 1.0));
    assert_eq!(r1, r2);
    let hit = assert_hit(get(&cache, &sp, &spec, "doc"));
    assert_eq!(hit.data, vector(4, 1.0));
}

// ── namespace keys ───────────────────────────────────────────────────────

#[test]
fn namespace_validation_rejects_unsafe_names() {
    let root = tmp_root("ns-validation");
    for bad in [
        "",
        " ",
        "a/b",
        "..",
        "a..b",
        ".hidden",
        "ns x",
        "ns\x00",
        &"x".repeat(129),
    ] {
        assert!(
            ArtifactCache::open(&root, bad.to_string()).is_err(),
            "namespace {bad:?} must be rejected"
        );
    }
    // Nothing was created by any rejected open.
    assert!(!root.exists());
}

#[test]
fn namespace_key_is_stable_distinct_and_path_safe() {
    let a1 = cc_semantic::cache::namespace_key("/repos/codecortex").expect("key");
    let a2 = cc_semantic::cache::namespace_key("/repos/codecortex").expect("key");
    let b = cc_semantic::cache::namespace_key("/repos/other").expect("key");
    assert_eq!(a1, a2);
    assert_ne!(a1, b);
    assert!(a1.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(a1.len(), 64);
    assert!(cc_semantic::cache::namespace_key("  ").is_err());
    assert!(ArtifactCache::open(&root(), a1).is_ok());
}

fn root() -> PathBuf {
    tmp_root("ns-key")
}

// ── artifact_ref (P6-003 留白兑现：cache 是唯一公开构造路径) ─────────────

#[test]
fn artifact_ref_is_stable_and_content_scoped() {
    let root = tmp_root("ref");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);

    let r1 = put(&cache, &sp, &spec, "doc", &vector(4, 1.0));
    let r2 = put(&cache, &sp, &spec, "doc", &vector(4, 1.0));
    assert_eq!(r1, r2);
    assert!(r1.as_str().starts_with("cas.v1:"));
    // Ref is scoped by every addressing component.
    let other_input = put(&cache, &sp, &spec, "other", &vector(4, 1.0));
    let other_space = put(
        &cache,
        &space("fake/model-b", 4),
        &doc_spec(&space("fake/model-b", 4)),
        "doc",
        &vector(4, 1.0),
    );
    let other_spec = put(
        &cache,
        &sp,
        &DocumentEncodingSpec::new(sp.clone(), Some("i".into()), 8_192, "t").expect("spec"),
        "doc",
        &vector(4, 1.0),
    );
    for other in [other_input, other_space, other_spec] {
        assert_ne!(r1, other);
    }
    // The only sanctioned public constructor is the cache itself.
    assert_eq!(assert_hit(get(&cache, &sp, &spec, "doc")).artifact_ref, r1);
}

// ── vector validation gate ───────────────────────────────────────────────

#[test]
fn invalid_vectors_are_rejected_before_touching_the_cache() {
    let root = tmp_root("vec-validation");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    let digest = spec.digest().expect("spec digest");
    let doc = input("doc");

    assert!(cache.put(&sp, &doc, &digest, &[1.0, 2.0], 1).is_err()); // wrong dim
    assert!(cache
        .put(&sp, &doc, &digest, &[1.0, f32::NAN, 3.0, 4.0], 1)
        .is_err());
    assert!(cache
        .put(&sp, &doc, &digest, &[1.0, f32::INFINITY, 3.0, 4.0], 1)
        .is_err());
    assert!(
        !root.exists(),
        "rejected vectors must not create the layout"
    );
}

// ── no-secret red line ───────────────────────────────────────────────────

#[test]
fn meta_contains_only_the_documented_fields() {
    let root = tmp_root("no-secrets");
    let cache = ArtifactCache::open(&root, "ns".to_string()).expect("open");
    let sp = space("fake/model-a", 4);
    let spec = doc_spec(&sp);
    put(&cache, &sp, &spec, "doc", &vector(4, 1.0));

    let spec_id = spec.digest().expect("spec digest");
    let meta_path = root
        .join("namespace-ns")
        .join(sp.digest().expect("space digest").as_str())
        .join(input("doc").as_str())
        .join(spec_id.as_str())
        .join(format!("{}.meta.json", spec_id.as_str()));
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).expect("read meta"))
            .expect("parse meta");
    let mut keys: Vec<String> = meta.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "checksum",
            "created_at",
            "dimension",
            "format_version",
            "input_digest",
            "model_id",
            "space_id",
            "spec_digest"
        ]
    );
    // And the payload is exactly the derived vector bytes — nothing else.
    let bin = std::fs::read(
        meta_path
            .parent()
            .expect("parent")
            .join(format!("{}.bin", spec_id.as_str())),
    )
    .expect("read bin");
    let mut expected = Vec::new();
    for v in vector(4, 1.0) {
        expected.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(bin, expected);
}

// ── cache root resolution (env-injected) ─────────────────────────────────

#[test]
fn cache_root_resolution_branches() {
    let home = Path::new("/home/u").to_path_buf();
    let lookup = |vars: Vec<(&'static str, String)>| {
        move |name: &str| -> Option<String> {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.clone())
        }
    };

    // Explicit override wins on every platform layout.
    for macos in [true, false] {
        assert_eq!(
            resolve_cache_root_with(
                lookup(vec![
                    (CACHE_ROOT_ENV, "/custom/root".to_string()),
                    ("HOME", "/home/u".to_string())
                ]),
                macos
            ),
            Some(PathBuf::from("/custom/root"))
        );
        // Empty override is treated as unset.
        assert_eq!(
            resolve_cache_root_with(lookup(vec![(CACHE_ROOT_ENV, "  ".to_string())]), macos),
            None
        );
    }

    // macOS convention: ~/Library/Caches.
    assert_eq!(
        resolve_cache_root_with(
            lookup(vec![
                ("HOME", "/home/u".to_string()),
                ("XDG_CACHE_HOME", "/xdg".to_string())
            ]),
            true
        ),
        Some(home.join("Library/Caches/codecortex/semantic"))
    );

    // Linux convention: $XDG_CACHE_HOME, falling back to ~/.cache.
    assert_eq!(
        resolve_cache_root_with(
            lookup(vec![
                ("HOME", "/home/u".to_string()),
                ("XDG_CACHE_HOME", "/xdg".to_string())
            ]),
            false
        ),
        Some(PathBuf::from("/xdg/codecortex/semantic"))
    );
    assert_eq!(
        resolve_cache_root_with(lookup(vec![("HOME", "/home/u".to_string())]), false),
        Some(home.join(".cache/codecortex/semantic"))
    );

    // No HOME at all: no default root (caller must provide an explicit one).
    assert_eq!(resolve_cache_root_with(lookup(vec![]), true), None);
    assert_eq!(resolve_cache_root_with(lookup(vec![]), false), None);
}

#[test]
fn process_env_override_reaches_resolve_cache_root() {
    // Single test mutating the real process env; no other test in this file
    // reads CODECORTEX_SEMANTIC_CACHE_ROOT via std::env.
    let guard = ENV_MUTEX.lock().expect("env mutex");
    let saved = std::env::var(CACHE_ROOT_ENV).ok();
    std::env::set_var(CACHE_ROOT_ENV, "/env/injected/root");
    assert_eq!(
        resolve_cache_root(),
        Some(PathBuf::from("/env/injected/root"))
    );
    match saved {
        Some(v) => std::env::set_var(CACHE_ROOT_ENV, v),
        None => std::env::remove_var(CACHE_ROOT_ENV),
    }
    drop(guard);
}

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
