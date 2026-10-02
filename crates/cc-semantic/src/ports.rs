//! Worker-side embedding provider port (skeleton, P6-002).
//!
//! Trait signatures follow the TASK-BRIEFS P6-009 draft. This is an
//! owner-owned freeze surface: after P6-008 closes, implementations may be
//! delegated (fake provider, filtered exact) but the port itself does not
//! change without re-running the freeze. Implementations must never touch the
//! network in the no-network closure (P6-020 checks the dependency graph);
//! `providers::fake` (P6-009) is the reference in-process implementation.

use std::time::Duration;

use cc_model::CcResult;

use crate::types::{InputDigest, QueryDigest, VectorSpace};

/// Batched embedding inputs on the document path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentInput {
    /// Digest of the embedded input bytes (dedup key on the cache path).
    pub input_digest: InputDigest,
    /// Exact bytes handed to the provider.
    pub bytes: Vec<u8>,
}

/// Batched embedding inputs on the query path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryInput {
    /// Digest of the embedded query bytes.
    pub digest: QueryDigest,
    /// Exact bytes handed to the provider.
    pub bytes: Vec<u8>,
}

impl DocumentInput {
    /// Sanctioned constructor (P6-003 digest tightening): the digest is
    /// computed over these exact bytes after canonical input validation, so
    /// the `digest ↔ bytes` binding cannot drift at construction time.
    pub fn from_bytes(bytes: &[u8]) -> CcResult<Self> {
        Ok(Self {
            input_digest: InputDigest::of_input(bytes)?,
            bytes: bytes.to_vec(),
        })
    }

    /// Re-checks the digest binding against the carried bytes.
    pub fn verify(&self) -> CcResult<()> {
        if InputDigest::of_input(&self.bytes)?.as_str() == self.input_digest.as_str() {
            Ok(())
        } else {
            Err(cc_model::CcError::InvalidParams(
                "DocumentInput digest does not match its bytes".into(),
            ))
        }
    }
}

impl QueryInput {
    /// Sanctioned constructor (P6-003 digest tightening), same contract as
    /// [`DocumentInput::from_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> CcResult<Self> {
        Ok(Self {
            digest: QueryDigest::of_input(bytes)?,
            bytes: bytes.to_vec(),
        })
    }

    /// Re-checks the digest binding against the carried bytes.
    pub fn verify(&self) -> CcResult<()> {
        if QueryDigest::of_input(&self.bytes)?.as_str() == self.digest.as_str() {
            Ok(())
        } else {
            Err(cc_model::CcError::InvalidParams(
                "QueryInput digest does not match its bytes".into(),
            ))
        }
    }
}

/// Provider-side failure taxonomy (P6-009 scripted-fault matrix, V15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    RateLimited {
        retry_after: Duration,
    },
    ServerError,
    AuthError,
    Timeout,
    Cancelled,
    /// Structurally invalid result: NaN/Inf/zero vector, dimension mismatch,
    /// or count mismatch. Rejected here so bad vectors never reach the cache.
    InvalidInput(String),
}

/// Embedding provider port. Batched and bounded: the caller guarantees batch
/// size (P6-013 admission), implementations do not split batches.
///
/// Callers must invoke this port without holding any DB lock or connection
/// (02-CONTRACTS C11).
pub trait EmbeddingProvider: Send + Sync {
    /// The frozen space this provider embeds into.
    fn space(&self) -> &VectorSpace;

    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError>;

    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct EchoProvider {
        space: VectorSpace,
        calls: Mutex<usize>,
    }

    impl EmbeddingProvider for EchoProvider {
        fn space(&self) -> &VectorSpace {
            &self.space
        }

        fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            *self.calls.lock().unwrap() += 1;
            // Contract: one output vector per input, order preserved.
            Ok(batch.iter().map(|_| vec![1.0_f32]).collect())
        }

        fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            Ok(batch.iter().map(|_| vec![0.5_f32]).collect())
        }
    }

    fn space() -> VectorSpace {
        VectorSpace::new("fake/model-a", 1).expect("valid space")
    }

    fn doc(text: &str) -> DocumentInput {
        DocumentInput::from_bytes(text.as_bytes()).expect("valid document input")
    }

    #[test]
    fn embed_documents_preserves_batch_order_and_count() {
        let provider = EchoProvider {
            space: space(),
            calls: Mutex::new(0),
        };
        let batch = vec![doc("a"), doc("b"), doc("c")];
        let out = provider.embed_documents(&batch).unwrap();
        assert_eq!(out.len(), batch.len());
        assert!(out.iter().all(|v| v == &vec![1.0_f32]));
        assert_eq!(*provider.calls.lock().unwrap(), 1);
    }

    #[test]
    fn embed_queries_maps_one_output_per_query() {
        let provider = EchoProvider {
            space: space(),
            calls: Mutex::new(0),
        };
        let batch = vec![QueryInput::from_bytes(b"q").expect("valid query input")];
        let out = provider.embed_queries(&batch).unwrap();
        assert_eq!(out, vec![vec![0.5_f32]]);
    }

    #[test]
    fn input_constructors_bind_digest_to_exact_bytes() {
        // P6-003 red/green: digests can no longer be minted independently of
        // the bytes they claim to describe.
        let d = doc("payload");
        assert!(d.verify().is_ok());
        assert_eq!(d.input_digest.as_str(), crate::spec::input_bytes_digest(b"payload").expect("digest"));

        let q = QueryInput::from_bytes(b"query").expect("valid query input");
        assert!(q.verify().is_ok());

        let mut tampered = doc("payload");
        tampered.bytes = b"other".to_vec();
        assert_ne!(
            tampered.input_digest.as_str(),
            crate::spec::input_bytes_digest(b"other").expect("digest")
        );
        assert!(tampered.verify().is_err());
    }

    #[test]
    fn provider_error_variants_are_distinguishable() {
        let e1 = ProviderError::RateLimited {
            retry_after: Duration::from_secs(2),
        };
        let e2 = ProviderError::InvalidInput("zero vector".into());
        assert_ne!(e1, ProviderError::ServerError);
        assert_ne!(e2, ProviderError::AuthError);
        assert_eq!(
            e1,
            ProviderError::RateLimited {
                retry_after: Duration::from_secs(2)
            }
        );
    }

    #[test]
    fn port_is_object_safe_and_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn EmbeddingProvider>();
    }
}
