//! P7-009 integration tests: query encoding path + bounded query-vector
//! cache.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! and the P7-009 brief. The document artifact cache (P6-008,
//! `tests/artifact_cache.rs`) is NOT touched by this path — asserted
//! structurally by `query_path_never_touches_the_document_cache` (byte-level:
//! no file appears under the shared cache root while queries encode).
//!
//! Q5 ruling under test: the query cache key is
//! `(namespace, QuerySpecDigest, QueryDigest)` WITHOUT `semantic_epoch` —
//! spec changes (instruction/tokenizer/space) invalidate, epoch advances do
//! not, cross-space reuse is structurally unreachable.

use std::sync::{Arc, Mutex};

use cc_model::CcError;
use cc_semantic::admission::{InputBudget, OversizeReason};
use cc_semantic::cache::{
    encode_queries, namespace_key, ArtifactCache, QueryCacheKey, QueryEncodeOutcome, QueryVector,
    QueryVectorCache,
};
use cc_semantic::ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput};
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use cc_semantic::spec::{DocumentEncodingSpec, QueryEncodingSpec, VectorSpace};

fn space(model: &str, dim: u32) -> VectorSpace {
    VectorSpace::new(model, dim).expect("valid space")
}

/// Query spec with the DECLARED estimator as tokenizer — the only value
/// `admission::tokenizer_gate` admits, so `encode_queries` planning passes.
fn qspec(model: &str, instruction: Option<&str>) -> QueryEncodingSpec {
    QueryEncodingSpec::new(
        space(model, 8),
        instruction.map(str::to_string),
        8_192,
        "utf8-bytes-div-ceil-4-v1",
    )
    .expect("valid query spec")
}

fn ns() -> String {
    namespace_key("proj-a").expect("namespace key")
}

fn budget(max_items: usize, max_bytes: usize) -> InputBudget {
    InputBudget::validated(max_items, max_bytes, 4_096).expect("valid budget")
}

fn key_for(namespace: &str, spec: &QueryEncodingSpec, text: &str) -> QueryCacheKey {
    let input = QueryInput::from_bytes(text.as_bytes()).expect("valid query input");
    QueryCacheKey::new(namespace, spec, &input).expect("valid query cache key")
}

fn vector(dim: usize, seed: f32) -> QueryVector {
    QueryVector {
        dimension: dim as u32,
        data: (0..dim).map(|i| seed + i as f32 * 0.5).collect(),
    }
}

/// A provider that records its query batch sizes, returns deterministic
/// per-text vectors, and optionally fails or poisons (NaN) — the encoder's
/// own validation gate and error mapping are tested against it.
struct RecordingProvider {
    space: VectorSpace,
    query_batches: Mutex<Vec<usize>>,
    fail_with: Option<ProviderError>,
    poison: bool,
}

impl RecordingProvider {
    fn new(space: VectorSpace) -> Self {
        Self {
            space,
            query_batches: Mutex::new(Vec::new()),
            fail_with: None,
            poison: false,
        }
    }

    fn query_batch_sizes(&self) -> Vec<usize> {
        self.query_batches.lock().expect("batch log").clone()
    }
}

impl EmbeddingProvider for RecordingProvider {
    fn space(&self) -> &VectorSpace {
        &self.space
    }

    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(batch
            .iter()
            .map(|_| vec![1.0_f32; self.space.dimension() as usize])
            .collect())
    }

    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.query_batches
            .lock()
            .expect("batch log")
            .push(batch.len());
        if let Some(err) = self.fail_with.clone() {
            return Err(err);
        }
        let dim = self.space.dimension() as usize;
        Ok(batch
            .iter()
            .map(|input| {
                if self.poison {
                    vec![f32::NAN; dim]
                } else {
                    (0..dim)
                        .map(|i| ((input.bytes[0] as usize + i) % 7 + 1) as f32 * 0.5)
                        .collect()
                }
            })
            .collect())
    }
}

fn encoded<K: std::fmt::Debug>(outcome: QueryEncodeOutcome<K>) -> (K, QueryVector) {
    match outcome {
        QueryEncodeOutcome::Encoded { key, vector } => (key, vector),
        other => panic!("expected Encoded, got {other:?}"),
    }
}

// ── key identity (Q5) ────────────────────────────────────────────────────

#[test]
fn query_cache_key_distinguishes_namespace_spec_and_text() {
    let namespace = ns();
    let spec = qspec("fake/model-a", None);
    let q1 = QueryInput::from_bytes(b"query one").expect("query input");
    let q2 = QueryInput::from_bytes(b"query two").expect("query input");

    let key = QueryCacheKey::new(&namespace, &spec, &q1).expect("key");
    let again = QueryCacheKey::new(&namespace, &spec, &q1).expect("key");
    assert_eq!(key, again); // stable across reconstruction

    // Same spec, different text → different key.
    assert_ne!(
        key,
        QueryCacheKey::new(&namespace, &spec, &q2).expect("key")
    );
    // Instruction change → different key (spec digest covers instruction).
    let instructed = qspec("fake/model-a", Some("answer with code"));
    assert_ne!(
        key,
        QueryCacheKey::new(&namespace, &instructed, &q1).expect("key")
    );
    // Different space (same dimension, different model) → different key:
    // cross-space reuse is structurally unreachable.
    let other_model = qspec("fake/model-b", None);
    assert_ne!(
        key,
        QueryCacheKey::new(&namespace, &other_model, &q1).expect("key")
    );
    // Different project namespace → different key.
    let other_ns = namespace_key("proj-b").expect("namespace key b");
    assert_ne!(key, QueryCacheKey::new(&other_ns, &spec, &q1).expect("key"));
}

#[test]
fn query_cache_key_debug_carries_no_query_text() {
    const MARKER: &str = "SECRET-QUERY-TEXT-p7-009";
    let namespace = ns();
    // Even the instruction must not leak into the key's rendered form.
    let spec = qspec("fake/model-a", Some(MARKER));
    let q = QueryInput::from_bytes(MARKER.as_bytes()).expect("query input");
    let key = QueryCacheKey::new(&namespace, &spec, &q).expect("key");
    assert!(!format!("{key:?}").contains(MARKER));
}

// ── bounded cache ────────────────────────────────────────────────────────

#[test]
fn query_cache_round_trips_and_reports_bounds() {
    let cache = QueryVectorCache::new(8, 1_024);
    let key = key_for(&ns(), &qspec("fake/model-a", None), "q");
    assert!(cache.get(&key).is_none());
    assert!(cache.is_empty());

    let v = vector(8, 0.25);
    cache.put(key.clone(), v.clone()).expect("put");
    assert_eq!(cache.get(&key), Some(v));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.total_payload_bytes(), 32);
}

#[test]
fn query_cache_evicts_lru_within_both_bounds() {
    let namespace = ns();
    let spec = qspec("fake/model-a", None);
    let k1 = key_for(&namespace, &spec, "one");
    let k2 = key_for(&namespace, &spec, "two");
    let k3 = key_for(&namespace, &spec, "three");

    // Entry-count bound.
    let cache = QueryVectorCache::new(2, 4_096);
    cache.put(k1.clone(), vector(8, 1.0)).expect("put");
    cache.put(k2.clone(), vector(8, 2.0)).expect("put");
    cache.put(k3.clone(), vector(8, 3.0)).expect("put");
    assert_eq!(cache.len(), 2);
    assert!(cache.get(&k1).is_none()); // oldest evicted
    assert!(cache.get(&k2).is_some());
    assert!(cache.get(&k3).is_some());

    // Recency refresh: touching k1-style ordering via get protects it.
    let cache = QueryVectorCache::new(2, 4_096);
    cache.put(k1.clone(), vector(8, 1.0)).expect("put");
    cache.put(k2.clone(), vector(8, 2.0)).expect("put");
    assert!(cache.get(&k1).is_some()); // k1 is now most recent
    cache.put(k3.clone(), vector(8, 3.0)).expect("put");
    assert!(cache.get(&k1).is_some()); // survives
    assert!(cache.get(&k2).is_none()); // least recently used evicted

    // Byte bound: three 32-byte entries against a 64-byte budget.
    let cache = QueryVectorCache::new(64, 64);
    cache.put(k1.clone(), vector(8, 1.0)).expect("put");
    cache.put(k2.clone(), vector(8, 2.0)).expect("put");
    assert_eq!(cache.total_payload_bytes(), 64);
    cache.put(k3.clone(), vector(8, 3.0)).expect("put");
    assert!(cache.get(&k1).is_none());
    assert_eq!(cache.total_payload_bytes(), 64);
}

#[test]
fn query_cache_skips_entries_that_cannot_fit() {
    let namespace = ns();
    let spec = qspec("fake/model-a", None);
    let key = key_for(&namespace, &spec, "q");

    // A single 32-byte entry against a 16-byte budget: never stored, no error.
    let cache = QueryVectorCache::new(4, 16);
    cache
        .put(key.clone(), vector(8, 1.0))
        .expect("put is not an error");
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.total_payload_bytes(), 0);

    // max_entries == 0 disables storing entirely.
    let disabled = QueryVectorCache::new(0, 1_024);
    disabled
        .put(key, vector(8, 1.0))
        .expect("put is not an error");
    assert!(disabled.get(&key_for(&namespace, &spec, "q")).is_none());
}

#[test]
fn query_cache_rejects_invalid_vectors_on_put() {
    let cache = QueryVectorCache::new(8, 4_096);
    let key = key_for(&ns(), &qspec("fake/model-a", None), "q");

    let cases = [
        QueryVector {
            dimension: 8,
            data: vec![f32::NAN; 8],
        },
        QueryVector {
            dimension: 8,
            data: vec![f32::INFINITY, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        },
        QueryVector {
            dimension: 8,
            data: vec![0.0; 8],
        },
        QueryVector {
            dimension: 8,
            data: vec![1.0; 4],
        }, // length mismatch
        QueryVector {
            dimension: 0,
            data: Vec::new(),
        },
    ];
    for bad in cases {
        assert!(cache.put(key.clone(), bad).is_err(), "bad vector accepted");
        assert!(cache.is_empty(), "an invalid vector reached the cache");
    }
}

#[test]
fn query_cache_concurrent_access_stays_bounded() {
    let cache = Arc::new(QueryVectorCache::new(8, 512));
    let namespace = Arc::new(ns());
    let spec = Arc::new(qspec("fake/model-a", None));
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let cache = Arc::clone(&cache);
            let namespace = Arc::clone(&namespace);
            let spec = Arc::clone(&spec);
            std::thread::spawn(move || {
                for i in 0..50u32 {
                    let text = format!("thread-{t}-query-{i}");
                    let input = QueryInput::from_bytes(text.as_bytes()).expect("query input");
                    let key = QueryCacheKey::new(&namespace, &spec, &input).expect("key");
                    let _ = cache.put(key, vector(8, i as f32 + 1.0));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("writer thread");
    }
    assert!(cache.len() <= 8);
    assert!(cache.total_payload_bytes() <= 512);
}

// ── encoding path ────────────────────────────────────────────────────────

#[test]
fn encode_queries_returns_vectors_in_input_order_across_batches() {
    let provider = FakeProvider::new(FakeProviderConfig::new(space("fake/model-a", 8)));
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);
    let namespace = ns();
    // Three queries against a two-item batch bound → two planned batches.
    let queries = vec![
        ("q1".to_string(), b"alpha query".to_vec()),
        ("q2".to_string(), b"beta query".to_vec()),
        ("q3".to_string(), b"gamma query".to_vec()),
    ];

    let outcomes = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &budget(2, 4_096),
        &queries,
    )
    .expect("encode");
    assert_eq!(outcomes.len(), 3);
    for (index, outcome) in outcomes.iter().enumerate() {
        let (key, v) = encoded(outcome.clone());
        assert_eq!(key, queries[index].0);
        assert_eq!(v.dimension, 8);
        assert_eq!(v.data.len(), 8);
        assert!(v.data.iter().all(|c| c.is_finite() && *c != 0.0));
    }
    // One provider call per planned batch (2 batches), never more.
    assert_eq!(provider.call_count(), 2);
}

#[test]
fn encode_queries_second_pass_hits_cache_without_provider_contact() {
    let provider = FakeProvider::new(FakeProviderConfig::new(space("fake/model-a", 8)));
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);
    let namespace = ns();
    let queries = vec![
        ("q1".to_string(), b"same question".to_vec()),
        ("q2".to_string(), b"another question".to_vec()),
    ];

    let first = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &budget(8, 4_096),
        &queries,
    )
    .expect("first encode");
    let calls_after_first = provider.call_count();
    assert_eq!(calls_after_first, 1); // one planned batch

    let second = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &budget(8, 4_096),
        &queries,
    )
    .expect("second encode");
    assert_eq!(provider.call_count(), calls_after_first); // zero new calls
    for (first, second) in first.iter().zip(&second) {
        assert_eq!(encoded(first.clone()).1, encoded(second.clone()).1);
    }
}

#[test]
fn encode_queries_never_reuses_vectors_across_specs_or_spaces() {
    let provider_a = FakeProvider::new(FakeProviderConfig::new(space("fake/model-a", 8)));
    let provider_b = FakeProvider::new(FakeProviderConfig::new(space("fake/model-b", 8)));
    let cache = QueryVectorCache::new(64, 65_536);
    let namespace = ns();
    let spec_a = qspec("fake/model-a", None);
    let queries = vec![("q".to_string(), b"one stable question".to_vec())];
    let b = budget(8, 4_096);

    let base =
        encode_queries(&provider_a, &cache, &namespace, &spec_a, &b, &queries).expect("encode a");
    assert_eq!(provider_a.call_count(), 1);
    // Same spec, same text → cache hit, no provider contact.
    encode_queries(&provider_a, &cache, &namespace, &spec_a, &b, &queries).expect("encode a again");
    assert_eq!(provider_a.call_count(), 1);

    // Instruction change → new spec digest → re-encode.
    let instructed = qspec("fake/model-a", Some("instruction"));
    encode_queries(&provider_a, &cache, &namespace, &instructed, &b, &queries)
        .expect("encode a with instruction");
    assert_eq!(provider_a.call_count(), 2);

    // Different space (same dimension, different model) → re-encode on the
    // other provider: cross-space reuse is unreachable.
    let spec_b = qspec("fake/model-b", None);
    let across =
        encode_queries(&provider_b, &cache, &namespace, &spec_b, &b, &queries).expect("encode b");
    assert_eq!(provider_b.call_count(), 1);
    assert_ne!(
        encoded(base.into_iter().next().expect("outcome")).1.data,
        encoded(across.into_iter().next().expect("outcome")).1.data
    );
}

#[test]
fn encode_queries_rebatches_only_misses_inside_one_planned_batch() {
    let provider = RecordingProvider::new(space("fake/model-a", 8));
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);
    let namespace = ns();
    let b = budget(8, 4_096);
    let warm = ("warm".to_string(), b"warm question".to_vec());
    let cold = ("cold".to_string(), b"cold question".to_vec());
    let fresh = ("fresh".to_string(), b"fresh question".to_vec());

    // First pass: both miss → one planned batch of two provider inputs.
    encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &b,
        &[warm.clone(), cold.clone()],
    )
    .expect("first pass");
    // Second pass: `warm` and `cold` hit, only `fresh` (1 input) reaches the
    // provider — inside the same single planned batch of three.
    let outcomes = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &b,
        &[warm.clone(), cold.clone(), fresh],
    )
    .expect("second pass");
    assert_eq!(outcomes.len(), 3);
    assert_eq!(provider.query_batch_sizes(), vec![2, 1]);
}

#[test]
fn encode_queries_skips_oversized_queries_and_never_sends_them() {
    let provider = RecordingProvider::new(space("fake/model-a", 8));
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);
    let namespace = ns();
    // 16-byte batch budget: "tiny" fits, the 100-byte query cannot.
    let queries = vec![
        ("tiny".to_string(), b"tiny".to_vec()),
        ("big".to_string(), vec![b'x'; 100]),
    ];

    let outcomes = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &budget(4, 16),
        &queries,
    )
    .expect("encode");
    assert_eq!(outcomes.len(), 2);
    match &outcomes[1] {
        QueryEncodeOutcome::Skipped { key, reason } => {
            assert_eq!(key, "big");
            assert!(matches!(reason, OversizeReason::BytesTooLarge { .. }));
        }
        other => panic!("expected Skipped, got {other:?}"),
    }
    assert_eq!(encoded(outcomes[0].clone()).0, "tiny");
    // The oversized text never reached the provider.
    assert_eq!(provider.query_batch_sizes(), vec![1]);
}

#[test]
fn encode_queries_rejects_invalid_vectors_and_caches_nothing() {
    let mut provider = RecordingProvider::new(space("fake/model-a", 8));
    provider.poison = true; // NaN vectors through the fake's own gate
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);
    let namespace = ns();
    let queries = vec![("q".to_string(), b"poisoned question".to_vec())];

    let err = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &budget(8, 4_096),
        &queries,
    )
    .expect_err("poisoned batch must fail");
    assert!(cache.is_empty(), "an invalid vector reached the cache");
    assert_eq!(provider.query_batch_sizes(), vec![1]);
    assert!(
        !err.to_string().is_empty(),
        "error carries a diagnostic: {err}"
    );
}

#[test]
fn encode_queries_maps_provider_failures_and_caches_nothing() {
    let mut provider = RecordingProvider::new(space("fake/model-a", 8));
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);
    let namespace = ns();
    let queries = vec![("q".to_string(), b"failing question".to_vec())];
    let b = budget(8, 4_096);

    provider.fail_with = Some(ProviderError::ServerError);
    let err = encode_queries(&provider, &cache, &namespace, &spec, &b, &queries)
        .expect_err("server error must fail the call");
    match err {
        CcError::Other(message) => {
            assert!(message.contains("provider server error"), "got: {message}");
        }
        other => panic!("expected CcError::Other, got {other:?}"),
    }

    provider.fail_with = Some(ProviderError::Cancelled);
    let err = encode_queries(&provider, &cache, &namespace, &spec, &b, &queries)
        .expect_err("cancellation must fail the call");
    assert!(matches!(err, CcError::QueryCancelled), "got: {err}");

    provider.fail_with = Some(ProviderError::Timeout);
    let err = encode_queries(&provider, &cache, &namespace, &spec, &b, &queries)
        .expect_err("timeout must fail the call");
    assert!(matches!(err, CcError::QueryTimedOut), "got: {err}");

    assert!(
        cache.is_empty(),
        "a failed batch left something in the cache"
    );
}

#[test]
fn encode_queries_leaks_no_query_text_anywhere() {
    const MARKER: &str = "SECRET-QUERY-p7-009-credential-like";
    let namespace = ns();
    let spec = qspec("fake/model-a", None);
    let queries = vec![("q".to_string(), MARKER.as_bytes().to_vec())];

    // Failure leg: the provider error path carries no text.
    let mut failing = RecordingProvider::new(space("fake/model-a", 8));
    failing.fail_with = Some(ProviderError::ServerError);
    let error_cache = QueryVectorCache::new(64, 65_536);
    let err = encode_queries(
        &failing,
        &error_cache,
        &namespace,
        &spec,
        &budget(8, 4_096),
        &queries,
    )
    .expect_err("must fail");
    let rendered = format!("{err:?} {err}");
    assert!(
        !rendered.contains(MARKER),
        "query text leaked into the error"
    );

    // Success leg: keys, outcomes and cache state carry no text.
    let provider = FakeProvider::new(FakeProviderConfig::new(space("fake/model-a", 8)));
    let cache = QueryVectorCache::new(64, 65_536);
    let outcomes = encode_queries(
        &provider,
        &cache,
        &namespace,
        &spec,
        &budget(8, 4_096),
        &queries,
    )
    .expect("encode");
    let outcome = outcomes.first().expect("one outcome").clone();
    let (key, _) = encoded(outcome.clone());
    let rendered = format!("{outcome:?} {key:?} {cache:?}");
    assert!(!rendered.contains(MARKER), "query text leaked into state");
}

#[test]
fn encode_queries_empty_input_is_a_provider_free_no_op() {
    let provider = RecordingProvider::new(space("fake/model-a", 8));
    let cache = QueryVectorCache::new(64, 65_536);
    let spec = qspec("fake/model-a", None);

    let empty: Vec<(String, Vec<u8>)> = Vec::new();
    let outcomes =
        encode_queries(&provider, &cache, &ns(), &spec, &budget(8, 4_096), &empty).expect("encode");
    assert!(outcomes.is_empty());
    assert_eq!(provider.query_batch_sizes(), Vec::<usize>::new());
}

// ── document-path non-interference ───────────────────────────────────────

#[test]
fn query_path_never_touches_the_document_cache() {
    let root = std::env::temp_dir().join(format!(
        "cc-semantic-p7009-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let namespace = ns();
    let artifact = ArtifactCache::open(&root, namespace.clone()).expect("artifact cache");

    // One document object on the document path.
    let doc_space = space("fake/model-a", 8);
    let doc_spec =
        DocumentEncodingSpec::new(doc_space.clone(), None, 8_192, "fake-tokenizer").expect("spec");
    artifact
        .put(
            &doc_space,
            &cc_semantic::types::InputDigest::of_input(b"document text").expect("digest"),
            &doc_spec.digest().expect("digest"),
            &[1.0_f32; 8],
            1_000,
        )
        .expect("doc put");
    let files_before = count_files(&root);

    // Query encoding over the SAME namespace writes nothing to disk.
    let provider = FakeProvider::new(FakeProviderConfig::new(doc_space.clone()));
    let cache = QueryVectorCache::new(64, 65_536);
    let query_spec = qspec("fake/model-a", None);
    let queries = vec![("q".to_string(), b"a query".to_vec())];
    encode_queries(
        &provider,
        &cache,
        &namespace,
        &query_spec,
        &budget(8, 4_096),
        &queries,
    )
    .expect("encode");
    assert_eq!(
        count_files(&root),
        files_before,
        "the query path wrote to the artifact cache"
    );
    // The document object still reads Hit.
    let read = artifact
        .get(
            &doc_space,
            &cc_semantic::types::InputDigest::of_input(b"document text").expect("digest"),
            &doc_spec.digest().expect("digest"),
        )
        .expect("doc get");
    assert!(matches!(read, cc_semantic::cache::CacheRead::Hit(_)));
}

fn count_files(root: &std::path::Path) -> usize {
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| {
                    if entry.path().is_dir() {
                        count_files(&entry.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}
