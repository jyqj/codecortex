//! Model capability declaration, spec-consistency validation, and the
//! capability probe protocol (P7-002).
//!
//! Boundary authorities: the frozen encoding surface
//! ([`crate::spec::VectorSpace`] / [`crate::spec::DocumentEncodingSpec`] /
//! [`crate::spec::QueryEncodingSpec`], P6-003) and the provider adapter
//! ([`crate::providers::openai_compatible`], P7-001). This module adds **no**
//! frozen constants and touches **no** freeze surface — it only adds new
//! types on top, per the spec.rs extension rule.
//!
//! ## Three layers, one rule: "支持差异不能吞"
//!
//! 1. **Declared capability** ([`ModelCapability`]): what the operator
//!    asserts about an embedding model — dimensions, metric, input token
//!    limit, batch limit, encoding formats, instruction support, and whether
//!    the endpoint accepts an explicit `dimensions` request field. Declared
//!    values come from `cc_model::config::SemanticProviderConfig`; every
//!    capability-relevant field is *required* when the provider is enabled
//!    ([`resolve_provider`]) — there is no generous silent default.
//! 2. **Consistency validation** ([`validate_capability`]): declared
//!    capability vs. the frozen space/spec surface. Any contradiction
//!    (dimension, metric, foreign model, `max_tokens` above the model
//!    limit, an `instruction` against a model without instruction support,
//!    missing `float` encoding) is a `CcError::Config` — a startup-refusing
//!    configuration error, never a soft warning and never a fake success.
//! 3. **Probe protocol v1** ([`CapabilityProber`]): the declared capability
//!    is verified against the *real* endpoint through the same injectable
//!    transport seam the adapter uses (zero real network this round, D1/D2;
//!    tests run against an in-memory mock). A probe sends tiny fixed marker
//!    inputs only:
//!    - **dimension probe**: one input; in `Configurable` mode the request
//!      body carries the explicit `dimensions` field. If the endpoint
//!      rejects the request while an identical request *without* the field
//!      succeeds, the verdict is the typed
//!      [`CapabilityProbeError::DimensionsUnsupported`] — the config path is
//!      `dimensions_mode = "fixed"`, never a fake success (tasks.json P7-002
//!      acceptance).
//!    - **batch-limit probe**: exactly `max_batch_items` inputs; the
//!      response must carry the full count with the structural gate shared
//!      with the adapter ([`crate::providers::openai_compatible::
//!      parse_embeddings_response`]). Declared limits above
//!      [`MAX_PROBE_BATCH_ITEMS`] are rejected at resolution time (protocol
//!      v1 bound; per-item token/byte admission is P7-003, not probed here).
//!    - **model echo**: a response naming a different model than the frozen
//!      space identity is a [`CapabilityProbeError::Mismatch`].
//!
//! ## Failure mapping and cache semantics
//!
//! - A probe verdict that *contradicts the declaration* (dimension/model
//!   echo/…) is [`CapabilityProbeError::Mismatch`].
//! - A probe that *cannot conclude* (transport failure, HTTP 5xx/429/401,
//!   malformed response) is [`CapabilityProbeError::Unavailable`]; the
//!   capability stays **unknown**.
//! - Only a *successful* verdict enters the [`CapabilityCache`] (keyed by
//!   the frozen [`SpaceDigest`]); every failure path leaves the cache
//!   untouched, so a later probe re-runs instead of trusting a stale or
//!   absent verification. [`verify_capability`] with a cache returns the
//!   cached verdict without touching the transport; `force = true`
//!   re-probes.
//! - Credentials only ever travel in the `Authorization` header via
//!   [`openai_compatible::EmbeddingApiKey`]; probe errors and the cache
//!   never contain key material.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cc_model::config::SemanticProviderConfig;
use cc_model::{CcError, CcResult};

use crate::providers::openai_compatible::{
    map_transport_error, parse_embeddings_response, EmbeddingApiKey, EmbeddingHttpTransport,
    HttpRequest, NormPolicy,
};
use crate::spec::{
    DistanceMetric, DocumentEncodingSpec, QueryEncodingSpec, MAX_MAX_TOKENS, MAX_MODEL_ID_BYTES,
    MIN_DIMENSION, MIN_MAX_TOKENS,
};
use crate::types::{SpaceDigest, VectorSpace};

/// Version of the probe protocol itself. Part of every
/// [`VerifiedCapability`]: a protocol change (different probe inputs,
/// different verdict rules) invalidates previously verified capabilities.
pub const PROBE_PROTOCOL_VERSION: u32 = 1;

/// Protocol-v1 upper bound for a *declarable* `max_batch_items`: the batch
/// probe sends exactly this many tiny marker inputs, so unbounded
/// declarations would make the probe unbounded. Raising it is a protocol
/// change (bump [`PROBE_PROTOCOL_VERSION`]), not a silent edit.
pub const MAX_PROBE_BATCH_ITEMS: u32 = 4_096;

/// Fixed marker text of the dimension probe. Deliberately tiny and
/// constant: the probe measures *capability*, never semantic content.
const PROBE_TEXT: &str = "codecortex capability probe v1";

fn config_error(message: impl Into<String>) -> CcError {
    CcError::Config(message.into())
}

/// Whether the embedding endpoint accepts an explicit `dimensions` field in
/// the request body (OpenAI-compatible `dimensions` parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionsMode {
    /// The endpoint accepts `dimensions`; probes carry the field and a 4xx
    /// on exactly the field is the typed `DimensionsUnsupported` verdict.
    Configurable,
    /// The model exposes a single native dimension; the field must not be
    /// sent and the returned dimension must equal the declared one.
    Fixed,
}

impl DimensionsMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DimensionsMode::Configurable => "configurable",
            DimensionsMode::Fixed => "fixed",
        }
    }

    pub fn parse(value: &str) -> CcResult<Self> {
        match value.trim().to_lowercase().as_str() {
            "configurable" => Ok(DimensionsMode::Configurable),
            "fixed" => Ok(DimensionsMode::Fixed),
            other => Err(config_error(format!(
                "semantic.dimensions_mode {other:?} is not supported; \
                 admitted values are \"configurable\" or \"fixed\""
            ))),
        }
    }
}

/// Request `encoding_format` values admitted by the probe protocol v1.
/// The adapter sends exactly `float`; a capability that does not declare
/// `Float` support is a configuration error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingFormat {
    Float,
}

impl EncodingFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            EncodingFormat::Float => "float",
        }
    }

    pub fn parse(value: &str) -> CcResult<Self> {
        match value.trim().to_lowercase().as_str() {
            "float" => Ok(EncodingFormat::Float),
            other => Err(config_error(format!(
                "semantic.encoding_formats entry {other:?} is not supported; \
                 the probe protocol v1 admits only \"float\""
            ))),
        }
    }
}

/// The declared capability of one embedding model, as resolved from
/// configuration. All fields are explicit operator assertions — this struct
/// has no constructor that invents a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapability {
    /// Must equal the space's `model_id` (validated by
    /// [`validate_capability`]); it names the model this sheet describes.
    pub model_id: String,
    /// Declared output dimension; must equal `space.dimension()`.
    pub dimensions: u32,
    /// Declared metric; must equal the frozen `space.distance()` (v1:
    /// cosine only).
    pub metric: DistanceMetric,
    pub dimensions_mode: DimensionsMode,
    /// Per-input token limit of the model. Document/query spec
    /// `max_tokens` must not exceed it.
    pub max_input_tokens: u32,
    /// Maximum batch size the endpoint accepts (and the probe verifies).
    pub max_batch_items: u32,
    /// Supported request `encoding_format` values; must contain
    /// [`EncodingFormat::Float`].
    pub encoding_formats: Vec<EncodingFormat>,
    /// Whether the model accepts instruction prefixes. A spec declaring an
    /// `instruction` against `false` is a config error.
    pub supports_instruction: bool,
}

impl ModelCapability {
    /// Structural bounds of a *declarable* capability (resolution-time
    /// gate). Range failures are `CcError::Config` naming the config key.
    pub fn validate_bounds(&self) -> CcResult<()> {
        if self.model_id.is_empty() || self.model_id.len() > MAX_MODEL_ID_BYTES {
            return Err(config_error(
                "semantic.model_id must be 1..=512 bytes when semantic.enabled is true",
            ));
        }
        if !(MIN_DIMENSION..=crate::spec::MAX_DIMENSION).contains(&self.dimensions) {
            return Err(config_error(format!(
                "semantic.dimensions must be in {MIN_DIMENSION}..={}",
                crate::spec::MAX_DIMENSION
            )));
        }
        if !(MIN_MAX_TOKENS..=MAX_MAX_TOKENS).contains(&self.max_input_tokens) {
            return Err(config_error(format!(
                "semantic.max_input_tokens must be in {MIN_MAX_TOKENS}..={MAX_MAX_TOKENS}"
            )));
        }
        if self.max_batch_items < 1 || self.max_batch_items > MAX_PROBE_BATCH_ITEMS {
            return Err(config_error(format!(
                "semantic.max_batch_items must be in 1..={MAX_PROBE_BATCH_ITEMS} \
                 (probe protocol v{PROBE_PROTOCOL_VERSION} bound)"
            )));
        }
        if self.encoding_formats.is_empty() {
            return Err(config_error(
                "semantic.encoding_formats must include \"float\" (the only encoding \
                 format the provider adapter sends)",
            ));
        }
        Ok(())
    }

    fn supports(&self, format: EncodingFormat) -> bool {
        self.encoding_formats.contains(&format)
    }
}

/// A successful probe verdict. Only this type ever enters the
/// [`CapabilityCache`]; failures are never cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCapability {
    pub model_id: String,
    pub dimension: u32,
    /// Digest of the verified frozen space — the cache key.
    pub space_digest: SpaceDigest,
    pub protocol: u32,
}

/// Typed probe verdict failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityProbeError {
    /// The endpoint's observed behavior *contradicts the declared
    /// capability* (wrong dimension, foreign model echo, degenerate
    /// vector). A configuration/declaration problem — retrying cannot fix
    /// it.
    Mismatch(String),
    /// The endpoint rejected the explicit `dimensions` request field while
    /// accepting the identical request without it. The config path is
    /// `dimensions_mode = "fixed"`; the capability is *not* fake-confirmed.
    DimensionsUnsupported,
    /// The probe could not conclude (transport failure, HTTP 429/401/5xx,
    /// malformed response). The capability stays unknown.
    Unavailable(crate::ports::ProviderError),
}

impl CapabilityProbeError {
    /// Maps a probe failure onto the unified config-error surface: every
    /// probe failure is a startup-refusing condition, with the remediation
    /// path named in the message.
    pub fn into_config_error(self) -> CcError {
        match self {
            CapabilityProbeError::Mismatch(detail) => config_error(format!(
                "capability probe contradicts the declared model capability: {detail}"
            )),
            CapabilityProbeError::DimensionsUnsupported => config_error(
                "embedding endpoint rejected the explicit `dimensions` request field; \
                 set semantic.dimensions_mode = \"fixed\" if the model exposes a \
                 single native dimension",
            ),
            CapabilityProbeError::Unavailable(error) => config_error(format!(
                "capability probe could not validate the provider \
                 (capability unknown, nothing cached): {error:?}"
            )),
        }
    }

    /// Whether this failure contradicts the declaration (vs. leaving the
    /// capability merely unknown).
    pub fn is_mismatch(&self) -> bool {
        matches!(self, CapabilityProbeError::Mismatch(_))
    }
}

/// Attribute a shared structural-gate failure to a typed verdict: dimension
/// and model-echo violations contradict the *declaration*; everything else
/// (transport, HTTP status, malformed shape) leaves the capability unknown.
fn classify(error: crate::ports::ProviderError) -> CapabilityProbeError {
    match error {
        crate::ports::ProviderError::InvalidInput(message)
            if message.contains("dimension")
                || message.contains("does not match configured model") =>
        {
            CapabilityProbeError::Mismatch(message)
        }
        other => CapabilityProbeError::Unavailable(other),
    }
}

/// The capability probe. Speaks the exact same wire protocol as the adapter
/// (endpoint + `/embeddings`, bearer credential, `encoding_format: "float"`)
/// through the same injectable transport seam — but with tiny fixed marker
/// inputs and typed verdicts instead of caller payloads.
pub struct CapabilityProber {
    endpoint: String,
    api_key: EmbeddingApiKey,
    timeout: Duration,
    transport: Arc<dyn EmbeddingHttpTransport>,
}

impl fmt::Debug for CapabilityProber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `EmbeddingApiKey`'s Debug is a fixed redaction; the transport is
        // only ever rendered as "<configured>".
        f.debug_struct("CapabilityProber")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key)
            .field("timeout", &self.timeout)
            .field("transport", &"<configured>")
            .finish()
    }
}

impl CapabilityProber {
    /// Sanctioned constructor; validates the endpoint shape eagerly so a
    /// bad configuration surfaces as a `CcError::Config`, not a mid-probe
    /// panic.
    pub fn new(
        transport: Arc<dyn EmbeddingHttpTransport>,
        endpoint: impl Into<String>,
        api_key: EmbeddingApiKey,
    ) -> CcResult<Self> {
        let endpoint = endpoint.into();
        let trimmed = endpoint.trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(config_error(
                "semantic.endpoint must be non-empty when semantic.enabled is true",
            ));
        }
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err(config_error("semantic.endpoint must be an http(s) URL"));
        }
        Ok(Self {
            endpoint: trimmed.to_owned(),
            api_key,
            timeout: Duration::from_secs(30),
            transport,
        })
    }

    /// Overrides the per-request deadline handed to the transport
    /// (default 30s, mirroring the adapter default).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Full probe protocol v1: dimension probe, then batch-limit probe.
    /// Both must pass for a [`VerifiedCapability`].
    pub fn probe(
        &self,
        space: &VectorSpace,
        capability: &ModelCapability,
    ) -> Result<VerifiedCapability, CapabilityProbeError> {
        self.probe_dimension(space, capability)?;
        self.probe_batch_limit(space, capability)?;
        let space_digest = space.digest().map_err(|error| {
            CapabilityProbeError::Unavailable(crate::ports::ProviderError::InvalidInput(
                error.to_string(),
            ))
        })?;
        Ok(VerifiedCapability {
            model_id: capability.model_id.clone(),
            dimension: capability.dimensions,
            space_digest,
            protocol: PROBE_PROTOCOL_VERSION,
        })
    }

    fn probe_dimension(
        &self,
        space: &VectorSpace,
        capability: &ModelCapability,
    ) -> Result<(), CapabilityProbeError> {
        let with_dimensions = match capability.dimensions_mode {
            DimensionsMode::Configurable => true,
            DimensionsMode::Fixed => false,
        };
        match self.call(space, 1, with_dimensions) {
            Ok(()) => Ok(()),
            Err(verdict) => {
                // The control probe runs only when the failure is a *request
                // rejection* (a 4xx-mapped `InvalidInput`): that is the one
                // shape where the `dimensions` field itself can be the
                // rejected part. A mismatch verdict (server accepted the
                // request and answered wrong) or a non-request transport
                // failure is already conclusive; re-probing would neither
                // change nor refine the verdict.
                let request_rejected = matches!(
                    verdict,
                    CapabilityProbeError::Unavailable(crate::ports::ProviderError::InvalidInput(_))
                );
                if with_dimensions && request_rejected {
                    match self.call(space, 1, false) {
                        Ok(()) => Err(CapabilityProbeError::DimensionsUnsupported),
                        Err(_) => Err(verdict),
                    }
                } else {
                    Err(verdict)
                }
            }
        }
    }

    fn probe_batch_limit(
        &self,
        space: &VectorSpace,
        capability: &ModelCapability,
    ) -> Result<(), CapabilityProbeError> {
        let items = capability.max_batch_items;
        self.call(
            space,
            items,
            capability.dimensions_mode == DimensionsMode::Configurable,
        )
    }

    /// One probe request with `items` marker inputs. Returns `Err` with the
    /// typed verdict on any failure.
    fn call(
        &self,
        space: &VectorSpace,
        items: u32,
        with_dimensions: bool,
    ) -> Result<(), CapabilityProbeError> {
        let request = self.build_request(space, items, with_dimensions);
        let response = match self.transport.post_json(request) {
            Ok(response) => response,
            // Transport failures carry no response semantics: the verdict is
            // always "capability unknown", mapped through the same provider
            // taxonomy the adapter uses.
            Err(error) => {
                return Err(CapabilityProbeError::Unavailable(map_transport_error(
                    error,
                )))
            }
        };
        // The probe shares the full P7-004 strong gate; it only ever asserts
        // structural/declaration facts, so it applies the default norm
        // policy (no range admission) — norm is an adapter-level semantic,
        // not a capability declaration.
        parse_embeddings_response(space, &response, items as usize, &NormPolicy::Accept)
            .map(|_| ())
            .map_err(classify)
    }

    fn build_request(&self, space: &VectorSpace, items: u32, with_dimensions: bool) -> HttpRequest {
        let inputs: Vec<String> = (0..items)
            .map(|i| format!("{PROBE_TEXT} batch item {i}"))
            .collect();
        let mut body = serde_json::json!({
            "model": space.model_id(),
            "input": inputs,
            "encoding_format": EncodingFormat::Float.as_str(),
        });
        if with_dimensions {
            body["dimensions"] = serde_json::json!(space.dimension());
        }
        // serde_json serialization of a plain JSON value cannot fail.
        let body_bytes =
            serde_json::to_vec(&body).expect("probe request serialization cannot fail");
        HttpRequest {
            url: format!("{}/embeddings", self.endpoint),
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                (
                    "Authorization".to_owned(),
                    format!("Bearer {}", self.api_key.expose_secret()),
                ),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            body: body_bytes,
            timeout: self.timeout,
        }
    }
}

/// Process-lifetime cache of successful probe verdicts, keyed by the frozen
/// space digest. Failure verdicts are structurally unable to enter: the
/// insert path takes a [`VerifiedCapability`] only.
#[derive(Default)]
pub struct CapabilityCache {
    entries: Mutex<HashMap<SpaceDigest, VerifiedCapability>>,
}

impl CapabilityCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cached verdict for `space`, if a probe previously succeeded for the
    /// exact same frozen space identity.
    pub fn get(&self, space: &VectorSpace) -> Option<VerifiedCapability> {
        let digest = space.digest().ok()?;
        self.entries.lock().unwrap().get(&digest).cloned()
    }

    /// Records a successful verdict. Keyed by the verdict's own
    /// `space_digest`, so a verdict cannot be filed under a foreign space.
    pub fn insert(&self, verified: &VerifiedCapability) {
        self.entries
            .lock()
            .unwrap()
            .insert(verified.space_digest.clone(), verified.clone());
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

/// Cached probe entry point: returns the cached verdict without touching
/// the transport, or probes and caches the success. Failures never populate
/// the cache. `force = true` re-probes and refreshes the cache.
pub fn verify_capability(
    prober: &CapabilityProber,
    cache: Option<&CapabilityCache>,
    space: &VectorSpace,
    capability: &ModelCapability,
    force: bool,
) -> Result<VerifiedCapability, CapabilityProbeError> {
    if !force {
        if let Some(hit) = cache.and_then(|cache| cache.get(space)) {
            return Ok(hit);
        }
    }
    let verified = prober.probe(space, capability)?;
    if let Some(cache) = cache {
        cache.insert(&verified);
    }
    Ok(verified)
}

/// Declared-capability vs. frozen-surface consistency validation. Any
/// contradiction is a `CcError::Config` — the caller must refuse to start
/// the semantic path. Validation order is fixed so error messages are
/// deterministic.
pub fn validate_capability(
    capability: &ModelCapability,
    space: &VectorSpace,
    document_spec: &DocumentEncodingSpec,
    query_spec: &QueryEncodingSpec,
) -> CcResult<()> {
    space.validate()?;
    capability.validate_bounds()?;
    if capability.model_id != space.model_id() {
        return Err(config_error(format!(
            "semantic.model_id {:?} does not match the frozen space model {:?}",
            capability.model_id,
            space.model_id()
        )));
    }
    if capability.dimensions != space.dimension() {
        return Err(config_error(format!(
            "semantic.dimensions {} does not match the frozen space dimension {}",
            capability.dimensions,
            space.dimension()
        )));
    }
    if capability.metric != space.distance() {
        return Err(config_error(format!(
            "semantic.metric {:?} does not match the frozen space metric {:?}",
            capability.metric,
            space.distance()
        )));
    }
    if !capability.supports(EncodingFormat::Float) {
        return Err(config_error(
            "semantic.encoding_formats must include \"float\": it is the only \
             encoding format the provider adapter sends",
        ));
    }
    if document_spec.space() != space {
        return Err(config_error(
            "document encoding spec is bound to a different frozen space than the \
             declared capability; spaces must not be mixed (ADR-0003)",
        ));
    }
    if query_spec.space() != space {
        return Err(config_error(
            "query encoding spec is bound to a different frozen space than the \
             declared capability; spaces must not be mixed (ADR-0003)",
        ));
    }
    if document_spec.max_tokens() > capability.max_input_tokens {
        return Err(config_error(format!(
            "document spec max_tokens {} exceeds the declared model input limit {} \
             (semantic.max_input_tokens)",
            document_spec.max_tokens(),
            capability.max_input_tokens
        )));
    }
    if query_spec.max_tokens() > capability.max_input_tokens {
        return Err(config_error(format!(
            "query spec max_tokens {} exceeds the declared model input limit {} \
             (semantic.max_input_tokens)",
            query_spec.max_tokens(),
            capability.max_input_tokens
        )));
    }
    if document_spec.instruction().is_some() && !capability.supports_instruction {
        return Err(config_error(
            "document spec declares an instruction but the model declares \
             supports_instruction = false; remove the instruction or correct the \
             capability declaration (支持差异不能吞)",
        ));
    }
    if query_spec.instruction().is_some() && !capability.supports_instruction {
        return Err(config_error(
            "query spec declares an instruction but the model declares \
             supports_instruction = false; remove the instruction or correct the \
             capability declaration (支持差异不能吞)",
        ));
    }
    Ok(())
}

/// Resolves the declared configuration into a validated frozen space and a
/// capability sheet. Requires `enabled`; every capability-relevant field is
/// required — missing keys are config errors naming the key (no generous
/// silent defaults). This function never reads the environment and never
/// resolves `api_key_ref` (credential policy is P7-007).
pub fn resolve_provider(
    config: &SemanticProviderConfig,
) -> CcResult<(VectorSpace, ModelCapability)> {
    if !config.enabled {
        return Err(config_error(
            "semantic.enabled is false: the semantic provider is disabled and its \
             configuration is inert; enable it to resolve a provider",
        ));
    }
    if config.model_id.is_empty() {
        return Err(config_error(
            "semantic.model_id is required when semantic.enabled is true",
        ));
    }
    let dimensions = config.dimensions.ok_or_else(|| {
        config_error("semantic.dimensions is required when semantic.enabled is true")
    })?;
    let max_input_tokens = config.max_input_tokens.ok_or_else(|| {
        config_error(
            "semantic.max_input_tokens is required when semantic.enabled is true \
             (declare the model's real per-input token limit)",
        )
    })?;
    let max_batch_items = config.max_batch_items.ok_or_else(|| {
        config_error(
            "semantic.max_batch_items is required when semantic.enabled is true \
             (declare the endpoint's real batch limit)",
        )
    })?;

    let metric = DistanceMetricAdapter::parse(&config.metric)?;
    let dimensions_mode = DimensionsMode::parse(&config.dimensions_mode)?;
    let mut encoding_formats = Vec::with_capacity(config.encoding_formats.len());
    for format in &config.encoding_formats {
        encoding_formats.push(EncodingFormat::parse(format)?);
    }

    let space = VectorSpace::new(config.model_id.clone(), dimensions)
        .map_err(|error| config_error(format!("semantic configuration rejected: {error}")))?;
    let capability = ModelCapability {
        model_id: config.model_id.clone(),
        dimensions,
        metric,
        dimensions_mode,
        max_input_tokens,
        max_batch_items,
        encoding_formats,
        supports_instruction: config.supports_instruction,
    };
    capability.validate_bounds()?;
    validate_capability_metric_vs_space(&capability, &space)?;
    Ok((space, capability))
}

/// v1 gate: the declared metric must be the frozen admitted metric.
/// (Kept as a named function so a future `DistanceMetric` variant fails to
/// compile here until explicitly re-admitted — same pattern as
/// `VectorSpace::validate`.)
fn validate_capability_metric_vs_space(
    capability: &ModelCapability,
    space: &VectorSpace,
) -> CcResult<()> {
    match capability.metric {
        DistanceMetric::Cosine => {
            if space.distance() != DistanceMetric::Cosine {
                return Err(config_error(
                    "semantic.metric must match the frozen space metric (cosine)",
                ));
            }
            Ok(())
        }
    }
}

/// Parse adapter for the config-level metric string.
struct DistanceMetricAdapter;

impl DistanceMetricAdapter {
    fn parse(value: &str) -> CcResult<DistanceMetric> {
        match value.trim().to_lowercase().as_str() {
            "cosine" => Ok(DistanceMetric::Cosine),
            other => Err(config_error(format!(
                "semantic.metric {other:?} is not supported; the frozen encoding \
                 spec admits only \"cosine\""
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{DocumentInput, EmbeddingProvider, ProviderError};
    use crate::providers::openai_compatible::{HttpResponse, TransportError};
    use crate::spec::{DocumentEncodingSpec, QueryEncodingSpec};
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    // ── Mock transport (request recording + scripted responses) ──────────

    #[derive(Default)]
    struct ProbeMockTransport {
        scripted: StdMutex<VecDeque<Result<HttpResponse, TransportError>>>,
        requests: StdMutex<Vec<HttpRequest>>,
    }

    impl ProbeMockTransport {
        fn with_ok(responses: Vec<HttpResponse>) -> Self {
            let mock = Self::default();
            mock.scripted
                .lock()
                .unwrap()
                .extend(responses.into_iter().map(Ok));
            mock
        }

        fn embeddings_response(
            status: u16,
            model: &str,
            dimension: usize,
            count: usize,
        ) -> HttpResponse {
            let data: Vec<serde_json::Value> = (0..count)
                .map(|index| {
                    // Distinct non-zero components; deterministic.
                    serde_json::json!({
                        "object": "embedding",
                        "index": index,
                        "embedding": (0..dimension)
                            .map(|c| 0.25f32 * ((index + c) as f32 % 4.0) + 0.5)
                            .collect::<Vec<f32>>(),
                    })
                })
                .collect();
            HttpResponse {
                status,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: serde_json::to_vec(&serde_json::json!({
                    "object": "list",
                    "model": model,
                    "data": data,
                }))
                .unwrap(),
            }
        }
    }

    impl EmbeddingHttpTransport for ProbeMockTransport {
        fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.lock().unwrap().push(request);
            self.scripted
                .lock()
                .unwrap()
                .pop_front()
                .expect("test scripted one response per probe request")
        }
    }

    // ── Fixtures ─────────────────────────────────────────────────────────

    const MODEL: &str = "text-embedding-test-001";
    const DIM: u32 = 4;

    fn enabled_config() -> SemanticProviderConfig {
        SemanticProviderConfig {
            enabled: true,
            model_id: MODEL.to_owned(),
            dimensions: Some(DIM),
            metric: "cosine".to_owned(),
            dimensions_mode: "configurable".to_owned(),
            endpoint: "https://provider.invalid/v1".to_owned(),
            api_key_ref: Some("env:TEST_KEY".to_owned()),
            max_input_tokens: Some(8_192),
            max_batch_items: Some(3),
            encoding_formats: vec!["float".to_owned()],
            supports_instruction: true,
            ..SemanticProviderConfig::default()
        }
    }

    fn capability() -> ModelCapability {
        ModelCapability {
            model_id: MODEL.to_owned(),
            dimensions: DIM,
            metric: DistanceMetric::Cosine,
            dimensions_mode: DimensionsMode::Configurable,
            max_input_tokens: 8_192,
            max_batch_items: 3,
            encoding_formats: vec![EncodingFormat::Float],
            supports_instruction: true,
        }
    }

    fn space() -> VectorSpace {
        VectorSpace::new(MODEL, DIM).expect("valid space")
    }

    fn doc_spec(space: &VectorSpace) -> DocumentEncodingSpec {
        DocumentEncodingSpec::new(space.clone(), None, 8_192, "cl100k").expect("valid doc spec")
    }

    fn query_spec(space: &VectorSpace) -> QueryEncodingSpec {
        QueryEncodingSpec::new(
            space.clone(),
            Some("code search query".into()),
            512,
            "cl100k",
        )
        .expect("valid query spec")
    }

    fn ok_probe_responses(batch: usize) -> Vec<HttpResponse> {
        vec![
            ProbeMockTransport::embeddings_response(200, MODEL, DIM as usize, 1),
            ProbeMockTransport::embeddings_response(200, MODEL, DIM as usize, batch),
        ]
    }

    fn prober(transport: Arc<ProbeMockTransport>) -> CapabilityProber {
        CapabilityProber::new(
            transport,
            "https://provider.invalid/v1",
            EmbeddingApiKey::new("sk-probe-secret"),
        )
        .expect("valid prober")
    }

    // ── Consistency validation: positive ─────────────────────────────────

    #[test]
    fn capability_matching_space_and_specs_passes() {
        let s = space();
        validate_capability(&capability(), &s, &doc_spec(&s), &query_spec(&s))
            .expect("consistent capability must pass");
    }

    #[test]
    fn resolve_provider_builds_a_space_with_the_frozen_identity() {
        let (space, resolved) = resolve_provider(&enabled_config()).expect("resolve");
        assert_eq!(space.model_id(), MODEL);
        assert_eq!(space.dimension(), DIM);
        assert_eq!(space.distance(), DistanceMetric::Cosine);
        assert_eq!(
            space.digest().expect("digest"),
            VectorSpace::new(MODEL, DIM)
                .expect("reference space")
                .digest()
                .expect("reference digest")
        );
        assert_eq!(resolved, capability());
    }

    // ── Consistency validation: negative (every mismatch is a config error) ─

    fn assert_config_error(result: CcResult<()>, needle: &str) {
        match result {
            Err(CcError::Config(message)) => assert!(
                message.contains(needle),
                "message {message:?} must name {needle:?}"
            ),
            other => panic!("expected a config error naming {needle:?}, got {other:?}"),
        }
    }

    #[test]
    fn capability_model_mismatch_is_a_config_error() {
        let s = space();
        let mut cap = capability();
        cap.model_id = "other/model-999".to_owned();
        assert_config_error(
            validate_capability(&cap, &s, &doc_spec(&s), &query_spec(&s)),
            "semantic.model_id",
        );
    }

    #[test]
    fn capability_dimension_mismatch_is_a_config_error() {
        let s = space();
        let mut cap = capability();
        cap.dimensions = 8;
        assert_config_error(
            validate_capability(&cap, &s, &doc_spec(&s), &query_spec(&s)),
            "semantic.dimensions",
        );
    }

    #[test]
    fn specs_bound_to_a_foreign_space_are_rejected() {
        let foreign = VectorSpace::new(MODEL, DIM * 2).expect("foreign space");
        let s = space();
        assert_config_error(
            validate_capability(&capability(), &s, &doc_spec(&foreign), &query_spec(&s)),
            "document encoding spec",
        );
        assert_config_error(
            validate_capability(&capability(), &s, &doc_spec(&s), &query_spec(&foreign)),
            "query encoding spec",
        );
    }

    #[test]
    fn spec_max_tokens_above_the_model_limit_is_rejected_on_both_paths() {
        let s = space();
        let mut cap = capability();
        cap.max_input_tokens = 1_024;
        let wide_doc =
            DocumentEncodingSpec::new(s.clone(), None, 4_096, "cl100k").expect("doc spec");
        // Within-limit doc spec so the query-side check is the one that fires.
        let narrow_doc =
            DocumentEncodingSpec::new(s.clone(), None, 512, "cl100k").expect("doc spec");
        let wide_query =
            QueryEncodingSpec::new(s.clone(), None, 4_096, "cl100k").expect("query spec");
        assert_config_error(
            validate_capability(&cap, &s, &wide_doc, &query_spec(&s)),
            "document spec max_tokens",
        );
        assert_config_error(
            validate_capability(&cap, &s, &narrow_doc, &wide_query),
            "query spec max_tokens",
        );
    }

    #[test]
    fn instruction_without_support_is_rejected_on_both_paths() {
        let s = space();
        let mut cap = capability();
        cap.supports_instruction = false;
        assert_config_error(
            validate_capability(&cap, &s, &doc_spec(&s), &query_spec(&s)),
            "query spec declares an instruction",
        );

        let instructed_doc =
            DocumentEncodingSpec::new(s.clone(), Some("doc instruction".into()), 8_192, "cl100k")
                .expect("doc spec");
        assert_config_error(
            validate_capability(&cap, &s, &instructed_doc, &query_spec(&s)),
            "document spec declares an instruction",
        );

        // Without any instruction the capability is fine.
        let plain_doc = DocumentEncodingSpec::new(s.clone(), None, 8_192, "cl100k").expect("doc");
        let plain_query = QueryEncodingSpec::new(s.clone(), None, 512, "cl100k").expect("query");
        validate_capability(&cap, &s, &plain_doc, &plain_query).expect("no instruction, no clash");
    }

    #[test]
    fn missing_float_encoding_support_is_a_config_error() {
        let s = space();
        let mut cap = capability();
        cap.encoding_formats = Vec::new();
        // Bounds already reject an empty sheet naming the required format.
        assert_config_error(
            validate_capability(&cap, &s, &doc_spec(&s), &query_spec(&s)),
            "\"float\"",
        );
        // A sheet with a non-float entry only is rejected by the same gate.
        cap.encoding_formats = vec![EncodingFormat::Float];
        validate_capability(&cap, &s, &doc_spec(&s), &query_spec(&s))
            .expect("float support satisfies the gate");
    }

    // ── resolve_provider: explicit rejection of every support difference ──

    #[test]
    fn disabled_config_is_rejected_by_resolve() {
        let mut config = enabled_config();
        config.enabled = false;
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.enabled is false",
        );
    }

    #[test]
    fn missing_required_fields_name_their_config_keys() {
        let mut config = enabled_config();
        config.dimensions = None;
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.dimensions is required",
        );

        let mut config = enabled_config();
        config.max_input_tokens = None;
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.max_input_tokens is required",
        );

        let mut config = enabled_config();
        config.max_batch_items = None;
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.max_batch_items is required",
        );

        let mut config = enabled_config();
        config.model_id = String::new();
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.model_id is required",
        );
    }

    #[test]
    fn unknown_metric_mode_and_format_strings_are_rejected_not_swallowed() {
        let mut config = enabled_config();
        config.metric = "euclidean".to_owned();
        assert_config_error(resolve_provider(&config).map(|_| ()), "semantic.metric");

        let mut config = enabled_config();
        config.dimensions_mode = "auto".to_owned();
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.dimensions_mode",
        );

        let mut config = enabled_config();
        config.encoding_formats = vec!["base64".to_owned()];
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.encoding_formats",
        );
    }

    #[test]
    fn batch_items_outside_the_probe_protocol_bound_are_rejected() {
        let mut config = enabled_config();
        config.max_batch_items = Some(0);
        assert_config_error(
            resolve_provider(&config).map(|_| ()),
            "semantic.max_batch_items",
        );

        let mut config = enabled_config();
        config.max_batch_items = Some(MAX_PROBE_BATCH_ITEMS + 1);
        assert_config_error(resolve_provider(&config).map(|_| ()), "probe protocol");
    }

    #[test]
    fn prober_rejects_bad_endpoints_as_config_errors() {
        let transport = Arc::new(ProbeMockTransport::default());
        let error =
            CapabilityProber::new(transport.clone(), "ftp://nope", EmbeddingApiKey::new("k"))
                .unwrap_err();
        assert!(matches!(error, CcError::Config(message) if message.contains("http(s)")));
        let error = CapabilityProber::new(transport, "", EmbeddingApiKey::new("k")).unwrap_err();
        assert!(matches!(error, CcError::Config(message) if message.contains("non-empty")));
    }

    // ── Probe protocol v1 (mock full chain, zero real network) ───────────

    #[test]
    fn dimension_probe_configurable_sends_the_dimensions_field() {
        let transport = Arc::new(ProbeMockTransport::with_ok(ok_probe_responses(3)));
        let prober = prober(transport.clone());
        let verdict = prober.probe(&space(), &capability()).expect("probe");
        assert_eq!(verdict.model_id, MODEL);
        assert_eq!(verdict.dimension, DIM);
        assert_eq!(verdict.protocol, PROBE_PROTOCOL_VERSION);
        assert_eq!(
            verdict.space_digest,
            space().digest().expect("digest"),
            "verdict is keyed by the frozen space digest"
        );

        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "one dimension probe + one batch probe");
        let dimension_request = &requests[0];
        assert_eq!(
            dimension_request.url,
            "https://provider.invalid/v1/embeddings"
        );
        let body: serde_json::Value = serde_json::from_slice(&dimension_request.body).unwrap();
        assert_eq!(body["dimensions"], serde_json::json!(DIM));
        assert_eq!(body["encoding_format"], "float");
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["input"].as_array().unwrap().len(), 1);
        assert!(
            dimension_request
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                    && v == "Bearer sk-probe-secret")
        );

        let batch_request = &requests[1];
        let body: serde_json::Value = serde_json::from_slice(&batch_request.body).unwrap();
        assert_eq!(
            body["input"].as_array().unwrap().len(),
            3,
            "declared batch limit"
        );
    }

    #[test]
    fn dimensions_unsupported_maps_to_the_typed_verdict_via_the_control_probe() {
        // Stub that 400s exactly when the request carries `dimensions` —
        // the acceptance case "供应商不支持dimensions时不伪成功".
        struct NoDimensionsStub {
            requests: StdMutex<Vec<HttpRequest>>,
        }
        impl EmbeddingHttpTransport for NoDimensionsStub {
            fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
                let carries_dimensions = serde_json::from_slice::<serde_json::Value>(&request.body)
                    .ok()
                    .and_then(|body| body.get("dimensions").cloned())
                    .is_some();
                self.requests.lock().unwrap().push(request);
                if carries_dimensions {
                    Ok(HttpResponse {
                        status: 400,
                        headers: Vec::new(),
                        body: b"unknown parameter".to_vec(),
                    })
                } else {
                    Ok(ProbeMockTransport::embeddings_response(
                        200,
                        MODEL,
                        DIM as usize,
                        1,
                    ))
                }
            }
        }

        let transport = Arc::new(NoDimensionsStub {
            requests: StdMutex::new(Vec::new()),
        });
        let prober = CapabilityProber::new(
            transport.clone(),
            "https://provider.invalid/v1",
            EmbeddingApiKey::new("sk-probe-secret"),
        )
        .expect("valid prober");
        let verdict = prober.probe(&space(), &capability()).unwrap_err();
        assert_eq!(verdict, CapabilityProbeError::DimensionsUnsupported);
        // Two dimension probes (with + control), never the batch probe.
        assert_eq!(transport.requests.lock().unwrap().len(), 2);

        // The config path names the remediation, and it is a config error —
        // not a fake success.
        let error = verdict.clone().into_config_error();
        assert!(matches!(error, CcError::Config(message) if message.contains("dimensions_mode")));
        assert!(!verdict.is_mismatch());
    }

    #[test]
    fn fixed_mode_probe_omits_the_dimensions_field() {
        let transport = Arc::new(ProbeMockTransport::with_ok(ok_probe_responses(3)));
        let prober = prober(transport.clone());
        let mut cap = capability();
        cap.dimensions_mode = DimensionsMode::Fixed;
        prober.probe(&space(), &cap).expect("fixed-mode probe");
        for request in transport.requests.lock().unwrap().iter() {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert!(
                body.get("dimensions").is_none(),
                "fixed mode must not send the field"
            );
        }
    }

    #[test]
    fn probe_request_bodies_are_exactly_the_declared_egress_surface() {
        // P7-007 audit point for the probe path: configurable mode declares
        // (and sends) exactly {model, input, encoding_format, dimensions};
        // fixed mode exactly {model, input, encoding_format}. Any extra
        // field would be an undeclared outbound surface.
        let transport = Arc::new(ProbeMockTransport::with_ok(ok_probe_responses(3)));
        prober(transport.clone())
            .probe(&space(), &capability())
            .expect("configurable probe");
        for request in transport.requests.lock().unwrap().iter() {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let mut keys: Vec<&str> = body
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec!["dimensions", "encoding_format", "input", "model"]
            );
        }

        let transport = Arc::new(ProbeMockTransport::with_ok(ok_probe_responses(3)));
        let mut cap = capability();
        cap.dimensions_mode = DimensionsMode::Fixed;
        prober(transport.clone())
            .probe(&space(), &cap)
            .expect("fixed probe");
        for request in transport.requests.lock().unwrap().iter() {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let mut keys: Vec<&str> = body
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, vec!["encoding_format", "input", "model"]);
        }
    }

    #[test]
    fn wrong_dimension_in_the_response_is_a_mismatch() {
        let transport = Arc::new(ProbeMockTransport::with_ok(vec![
            ProbeMockTransport::embeddings_response(200, MODEL, 8, 1),
        ]));
        let prober = prober(transport);
        let mut cap = capability();
        cap.dimensions_mode = DimensionsMode::Fixed;
        let verdict = prober.probe(&space(), &cap).unwrap_err();
        assert!(
            verdict.is_mismatch(),
            "declared dimension contradicted: {verdict:?}"
        );
        assert!(matches!(
            verdict.into_config_error(),
            CcError::Config(message) if message.contains("capability probe contradicts")
        ));
    }

    #[test]
    fn model_echo_mismatch_is_a_mismatch() {
        let transport = Arc::new(ProbeMockTransport::with_ok(vec![
            ProbeMockTransport::embeddings_response(200, "other/model-999", DIM as usize, 1),
        ]));
        let verdict = prober(transport)
            .probe(&space(), &capability())
            .unwrap_err();
        assert!(verdict.is_mismatch());
    }

    #[test]
    fn batch_count_mismatch_is_unavailable_not_a_mismatch() {
        // Batch probe scripted to return 2 entries for a declared limit of 3:
        // the response shape is valid but the *limit claim* is unverifiable.
        let transport = Arc::new(ProbeMockTransport::with_ok(vec![
            ProbeMockTransport::embeddings_response(200, MODEL, DIM as usize, 1),
            ProbeMockTransport::embeddings_response(200, MODEL, DIM as usize, 2),
        ]));
        let verdict = prober(transport)
            .probe(&space(), &capability())
            .unwrap_err();
        assert!(matches!(verdict, CapabilityProbeError::Unavailable(_)));
    }

    #[test]
    fn transport_failures_map_to_unavailable_with_the_provider_taxonomy() {
        let cases = [
            TransportError::Timeout,
            TransportError::Cancelled,
            TransportError::Io("connection reset".to_owned()),
        ];
        for transport_error in cases {
            let transport = Arc::new(
                ProbeMockTransport::with_ok(Vec::new()).into_scripted_error(transport_error),
            );
            let verdict = prober(transport)
                .probe(&space(), &capability())
                .unwrap_err();
            match verdict {
                CapabilityProbeError::Unavailable(ProviderError::Timeout) => {}
                CapabilityProbeError::Unavailable(ProviderError::Cancelled) => {}
                CapabilityProbeError::Unavailable(ProviderError::ServerError) => {}
                other => panic!("expected Unavailable, got {other:?}"),
            }
        }
    }

    impl ProbeMockTransport {
        fn into_scripted_error(self, error: TransportError) -> Self {
            self.scripted.lock().unwrap().push_back(Err(error));
            self
        }
    }

    // ── Capability cache semantics ────────────────────────────────────────

    #[test]
    fn cached_verdicts_avoid_the_transport_and_failures_are_never_cached() {
        let transport = Arc::new(ProbeMockTransport::with_ok(ok_probe_responses(3)));
        let prober = prober(transport.clone());
        let cache = CapabilityCache::new();
        let s = space();
        let cap = capability();

        let first = verify_capability(&prober, Some(&cache), &s, &cap, false).expect("first probe");
        assert_eq!(transport.requests.lock().unwrap().len(), 2);

        // Cache hit: no additional transport calls.
        let second = verify_capability(&prober, Some(&cache), &s, &cap, false).expect("cached");
        assert_eq!(second, first);
        assert_eq!(transport.requests.lock().unwrap().len(), 2);

        // Force re-probes and refreshes.
        transport
            .scripted
            .lock()
            .unwrap()
            .extend(ok_probe_responses(3).into_iter().map(Ok));
        verify_capability(&prober, Some(&cache), &s, &cap, true).expect("forced probe");
        assert_eq!(transport.requests.lock().unwrap().len(), 4);

        // A failing probe leaves the cache untouched (verdict still
        // available from the earlier success, cache size unchanged).
        let failing =
            Arc::new(ProbeMockTransport::default().into_scripted_error(TransportError::Timeout));
        let failing_prober = CapabilityProber::new(
            failing,
            "https://provider.invalid/v1",
            EmbeddingApiKey::new("sk-probe-secret"),
        )
        .expect("valid prober");
        let verdict = verify_capability(&failing_prober, Some(&cache), &s, &cap, true).unwrap_err();
        assert!(matches!(verdict, CapabilityProbeError::Unavailable(_)));
        assert_eq!(cache.len(), 1, "failures never enter the cache");
    }

    #[test]
    fn a_verdict_cannot_be_filed_under_a_foreign_space() {
        let cache = CapabilityCache::new();
        let verified = VerifiedCapability {
            model_id: MODEL.to_owned(),
            dimension: DIM,
            space_digest: space().digest().expect("digest"),
            protocol: PROBE_PROTOCOL_VERSION,
        };
        cache.insert(&verified);
        let foreign = VectorSpace::new("other/model", DIM).expect("foreign space");
        assert!(cache.get(&foreign).is_none());
        assert_eq!(cache.len(), 1);
    }

    // ── Credential hygiene ────────────────────────────────────────────────

    #[test]
    fn probe_errors_never_leak_the_api_key() {
        let key = "sk-probe-secret-value";
        let transport = Arc::new(
            ProbeMockTransport::default()
                .into_scripted_error(TransportError::Io("connection reset".to_owned())),
        );
        let prober = CapabilityProber::new(
            transport,
            "https://provider.invalid/v1",
            EmbeddingApiKey::new(key),
        )
        .expect("prober");
        let verdict = prober.probe(&space(), &capability()).unwrap_err();
        let rendered = format!("{verdict:?}");
        assert!(!rendered.contains(key), "leaked key in: {rendered}");
        let config_error = format!("{:?}", verdict.into_config_error());
        assert!(!config_error.contains(key), "leaked key in: {config_error}");
    }

    // ── Integration with the P7-001 adapter (same space, same gate) ───────

    #[test]
    fn adapter_embeds_after_capability_probe_over_the_same_mock_chain() {
        use crate::providers::openai_compatible::{
            OpenAiCompatibleConfig, OpenAiCompatibleProvider,
        };

        // One mock serves both the probe (2 requests) and the adapter (1).
        let transport = Arc::new(ProbeMockTransport::with_ok(ok_probe_responses(3)));
        // The embed response is scripted after the probe responses.
        transport
            .scripted
            .lock()
            .unwrap()
            .push_back(Ok(ProbeMockTransport::embeddings_response(
                200,
                MODEL,
                DIM as usize,
                1,
            )));

        let (resolved_space, cap) = resolve_provider(&enabled_config()).expect("resolve");
        let s = resolved_space.clone();
        validate_capability(&cap, &s, &doc_spec(&s), &query_spec(&s)).expect("consistent");

        let cache = CapabilityCache::new();
        let prober = prober(transport.clone());
        let verified = verify_capability(&prober, Some(&cache), &s, &cap, false).expect("verified");

        let config = OpenAiCompatibleConfig {
            space: s.clone(),
            endpoint: "https://provider.invalid/v1".to_owned(),
            api_key: EmbeddingApiKey::new("sk-probe-secret"),
            timeout: Duration::from_secs(5),
            norm_policy: NormPolicy::Accept,
            transport: Some(transport.clone() as Arc<dyn EmbeddingHttpTransport>),
            egress: crate::policy::EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        };
        let adapter = OpenAiCompatibleProvider::new(config);
        let input = DocumentInput::from_bytes(b"payload after verification").expect("input");
        let vectors = adapter.embed_documents(&[input]).expect("embed");
        assert_eq!(vectors[0].len(), verified.dimension as usize);
        assert_eq!(vectors[0].len(), s.dimension() as usize);
        assert_eq!(transport.requests.lock().unwrap().len(), 3);
    }
}
