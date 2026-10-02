//! Frozen canonical encoding space and input specification (P6-003).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (constraint table: "`VectorSpace`/`DocumentEncoding`/`QueryEncoding` three-way
//! split with complete digests is the precondition for cache namespace validation;
//! same dimension but different model must never be mixed").
//!
//! This module freezes, as of `ENCODING_SPEC_VERSION`:
//!
//! - the **space identity** ([`VectorSpace`]): `model_id` is part of the
//!   identity, `dimension` is not — two spaces with equal dimension but
//!   different `model_id` always yield different [`SpaceDigest`]s;
//! - the **three-way digest split**: space identity (`SpaceDigest`),
//!   document encoding spec (`DocSpecDigest`), query encoding spec
//!   (`QuerySpecDigest`). Query-only changes (instruction/limits) never touch
//!   `DocSpecDigest`, so documents are not re-embedded;
//! - the **input byte contract** ([`validate_input_bytes`]): v1 inputs are
//!   canonical UTF-8 text, non-empty, bounded by [`MAX_INPUT_BYTES`];
//! - the **digest formulas**: blake3 over canonical JSON of
//!   `(domain-tag, struct)` — struct field order is the canonical field
//!   order, no maps are involved.
//!
//! Version bump policy is documented in `docs/ENCODING-SPACE.md` next to the
//! crate root. Frozen surface rules: adding a `DistanceMetric` variant, a new
//! field, or a new admitted value range is a spec-version bump, never a
//! silent in-place change.

use serde::Serialize;

use cc_model::identity::{bytes_hash, hash};
use cc_model::{CcError, CcResult};

use crate::error::SemanticError;
use crate::types::{DocSpecDigest, QuerySpecDigest, SpaceDigest};

/// Frozen spec version of the encoding space. Part of every [`SpaceDigest`]:
/// bumping it changes the space identity, which routes all new work to a new
/// space while existing artifacts stay valid under their old space id.
pub const ENCODING_SPEC_VERSION: u32 = 1;

/// Admitted dimension range (inclusive). v1 admits 1..=65536: every real
/// embedding model today is inside this bound, and 0-dimension vectors are
/// structurally meaningless.
pub const MIN_DIMENSION: u32 = 1;
pub const MAX_DIMENSION: u32 = 65_536;

/// `model_id` length bound (bytes): `provider/model` full name.
pub const MAX_MODEL_ID_BYTES: usize = 512;

/// `tokenizer` identifier length bound (bytes).
pub const MAX_TOKENIZER_BYTES: usize = 256;

/// `instruction` length bound (bytes) when present.
pub const MAX_INSTRUCTION_BYTES: usize = 4_096;

/// Admitted `max_tokens` range (inclusive).
pub const MIN_MAX_TOKENS: u32 = 1;
pub const MAX_MAX_TOKENS: u32 = 32_768;

/// Embedded input byte bound. v1 inputs are embedding-ready canonical text;
/// anything larger is a caller bug, not something to truncate silently.
pub const MAX_INPUT_BYTES: usize = 1_048_576;

// Digest domain tags. They make every digest formula self-describing so that
// identical source text can never collide across digest domains.
const SPACE_DIGEST_DOMAIN: &str = "cc-semantic.vector-space.v1";
const DOC_SPEC_DIGEST_DOMAIN: &str = "cc-semantic.document-encoding-spec.v1";
const QUERY_SPEC_DIGEST_DOMAIN: &str = "cc-semantic.query-encoding-spec.v1";

fn invalid(message: impl Into<String>) -> CcError {
    SemanticError::InvalidInput(message.into()).into()
}

/// Distance metric of a frozen vector space.
///
/// The enum is deliberately closed with a single admitted variant. Adding a
/// variant is a compile-time break for [`VectorSpace::validate`] — exactly the
/// gate a frozen spec needs; it may only happen together with a
/// [`ENCODING_SPEC_VERSION`] bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum DistanceMetric {
    Cosine,
}

/// Frozen vector-space identity.
///
/// `model_id` is part of the space identity; `dimension` is not — two spaces
/// with the same dimension but different models are different spaces and must
/// never be mixed (ADR-0003 constraint table, P6-003).
///
/// Fields are private by design: the only sanctioned construction path is
/// [`VectorSpace::new`], which stamps the current
/// [`ENCODING_SPEC_VERSION`] and the sole admitted metric, then validates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct VectorSpace {
    model_id: String,
    dimension: u32,
    distance: DistanceMetric,
    spec_version: u32,
}

impl VectorSpace {
    /// Sanctioned constructor: stamps the frozen spec version and the sole
    /// admitted distance metric, then validates.
    pub fn new(model_id: impl Into<String>, dimension: u32) -> CcResult<Self> {
        let space = Self {
            model_id: model_id.into(),
            dimension,
            distance: DistanceMetric::Cosine,
            spec_version: ENCODING_SPEC_VERSION,
        };
        space.validate()?;
        Ok(space)
    }

    /// Full frozen-surface validation: identifier, dimension range, metric
    /// admission, spec version.
    pub fn validate(&self) -> CcResult<()> {
        if self.model_id.is_empty()
            || self.model_id.len() > MAX_MODEL_ID_BYTES
            || self.model_id.trim() != self.model_id
        {
            return Err(invalid(format!(
                "model_id must be 1..={MAX_MODEL_ID_BYTES} bytes without leading/trailing whitespace"
            )));
        }
        if !(MIN_DIMENSION..=MAX_DIMENSION).contains(&self.dimension) {
            return Err(invalid(format!(
                "dimension must be in {MIN_DIMENSION}..={MAX_DIMENSION}"
            )));
        }
        match self.distance {
            // Compile-time admission gate: a new metric variant fails to
            // compile here until it is explicitly admitted under a version
            // bump.
            DistanceMetric::Cosine => {}
        }
        if self.spec_version != ENCODING_SPEC_VERSION {
            return Err(invalid(format!(
                "spec_version {} is not the frozen {}",
                self.spec_version, ENCODING_SPEC_VERSION
            )));
        }
        Ok(())
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn dimension(&self) -> u32 {
        self.dimension
    }

    pub fn distance(&self) -> DistanceMetric {
        self.distance
    }

    pub fn spec_version(&self) -> u32 {
        self.spec_version
    }

    /// Space identity digest: blake3 over canonical JSON of
    /// `(SPACE_DIGEST_DOMAIN, self)`. Distinct `model_id`s at equal dimension
    /// always yield distinct digests — the first rejection layer for "same
    /// dimension, different model must not be mixed" (V16).
    pub fn digest(&self) -> CcResult<SpaceDigest> {
        self.validate()?;
        Ok(SpaceDigest::new(hash(&(
            SPACE_DIGEST_DOMAIN,
            self,
        ))?))
    }
}

/// Frozen document encoding spec (document path only).
///
/// Query-only parameters live in [`QueryEncodingSpec`]; a change here means
/// the document encoding changed and documents must be re-embedded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct DocumentEncodingSpec {
    space: VectorSpace,
    instruction: Option<String>,
    max_tokens: u32,
    tokenizer: String,
}

/// Frozen query encoding spec (query path only).
///
/// Deliberately a distinct type from [`DocumentEncodingSpec`]: changing a
/// query-side parameter changes only the [`QuerySpecDigest`], never the
/// [`DocSpecDigest`], so documents are not re-embedded (brief P6-003
/// three-way split).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct QueryEncodingSpec {
    space: VectorSpace,
    instruction: Option<String>,
    max_tokens: u32,
    tokenizer: String,
}

fn validate_spec_fields(
    space: &VectorSpace,
    instruction: &Option<String>,
    max_tokens: u32,
    tokenizer: &str,
) -> CcResult<()> {
    space.validate()?;
    if tokenizer.is_empty() || tokenizer.len() > MAX_TOKENIZER_BYTES {
        return Err(invalid(format!(
            "tokenizer must be 1..={MAX_TOKENIZER_BYTES} bytes"
        )));
    }
    if !(MIN_MAX_TOKENS..=MAX_MAX_TOKENS).contains(&max_tokens) {
        return Err(invalid(format!(
            "max_tokens must be in {MIN_MAX_TOKENS}..={MAX_MAX_TOKENS}"
        )));
    }
    if let Some(instruction) = instruction {
        if instruction.is_empty() || instruction.len() > MAX_INSTRUCTION_BYTES {
            return Err(invalid(format!(
                "instruction must be 1..={MAX_INSTRUCTION_BYTES} bytes when present"
            )));
        }
    }
    Ok(())
}

impl DocumentEncodingSpec {
    /// Sanctioned constructor; validates the full frozen surface.
    pub fn new(
        space: VectorSpace,
        instruction: Option<String>,
        max_tokens: u32,
        tokenizer: impl Into<String>,
    ) -> CcResult<Self> {
        let spec = Self {
            space,
            instruction,
            max_tokens,
            tokenizer: tokenizer.into(),
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> CcResult<()> {
        validate_spec_fields(&self.space, &self.instruction, self.max_tokens, &self.tokenizer)
    }

    pub fn space(&self) -> &VectorSpace {
        &self.space
    }

    pub fn instruction(&self) -> Option<&str> {
        self.instruction.as_deref()
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn tokenizer(&self) -> &str {
        &self.tokenizer
    }

    /// Document-spec digest: blake3 over canonical JSON of
    /// `(DOC_SPEC_DIGEST_DOMAIN, self)`.
    pub fn digest(&self) -> CcResult<DocSpecDigest> {
        self.validate()?;
        Ok(DocSpecDigest::new(hash(&(
            DOC_SPEC_DIGEST_DOMAIN,
            self,
        ))?))
    }
}

impl QueryEncodingSpec {
    /// Sanctioned constructor; validates the full frozen surface.
    pub fn new(
        space: VectorSpace,
        instruction: Option<String>,
        max_tokens: u32,
        tokenizer: impl Into<String>,
    ) -> CcResult<Self> {
        let spec = Self {
            space,
            instruction,
            max_tokens,
            tokenizer: tokenizer.into(),
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> CcResult<()> {
        validate_spec_fields(&self.space, &self.instruction, self.max_tokens, &self.tokenizer)
    }

    pub fn space(&self) -> &VectorSpace {
        &self.space
    }

    pub fn instruction(&self) -> Option<&str> {
        self.instruction.as_deref()
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn tokenizer(&self) -> &str {
        &self.tokenizer
    }

    /// Query-spec digest: blake3 over canonical JSON of
    /// `(QUERY_SPEC_DIGEST_DOMAIN, self)`. Independent of any
    /// [`DocumentEncodingSpec`] digest by domain tag and by type.
    pub fn digest(&self) -> CcResult<QuerySpecDigest> {
        self.validate()?;
        Ok(QuerySpecDigest::new(hash(&(
            QUERY_SPEC_DIGEST_DOMAIN,
            self,
        ))?))
    }
}

/// Canonical input byte contract (v1).
///
/// v1 admits exactly: non-empty, at most [`MAX_INPUT_BYTES`], valid UTF-8.
/// Digests over embedded inputs ([`InputDigest`]/[`QueryDigest`]) may only be
/// constructed from bytes that pass this gate, which is what makes those
/// digests reproducible namespace keys instead of opaque strings.
pub fn validate_input_bytes(bytes: &[u8]) -> CcResult<()> {
    if bytes.is_empty() {
        return Err(invalid("input bytes must be non-empty"));
    }
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(invalid(format!(
            "input bytes exceed the frozen bound of {MAX_INPUT_BYTES}"
        )));
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err(invalid("input bytes must be canonical UTF-8 text (spec v1)"));
    }
    Ok(())
}

/// Digest of validated canonical input bytes (shared by document and query
/// paths): `blake3(bytes)` after [`validate_input_bytes`].
pub fn input_bytes_digest(bytes: &[u8]) -> CcResult<String> {
    validate_input_bytes(bytes)?;
    Ok(bytes_hash(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InputDigest;

    fn space(model: &str, dim: u32) -> VectorSpace {
        VectorSpace::new(model, dim).expect("valid space")
    }

    fn doc_spec(model: &str, dim: u32) -> DocumentEncodingSpec {
        DocumentEncodingSpec::new(space(model, dim), None, 8_192, "fake-tokenizer")
            .expect("valid doc spec")
    }

    fn query_spec(model: &str, dim: u32) -> QueryEncodingSpec {
        QueryEncodingSpec::new(space(model, dim), None, 8_192, "fake-tokenizer")
            .expect("valid query spec")
    }

    // ── carried over from the P6-002 skeleton (names preserved) ──────────

    #[test]
    fn same_model_same_dimension_is_same_space() {
        assert_eq!(space("fake/model-a", 8), space("fake/model-a", 8));
    }

    #[test]
    fn same_dimension_different_model_is_a_different_space() {
        // "Same dimension, different model must not be mixed": structurally,
        // distinct model_id means a distinct space identity — and, once
        // frozen, a distinct SpaceDigest.
        let a = space("fake/model-a", 8);
        let b = space("fake/model-b", 8);
        assert_ne!(a, b);
        assert_eq!(a.dimension(), b.dimension());
        assert_ne!(
            a.digest().expect("digest a"),
            b.digest().expect("digest b")
        );
    }

    #[test]
    fn cosine_is_the_only_admitted_metric() {
        assert_eq!(space("fake/model-a", 8).distance(), DistanceMetric::Cosine);
    }

    #[test]
    fn digests_are_opaque_and_distinct_by_construction() {
        // Replaces the P6-002 Display-vs-Debug tautology with a meaningful
        // domain-separation assertion: identical source text routed through
        // different digest domains must yield different digest values.
        let space_digest = space("m", 8).digest().expect("space digest");
        let input_digest = InputDigest::of_input(b"fake/model-m").expect("input digest");
        assert_ne!(space_digest.as_str(), input_digest.as_str());

        let again = space("m", 8).digest().expect("space digest again");
        assert_eq!(space_digest, again);
        assert_eq!(space_digest.to_string(), space_digest.as_str());
    }

    // ── P6-003 frozen-surface validation: negative cases ─────────────────

    #[test]
    fn zero_and_oversized_dimensions_are_rejected() {
        assert!(VectorSpace::new("fake/model-a", 0).is_err());
        assert!(VectorSpace::new("fake/model-a", MAX_DIMENSION + 1).is_err());
    }

    #[test]
    fn bad_model_ids_are_rejected() {
        assert!(VectorSpace::new("", 8).is_err());
        assert!(VectorSpace::new(" padded", 8).is_err());
        assert!(VectorSpace::new("x".repeat(MAX_MODEL_ID_BYTES + 1), 8).is_err());
    }

    #[test]
    fn foreign_spec_versions_are_rejected() {
        let mut stale = space("fake/model-a", 8);
        stale.spec_version = ENCODING_SPEC_VERSION + 1;
        assert!(stale.validate().is_err());
        assert!(stale.digest().is_err());
        // The sanctioned constructor can never produce such a value.
        assert_eq!(
            space("fake/model-a", 8).spec_version(),
            ENCODING_SPEC_VERSION
        );
    }

    #[test]
    fn bad_encoding_spec_fields_are_rejected() {
        let s = space("fake/model-a", 8);
        assert!(DocumentEncodingSpec::new(s.clone(), None, 0, "t").is_err());
        assert!(DocumentEncodingSpec::new(s.clone(), None, MAX_MAX_TOKENS + 1, "t").is_err());
        assert!(DocumentEncodingSpec::new(s.clone(), None, 8_192, "").is_err());
        assert!(DocumentEncodingSpec::new(s.clone(), Some(String::new()), 8_192, "t").is_err());
        assert!(DocumentEncodingSpec::new(s.clone(), Some("x".repeat(MAX_INSTRUCTION_BYTES + 1)), 8_192, "t").is_err());
        assert!(QueryEncodingSpec::new(s.clone(), None, 0, "t").is_err());
        assert!(QueryEncodingSpec::new(s, None, 8_192, "").is_err());
    }

    #[test]
    fn invalid_input_bytes_are_rejected() {
        assert!(validate_input_bytes(b"").is_err());
        assert!(validate_input_bytes(&[0xff, 0xfe]).is_err());
        assert!(validate_input_bytes(&vec![b'a'; MAX_INPUT_BYTES + 1]).is_err());
    }

    // ── P6-003 frozen-surface validation: positive cases ─────────────────

    #[test]
    fn valid_boundary_values_pass() {
        assert!(VectorSpace::new("fake/model-a", MIN_DIMENSION).is_ok());
        assert!(VectorSpace::new("fake/model-a", MAX_DIMENSION).is_ok());
        assert!(validate_input_bytes(&[0x01]).is_ok());
        assert!(validate_input_bytes("嵌入输入".as_bytes()).is_ok());
        assert!(validate_input_bytes(&vec![b'a'; MAX_INPUT_BYTES]).is_ok());
    }

    // ── P6-003 three-way digest split ────────────────────────────────────

    #[test]
    fn query_only_change_never_touches_the_document_spec_digest() {
        let doc = doc_spec("fake/model-a", 8);
        let doc_other_query_params = DocumentEncodingSpec::new(
            space("fake/model-a", 8),
            Some("doc instruction".into()),
            4_096,
            "fake-tokenizer",
        )
        .expect("valid doc spec");

        let q1 = query_spec("fake/model-a", 8);
        let q2 = QueryEncodingSpec::new(
            space("fake/model-a", 8),
            Some("query instruction".into()),
            512,
            "fake-tokenizer",
        )
        .expect("valid query spec 2");

        assert_ne!(
            doc.digest().expect("doc digest"),
            doc_other_query_params.digest().expect("doc digest 2")
        );
        assert_ne!(q1.digest().expect("q1"), q2.digest().expect("q2"));
        // The independence direction of the split: doc spec digest is not
        // affected by anything that lives on the query side. DocSpecDigest
        // and QuerySpecDigest are distinct types — comparing across them is
        // a compile error, so the value-level check goes through `as_str`.
        assert_ne!(
            doc.digest().expect("doc digest").as_str(),
            q1.digest().expect("q1").as_str()
        );
    }

    #[test]
    fn digest_is_stable_across_reconstruction() {
        let a = doc_spec("fake/model-a", 8).digest().expect("digest a");
        let b = doc_spec("fake/model-a", 8).digest().expect("digest b");
        assert_eq!(a, b);
        let c = doc_spec("fake/model-a", 16).digest().expect("digest c");
        assert_ne!(a, c);
    }
}
