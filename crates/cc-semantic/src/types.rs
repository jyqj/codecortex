//! Opaque digest newtypes for the semantic persistence seam (P6-002/P6-003).
//!
//! Construction is sealed as of P6-003: the raw `new` is crate-internal, and
//! every public constructor routes through the frozen canonical specification
//! in [`crate::spec`] — input digests only from validated canonical input
//! bytes, spec digests only from validated frozen specs. None of these types
//! touches the filesystem.
//!
//! Three-way digest semantics (TASK-BRIEFS P6-003, ADR-0003 constraint
//! table): [`SpaceDigest`] identifies the frozen vector space,
//! [`DocSpecDigest`] identifies the document encoding spec, and
//! [`QuerySpecDigest`] identifies the query encoding spec.
//! [`InputDigest`]/[`QueryDigest`] are the byte-level digests of the actual
//! embedded inputs on the document/query paths respectively.

use std::fmt;

use crate::spec::input_bytes_digest;
use cc_model::CcResult;

// The frozen space types moved to `spec.rs` in P6-003 (as announced by the
// P6-002 skeleton); re-exported here so the `crate::types::` path used by the
// frozen port signatures stays valid.
pub use crate::spec::{DistanceMetric, VectorSpace};

macro_rules! opaque_digest {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Crate-internal escape hatch. The public construction surface
            /// is the validated paths: `of_input` for input digests and
            /// `digest()` on the frozen specs for spec digests. `allow` covers
            /// types whose validated constructor lands in a later task
            /// (`ArtifactRef` → P6-008) without weakening the seal.
            #[allow(dead_code)]
            pub(crate) fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque_digest! {
    /// Digest of a frozen [`VectorSpace`]: blake3 over canonical JSON of
    /// `(space domain tag, spec)`. Only [`VectorSpace::digest`] constructs
    /// one. Different `model_id`s always yield different space digests, even
    /// at equal dimension — mixing spaces is rejected at this layer.
    SpaceDigest
}

opaque_digest! {
    /// Digest of the actual embedded input bytes on the document path:
    /// blake3 over validated canonical input bytes. Only
    /// [`InputDigest::of_input`] constructs one; this is the dedup key on the
    /// artifact-cache path (P6-008).
    InputDigest
}

opaque_digest! {
    /// Digest of a frozen document encoding spec (space + instruction/limits/
    /// tokenizer). Only [`crate::spec::DocumentEncodingSpec::digest`]
    /// constructs one. A change here means documents must be re-embedded.
    DocSpecDigest
}

opaque_digest! {
    /// Digest of the actual embedded input bytes on the query path (the
    /// query-path analog of [`InputDigest`]).
    ///
    /// This is *not* the query encoding-spec digest: spec-level query
    /// identity is [`QuerySpecDigest`] (P6-003 three-way split). Changing a
    /// query spec parameter never changes this digest and never re-embeds
    /// documents.
    QueryDigest
}

opaque_digest! {
    /// Digest of a frozen query encoding spec. Only
    /// [`crate::spec::QueryEncodingSpec::digest`] constructs one.
    /// Query-only spec changes never alter [`DocSpecDigest`], so documents
    /// are not re-embedded.
    QuerySpecDigest
}

opaque_digest! {
    /// Content-addressed reference into the artifact cache, stored as
    /// `semantic_manifest.artifact_ref` (P6-005). As of P6-008 its sole
    /// sanctioned public constructor is [`crate::cache::ArtifactCache::put`];
    /// the format is `cas.v1:<namespace>:<space_id>:<input_digest>:
    /// <spec_digest>:<checksum>` (ADR addressing tuple incl. checksum).
    ArtifactRef
}

impl InputDigest {
    /// Only sanctioned constructor: digests validated canonical input bytes
    /// (spec v1: non-empty, bounded, UTF-8 — see
    /// [`crate::spec::validate_input_bytes`]).
    pub fn of_input(bytes: &[u8]) -> CcResult<Self> {
        Ok(Self::new(input_bytes_digest(bytes)?))
    }
}

impl QueryDigest {
    /// Only sanctioned constructor: digests validated canonical query input
    /// bytes, same contract as [`InputDigest::of_input`].
    pub fn of_input(bytes: &[u8]) -> CcResult<Self> {
        Ok(Self::new(input_bytes_digest(bytes)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_digests_reject_invalid_bytes() {
        assert!(InputDigest::of_input(b"").is_err());
        assert!(InputDigest::of_input(&[0xff, 0xfe]).is_err());
        assert!(QueryDigest::of_input(b"").is_err());
    }

    #[test]
    fn input_digests_are_deterministic_and_distinct() {
        let a = InputDigest::of_input(b"same").expect("digest");
        let b = InputDigest::of_input(b"same").expect("digest");
        let c = InputDigest::of_input(b"other").expect("digest");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn digest_display_matches_inner_string() {
        let d = InputDigest::of_input(b"text").expect("digest");
        assert_eq!(d.to_string(), d.as_str());
        assert_eq!(d.into_inner(), cc_model::identity::bytes_hash(b"text"));
    }
}
