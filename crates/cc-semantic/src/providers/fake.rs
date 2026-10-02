//! Deterministic in-process fake embedding provider (P6-009).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (constraint row P6-009: "worker-side implementation, never touches the
//! network; the port must be frozen before this starts") and
//! `artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
//! P6-009. This provider is a *test instrument*, not a semantic quality
//! claim: its vectors carry no linguistic meaning (V15 acceptance: fake
//! results never count as real model success — `06-VALIDATION.md:15`).
//!
//! ## Deterministic vector algorithm (`FAKE_PROVIDER_ALGORITHM_VERSION = 1`)
//!
//! Same input digest under the same frozen space always yields the bitwise
//! identical vector, on every machine, for the lifetime of algorithm version
//! 1:
//!
//! 1. **Key material** (UTF-8, ASCII only, every field delimited):
//!    `cc-semantic.fake-provider.v1/alg=1/kind={document|query}/model={model_id}/dim={dimension}/digest={digest}`.
//!    The `kind` tag domain-separates the document and query paths (real
//!    models embed asymmetric pairs differently too); `model_id` is part of
//!    the space identity, so a different model never reproduces another
//!    model's vectors.
//! 2. **Keystream**: `blake3::Hasher::update(key_material)` then
//!    `finalize_xof()`, `fill()`ing exactly `dimension * 4` bytes — the
//!    official extendable-output function, not a homemade PRNG.
//! 3. **Component mapping**: each little-endian `u32` is mapped to
//!    `((u as f64) * (2 / 2^32) - 1)` — an *exact* f64 intermediate, then a
//!    single IEEE-754 round-to-nearest cast to `f32`.
//! 4. **Normalization**: the L2 norm is accumulated in `f64` by sequential
//!    fold (`sqrt(sum(v*v))`) and each component divided by it (f64 divide,
//!    one cast back to `f32`), placing the vector on the unit sphere as the
//!    frozen `DistanceMetric::Cosine` requires. A degenerate all-zero
//!    pre-normalization vector (probability ~0) is left as zero and caught by
//!    the output gate below.
//!
//! Every arithmetic step is basic IEEE-754 (`*`, `-`, `/`, `sqrt`, defined
//! integer→float casts) with fixed operand order — Rust performs no
//! reassociation — so the result is bitwise reproducible cross-machine. Any
//! change to this pipeline bumps [`FAKE_PROVIDER_ALGORITHM_VERSION`] and is a
//! breaking change for recorded fixtures.
//!
//! ## Scripted faults (V15 matrix, P6-016/P6-015 reuse)
//!
//! [`FakeProviderConfig`] scripts failures without any I/O: a call-ordinal
//! fault (`fail_after_n_calls` + `fail_with`), per-call latency
//! (`delay_per_call`), and per-digest zero-vector injection
//! (`zero_vector_digests`). The zero-vector injection feeds the *output
//! gate*: every produced vector is checked for dimension match, finiteness
//! and non-zero before returning (NaN/Inf/zero/dimension mismatch →
//! `ProviderError::InvalidInput`), so a bad vector can never reach the
//! artifact cache (V15 validity gate; pairs with the cache-side
//! `ArtifactCache::put` validation from P6-008).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput};
use crate::types::VectorSpace;

/// Version of the deterministic vector pipeline documented in the module
/// header. Part of the key material; bumping it is a breaking change for
/// every recorded fixture and reproduces nothing from the previous version.
pub const FAKE_PROVIDER_ALGORITHM_VERSION: u32 = 1;

/// Domain tag of the key material (digest-domain separation per
/// `crates/cc-semantic/docs/ENCODING-SPACE.md` §3).
const FAKE_DOMAIN: &str = "cc-semantic.fake-provider.v1";

/// `kind` tag of the document path in the key material.
const DOC_KIND: &str = "document";
/// `kind` tag of the query path in the key material.
const QUERY_KIND: &str = "query";

/// Script for the fake provider (TASK-BRIEFS P6-009 draft fields; the
/// zero-vector switch is keyed by digest *strings* so it covers both the
/// document (`InputDigest`) and query (`QueryDigest`) paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeProviderConfig {
    /// The frozen space the provider embeds into; vectors are exactly
    /// `space.dimension()` long and keyed by `space.model_id()`.
    pub space: VectorSpace,
    /// Fault arming: once the 1-based call ordinal (counted across *both*
    /// embed methods, one batch = one call, failing calls included) exceeds
    /// this number, every further call fails with `fail_with`. `None` = the
    /// fault never fires.
    pub fail_after_n_calls: Option<usize>,
    /// The error the armed fault injects. `None` while the fault is armed
    /// defaults to [`ProviderError::ServerError`].
    pub fail_with: Option<ProviderError>,
    /// Artificial per-call latency, applied on success and failure paths
    /// alike (simulates provider round-trip time; `Duration::ZERO` default).
    pub delay_per_call: Duration,
    /// Digests (hex) whose vectors are deliberately produced as the all-zero
    /// vector — the injected invalid-vector fault that the output gate must
    /// reject with [`ProviderError::InvalidInput`].
    pub zero_vector_digests: Vec<String>,
}

impl FakeProviderConfig {
    /// Defaults for `space`: no fault armed, no delay, no zero-vector
    /// injection — the provider is naturally successful.
    pub fn new(space: VectorSpace) -> Self {
        Self {
            space,
            fail_after_n_calls: None,
            fail_with: None,
            delay_per_call: Duration::ZERO,
            zero_vector_digests: Vec::new(),
        }
    }
}

/// The deterministic fake provider. Pure local computation: blake3 keystream
/// expansion over caller-supplied digests — no network, no filesystem, no
/// environment, no wall clock on the vector path. Thread-safe ([`Send`] +
/// [`Sync`]; the call counter is atomic).
#[derive(Debug)]
pub struct FakeProvider {
    config: FakeProviderConfig,
    calls: AtomicUsize,
}

impl FakeProvider {
    /// Construct from a script; the space is validated up front so a bad
    /// space can never surface lazily inside an embed call.
    pub fn new(config: FakeProviderConfig) -> Self {
        // `space()` validates again on every port read; this is the eager
        // sanity check at the sanctioned constructor.
        config
            .space
            .validate()
            .expect("FakeProviderConfig space must be a valid frozen VectorSpace");
        Self {
            config,
            calls: AtomicUsize::new(0),
        }
    }

    /// Total embed calls so far, across both methods, failing calls included
    /// (the counter P6-014 uses to assert "provider called 0 times" after a
    /// rebuild-with-cache).
    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Shared embed pipeline: latency, scripted fault, deterministic
    /// generation, output gate. `kind` and the per-input digests are
    /// supplied by the two port methods.
    fn embed(
        &self,
        kind: &str,
        digests: &[&str],
    ) -> Result<Vec<Vec<f32>>, ProviderError> {
        let ordinal = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.config.delay_per_call > Duration::ZERO {
            std::thread::sleep(self.config.delay_per_call);
        }
        if let Some(after) = self.config.fail_after_n_calls {
            if ordinal > after {
                return Err(self.config.fail_with.clone().unwrap_or(ProviderError::ServerError));
            }
        }
        let dim = self.config.space.dimension() as usize;
        let vectors: Vec<Vec<f32>> = digests
            .iter()
            .map(|digest| {
                if self.config.zero_vector_digests.iter().any(|d| d == digest) {
                    vec![0.0_f32; dim]
                } else {
                    deterministic_vector(kind, &self.config.space, digest, dim)
                }
            })
            .collect();
        for (digest, vector) in digests.iter().zip(&vectors) {
            validate_output(&self.config.space, digest, vector)?;
        }
        Ok(vectors)
    }
}

impl EmbeddingProvider for FakeProvider {
    fn space(&self) -> &VectorSpace {
        &self.config.space
    }

    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let digests: Vec<&str> = batch.iter().map(|d| d.input_digest.as_str()).collect();
        self.embed(DOC_KIND, &digests)
    }

    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let digests: Vec<&str> = batch.iter().map(|q| q.digest.as_str()).collect();
        self.embed(QUERY_KIND, &digests)
    }
}

/// The documented v1 pipeline (module header §"Deterministic vector
/// algorithm"): blake3 XOF keystream → exact f64 mapping → unit normalization.
/// Private by design; the only observable surface is `embed_*`.
fn deterministic_vector(kind: &str, space: &VectorSpace, digest: &str, dim: usize) -> Vec<f32> {
    let key_material = format!(
        "{FAKE_DOMAIN}/alg={FAKE_PROVIDER_ALGORITHM_VERSION}/kind={kind}/model={}/dim={}/digest={digest}",
        space.model_id(),
        space.dimension(),
    );
    let mut hasher = blake3::Hasher::new();
    hasher.update(key_material.as_bytes());
    let mut stream = hasher.finalize_xof();
    let mut raw = vec![0_u8; dim * 4];
    stream.fill(&mut raw);

    let mut components = Vec::with_capacity(dim);
    let mut sum_sq = 0.0_f64;
    for chunk in raw.chunks_exact(4) {
        let u = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let v = ((u as f64) * (2.0 / 4_294_967_296.0) - 1.0) as f32;
        sum_sq += (v as f64) * (v as f64);
        components.push(v);
    }
    let norm = sum_sq.sqrt();
    if norm > 0.0 {
        for v in &mut components {
            *v = ((*v as f64) / norm) as f32;
        }
    }
    components
}

/// Output validity gate (V15: count/dimension/NaN/Inf/zero are rejected at
/// the provider boundary so bad vectors never reach the artifact cache).
fn validate_output(space: &VectorSpace, digest: &str, vector: &[f32]) -> Result<(), ProviderError> {
    if vector.len() != space.dimension() as usize {
        return Err(ProviderError::InvalidInput(format!(
            "fake provider produced a vector of length {} for digest {digest}, expected dimension {}",
            vector.len(),
            space.dimension()
        )));
    }
    if let Some(index) = vector.iter().position(|v| !v.is_finite()) {
        return Err(ProviderError::InvalidInput(format!(
            "fake provider produced a non-finite component at index {index} for digest {digest}"
        )));
    }
    if vector.iter().all(|&v| v == 0.0) {
        return Err(ProviderError::InvalidInput(format!(
            "fake provider produced a zero vector for digest {digest}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InputDigest;
    use crate::ports::DocumentInput;

    fn provider(model: &str, dim: u32) -> FakeProvider {
        let space = VectorSpace::new(model, dim).expect("valid space");
        FakeProvider::new(FakeProviderConfig::new(space))
    }

    fn doc(text: &str) -> DocumentInput {
        DocumentInput::from_bytes(text.as_bytes()).expect("valid document input")
    }

    fn query(text: &str) -> QueryInput {
        QueryInput::from_bytes(text.as_bytes()).expect("valid query input")
    }

    /// Bitwise equality of two vectors (determinism means `f32::to_bits`
    /// equality, not approximate float equality).
    fn assert_bits_eq(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "component {i} differs bitwise: {} vs {}",
                x.to_bits(),
                y.to_bits()
            );
        }
    }

    // ── Determinism ──────────────────────────────────────────────────────

    #[test]
    fn same_digest_yields_bit_identical_vectors_across_calls_and_instances() {
        let a = provider("fake/model-a", 8);
        let b = provider("fake/model-a", 8);
        let batch = vec![doc("identical payload")];
        let v1 = a.embed_documents(&batch).unwrap().remove(0);
        let v2 = a.embed_documents(&batch).unwrap().remove(0);
        let v3 = b.embed_documents(&batch).unwrap().remove(0);
        assert_bits_eq(&v1, &v2);
        assert_bits_eq(&v1, &v3);
    }

    #[test]
    fn different_digests_yield_different_vectors() {
        let p = provider("fake/model-a", 8);
        let out = p.embed_documents(&[doc("alpha"), doc("beta")]).unwrap();
        assert_ne!(out[0], out[1]);
    }

    #[test]
    fn vectors_are_unit_length_for_the_frozen_cosine_metric() {
        let p = provider("fake/model-a", 64);
        let v = p.embed_documents(&[doc("norm check")]).unwrap().remove(0);
        let norm: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9, "norm was {norm}");
    }

    #[test]
    fn v1_pipeline_is_bitwise_reproducible_golden() {
        // Golden fixture for algorithm version 1: the exact `f32::to_bits`
        // of the first four components of a fixed input under a fixed space.
        // Recorded on 2026-10-02; if this ever differs, the pipeline changed
        // and `FAKE_PROVIDER_ALGORITHM_VERSION` must bump with new fixtures —
        // bitwise cross-machine reproducibility is the contract.
        let p = provider("fake/golden-model", 8);
        let v = p.embed_documents(&[doc("golden fixture input")]).unwrap().remove(0);
        let expected: [u32; 4] = [3169967527, 1032920341, 3201138986, 1039597886];
        for (i, bits) in expected.iter().enumerate() {
            assert_eq!(v[i].to_bits(), *bits, "golden component {i} drifted");
        }
    }

    // ── Space / dimension contract ───────────────────────────────────────

    #[test]
    fn space_contract_dimension_and_identity_hold() {
        let p = provider("fake/model-a", 16);
        assert_eq!(p.space().model_id(), "fake/model-a");
        assert_eq!(p.space().dimension(), 16);
        let v = p.embed_documents(&[doc("dim check")]).unwrap().remove(0);
        assert_eq!(v.len(), 16);
    }

    #[test]
    fn model_id_and_dimension_are_part_of_the_derivation() {
        let a = provider("fake/model-a", 8).embed_documents(&[doc("same")]).unwrap().remove(0);
        let b = provider("fake/model-b", 8).embed_documents(&[doc("same")]).unwrap().remove(0);
        let c = provider("fake/model-a", 16).embed_documents(&[doc("same")]).unwrap().remove(0);
        assert_ne!(a, b, "same dimension, different model must not reproduce");
        assert_eq!(a.len(), 8);
        assert_eq!(c.len(), 16);
    }

    // ── Batch contract ───────────────────────────────────────────────────

    #[test]
    fn batch_order_is_preserved_on_both_paths() {
        let p = provider("fake/model-a", 8);
        let docs = vec![doc("d-one"), doc("d-two"), doc("d-three")];
        let out = p.embed_documents(&docs).unwrap();
        assert_eq!(out.len(), docs.len(), "one output per input, batch not split");
        for (input, vector) in docs.iter().zip(&out) {
            let single = p.embed_documents(std::slice::from_ref(input)).unwrap().remove(0);
            assert_bits_eq(vector, &single);
        }
        let queries = vec![query("q-one"), query("q-two")];
        let out = p.embed_queries(&queries).unwrap();
        assert_eq!(out.len(), queries.len());
        for (input, vector) in queries.iter().zip(&out) {
            let single = p.embed_queries(std::slice::from_ref(input)).unwrap().remove(0);
            assert_bits_eq(vector, &single);
        }
    }

    #[test]
    fn document_and_query_paths_are_domain_separated() {
        let p = provider("fake/model-a", 8);
        let d = p.embed_documents(&[doc("shared bytes")]).unwrap().remove(0);
        let q = p.embed_queries(&[query("shared bytes")]).unwrap().remove(0);
        assert_ne!(d, q, "kind tag must domain-separate the two paths");
    }

    #[test]
    fn empty_batch_returns_empty_success() {
        let p = provider("fake/model-a", 8);
        assert!(p.embed_documents(&[]).unwrap().is_empty());
        assert!(p.embed_queries(&[]).unwrap().is_empty());
    }

    // ── Scripted faults ──────────────────────────────────────────────────

    #[test]
    fn fault_fires_after_n_calls_with_the_configured_error() {
        let space = VectorSpace::new("fake/model-a", 8).expect("valid space");
        let p = FakeProvider::new(FakeProviderConfig {
            fail_after_n_calls: Some(1),
            fail_with: Some(ProviderError::RateLimited {
                retry_after: Duration::from_secs(3),
            }),
            ..FakeProviderConfig::new(space)
        });
        let batch = vec![doc("x")];
        assert!(p.embed_documents(&batch).is_ok(), "call 1 (ordinal ≤ n) succeeds");
        assert_eq!(p.call_count(), 1);
        let err = p.embed_documents(&batch).unwrap_err();
        assert_eq!(
            err,
            ProviderError::RateLimited {
                retry_after: Duration::from_secs(3)
            }
        );
        assert!(p.embed_queries(&[query("y")]).is_err(), "fault spans both methods");
        assert_eq!(p.call_count(), 3, "failing calls are counted too");
    }

    #[test]
    fn armed_fault_without_error_defaults_to_server_error() {
        let space = VectorSpace::new("fake/model-a", 8).expect("valid space");
        let p = FakeProvider::new(FakeProviderConfig {
            fail_after_n_calls: Some(0),
            fail_with: None,
            ..FakeProviderConfig::new(space)
        });
        assert_eq!(
            p.embed_documents(&[doc("x")]).unwrap_err(),
            ProviderError::ServerError
        );
    }

    #[test]
    fn full_provider_error_matrix_is_injectable_and_distinguishable() {
        // The P6-016 fault matrix replays every variant through the same
        // script surface; each must come back exactly as injected.
        let scripts: Vec<ProviderError> = vec![
            ProviderError::RateLimited {
                retry_after: Duration::from_millis(7),
            },
            ProviderError::ServerError,
            ProviderError::AuthError,
            ProviderError::Timeout,
            ProviderError::Cancelled,
            ProviderError::InvalidInput("injected".into()),
        ];
        for expected in scripts {
            let space = VectorSpace::new("fake/model-a", 8).expect("valid space");
            let p = FakeProvider::new(FakeProviderConfig {
                fail_after_n_calls: Some(0),
                fail_with: Some(expected.clone()),
                ..FakeProviderConfig::new(space)
            });
            assert_eq!(p.embed_documents(&[doc("x")]).unwrap_err(), expected);
        }
    }

    #[test]
    fn zero_vector_injection_is_rejected_by_the_output_gate() {
        let space = VectorSpace::new("fake/model-a", 8).expect("valid space");
        let bad = InputDigest::of_input(b"poisoned").expect("digest").into_inner();
        let p = FakeProvider::new(FakeProviderConfig {
            zero_vector_digests: vec![bad.clone()],
            ..FakeProviderConfig::new(space)
        });
        // A single bad input fails with the digest named in the error.
        let err = p.embed_documents(&[doc("poisoned")]).unwrap_err();
        match err {
            ProviderError::InvalidInput(message) => {
                assert!(message.contains(&bad), "error must name the digest: {message}");
                assert!(message.contains("zero vector"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
        // A batch with one bad input fails as a whole — no partial vector
        // list is ever returned (bad vectors cannot reach the cache).
        let batch = vec![doc("clean"), doc("poisoned"), doc("clean-2")];
        assert!(p.embed_documents(&batch).is_err());
        // The query path is covered by the same digest switch.
        assert!(p.embed_queries(&[query("poisoned")]).is_err());
    }

    #[test]
    fn delay_is_applied_per_call() {
        let space = VectorSpace::new("fake/model-a", 8).expect("valid space");
        let p = FakeProvider::new(FakeProviderConfig {
            delay_per_call: Duration::from_millis(5),
            ..FakeProviderConfig::new(space)
        });
        let start = std::time::Instant::now();
        assert!(p.embed_documents(&[doc("x")]).is_ok());
        assert!(start.elapsed() >= Duration::from_millis(5));
    }

    #[test]
    fn config_defaults_are_naturally_successful() {
        // An invalid VectorSpace is unrepresentable by construction (private
        // fields; the sanctioned `new` validates), so the constructor's eager
        // `validate` is belt-and-braces. What is testable: the default script
        // carries no fault, no latency, no injection — the provider succeeds
        // out of the box.
        let space = VectorSpace::new("fake/model-a", 8).expect("valid space");
        let config = FakeProviderConfig::new(space);
        assert_eq!(config.fail_after_n_calls, None);
        assert_eq!(config.fail_with, None);
        assert_eq!(config.delay_per_call, Duration::ZERO);
        assert!(config.zero_vector_digests.is_empty());
        let p = FakeProvider::new(config);
        assert!(p.embed_documents(&[doc("x")]).is_ok());
        assert!(p.embed_queries(&[query("x")]).is_ok());
    }

    // ── Cache loop (embed → put → get → bitwise) ─────────────────────────

    #[test]
    fn embed_put_get_roundtrip_is_bitwise_lossless() {
        use crate::cache::ArtifactCache;
        use crate::spec::{DocumentEncodingSpec, VectorSpace as Space};
        use crate::types::{DocSpecDigest, InputDigest};

        static UNIQUE: AtomicUsize = AtomicUsize::new(0);
        let tag = UNIQUE.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "cc-semantic-p6009-roundtrip-{}-{tag}",
            std::process::id()
        ));

        let space = Space::new("fake/model-a", 8).expect("valid space");
        let p = FakeProvider::new(FakeProviderConfig::new(space.clone()));
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
            .expect("valid spec");
        let spec_digest: DocSpecDigest = spec.digest().expect("spec digest");
        let input = InputDigest::of_input(b"roundtrip payload").expect("valid input");

        let embedded = p
            .embed_documents(&[doc("roundtrip payload")])
            .expect("embed")
            .remove(0);

        let cache = ArtifactCache::open(&root, "p6009-test".to_string()).expect("open cache");
        let reference = cache
            .put(&space, &input, &spec_digest, &embedded, 1_000)
            .expect("put");

        match cache.get(&space, &input, &spec_digest).expect("get") {
            crate::cache::CacheRead::Hit(validated) => {
                assert_eq!(validated.dimension, 8);
                assert_eq!(validated.artifact_ref, reference);
                assert_bits_eq(&validated.data, &embedded);
            }
            other => panic!("expected a cache hit, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
