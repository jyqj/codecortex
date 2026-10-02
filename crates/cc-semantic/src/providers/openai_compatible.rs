//! OpenAI-compatible embedding provider adapter (P7-001).
//!
//! Adapts the `/embeddings` endpoint semantics of OpenAI-compatible servers
//! onto the frozen [`crate::ports::EmbeddingProvider`] port. The port itself
//! (`crates/cc-semantic/src/ports.rs`) is an owner-owned freeze surface and
//! is **not modified** by this module.
//!
//! ## Network boundary (user decision D1/D2, this round)
//!
//! **No real provider calls are authorized this round.** All HTTP I/O goes
//! through the injectable [`EmbeddingHttpTransport`] trait; the shipped
//! default is *no transport* ([`OpenAiCompatibleConfig::transport`] is
//! `Option<_>` = `None`), and a provider constructed without a transport is
//! **fail-closed disabled**: every embed call returns
//! [`ProviderError::InvalidInput`] naming the disabled state. No production
//! transport implementation ships in this round (the live leg is
//! *conditional blocked*); tests inject an in-memory mock transport and never
//! touch the network. This preserves the P6-020 no-network closure at the
//! crate dependency level: `cc-semantic` gains **zero new dependencies** for
//! this adapter (no HTTP client crate).
//!
//! When a real transport is later authorized it implements
//! [`EmbeddingHttpTransport`] (one POST-JSON method) and is injected at the
//! composition root; nothing in this module changes.
//!
//! ## Request/response model
//!
//! POST `{endpoint}/embeddings` with
//! `{"model": <space.model_id()>, "input": [<utf-8 text>...],
//! "encoding_format": "float"}`, `Authorization: Bearer <api_key>`. The
//! configured model is `space.model_id()` — the model is part of the frozen
//! space identity, so it cannot drift between request and cache key. The
//! document and query paths use the same endpoint shape (asymmetric
//! instruction-prefixed models are a later-round concern; `DocumentInput`
//! and `QueryInput` bytes are already validated UTF-8 by their sanctioned
//! constructors).
//!
//! ## Error mapping (six frozen `ProviderError` variants)
//!
//! | Condition                                              | Variant |
//! |--------------------------------------------------------|---------|
//! | transport not configured (disabled, fail-closed)        | `InvalidInput` (non-retryable config error; message names the disabled state) |
//! | transport timeout                                       | `Timeout` |
//! | transport cancelled                                     | `Cancelled` |
//! | transport I/O / connection failure                      | `ServerError` (retryable) |
//! | HTTP 429                                                | `RateLimited` (`Retry-After` seconds header honored; 1s fallback) |
//! | HTTP 401 / 403                                          | `AuthError` |
//! | HTTP 5xx                                                | `ServerError` |
//! | other HTTP 4xx (400/404/413/422/…)                      | `InvalidInput` (non-retryable; only the status code is carried, response bodies are never embedded in errors) |
//! | malformed / structurally invalid response body          | `InvalidInput` |
//! | count mismatch (`data` length ≠ batch length)           | `InvalidInput` (checked *before* any entry is parsed — breadth bombs die at the gate) |
//! | index set ≠ exactly `0..n` (duplicates, gaps, negative) | `InvalidInput` |
//! | dimension ≠ `space.dimension()`                         | `InvalidInput` |
//! | response `model` echo missing, non-string, or ≠ request | `InvalidInput` |
//! | NaN / Inf / zero vector / non-numeric component         | `InvalidInput` |
//! | L2 norm outside the configured [`NormPolicy`] range     | `InvalidInput` |
//! | HTTP 3xx (redirect)                                     | `InvalidInput` (endpoint misconfiguration; redirects are never followed) |
//! | HTTP 1xx or a status outside 100..=599                  | `ServerError` (retryable; the provider/proxy violated HTTP semantics) |
//! | 2xx without `Content-Type: application/json`            | `InvalidInput` |
//! | 2xx body above the derived size bound                   | `InvalidInput` (rejected before JSON parsing) |
//! | `usage` present but not an object                       | `InvalidInput` |
//! | fractional / negative / string `index`                  | `InvalidInput` (field types are strict, not coerced) |
//! | input bytes not valid UTF-8                             | `InvalidInput` (before any transport call) |
//!
//! ## Response strong validation (P7-004)
//!
//! The shared gate [`parse_embeddings_response`] validates every response at
//! four levels, in a fixed diagnostic order:
//!
//! 1. **Protocol**: only a 2xx status enters the body gate, and only with a
//!    JSON content type (media type matched case-insensitively before any
//!    `;` parameter, so `application/json; charset=utf-8` passes). Error
//!    statuses keep the six-variant mapping; for non-401/403/429 4xx the
//!    OpenAI error envelope `{"error": {"type", "code", ...}}` is recognized
//!    and only the short enum-like `type`/`code` fields (capped at
//!    [`MAX_ERROR_FIELD_CHARS`] chars) may join the diagnostic — the free-form
//!    `message` field is *never* surfaced because servers echo request text
//!    there. JSON nesting deeper than serde_json's built-in 128-level limit
//!    fails parsing and is rejected like any malformed body (depth-bomb
//!    defense; also covered by a dedicated test).
//! 2. **Security**: the 2xx body is size-capped by a bound *derived from the
//!    request itself* (`count × (dimension × 64 bytes/component + 256 bytes
//!    entry overhead) + 1024 bytes slack`) and checked **before** parsing, so
//!    an oversized body never costs parse work; the `data` count is checked
//!    **before** any entry is decoded, so a breadth bomb (millions of entries
//!    for a one-item batch) dies at the count gate. Deep JSON is handled by
//!    serde_json's 128-level recursion limit. No logging exists in this
//!    module and response bodies, request bodies, and the API key never
//!    appear in any error message (only short structural facts: lengths,
//!    indices, status codes, field types).
//! 3. **Schema**: field types are strict — `model` must be a string, `data`
//!    an array of objects, `index` a non-negative integer (`0.5` and `"0"`
//!    are rejected, not coerced), `embedding` an array of JSON numbers
//!    (base64-string embeddings are rejected), `usage` — when present — an
//!    object. Unknown extra fields are tolerated (OpenAI-compatible servers
//!    routinely add fields).
//! 4. **Semantic**: the `model` echo is *required* and must equal the
//!    configured model; every vector must have exactly `space.dimension()`
//!    finite f32 components (a finite f64 above the f32 range overflows the
//!    cast and is rejected), must not be all-zero, and — when the configured
//!    [`NormPolicy`] says so — must carry an L2 norm inside the configured
//!    inclusive range; the index set must be exactly `0..n` (out-of-order is
//!    reordered by index, gaps/duplicates rejected).
//!
//! Any single violation rejects the **whole batch** with a non-retryable
//! `ProviderError::InvalidInput` (one-bad-vector-rejects-the-batch, per the
//! P7 planning brief: bad vectors never reach the cache, and a partially
//! valid batch is not worth partial payment). A rejection therefore can never
//! produce a cache write: the [`crate::cache::ArtifactCache::put`] path only
//! runs on `Ok` output, which the strong gate only emits for fully valid
//! responses (`error_vectors_never_reach_the_artifact_cache` pins this).
//!
//! Error messages never contain the API key, request bodies, or response
//! bodies. This module performs no logging at all.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::policy::{EgressPolicy, GuardedTransport};
use crate::ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput};
use crate::types::VectorSpace;

/// Injected credential (P7-001: credentials only ever arrive through
/// configuration; never hardcoded, never logged). [`fmt::Debug`] is the only
/// formatting impl and prints a fixed redaction marker, so the key cannot
/// leak through `{:?}`; there is deliberately no `Display` impl.
#[derive(Clone, PartialEq, Eq)]
pub struct EmbeddingApiKey(String);

impl EmbeddingApiKey {
    /// Wraps a caller-supplied secret. The caller is responsible for
    /// sourcing it from configuration (env/file/secret store) — this type
    /// never reads the environment itself.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// The only way to read the key back; the adapter uses it exactly once,
    /// to build the `Authorization` header. Callers should treat the return
    /// value as a secret in their own scope too.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EmbeddingApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Fixed redaction: the key material never reaches Debug output.
        f.write_str("EmbeddingApiKey([REDACTED])")
    }
}

/// An outbound HTTP request produced by the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Absolute URL, `endpoint + "/embeddings"`.
    pub url: String,
    /// Header list including `Authorization: Bearer …` (do not log).
    pub headers: Vec<(String, String)>,
    /// JSON request body.
    pub body: Vec<u8>,
    /// Deadline the transport must honor for this single request.
    pub timeout: Duration,
}

/// An inbound HTTP response consumed by the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code (2xx success).
    pub status: u16,
    /// Response headers (case-insensitive lookup for `Retry-After`).
    pub headers: Vec<(String, String)>,
    /// Response body bytes.
    pub body: Vec<u8>,
}

/// Transport-level failure, before any response semantics are known. This is
/// the seam's own taxonomy; [`ProviderError`] mapping happens in the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Connection/IO failure (DNS, TLS, reset, …).
    Io(String),
    /// The per-request deadline elapsed.
    Timeout,
    /// The caller's cancellation propagated.
    Cancelled,
}

/// Injectable HTTP transport seam. One method: a JSON POST. Implementations
/// must honor [`HttpRequest::timeout`] and must not retry (retry policy is a
/// caller concern). `Send + Sync` so the provider stays `Send + Sync`.
///
/// **No implementation ships in this round** (D1/D2: real provider calls are
/// conditional blocked). Tests provide an in-memory mock; a future real
/// transport is a standalone impl of this trait injected at the composition
/// root.
///
/// ## Transport contract (P7-007 egress policy; binding for every
/// implementation)
///
/// 1. **Deadline**: the per-request [`HttpRequest::timeout`] is mandatory —
///    the call must fail (e.g. [`TransportError::Timeout`]) once it elapses;
///    a transport without a deadline is a contract violation. (The policy
///    guard wraps every injected transport and refuses zero timeouts.)
/// 2. **Redirects are never followed.** A 3xx answer must be surfaced as-is
///    (the adapter rejects it as non-retryable `InvalidInput`). If the
///    underlying HTTP client cannot disable auto-follow, an implementation
///    MUST at minimum strip the `Authorization` header before any redirect
///    hop and MUST still surface the final 3xx status — a redirect response
///    must never produce embeddings, because following one silently changes
///    the credential-bearing target URL.
/// 3. **No retries**: retry policy is a caller concern
///    ([`RetryingProvider`]); a transport that retries breaks the attempt
///    bounds and the circuit-breaker accounting.
/// 4. **No logging** of request headers or bodies — the `Authorization`
///    header carries the credential and the body carries user input text.
pub trait EmbeddingHttpTransport: Send + Sync {
    fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// Semantic-level L2 norm policy applied by the strong response gate
/// (P7-004; the planning brief's "norm 策略可配"). The frozen zero-vector
/// rejection always applies regardless of this policy.
#[derive(Default, Debug, Clone, PartialEq)]
pub enum NormPolicy {
    /// No range admission: only the always-on structural checks (finite,
    /// non-zero, dimension) constrain vectors. The default, preserving the
    /// P7-001 gate semantics.
    #[default]
    Accept,
    /// Reject any vector whose L2 norm falls outside the **inclusive** range
    /// `[min, max]`. Intended for unit-normalized spaces (e.g.
    /// `min = max = 1.0`, or a small tolerance band); the norm is computed in
    /// f64 so a legitimate vector of finite f32 components can never overflow
    /// the accumulation.
    RejectOutside { min: f32, max: f32 },
}

impl NormPolicy {
    /// Structural validation of the range itself: both bounds finite and
    /// non-negative (a norm is never negative), `min <= max`. Called eagerly
    /// by the provider constructor so a bad policy surfaces at composition,
    /// not mid-batch.
    pub fn validate(&self) -> cc_model::CcResult<()> {
        let (min, max) = match self {
            Self::Accept => return Ok(()),
            Self::RejectOutside { min, max } => (*min, *max),
        };
        if !min.is_finite() || !max.is_finite() || min < 0.0 || max < 0.0 || min > max {
            return Err(cc_model::CcError::InvalidParams(format!(
                "NormPolicy::RejectOutside bounds must be finite, non-negative and ordered \
                 (min <= max), got [{min}, {max}]"
            )));
        }
        Ok(())
    }

    fn admits(&self, norm: f64) -> bool {
        match self {
            Self::Accept => true,
            Self::RejectOutside { min, max } => norm >= f64::from(*min) && norm <= f64::from(*max),
        }
    }
}

/// Adapter configuration. All fields are injected by the caller (composition
/// root); nothing is read from the environment here. The default transport
/// is `None` = the provider is disabled and fail-closed.
#[derive(Clone)]
pub struct OpenAiCompatibleConfig {
    /// The frozen space this provider embeds into. `model_id()` is sent as
    /// the request `model`; vectors are exactly `dimension()` long.
    pub space: VectorSpace,
    /// Base URL of the OpenAI-compatible server, e.g. `https://host/v1`
    /// (trailing slashes are trimmed). Never a credential.
    pub endpoint: String,
    /// Injected credential; only ever placed in the `Authorization` header.
    pub api_key: EmbeddingApiKey,
    /// Per-request deadline handed to the transport. Default 30s.
    pub timeout: Duration,
    /// Semantic-level norm admission for response vectors (P7-004). Default
    /// [`NormPolicy::Accept`] keeps the P7-001 gate semantics.
    pub norm_policy: NormPolicy,
    /// The injected HTTP transport. `None` (the default) keeps the provider
    /// **disabled**: every call fails closed with a non-retryable
    /// `InvalidInput` naming the disabled state — no network path exists.
    /// When present, the adapter wraps it in the P7-007
    /// [`crate::policy::GuardedTransport`] at construction.
    pub transport: Option<Arc<dyn EmbeddingHttpTransport>>,
    /// Outbound egress policy (P7-007). Default is the closed state
    /// (`network_opt_in = false`, `allow_http = false`): a transport may not
    /// be assembled without the explicit opt-in and endpoints are https-only.
    pub egress: EgressPolicy,
}

impl fmt::Debug for OpenAiCompatibleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatibleConfig")
            .field("space", &self.space)
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key)
            .field("timeout", &self.timeout)
            .field("egress", &self.egress)
            .field(
                "transport",
                &self.transport.as_ref().map(|_| "<configured>"),
            )
            .finish()
    }
}

impl OpenAiCompatibleConfig {
    /// Defaults for `timeout` (30s), `norm_policy` ([`NormPolicy::Accept`]),
    /// `transport` (`None` = disabled) and `egress` (closed: no network,
    /// https only).
    pub fn new(space: VectorSpace, endpoint: impl Into<String>, api_key: EmbeddingApiKey) -> Self {
        Self {
            space,
            endpoint: endpoint.into(),
            api_key,
            timeout: Duration::from_secs(30),
            norm_policy: NormPolicy::Accept,
            transport: None,
            egress: EgressPolicy::default(),
        }
    }
}

/// OpenAI-compatible adapter onto the frozen [`EmbeddingProvider`] port.
/// Thread-safe (`Send + Sync`) via the `Send + Sync` transport.
#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleProvider {
    /// Sanctioned constructor; validates the space, the endpoint shape, the
    /// norm policy and the egress policy eagerly so a bad configuration can
    /// never surface lazily mid-batch. A transport injected without the
    /// explicit `network_opt_in` (P7-007 default no-network) is a
    /// composition error, fail-fast. A present transport is wrapped in the
    /// [`GuardedTransport`] policy guard, so scheme admission, the mandatory
    /// timeout and redirect rejection hold regardless of the inner
    /// implementation.
    pub fn new(config: OpenAiCompatibleConfig) -> Self {
        config
            .space
            .validate()
            .expect("OpenAiCompatibleConfig space must be a valid frozen VectorSpace");
        config.norm_policy.validate().expect(
            "OpenAiCompatibleConfig norm_policy must be a finite non-negative ordered range",
        );
        let endpoint = config.endpoint.trim_end_matches('/');
        assert!(
            !endpoint.is_empty(),
            "OpenAiCompatibleConfig endpoint must be non-empty"
        );
        assert!(
            endpoint.starts_with("http://") || endpoint.starts_with("https://"),
            "OpenAiCompatibleConfig endpoint must be an http(s) URL"
        );
        config.egress.validate_endpoint(endpoint).expect(
            "OpenAiCompatibleConfig endpoint must pass the egress policy (https default; \
                     semantic.allow_http opts into plaintext)",
        );
        let mut config = config;
        if let Some(transport) = config.transport.take() {
            assert!(
                config.egress.network_opt_in,
                "OpenAiCompatibleConfig: a transport may not be assembled without the explicit \
                 network opt-in (semantic.network_opt_in, P7-007 default no-network)"
            );
            config.transport = Some(Arc::new(GuardedTransport::new(
                transport,
                config.egress.clone(),
            )) as Arc<dyn EmbeddingHttpTransport>);
        }
        Self { config }
    }

    /// Shared pipeline for both port methods: UTF-8 guard → transport call →
    /// status/error mapping → structural response validation. `inputs` is
    /// one text per batch entry, in batch order.
    fn embed(&self, inputs: Vec<&[u8]>) -> Result<Vec<Vec<f32>>, ProviderError> {
        // Empty batch is a no-op on the port contract (parity with the
        // FakeProvider reference): no transport access, no disabled error.
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        // Fail-closed disabled state: no transport means no network path at
        // all, and the caller is told so with a non-retryable error.
        let transport = self.config.transport.as_ref().ok_or_else(|| {
            ProviderError::InvalidInput(
                "openai-compatible provider is disabled: no transport configured \
                 (live provider calls are conditional blocked this round)"
                    .into(),
            )
        })?;
        // The sanctioned constructors bind digests to validated UTF-8 bytes;
        // direct struct-literal construction could still bypass that, so the
        // adapter re-checks before anything leaves the process.
        let mut texts = Vec::with_capacity(inputs.len());
        for (i, bytes) in inputs.iter().enumerate() {
            let text = std::str::from_utf8(bytes).map_err(|_| {
                ProviderError::InvalidInput(format!(
                    "openai-compatible input {i} is not valid UTF-8"
                ))
            })?;
            texts.push(text.to_owned());
        }

        let request = self.build_request(&texts);
        let response = transport.post_json(request).map_err(map_transport_error)?;
        self.parse_response(&response, texts.len())
    }

    /// Builds the outbound request for `texts` (P7-007: public so the
    /// composition root and the policy layer can audit the egress surface
    /// via [`crate::policy::audit_egress`] — the body is exactly
    /// `{"model", "input", "encoding_format"}` and the key travels only in
    /// the `Authorization` header).
    pub fn build_request(&self, texts: &[String]) -> HttpRequest {
        let url = format!("{}/embeddings", self.config.endpoint.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.config.space.model_id(),
            "input": texts,
            "encoding_format": "float",
        });
        // serde_json serialization of a plain JSON value cannot fail.
        let body_bytes = serde_json::to_vec(&body).expect("request serialization cannot fail");
        HttpRequest {
            url,
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                (
                    "Authorization".to_owned(),
                    format!("Bearer {}", self.config.api_key.expose_secret()),
                ),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            body: body_bytes,
            timeout: self.config.timeout,
        }
    }

    /// Status mapping + the full strong-validation gate (P7-004): protocol,
    /// security, schema and semantic levels in a fixed diagnostic order.
    fn parse_response(
        &self,
        response: &HttpResponse,
        expected_count: usize,
    ) -> Result<Vec<Vec<f32>>, ProviderError> {
        parse_embeddings_response(
            &self.config.space,
            response,
            expected_count,
            &self.config.norm_policy,
        )
    }
}

/// Per-component JSON budget used by the derived body-size bound: a f64
/// literal never needs more than ~24 characters; 64 leaves generous headroom
/// for pretty-printed spacing.
const MAX_COMPONENT_JSON_BYTES: usize = 64;
/// Per-`data`-entry overhead (keys, `object`, `index`, punctuation) in the
/// derived body-size bound.
const MAX_ENTRY_OVERHEAD_BYTES: usize = 256;
/// Top-level slack (envelope keys, `usage`, whitespace) in the derived
/// body-size bound.
const MAX_BODY_SLACK_BYTES: usize = 1024;
/// Upper bound on how many characters of a provider error envelope's
/// enum-like `type`/`code` fields may join a diagnostic message. The
/// free-form `message` field is never surfaced at all (servers echo request
/// text there).
const MAX_ERROR_FIELD_CHARS: usize = 128;

/// Derived upper bound for a legitimate 2xx response body, computed from the
/// request itself (`count × (dimension × component budget + entry overhead)`
/// plus envelope slack). No configuration knob: a genuine response always
/// fits because the bound scales with exactly what was asked for.
fn max_response_body_bytes(expected_count: usize, dimension: u32) -> usize {
    expected_count
        .saturating_mul(
            (dimension as usize)
                .saturating_mul(MAX_COMPONENT_JSON_BYTES)
                .saturating_add(MAX_ENTRY_OVERHEAD_BYTES),
        )
        .saturating_add(MAX_BODY_SLACK_BYTES)
}

/// Protocol-level content-type check for 2xx responses: the media type
/// (everything before any `;` parameter) must be `application/json`,
/// case-insensitive.
fn content_type_is_json(response: &HttpResponse) -> bool {
    header(response, "content-type")
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
        .unwrap_or(false)
}

/// Extracts a *safe* diagnostic from an OpenAI-style error envelope
/// `{"error": {"message", "type", "code"}}`: only the short enum-like
/// `type`/`code` string fields may join a message (capped at
/// [`MAX_ERROR_FIELD_CHARS`] chars); the free-form `message` field is
/// deliberately never surfaced because servers echo request text there.
/// Any body that does not parse as that envelope yields an empty string.
fn safe_error_diagnostic(response: &HttpResponse) -> String {
    let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&response.body) else {
        return String::new();
    };
    let Some(fields) = parsed.get("error").and_then(|error| error.as_object()) else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    for key in ["type", "code"] {
        if let Some(value) = fields.get(key).and_then(|value| value.as_str()) {
            if !value.is_empty() {
                let capped: String = value.chars().take(MAX_ERROR_FIELD_CHARS).collect();
                parts.push(format!("{key}={capped:?}"));
            }
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// The shared strong-validation gate (P7-004). Used by the adapter's real
/// embed path and the capability probe (P7-002): both paths exercise the
/// *same* gate so a probe verdict and a real embed call can never disagree
/// about what a valid response is.
///
/// Validation levels and their fixed order — see the module docs for the
/// full contract:
/// 1. protocol: status semantics, 2xx content type, OpenAI error envelope;
/// 2. security: derived body-size bound (pre-parse), `data` count gate
///    (pre-entry), serde_json's 128-level depth limit;
/// 3. schema: strict field types, no coercion;
/// 4. semantic: model echo, dimension, finiteness, non-zero, norm policy,
///    index set exactly `0..n`.
///
/// Any violation rejects the whole batch as non-retryable
/// [`ProviderError::InvalidInput`]; retryable conditions are only ever the
/// provider-side ones (5xx, 429, transport failures).
pub(crate) fn parse_embeddings_response(
    space: &VectorSpace,
    response: &HttpResponse,
    expected_count: usize,
    norm_policy: &NormPolicy,
) -> Result<Vec<Vec<f32>>, ProviderError> {
    // ── Protocol level: HTTP status semantics ─────────────────────────────
    let status = response.status;
    if status == 429 {
        let retry_after = header(response, "retry-after")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(1));
        return Err(ProviderError::RateLimited { retry_after });
    }
    if status == 401 || status == 403 {
        return Err(ProviderError::AuthError);
    }
    if (500..=599).contains(&status) {
        return Err(ProviderError::ServerError);
    }
    // Informational-only or out-of-range statuses are a provider/proxy
    // protocol violation, not a property of our request: retryable.
    if !(100..=599).contains(&status) || (100..200).contains(&status) {
        return Err(ProviderError::ServerError);
    }
    // A redirect means the endpoint configuration is wrong; retrying the
    // same request cannot help and following redirects would silently change
    // the credential-bearing target URL.
    if (300..400).contains(&status) {
        return Err(ProviderError::InvalidInput(format!(
            "openai-compatible endpoint returned a redirect status http {status}; \
             redirects are never followed — fix the endpoint configuration"
        )));
    }
    if !(200..=299).contains(&status) {
        // Remaining 4xx: the request itself is wrong and will not succeed
        // on retry. Only the status code and the error envelope's short
        // enum-like fields enter the message — response bodies (and in
        // particular the error `message` field, which echoes request text)
        // are never embedded in errors.
        let diagnostic = safe_error_diagnostic(response);
        return Err(ProviderError::InvalidInput(format!(
            "openai-compatible endpoint rejected the request with http {status}{diagnostic}"
        )));
    }

    // ── Protocol level: 2xx must declare a JSON body ──────────────────────
    if !content_type_is_json(response) {
        return Err(ProviderError::InvalidInput(
            "openai-compatible success response does not declare a JSON content type".into(),
        ));
    }

    // ── Security level: derived size bound, checked before any parsing ────
    let bound = max_response_body_bytes(expected_count, space.dimension());
    if response.body.len() > bound {
        return Err(ProviderError::InvalidInput(format!(
            "openai-compatible response body of {} bytes exceeds the derived bound of {} bytes \
             (batch of {expected_count} × dimension {})",
            response.body.len(),
            bound,
            space.dimension()
        )));
    }

    // serde_json's built-in 128-level recursion limit also bounds JSON depth
    // here: a deeply nested bomb fails parsing exactly like any malformed
    // body (pinned by a dedicated test).
    let parsed: serde_json::Value = serde_json::from_slice(&response.body).map_err(|_| {
        ProviderError::InvalidInput("openai-compatible response body is not valid JSON".into())
    })?;
    // ── Schema level: strict envelope shape ───────────────────────────────
    let obj = parsed.as_object().ok_or_else(|| {
        ProviderError::InvalidInput("openai-compatible response is not a JSON object".into())
    })?;

    // ── Semantic level: model echo is required and must match ─────────────
    // The model is part of the frozen space identity; a response without an
    // echo is a malformed envelope (capability unknown), a mismatched echo
    // contradicts the declaration (foreign-space vectors must never reach
    // the cache).
    let model = obj.get("model").ok_or_else(|| {
        ProviderError::InvalidInput("openai-compatible response carries no `model` echo".into())
    })?;
    let model = model.as_str().ok_or_else(|| {
        ProviderError::InvalidInput(
            "openai-compatible response `model` field is not a string".into(),
        )
    })?;
    if model != space.model_id() {
        return Err(ProviderError::InvalidInput(format!(
            "openai-compatible response model {model:?} does not match configured model {:?}",
            space.model_id()
        )));
    }

    // Schema strictness for the optional usage block: a malformed envelope
    // shape is rejected, not tolerated silently.
    if let Some(usage) = obj.get("usage") {
        if !usage.is_object() {
            return Err(ProviderError::InvalidInput(
                "openai-compatible response `usage` field is not an object".into(),
            ));
        }
    }

    let data = obj.get("data").and_then(|d| d.as_array()).ok_or_else(|| {
        ProviderError::InvalidInput("openai-compatible response is missing a data array".into())
    })?;

    // ── Security level: breadth bomb dies at the count gate, before a single
    // entry (and none of its components) is decoded. The exact message of the
    // pre-existing count check is preserved.
    if data.len() != expected_count {
        return Err(ProviderError::InvalidInput(format!(
            "openai-compatible response returned {} vectors, expected {}",
            data.len(),
            expected_count
        )));
    }

    let expected = space.dimension() as usize;

    // ── Schema + semantic level: strict per-entry validation ──────────────
    // Collect (index, vector) pairs, then require the index set to be
    // exactly 0..n so reordering is trusted but gaps/duplicates are not.
    let mut indexed: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
    for (position, item) in data.iter().enumerate() {
        let entry = item.as_object().ok_or_else(|| {
            ProviderError::InvalidInput(format!(
                "openai-compatible data entry {position} is not an object"
            ))
        })?;
        // Strict field type: a non-negative *integer*. Fractional (0.5),
        // negative and string indices are rejected, never coerced.
        let index = entry.get("index").and_then(|v| v.as_u64()).ok_or_else(|| {
            ProviderError::InvalidInput(format!(
                "openai-compatible data entry {position} index is not a non-negative integer"
            ))
        })? as usize;
        // Strict field type: an embedding must be a numeric array;
        // base64-string and object embeddings are rejected.
        let raw = entry
            .get("embedding")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ProviderError::InvalidInput(format!(
                    "openai-compatible data entry {position} has no embedding array"
                ))
            })?;
        let mut vector = Vec::with_capacity(raw.len());
        for (component_index, component) in raw.iter().enumerate() {
            let value = component.as_f64().ok_or_else(|| {
                ProviderError::InvalidInput(format!(
                    "openai-compatible data entry {position} has a non-numeric component at index {component_index}"
                ))
            })?;
            let value = value as f32;
            if !value.is_finite() {
                return Err(ProviderError::InvalidInput(format!(
                    "openai-compatible data entry {position} has a non-finite component at index {component_index}"
                )));
            }
            vector.push(value);
        }
        if vector.len() != expected {
            return Err(ProviderError::InvalidInput(format!(
                "openai-compatible data entry {position} has dimension {}, expected {}",
                vector.len(),
                expected
            )));
        }
        if vector.iter().all(|&v| v == 0.0) {
            return Err(ProviderError::InvalidInput(format!(
                "openai-compatible data entry {position} is a zero vector"
            )));
        }
        // ── Semantic level: norm policy ───────────────────────────────────
        if let NormPolicy::RejectOutside { .. } = norm_policy {
            // f64 accumulation: (3.4e38)^2 × dimension ≤ ~4.7e80 for the
            // frozen MAX_DIMENSION, far below the f64 ceiling — a legitimate
            // all-finite f32 vector can never overflow the sum.
            let sum_squares: f64 = vector.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
            let norm = sum_squares.sqrt();
            if !norm_policy.admits(norm) {
                let (min, max) = match norm_policy {
                    NormPolicy::RejectOutside { min, max } => (*min, *max),
                    NormPolicy::Accept => unreachable!("guarded above"),
                };
                return Err(ProviderError::InvalidInput(format!(
                    "openai-compatible data entry {position} has L2 norm {norm:.6} outside \
                     the configured [{min}, {max}] range"
                )));
            }
        }
        indexed.push((index, vector));
    }

    // ── Semantic level: index completeness and uniqueness ─────────────────
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position {
            return Err(ProviderError::InvalidInput(format!(
                "openai-compatible response indices are not exactly 0..{} (position {position} carries index {index})",
                indexed.len()
            )));
        }
    }
    Ok(indexed.into_iter().map(|(_, vector)| vector).collect())
}

pub(crate) fn map_transport_error(error: TransportError) -> ProviderError {
    match error {
        TransportError::Timeout => ProviderError::Timeout,
        TransportError::Cancelled => ProviderError::Cancelled,
        // `ProviderError::ServerError` carries no payload; the transport's
        // IO detail stays out of the error (it may contain host strings).
        TransportError::Io(..) => ProviderError::ServerError,
    }
}

fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

impl EmbeddingProvider for OpenAiCompatibleProvider {
    fn space(&self) -> &VectorSpace {
        &self.config.space
    }

    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let inputs: Vec<&[u8]> = batch.iter().map(|d| d.bytes.as_slice()).collect();
        self.embed(inputs)
    }

    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let inputs: Vec<&[u8]> = batch.iter().map(|q| q.bytes.as_slice()).collect();
        self.embed(inputs)
    }
}

// ── bounded retry + circuit breaker (P7-006) ────────────────────────────────
//
// ## Layering (分层口径, frozen by the retry-layering tests)
//
// The outbox attempt budget (`semantic_outbox.retry_semantic_task` /
// `WorkerLimits.max_attempts`) and the call-layer retry budget live on
// DIFFERENT layers and never consume each other:
//
// - **One outbox attempt = one `EmbedHandler::handle` call = at most one
//   full call-layer retry sequence.** The sequence's own bound
//   ([`RetryPolicy::max_attempts`], its `total_deadline`, its optional
//   cost cap) is freshly granted at the start of every outbox attempt;
//   whatever happened in the previous attempt does not shrink it.
// - **The call layer NEVER writes queue state.** [`RetryingProvider`] holds
//   no DB handle, lease, or transaction (structural: its public surface
//   accepts none) — every failure it finally gives up on is returned as a
//   plain [`ProviderError`], and the queue's fenced retry
//   (`retry_semantic_task`) is what increments `attempt_count` by exactly
//   ONE per outbox attempt, no matter how many provider calls the call
//   layer made inside it.
// - **The gate cooldown is reported, not waited out, at this layer.** A
//   `RateLimited` answer feeds `ProviderGate::note_rate_limited` (P7-005's
//   first-consumer contract); when the shared gate is STILL globally
//   suspended after a retry wait, the loop consumes that signal by
//   STOPPING (fail fast, return the last error) instead of parking its
//   thread against the cooldown — at most ONE wait is taken before the
//   signal is consumed, and the fenced retry re-claims the task after the
//   queue's own backoff.
//
// The breaker and the retry loop are passive, CALLED components: no thread,
// timer, or daemon exists here; all waiting happens on the caller's thread
// through the injected [`RetryClock`], and time only advances when the
// caller drives it.

use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;

use cc_model::config::SemanticProviderConfig;
use cc_model::{CcError, CcResult};

use crate::admission::{
    estimate_tokens, AttemptOutcome, BudgetAdmission, CostBudget, ProviderCallReceipt,
    ProviderGate, ReceiptLedger, ReceiptPath, UncertainReason, UsageReceipt,
    PROVIDER_ATTEMPT_COST_UNITS,
};

/// Injected clock for the retry/breaker layer. Two duties, both explicit:
/// monotonic-ish wall reading (`now_millis`) for deadlines and breaker
/// windows, and the backoff wait (`sleep`) — which runs on the CALLING
/// thread (red line: no implicit thread or timer is ever created; the
/// breaker and the retry loop are called components).
pub trait RetryClock: Send + Sync {
    /// Current time in milliseconds, the same unit every window/deadline in
    /// this module is computed in.
    fn now_millis(&self) -> u64;
    /// Block the calling thread for `duration` (real clock) / advance
    /// virtually (deterministic mock).
    fn sleep(&self, duration: Duration);
}

/// The production clock: real system time, real blocking sleep on the
/// calling thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRetryClock;

impl RetryClock for SystemRetryClock {
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Deterministic clock for tests: time advances ONLY when the test says so
/// (`advance`/`advance_to`); `sleep` records its duration and moves the
/// virtual clock instead of blocking. It is a test/determinism utility, not
/// a timer — nothing runs on its own.
#[derive(Debug, Default)]
pub struct MockRetryClock {
    now_ms: AtomicU64,
    sleeps: Mutex<Vec<Duration>>,
}

impl MockRetryClock {
    pub fn new(start_ms: u64) -> Self {
        Self {
            now_ms: AtomicU64::new(start_ms),
            sleeps: Mutex::new(Vec::new()),
        }
    }

    pub fn advance(&self, by: Duration) {
        self.now_ms
            .fetch_add(by.as_millis() as u64, AtomicOrdering::SeqCst);
    }

    pub fn now(&self) -> u64 {
        self.now_ms.load(AtomicOrdering::SeqCst)
    }

    /// Every sleep the retry loop performed, in order (backoff-sequence
    /// assertions).
    pub fn recorded_sleeps(&self) -> Vec<Duration> {
        self.sleeps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

impl RetryClock for MockRetryClock {
    fn now_millis(&self) -> u64 {
        self.now()
    }

    fn sleep(&self, duration: Duration) {
        self.sleeps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(duration);
        self.advance(duration);
    }
}

/// Bounds of ONE call-layer retry sequence (P7-006). Validated by
/// [`RetryPolicy::validated`]; [`RetryPolicy::disabled`] is the
/// configuration default (`semantic.retry_max_attempts = 0`): exactly one
/// provider attempt, no call-layer retry at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// TOTAL provider attempts of one sequence, first call included.
    /// Must be ≥ 1; `1` = no retry.
    pub max_attempts: u32,
    /// Exponential backoff base: attempt `n` waits
    /// `min(base_backoff × 2^n, max_backoff)`, shrunk by up to
    /// [`BACKOFF_JITTER_FRACTION`] (deterministic jitter, see
    /// [`RetryPolicy::backoff_delay`]).
    pub base_backoff: Duration,
    /// Cap of the exponential growth.
    pub max_backoff: Duration,
    /// A `RateLimited` answer's `Retry-After` overrides the computed
    /// backoff for that one wait when `true` (default).
    pub respect_retry_after: bool,
    /// Wall-clock budget of the WHOLE sequence: a wait that would push the
    /// loop past it is not taken — the last error is returned instead.
    pub total_deadline: Duration,
    /// Optional cost cap of one sequence, in abstract cost units. Until the
    /// receipt layer (P7-008) reports real costs the placeholder rate is
    /// one unit per provider attempt; reaching the cap ends the sequence.
    pub max_cost_units: Option<u64>,
}

/// Max share of any computed backoff that deterministic jitter may shave
/// (a uniform shrink in `[1 − J, 1]`; never an increase, so the cap bound
/// `max_backoff` still holds with jitter on).
pub const BACKOFF_JITTER_FRACTION: f64 = 0.25;

impl RetryPolicy {
    /// Configuration default: no call-layer retry (`max_attempts = 1`).
    pub fn disabled() -> Self {
        Self {
            max_attempts: 1,
            base_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_millis(8_000),
            respect_retry_after: true,
            total_deadline: Duration::from_millis(30_000),
            max_cost_units: None,
        }
    }

    /// Validated constructor: `max_attempts ≥ 1`, `base_backoff ≥ 1ms`,
    /// `max_backoff ≥ base_backoff`, `total_deadline ≥ 1ms`, and a cost
    /// cap, when set, of at least 1 (a zero cap would forbid the very
    /// first call — say `disabled` instead).
    pub fn validated(
        max_attempts: u32,
        base_backoff: Duration,
        max_backoff: Duration,
        respect_retry_after: bool,
        total_deadline: Duration,
        max_cost_units: Option<u64>,
    ) -> CcResult<Self> {
        if max_attempts == 0 {
            return Err(CcError::InvalidParams(
                "retry policy max_attempts must be at least 1 (0 means disabled: \
                 use RetryPolicy::disabled instead)"
                    .into(),
            ));
        }
        if base_backoff.is_zero() {
            return Err(CcError::InvalidParams(
                "retry policy base_backoff must be at least 1ms".into(),
            ));
        }
        if max_backoff < base_backoff {
            return Err(CcError::InvalidParams(format!(
                "retry policy max_backoff ({max_backoff:?}) must not be smaller \
                 than base_backoff ({base_backoff:?})"
            )));
        }
        if total_deadline.is_zero() {
            return Err(CcError::InvalidParams(
                "retry policy total_deadline must be at least 1ms".into(),
            ));
        }
        if max_cost_units == Some(0) {
            return Err(CcError::InvalidParams(
                "retry policy max_cost_units, when set, must be at least 1 \
                 (a zero cap would forbid the first attempt)"
                    .into(),
            ));
        }
        Ok(Self {
            max_attempts,
            base_backoff,
            max_backoff,
            respect_retry_after,
            total_deadline,
            max_cost_units,
        })
    }

    /// Map the `semantic.*` retry keys (P7-006) onto a policy.
    /// `retry_max_attempts == 0` → [`RetryPolicy::disabled`] (调用层重试
    /// 默认关闭). Every invalid combination is a config error that names
    /// the key, never a silent fallback.
    pub fn from_provider_config(config: &SemanticProviderConfig) -> CcResult<Self> {
        if config.retry_max_attempts == 0 {
            return Ok(Self::disabled());
        }
        Self::validated(
            config.retry_max_attempts,
            Duration::from_millis(config.retry_base_backoff_ms),
            Duration::from_millis(config.retry_max_backoff_ms),
            config.retry_respect_retry_after,
            Duration::from_millis(config.retry_total_deadline_ms),
            config.retry_max_cost_units,
        )
    }

    /// Backoff before the retry AFTER `attempt` (0-based), shrunk by
    /// `jitter_roll ∈ [0, 1]` uniformly up to
    /// [`BACKOFF_JITTER_FRACTION`]. Pure and deterministic: the roll is a
    /// parameter, the loop derives it from the injected clock. Jitter only
    /// ever SHRINKS a wait, so `max_backoff` stays a hard upper bound.
    pub fn backoff_delay(&self, attempt: u32, jitter_roll: f64) -> Duration {
        let shift = attempt.min(20);
        let exponential = self
            .base_backoff
            .saturating_mul(1u32 << shift)
            .min(self.max_backoff);
        let factor = 1.0 - BACKOFF_JITTER_FRACTION * jitter_roll.clamp(0.0, 1.0);
        exponential.mul_f64(factor)
    }
}

/// Splitmix64 finalizer over a clock+attempt seed: enough dispersion for
/// backoff jitter, zero dependencies, fully deterministic given the
/// injected clock.
fn jitter_roll(seed: u64) -> f64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// How long a tripped breaker stays open for an [`ProviderError::AuthError`]
/// trip, as a multiple of the configured window (credentials do not heal in
/// seconds; a long window keeps the process from hammering a rejecting
/// provider). Operators re-enable by fixing the credential and restarting /
/// re-initializing the breaker at the composition root.
pub const AUTH_OPEN_WINDOW_MULTIPLIER: u32 = 10;

/// Public three-state view of the breaker ([`CircuitBreaker::state`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation; every call passes.
    Closed,
    /// Tripped: calls fail fast WITHOUT touching the provider until the
    /// window elapses.
    Open {
        /// Time until the window elapses (zero if it already has — the
        /// next admission lazily transitions to half-open).
        remaining: Duration,
    },
    /// The open window has elapsed; ONE probe call is admitted at a time.
    /// Probe success closes the circuit, probe failure re-opens it.
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakerPhase {
    Closed,
    /// Open until this many milliseconds on the injected clock.
    OpenUntil(u64),
    HalfOpen,
}

#[derive(Debug)]
struct BreakerInner {
    phase: BreakerPhase,
    consecutive_failures: u32,
    /// Half-open single-probe slot: `true` between an admitted probe and
    /// its outcome record.
    probe_in_flight: bool,
}

impl Default for BreakerInner {
    fn default() -> Self {
        Self {
            phase: BreakerPhase::Closed,
            consecutive_failures: 0,
            probe_in_flight: false,
        }
    }
}

/// Configuration of the process-wide circuit breaker (P7-006). The
/// threshold counts CONSECUTIVE retryable failures
/// (`ServerError`/`Timeout`); a success resets the count. `RateLimited`,
/// `InvalidInput` and `Cancelled` are deliberately NOT breaker failures
/// (quota, input shape, caller cancellation — none indicates a broken
/// provider); rate limiting has its own mechanism (the gate pause).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerLimits {
    /// Consecutive retryable failures that trip the breaker. `0` = the
    /// breaker is disabled and never opens.
    pub failure_threshold: u32,
    /// Open-window length; also the spacing between half-open probes, and
    /// (× [`AUTH_OPEN_WINDOW_MULTIPLIER`]) the window of an auth trip.
    pub open_window: Duration,
}

impl BreakerLimits {
    /// Disabled breaker: admits everything, records nothing.
    pub fn disabled() -> Self {
        Self {
            failure_threshold: 0,
            open_window: Duration::ZERO,
        }
    }

    /// Validated constructor: a non-zero threshold needs a window of at
    /// least 1ms (an instant open/close cycle would not protect anyone).
    pub fn validated(failure_threshold: u32, open_window: Duration) -> CcResult<Self> {
        if failure_threshold > 0 && open_window.is_zero() {
            return Err(CcError::InvalidParams(
                "breaker open_window must be at least 1ms when the failure \
                 threshold is non-zero (0 threshold means disabled)"
                    .into(),
            ));
        }
        Ok(Self {
            failure_threshold,
            open_window,
        })
    }

    /// Map the `semantic.*` breaker keys (P7-006) onto limits. Unlike the
    /// retry keys the breaker defaults ON (a threshold of 5): it only ever
    /// reacts to sustained failure and can never affect a success path.
    /// `breaker_failure_threshold = 0` disables it explicitly.
    pub fn from_provider_config(config: &SemanticProviderConfig) -> CcResult<Self> {
        Self::validated(
            config.breaker_failure_threshold,
            Duration::from_millis(config.breaker_open_ms),
        )
    }
}

/// Admission decision of the breaker for one would-be call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerAdmission {
    /// The call may proceed.
    Admitted,
    /// The circuit is open (or the single half-open probe is taken): fail
    /// fast, do NOT touch the provider, retry no earlier than `retry_in`.
    Rejected {
        /// Estimated time until the circuit may admit again.
        retry_in: Duration,
    },
}

/// The process-wide provider circuit breaker (P7-006). ONE instance per
/// process, assembled by the composition root beside the
/// [`ProviderGate`] singleton (`cc-server::service_factory`) so all
/// workers share one failure picture. Passive by construction: a called
/// component with a Mutex and an injected clock — no thread, timer, or
/// daemon; open-window elapsing is observed LAZILY at the next
/// admission/state read, never pushed by a timer.
///
/// Three states, one probe:
///
/// - **Closed** → counts consecutive retryable failures
///   ([`CircuitBreaker::record_failure`]); at
///   [`BreakerLimits::failure_threshold`] the circuit opens.
/// - **Open** → [`CircuitBreaker::admit`] rejects without touching the
///   provider. When a read observes the window elapsed, the state
///   lazily becomes half-open.
/// - **HalfOpen** → exactly ONE probe call is admitted at a time
///   (single slot; concurrent callers are rejected). Probe success
///   ([`CircuitBreaker::record_success`]) closes the circuit and resets
///   the counter; probe failure re-opens it for a fresh window.
///
/// [`ProviderError::AuthError`] trips the circuit open immediately (from
/// any state) for [`AUTH_OPEN_WINDOW_MULTIPLIER`] × the window.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    limits: BreakerLimits,
    inner: Arc<Mutex<BreakerInner>>,
}

impl CircuitBreaker {
    pub fn new(limits: BreakerLimits) -> Self {
        Self {
            limits,
            inner: Arc::new(Mutex::new(BreakerInner::default())),
        }
    }

    /// Explicitly disabled breaker (threshold 0): admits everything.
    pub fn disabled() -> Self {
        Self::new(BreakerLimits::disabled())
    }

    pub fn limits(&self) -> &BreakerLimits {
        &self.limits
    }

    /// Read-only three-state view. Applies the same lazy open→half-open
    /// transition as [`CircuitBreaker::admit`] but never takes the probe
    /// slot.
    pub fn state(&self, clock: &dyn RetryClock) -> CircuitState {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.refresh_phase(&mut inner, clock);
        match inner.phase {
            BreakerPhase::Closed => CircuitState::Closed,
            BreakerPhase::HalfOpen => CircuitState::HalfOpen,
            BreakerPhase::OpenUntil(until) => CircuitState::Open {
                remaining: Duration::from_millis(until.saturating_sub(clock.now_millis())),
            },
        }
    }

    /// Consecutive-failure counter (observability; 0 after a success).
    pub fn consecutive_failures(&self) -> u32 {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .consecutive_failures
    }

    /// Ask whether one call may proceed. In half-open this CONSUMES the
    /// single probe slot — the caller owes the breaker exactly one
    /// `record_*` call afterwards, whatever the outcome.
    pub fn admit(&self, clock: &dyn RetryClock) -> BreakerAdmission {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.refresh_phase(&mut inner, clock);
        match inner.phase {
            BreakerPhase::Closed => BreakerAdmission::Admitted,
            BreakerPhase::HalfOpen => {
                if inner.probe_in_flight {
                    BreakerAdmission::Rejected {
                        retry_in: self.limits.open_window,
                    }
                } else {
                    inner.probe_in_flight = true;
                    BreakerAdmission::Admitted
                }
            }
            BreakerPhase::OpenUntil(until) => BreakerAdmission::Rejected {
                retry_in: Duration::from_millis(until.saturating_sub(clock.now_millis())),
            },
        }
    }

    /// Record a successful call: close the circuit, reset the counter,
    /// release the probe slot.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.phase = BreakerPhase::Closed;
        inner.consecutive_failures = 0;
        inner.probe_in_flight = false;
    }

    /// Record a retryable failure (`ServerError`/`Timeout`): counts toward
    /// the threshold in closed state; a failed half-open probe re-opens the
    /// circuit for a fresh window.
    pub fn record_failure(&self, clock: &dyn RetryClock) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.release_probe(&mut inner);
        match inner.phase {
            BreakerPhase::HalfOpen => {
                inner.phase = BreakerPhase::OpenUntil(self.open_until(clock));
                inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
            }
            _ => {
                inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
                if self.limits.failure_threshold > 0
                    && inner.consecutive_failures >= self.limits.failure_threshold
                {
                    inner.phase = BreakerPhase::OpenUntil(self.open_until(clock));
                }
            }
        }
    }

    /// Record an auth failure: trip the circuit open immediately (any
    /// state) for [`AUTH_OPEN_WINDOW_MULTIPLIER`] × the window.
    pub fn record_auth_failure(&self, clock: &dyn RetryClock) {
        let long_window = self
            .limits
            .open_window
            .saturating_mul(AUTH_OPEN_WINDOW_MULTIPLIER);
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.phase = BreakerPhase::OpenUntil(
            clock
                .now_millis()
                .saturating_add(long_window.as_millis() as u64),
        );
        inner.probe_in_flight = false;
    }

    /// Record an outcome that is neither breaker-relevant success nor
    /// failure (`RateLimited`, `InvalidInput`, `Cancelled`): release the
    /// probe slot if one was taken, change nothing else.
    pub fn record_neutral(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.release_probe(&mut inner);
    }

    fn release_probe(&self, inner: &mut BreakerInner) {
        inner.probe_in_flight = false;
    }

    fn open_until(&self, clock: &dyn RetryClock) -> u64 {
        clock
            .now_millis()
            .saturating_add(self.limits.open_window.as_millis() as u64)
    }

    /// Lazy open→half-open transition (callers hold the lock).
    fn refresh_phase(&self, inner: &mut BreakerInner, clock: &dyn RetryClock) {
        if let BreakerPhase::OpenUntil(until) = inner.phase {
            if clock.now_millis() >= until {
                inner.phase = BreakerPhase::HalfOpen;
                inner.probe_in_flight = false;
            }
        }
    }
}

/// The retrying decorator over any [`EmbeddingProvider`] (P7-006): bounded
/// call-layer retries for the retryable error classes, exponential backoff
/// with deterministic jitter, `Retry-After` honoring, a wall-clock
/// deadline, an optional cost cap, 429 cooperation with the shared
/// [`ProviderGate`], and circuit-breaker admission around every attempt.
/// See the module-level "Layering" note: this decorator NEVER writes queue
/// state and its budget is independent of the outbox attempt budget.
///
/// Error routing (the frozen six variants):
///
/// | Error | Retry? | Breaker | Gate |
/// |---|---|---|---|
/// | `RateLimited` | yes, wait = `Retry-After` | neutral | `note_rate_limited`; loop STOPS if the gate is STILL suspended after a wait |
/// | `ServerError` / `Timeout` | yes, exponential backoff | failure counted | — |
/// | `AuthError` | never | trips open (long window) | — |
/// | `InvalidInput` / `Cancelled` | never (pass through) | neutral | — |
/// | breaker rejection | no (fail fast, one return) | — | — |
///
/// With a receipt ledger attached ([`RetryingProvider::with_receipts`],
/// P7-008) every attempt — retries, breaker refusals, and pre-call budget
/// refusals included — leaves one structured
/// [`ProviderCallReceipt`]: input batch size, the P7-003 token estimate,
/// the 1-based attempt ordinal, the outcome, the duration measured on the
/// injected clock, the placeholder-rate cost
/// ([`PROVIDER_ATTEMPT_COST_UNITS`]), and the `uncertain` marker for
/// breaker-open refusals and timed-out attempts. Reported provider usage
/// is `None` (the frozen port does not surface usage): unknown, never
/// zero-filled. The optional [`CostBudget`] is the process shutdown
/// threshold: once the ledger's lifetime total reaches the cap, calls are
/// refused BEFORE they are made.
#[derive(Clone)]
pub struct RetryingProvider {
    inner: Arc<dyn EmbeddingProvider>,
    policy: RetryPolicy,
    breaker: Arc<CircuitBreaker>,
    gate: Option<Arc<ProviderGate>>,
    clock: Arc<dyn RetryClock>,
    ledger: Option<Arc<ReceiptLedger>>,
    budget: CostBudget,
}

/// What the receipt layer needs to know about ONE port call, measured on
/// the caller side before the retry loop starts (batch size and the
/// declared estimator over the exact input bytes — never the bytes
/// themselves).
struct CallMeta {
    space_model: String,
    path: ReceiptPath,
    batch_items: usize,
    estimated_tokens: u64,
}

impl RetryingProvider {
    /// Decorate `inner` with the process-singleton breaker (and, when the
    /// gate is configured, the shared gate) using the real clock.
    pub fn new(
        inner: Arc<dyn EmbeddingProvider>,
        policy: RetryPolicy,
        breaker: Arc<CircuitBreaker>,
        gate: Option<Arc<ProviderGate>>,
    ) -> Self {
        Self::with_clock(inner, policy, breaker, gate, Arc::new(SystemRetryClock))
    }

    /// Same decoration with an injected clock (deterministic tests).
    pub fn with_clock(
        inner: Arc<dyn EmbeddingProvider>,
        policy: RetryPolicy,
        breaker: Arc<CircuitBreaker>,
        gate: Option<Arc<ProviderGate>>,
        clock: Arc<dyn RetryClock>,
    ) -> Self {
        assert!(policy.max_attempts >= 1, "RetryPolicy must be validated");
        assert!(
            policy.max_backoff >= policy.base_backoff,
            "RetryPolicy must be validated"
        );
        Self {
            inner,
            policy,
            breaker,
            gate,
            clock,
            ledger: None,
            budget: CostBudget::new(None),
        }
    }

    /// Attaches the P7-008 receipt ledger and the cost shutdown budget:
    /// one [`ProviderCallReceipt`] per attempt (retries and pre-call
    /// refusals included), and — once the ledger's lifetime cost total
    /// reaches `budget.max_units` — refusals BEFORE the call (the
    /// degrade-precedent shutdown threshold). Without a ledger the budget
    /// has nothing to read and is ignored.
    pub fn with_receipts(mut self, ledger: Arc<ReceiptLedger>, budget: CostBudget) -> Self {
        self.ledger = Some(ledger);
        self.budget = budget;
        self
    }

    /// Records one receipt on the attached ledger (a no-op without one).
    fn record_receipt(
        &self,
        meta: &CallMeta,
        attempt: u64,
        outcome: AttemptOutcome,
        duration_ms: u64,
        cost_units: u64,
        uncertain: Option<UncertainReason>,
    ) {
        if let Some(ledger) = &self.ledger {
            let usage = UsageReceipt {
                // The frozen port does not surface usage: reported stays
                // unknown (never zero-filled) until that leg lands.
                reported: None,
                estimated: Some(meta.estimated_tokens),
                cache_reuse: false,
            };
            let receipt = ProviderCallReceipt {
                space_model: meta.space_model.clone(),
                path: meta.path,
                batch_items: meta.batch_items,
                usage,
                attempt,
                outcome,
                duration_ms,
                cost_units,
                uncertain,
            };
            ledger.record(receipt, self.clock.now_millis());
        }
    }

    /// The configured policy (observability).
    pub fn policy(&self) -> &RetryPolicy {
        &self.policy
    }

    /// The shared breaker this decorator admits through.
    pub fn breaker(&self) -> &CircuitBreaker {
        &self.breaker
    }

    /// Shared pipeline of both port methods: the retry loop around one
    /// inner call. `meta` feeds the receipt ledger (P7-008); with no
    /// ledger attached the loop behaves exactly as before.
    fn run(
        &self,
        meta: CallMeta,
        mut call: impl FnMut() -> Result<Vec<Vec<f32>>, ProviderError>,
    ) -> Result<Vec<Vec<f32>>, ProviderError> {
        let deadline_millis = self
            .clock
            .now_millis()
            .saturating_add(self.policy.total_deadline.as_millis() as u64);
        let mut cost_used: u64 = 0;
        for attempt in 0..self.policy.max_attempts {
            let attempt_ordinal = attempt as u64 + 1;
            // Breaker admission around EVERY attempt (the probe contract:
            // every admitted attempt owes exactly one record below).
            if let BreakerAdmission::Rejected { .. } = self.breaker.admit(&*self.clock) {
                // Circuit open: fail fast WITHOUT touching the provider.
                // Surfaces as the retryable-looking but loop-terminal
                // `ServerError` (the frozen taxonomy has no dedicated
                // variant; the breaker's own state is the observable).
                // Receipt: the refusal is billing-certain for THIS call
                // (nothing sent) but carries the BreakerOpen uncertainty —
                // the failures that tripped the breaker may already be
                // billed, and the work will be retried.
                self.record_receipt(
                    &meta,
                    attempt_ordinal,
                    AttemptOutcome::RejectedByBreaker,
                    0,
                    0,
                    Some(UncertainReason::BreakerOpen),
                );
                return Err(ProviderError::ServerError);
            }
            // Cost shutdown threshold (P7-008): refused BEFORE the call,
            // aligned with the degrade-precedent budget pattern. The
            // breaker admission above already ran (in half-open it took
            // the probe slot), so this refusal settles the breaker debt
            // as NEUTRAL — the call never reached the provider, so it is
            // neither success nor failure; without this the admitted
            // probe would strand the half-open slot forever.
            if let Some(ledger) = &self.ledger {
                if let BudgetAdmission::Exhausted { .. } =
                    self.budget.admit(ledger.total_cost_units())
                {
                    self.breaker.record_neutral();
                    self.record_receipt(
                        &meta,
                        attempt_ordinal,
                        AttemptOutcome::RejectedByBudget,
                        0,
                        0,
                        None,
                    );
                    return Err(ProviderError::ServerError);
                }
            }
            let started_millis = self.clock.now_millis();
            let result = call();
            let duration_ms = self.clock.now_millis().saturating_sub(started_millis);
            cost_used = cost_used.saturating_add(PROVIDER_ATTEMPT_COST_UNITS);
            // Receipt for the attempt itself. A `Timeout` leaves the
            // explicit uncertainty marker: whether the provider received,
            // processed, and billed the request is undecidable from here.
            let (outcome, uncertain) = match &result {
                Ok(_) => (AttemptOutcome::Succeeded, None),
                Err(ProviderError::Timeout) => (
                    AttemptOutcome::Failed,
                    Some(UncertainReason::TimeoutIndeterminate),
                ),
                Err(_) => (AttemptOutcome::Failed, None),
            };
            self.record_receipt(
                &meta,
                attempt_ordinal,
                outcome,
                duration_ms,
                PROVIDER_ATTEMPT_COST_UNITS,
                uncertain,
            );
            match result {
                Ok(vectors) => {
                    self.breaker.record_success();
                    return Ok(vectors);
                }
                Err(e) => {
                    match &e {
                        // Non-retryable, pass through; no breaker effect.
                        ProviderError::InvalidInput(_) | ProviderError::Cancelled => {
                            self.breaker.record_neutral();
                            return Err(e);
                        }
                        // Non-retryable AND breaker-breaking (long window).
                        ProviderError::AuthError => {
                            self.breaker.record_auth_failure(&*self.clock);
                            return Err(e);
                        }
                        // 429: report to the shared gate (P7-005's
                        // "retry loop is the second consumer"), breaker-neutral.
                        ProviderError::RateLimited { retry_after } => {
                            self.breaker.record_neutral();
                            if let Some(gate) = &self.gate {
                                gate.note_rate_limited(*retry_after);
                            }
                        }
                        // Retryable transport-grade failures.
                        ProviderError::ServerError | ProviderError::Timeout => {
                            self.breaker.record_failure(&*self.clock);
                        }
                    }
                    let last = e;
                    // Budget checks, in order: cost cap, attempt bound,
                    // deadline, gate suspension.
                    if let Some(cap) = self.policy.max_cost_units {
                        if cost_used >= cap {
                            return Err(last);
                        }
                    }
                    if attempt + 1 >= self.policy.max_attempts {
                        return Err(last);
                    }
                    let delay = match &last {
                        ProviderError::RateLimited { retry_after }
                            if self.policy.respect_retry_after =>
                        {
                            *retry_after
                        }
                        _ => {
                            let roll = jitter_roll(
                                self.clock.now_millis()
                                    ^ (attempt as u64).wrapping_mul(0xFF51_AFD7_ED55_8CCD),
                            );
                            self.policy.backoff_delay(attempt, roll)
                        }
                    };
                    let now = self.clock.now_millis();
                    if now >= deadline_millis
                        || now.saturating_add(delay.as_millis() as u64) >= deadline_millis
                    {
                        return Err(last);
                    }
                    self.clock.sleep(delay);
                    // Consume the gate's `Suspended` signal AFTER the wait
                    // (never instead of it): a `RateLimited` wait IS the
                    // cooldown, so the common case passes straight through;
                    // if the shared gate is STILL globally suspended —
                    // someone else's longer `Retry-After`, or a foreign
                    // cooldown — this loop stops here instead of parking
                    // its thread against it, and the queue's fenced retry
                    // re-claims the task after the cooldown. At most ONE
                    // wait is ever taken before the signal is consumed.
                    if let Some(gate) = &self.gate {
                        if gate.snapshot().suspended_for.is_some() {
                            return Err(last);
                        }
                    }
                }
            }
        }
        // Unreachable for a validated policy (the last attempt returns
        // inside the loop); kept for exhaustiveness.
        Err(ProviderError::ServerError)
    }
}

impl fmt::Debug for RetryingProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryingProvider")
            .field("policy", &self.policy)
            .field("breaker", &self.breaker)
            .field("gate", &self.gate.is_some())
            .finish_non_exhaustive()
    }
}

impl EmbeddingProvider for RetryingProvider {
    fn space(&self) -> &VectorSpace {
        self.inner.space()
    }

    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        // Port-contract parity: an empty batch is a no-op on every
        // implementation — no attempt, no breaker contact, no cost, and
        // (P7-008) no receipt.
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let meta = CallMeta {
            space_model: self.inner.space().model_id().to_owned(),
            path: ReceiptPath::Documents,
            batch_items: batch.len(),
            // P7-003 estimator口径: per-item `utf8-bytes-div-ceil-4-v1`,
            // summed — the same figure the batch planner reports.
            estimated_tokens: batch.iter().map(|d| estimate_tokens(&d.bytes) as u64).sum(),
        };
        self.run(meta, || self.inner.embed_documents(batch))
    }

    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let meta = CallMeta {
            space_model: self.inner.space().model_id().to_owned(),
            path: ReceiptPath::Queries,
            batch_items: batch.len(),
            estimated_tokens: batch.iter().map(|q| estimate_tokens(&q.bytes) as u64).sum(),
        };
        self.run(meta, || self.inner.embed_queries(batch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::DocumentInput;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockTransport {
        scripted: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
        requests: Mutex<Vec<HttpRequest>>,
    }

    impl MockTransport {
        fn with_response(response: HttpResponse) -> Self {
            let mock = Self::default();
            mock.scripted.lock().unwrap().push_back(Ok(response));
            mock
        }

        fn with_error(error: TransportError) -> Self {
            let mock = Self::default();
            mock.scripted.lock().unwrap().push_back(Err(error));
            mock
        }

        fn embeddings_response(
            status: u16,
            model: &str,
            entries: Vec<(usize, Vec<f32>)>,
        ) -> HttpResponse {
            let data: Vec<serde_json::Value> = entries
                .into_iter()
                .map(|(index, embedding)| {
                    serde_json::json!({
                        "object": "embedding",
                        "index": index,
                        "embedding": embedding,
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

    impl EmbeddingHttpTransport for MockTransport {
        fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.lock().unwrap().push(request);
            self.scripted
                .lock()
                .unwrap()
                .pop_front()
                .expect("test scripted one response per call")
        }
    }

    // ── Fixtures ──────────────────────────────────────────────────────────

    const MODEL: &str = "text-embedding-test-001";
    const DIM: u32 = 4;

    fn space() -> VectorSpace {
        VectorSpace::new(MODEL, DIM).expect("valid space")
    }

    fn config(transport: Option<Arc<dyn EmbeddingHttpTransport>>) -> OpenAiCompatibleConfig {
        OpenAiCompatibleConfig {
            space: space(),
            endpoint: "https://provider.invalid/v1".to_owned(),
            api_key: EmbeddingApiKey::new("sk-test-secret"),
            timeout: Duration::from_secs(5),
            norm_policy: NormPolicy::Accept,
            transport,
            egress: crate::policy::EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        }
    }

    fn provider(transport: Option<Arc<dyn EmbeddingHttpTransport>>) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(config(transport))
    }

    fn doc(text: &str) -> DocumentInput {
        DocumentInput::from_bytes(text.as_bytes()).expect("valid document input")
    }

    fn query(text: &str) -> QueryInput {
        QueryInput::from_bytes(text.as_bytes()).expect("valid query input")
    }

    fn ok_response(entries: Vec<(usize, Vec<f32>)>) -> HttpResponse {
        MockTransport::embeddings_response(200, MODEL, entries)
    }

    /// A raw 200 response carrying an ad-hoc JSON body, already declaring the
    /// required JSON content type (fixtures that test *body* validation).
    fn raw_json_response(body: serde_json::Value) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    // ── Disabled / fail-closed (D1/D2 default) ────────────────────────────

    #[test]
    fn no_transport_is_fail_closed_disabled() {
        let p = provider(None);
        let err = p.embed_documents(&[doc("x")]).unwrap_err();
        match err {
            ProviderError::InvalidInput(message) => {
                assert!(
                    message.contains("disabled"),
                    "must name the disabled state: {message}"
                );
            }
            other => panic!("expected InvalidInput for disabled provider, got {other:?}"),
        }
        assert_eq!(
            p.embed_queries(&[query("x")]).unwrap_err(),
            p.embed_documents(&[doc("x")]).unwrap_err()
        );
    }

    #[test]
    fn empty_batch_short_circuits_without_transport_or_disabled_error() {
        // No transport configured: an empty batch still returns Ok(empty)
        // (contract parity with FakeProvider) — nothing to embed, no call.
        let p = provider(None);
        assert!(p.embed_documents(&[]).unwrap().is_empty());
        assert!(p.embed_queries(&[]).unwrap().is_empty());
    }

    // ── Request shape ─────────────────────────────────────────────────────

    #[test]
    fn request_carries_model_input_order_and_bearer_auth() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![
            (0, vec![0.1, 0.2, 0.3, 0.4]),
            (1, vec![0.4, 0.3, 0.2, 0.1]),
        ])));
        let p = provider(Some(transport.clone()));
        let batch = vec![doc("alpha"), doc("beta")];
        let out = p.embed_documents(&batch).unwrap();
        assert_eq!(out.len(), 2);

        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.url, "https://provider.invalid/v1/embeddings");
        assert_eq!(request.timeout, Duration::from_secs(5));
        assert!(
            request
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                    && v == "Bearer sk-test-secret"),
            "must send the configured bearer credential"
        );
        assert!(request
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/json"),);
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["input"], serde_json::json!(["alpha", "beta"]));
        assert_eq!(body["encoding_format"], "float");
    }

    #[test]
    fn query_path_uses_the_same_request_shape() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![(
            0,
            vec![1.0, 0.0, 0.0, 0.0],
        )])));
        let p = provider(Some(transport.clone()));
        p.embed_queries(&[query("q")]).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&transport.requests.lock().unwrap()[0].body).unwrap();
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["input"], serde_json::json!(["q"]));
    }

    #[test]
    fn trailing_slash_on_endpoint_is_trimmed() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![(
            0,
            vec![1.0, 0.0, 0.0, 0.0],
        )])));
        let mut cfg = config(Some(transport.clone()));
        cfg.endpoint = "https://provider.invalid/v1///".to_owned();
        let p = OpenAiCompatibleProvider::new(cfg);
        p.embed_documents(&[doc("x")]).unwrap();
        assert_eq!(
            transport.requests.lock().unwrap()[0].url,
            "https://provider.invalid/v1/embeddings"
        );
    }

    // ── Success semantics ─────────────────────────────────────────────────

    #[test]
    fn response_vectors_are_reordered_by_index_to_batch_order() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![
            (1, vec![0.4, 0.3, 0.2, 0.1]),
            (0, vec![0.1, 0.2, 0.3, 0.4]),
        ])));
        let p = provider(Some(transport));
        let out = p.embed_documents(&[doc("first"), doc("second")]).unwrap();
        assert_eq!(out[0], vec![0.1, 0.2, 0.3, 0.4], "index 0 first");
        assert_eq!(out[1], vec![0.4, 0.3, 0.2, 0.1], "index 1 second");
    }

    #[test]
    fn empty_success_response_is_rejected_when_batch_is_nonempty() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![])));
        let p = provider(Some(transport));
        assert_eq!(
            p.embed_documents(&[doc("x")]).unwrap_err(),
            ProviderError::InvalidInput(
                "openai-compatible response returned 0 vectors, expected 1".into()
            )
        );
    }

    // ── Structural rejection (dimension / model / count / index) ──────────

    #[test]
    fn dimension_mismatch_is_rejected() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![(
            0,
            vec![1.0, 2.0],
        )])));
        let p = provider(Some(transport));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("dimension")
        ));
    }

    #[test]
    fn model_mismatch_is_rejected() {
        let transport = Arc::new(MockTransport::with_response(
            MockTransport::embeddings_response(
                200,
                "other/model-999",
                vec![(0, vec![1.0, 0.0, 0.0, 0.0])],
            ),
        ));
        let p = provider(Some(transport));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("does not match configured model")
        ));
    }

    #[test]
    fn count_mismatch_is_rejected() {
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![
            (0, vec![1.0, 0.0, 0.0, 0.0]),
            (1, vec![0.0, 1.0, 0.0, 0.0]),
        ])));
        let p = provider(Some(transport));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("expected 1")
        ));
    }

    #[test]
    fn duplicate_index_is_rejected() {
        // Duplicates pass the count check only when they also fill 0..n —
        // here 2 entries with index 0 twice: count ok (2==... no, batch is 1)
        // so craft batch=2 with duplicate index 0.
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![
            (0, vec![1.0, 0.0, 0.0, 0.0]),
            (0, vec![0.0, 1.0, 0.0, 0.0]),
        ])));
        let p = provider(Some(transport));
        let batch = vec![doc("a"), doc("b")];
        assert!(matches!(
            p.embed_documents(&batch),
            Err(ProviderError::InvalidInput(message)) if message.contains("not exactly 0..2")
        ));
    }

    #[test]
    fn non_numeric_component_is_rejected() {
        let response = raw_json_response(serde_json::json!({
            "model": MODEL,
            "data": [{"index": 0, "embedding": [0.1, "oops", 0.3, 0.4]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("non-numeric")
        ));
    }

    #[test]
    fn missing_data_array_and_malformed_json_are_rejected() {
        let no_data = raw_json_response(serde_json::json!({"model": MODEL, "unexpected": true}));
        let p = provider(Some(Arc::new(MockTransport::with_response(no_data))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("data array")
        ));

        let malformed = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: b"{not json".to_vec(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(malformed))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("not valid JSON")
        ));
    }

    // ── HTTP status → six-variant mapping ─────────────────────────────────

    #[test]
    fn http_429_maps_to_rate_limited_with_retry_after() {
        // A zero-repeat initializer evaluates and drops its entry once.
        // Preserve that empty-vector construction explicitly (the original
        // repeated 1.0 literal has no side effects), then build the empty data.
        let discarded_entry = (0_usize, Vec::<f32>::new());
        drop(discarded_entry);
        let mut response = ok_response(Vec::new());
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["data"], serde_json::json!([]));
        response.status = 429;
        response.headers = vec![("Retry-After".to_owned(), "7".to_owned())];
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert_eq!(
            p.embed_documents(&[doc("x")]).unwrap_err(),
            ProviderError::RateLimited {
                retry_after: Duration::from_secs(7)
            }
        );
    }

    #[test]
    fn http_429_without_retry_after_uses_the_fallback() {
        let mut response = ok_response(vec![]);
        response.status = 429;
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert_eq!(
            p.embed_documents(&[doc("x")]).unwrap_err(),
            ProviderError::RateLimited {
                retry_after: Duration::from_secs(1)
            }
        );
    }

    #[test]
    fn http_401_and_403_map_to_auth_error() {
        for status in [401_u16, 403] {
            let response = HttpResponse {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            };
            let p = provider(Some(Arc::new(MockTransport::with_response(response))));
            assert_eq!(
                p.embed_documents(&[doc("x")]).unwrap_err(),
                ProviderError::AuthError
            );
        }
    }

    #[test]
    fn http_5xx_maps_to_server_error() {
        let response = HttpResponse {
            status: 503,
            headers: Vec::new(),
            body: Vec::new(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert_eq!(
            p.embed_documents(&[doc("x")]).unwrap_err(),
            ProviderError::ServerError
        );
    }

    #[test]
    fn other_http_4xx_maps_to_non_retryable_invalid_input_without_body_echo() {
        let response = HttpResponse {
            status: 400,
            headers: Vec::new(),
            body: b"secret-ish body never echoed".to_vec(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        match p.embed_documents(&[doc("x")]).unwrap_err() {
            ProviderError::InvalidInput(message) => {
                assert!(message.contains("http 400"));
                assert!(
                    !message.contains("secret-ish"),
                    "response body must not leak into errors"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn transport_errors_map_to_timeout_cancelled_and_server_error() {
        let cases = [
            (TransportError::Timeout, ProviderError::Timeout),
            (TransportError::Cancelled, ProviderError::Cancelled),
            (
                TransportError::Io("connection reset".into()),
                ProviderError::ServerError,
            ),
        ];
        for (transport_error, expected) in cases {
            let p = provider(Some(Arc::new(MockTransport::with_error(transport_error))));
            assert_eq!(p.embed_documents(&[doc("x")]).unwrap_err(), expected);
        }
    }

    // ── Input guards ──────────────────────────────────────────────────────

    #[test]
    fn non_utf8_input_is_rejected_before_any_transport_call() {
        // Built via struct literal to bypass the sanctioned UTF-8-validating
        // constructor; the adapter must still refuse to send it.
        let bytes = vec![0xff, 0xfe, 0x00, 0x41];
        let digest = crate::types::InputDigest::of_input(b"placeholder").unwrap();
        let hostile = DocumentInput {
            input_digest: digest,
            bytes: bytes.clone(),
        };
        let transport = Arc::new(MockTransport::default());
        let p = provider(Some(transport.clone()));
        assert!(matches!(
            p.embed_documents(&[hostile]),
            Err(ProviderError::InvalidInput(message)) if message.contains("UTF-8")
        ));
        assert!(
            transport.requests.lock().unwrap().is_empty(),
            "nothing may reach the transport"
        );
    }

    // ── Credential hygiene ────────────────────────────────────────────────

    #[test]
    fn api_key_is_redacted_in_debug_and_never_in_error_messages() {
        let key = EmbeddingApiKey::new("sk-super-secret-value");
        let rendered = format!("{key:?}");
        assert!(!rendered.contains("sk-super-secret-value"));
        assert!(rendered.contains("[REDACTED]"));

        // A failing call must not surface the key in any error message.
        let response = HttpResponse {
            status: 401,
            headers: Vec::new(),
            body: Vec::new(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        // Every rendered error path must be free of the configured key.
        let err = p.embed_documents(&[doc("x")]).unwrap_err();
        let err_debug = format!("{err:?}");
        assert!(
            !err_debug.contains("sk-test-secret"),
            "leaked key in: {err_debug}"
        );
        let cfg_debug = format!("{:?}", config(None));
        assert!(!cfg_debug.contains("sk-test-secret"));
    }

    #[test]
    fn zero_vector_and_nan_responses_are_rejected() {
        let zero = ok_response(vec![(0, vec![0.0; 4])]);
        let p = provider(Some(Arc::new(MockTransport::with_response(zero))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("zero vector")
        ));

        // JSON cannot carry NaN/Inf literals, but a finite f64 literal above
        // the f32 range (1e39) overflows on the adapter's f64→f32 cast; the
        // post-parse finiteness gate must catch exactly that.
        let overflowing = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: format!(
                r#"{{"model":"{MODEL}","data":[{{"index":0,"embedding":[0.1,1e39,0.3,0.4]}}]}}"#
            )
            .into_bytes(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(overflowing))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("non-finite")
        ));
    }

    #[test]
    fn invalid_endpoint_is_rejected_eagerly() {
        let mut cfg = config(None);
        cfg.endpoint = "ftp://nope".to_owned();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            OpenAiCompatibleProvider::new(cfg)
        }))
        .is_err());
    }

    // ── Port interchangeability: FakeProvider ↔ adapter, cache roundtrip ──

    #[test]
    fn adapter_is_interchangeable_with_fake_provider_on_the_port_and_in_the_cache_loop() {
        use crate::cache::ArtifactCache;
        use crate::providers::fake::{FakeProvider, FakeProviderConfig};
        use crate::spec::{DocumentEncodingSpec, VectorSpace as Space};
        use crate::types::{DocSpecDigest, InputDigest};

        fn run_cache_roundtrip<P: EmbeddingProvider>(provider: &P, tag: &str) -> usize {
            static UNIQUE: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "cc-semantic-p7001-roundtrip-{tag}-{}-{}",
                std::process::id(),
                UNIQUE.fetch_add(1, Ordering::SeqCst)
            ));
            let space = provider.space().clone();
            let spec = DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
                .expect("valid spec");
            let spec_digest: DocSpecDigest = spec.digest().expect("spec digest");
            let payload = b"interchange payload";
            let input = InputDigest::of_input(payload).expect("valid input");

            let embedded = provider
                .embed_documents(&[doc("interchange payload")])
                .expect("embed");
            assert_eq!(embedded[0].len(), space.dimension() as usize);

            let cache = ArtifactCache::open(&root, "p7001-test".to_string()).expect("open cache");
            let reference = cache
                .put(&space, &input, &spec_digest, &embedded[0], 1_000)
                .expect("put");
            match cache.get(&space, &input, &spec_digest).expect("get") {
                crate::cache::CacheRead::Hit(validated) => {
                    assert_eq!(validated.artifact_ref, reference);
                    assert_eq!(validated.data, embedded[0]);
                }
                other => panic!("expected a cache hit, got {other:?}"),
            }
            let _ = std::fs::remove_dir_all(&root);
            embedded[0].len()
        }

        // Same port, same cache loop, two interchangeable implementations.
        let fake_space = Space::new("fake/model-a", 8).expect("valid space");
        let fake = FakeProvider::new(FakeProviderConfig::new(fake_space.clone()));
        let adapter = provider(Some(Arc::new(MockTransport::with_response(ok_response(
            vec![(0, vec![0.25, -0.5, 0.75, 0.125])],
        )))));

        // Adapter's frozen space identity matches what it reports on the port.
        assert_eq!(adapter.space().model_id(), MODEL);
        assert_eq!(adapter.space().dimension(), DIM);

        let fake_dim = run_cache_roundtrip(&fake, "fake");
        let adapter_dim = run_cache_roundtrip(&adapter, "adapter");
        assert_eq!(fake_dim, 8);
        assert_eq!(adapter_dim, DIM as usize);

        // Port object safety: both fit behind dyn EmbeddingProvider.
        let implementations: Vec<&dyn EmbeddingProvider> = vec![&fake, &adapter];
        assert_eq!(implementations.len(), 2);
    }

    // ── P7-004: response strong validation (protocol / security / schema /
    // semantic levels). Every rejection below is a non-retryable
    // InvalidInput; the retryable set stays exactly {429, 5xx, transport}. ──

    // ── Schema level: strict field types, no coercion ─────────────────────

    #[test]
    fn fractional_negative_and_string_indices_are_rejected() {
        for index_json in ["0.5", "-1", "\"0\""] {
            let body = format!(
                r#"{{"model":"{MODEL}","data":[{{"index":{index_json},"embedding":[1.0,0.0,0.0,0.0]}}]}}"#
            );
            let response = HttpResponse {
                status: 200,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: body.into_bytes(),
            };
            let p = provider(Some(Arc::new(MockTransport::with_response(response))));
            assert!(
                matches!(
                    p.embed_documents(&[doc("x")]),
                    Err(ProviderError::InvalidInput(message)) if message.contains("not a non-negative integer")
                ),
                "index {index_json} must be rejected as a type violation"
            );
        }
    }

    #[test]
    fn embedding_must_be_a_numeric_array_not_a_string_or_object() {
        for embedding_json in ["\"QUJDREVG\"", "{\"values\":[1.0,0.0,0.0,0.0]}"] {
            let body = format!(
                r#"{{"model":"{MODEL}","data":[{{"index":0,"embedding":{embedding_json}}}]}}"#
            );
            let response = HttpResponse {
                status: 200,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: body.into_bytes(),
            };
            let p = provider(Some(Arc::new(MockTransport::with_response(response))));
            assert!(
                matches!(
                    p.embed_documents(&[doc("x")]),
                    Err(ProviderError::InvalidInput(message)) if message.contains("no embedding array")
                ),
                "embedding {embedding_json} must be rejected as a type violation"
            );
        }
    }

    #[test]
    fn model_echo_is_required_and_must_be_a_string() {
        // Missing echo: a malformed envelope, not a declaration contradiction.
        let missing = raw_json_response(serde_json::json!({
            "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(missing))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("no `model` echo")
        ));

        // Non-string echo: strict schema, no coercion.
        let numeric = raw_json_response(serde_json::json!({
            "model": 42,
            "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(numeric))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("`model` field is not a string")
        ));

        // A mismatched echo keeps its pre-existing diagnostic (and therefore
        // the capability probe's Mismatch classification).
        let mismatched = raw_json_response(serde_json::json!({
            "model": "other/model-999",
            "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(mismatched))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("does not match configured model")
        ));
    }

    #[test]
    fn usage_field_must_be_an_object_when_present() {
        let bad_usage = raw_json_response(serde_json::json!({
            "model": MODEL,
            "usage": "12 tokens",
            "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(bad_usage))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("`usage` field is not an object")
        ));

        let good_usage = raw_json_response(serde_json::json!({
            "object": "list",
            "model": MODEL,
            "usage": {"prompt_tokens": 3, "total_tokens": 3},
            "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(good_usage))));
        assert!(p.embed_documents(&[doc("x")]).is_ok());
    }

    // ── Semantic level: norm policy ───────────────────────────────────────

    #[test]
    fn norm_policy_rejects_vectors_outside_the_configured_range() {
        let mut cfg = config(None);
        cfg.transport = Some(Arc::new(MockTransport::with_response(ok_response(vec![(
            0,
            vec![0.6, 0.6, 0.6, 0.6],
        )]))));
        cfg.norm_policy = NormPolicy::RejectOutside { min: 1.0, max: 1.0 };
        let p = OpenAiCompatibleProvider::new(cfg);
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("L2 norm")
        ));

        // In-range (and boundary-inclusive) vectors pass: [0.5; 4] has an
        // exactly representable L2 norm of 1.0.
        let mut cfg = config(None);
        cfg.transport = Some(Arc::new(MockTransport::with_response(ok_response(vec![(
            0,
            vec![0.5, 0.5, 0.5, 0.5],
        )]))));
        cfg.norm_policy = NormPolicy::RejectOutside { min: 1.0, max: 1.0 };
        let p = OpenAiCompatibleProvider::new(cfg);
        assert_eq!(p.embed_documents(&[doc("x")]).unwrap(), vec![vec![0.5; 4]]);
    }

    #[test]
    fn norm_policy_accept_is_the_default_and_admits_unnormalized_vectors() {
        // Without a policy the P7-001 semantics hold: any finite non-zero
        // vector passes regardless of its norm.
        let transport = Arc::new(MockTransport::with_response(ok_response(vec![(
            0,
            vec![2.0, 2.0, 2.0, 2.0],
        )])));
        let p = provider(Some(transport));
        assert!(p.embed_documents(&[doc("x")]).is_ok());
        assert_eq!(config(None).norm_policy, NormPolicy::Accept);
    }

    #[test]
    fn norm_policy_is_validated_eagerly() {
        for bad in [
            NormPolicy::RejectOutside { min: 1.0, max: 0.5 },
            NormPolicy::RejectOutside {
                min: -1.0,
                max: 1.0,
            },
            NormPolicy::RejectOutside {
                min: 0.0,
                max: f32::NAN,
            },
        ] {
            let mut cfg = config(None);
            cfg.norm_policy = bad;
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                OpenAiCompatibleProvider::new(cfg.clone())
            }))
            .is_err());
        }
    }

    #[test]
    fn negative_zero_vector_is_rejected() {
        let response = raw_json_response(serde_json::json!({
            "model": MODEL,
            "data": [{"index": 0, "embedding": [-0.0, -0.0, -0.0, -0.0]}],
        }));
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("zero vector")
        ));
    }

    // ── Protocol level: content type, HTTP semantics, error envelope ──────

    #[test]
    fn success_responses_require_a_json_content_type() {
        let body = serde_json::json!({
            "model": MODEL,
            "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
        });

        let missing = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: serde_json::to_vec(&body).unwrap(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(missing))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("JSON content type")
        ));

        let html = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "text/html".to_owned())],
            body: serde_json::to_vec(&body).unwrap(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(html))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("JSON content type")
        ));

        // The charset parameter is fine: only the media type is matched.
        let with_charset = HttpResponse {
            status: 200,
            headers: vec![(
                "Content-Type".to_owned(),
                "application/json; charset=utf-8".to_owned(),
            )],
            body: serde_json::to_vec(&body).unwrap(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(with_charset))));
        assert!(p.embed_documents(&[doc("x")]).is_ok());
    }

    #[test]
    fn redirect_informational_and_unknown_statuses_follow_http_semantics() {
        // 3xx: endpoint misconfiguration — non-retryable, redirects are never
        // followed (the credential-bearing URL must not silently change).
        // P7-007 mechanism assertion: the seam under test receives EXACTLY
        // one request — no redirect is re-issued with (or without) the
        // Authorization header.
        let mut redirect = ok_response(vec![(0, vec![1.0, 0.0, 0.0, 0.0])]);
        redirect.status = 302;
        let transport = Arc::new(MockTransport::with_response(redirect));
        let p = provider(Some(transport.clone()));
        match p.embed_documents(&[doc("x")]).unwrap_err() {
            ProviderError::InvalidInput(message) => {
                assert!(message.contains("redirect"), "got: {message}");
                assert!(message.contains("http 302"));
            }
            other => panic!("expected InvalidInput for 302, got {other:?}"),
        }
        assert_eq!(
            transport.requests.lock().unwrap().len(),
            1,
            "a redirect must never be followed (no second request, with or without auth)"
        );

        // 1xx and out-of-range statuses are provider/proxy protocol
        // violations: retryable ServerError.
        for status in [100_u16, 999] {
            let response = HttpResponse {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            };
            let p = provider(Some(Arc::new(MockTransport::with_response(response))));
            assert_eq!(
                p.embed_documents(&[doc("x")]).unwrap_err(),
                ProviderError::ServerError,
                "status {status} must map to the retryable ServerError"
            );
        }
    }

    #[test]
    fn provider_error_envelope_diagnostics_exclude_the_free_form_message() {
        let response = HttpResponse {
            status: 400,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: serde_json::to_vec(&serde_json::json!({
                "error": {
                    "message": "your input CONFIDENTIAL-TEXT was too long",
                    "type": "invalid_request_error",
                    "code": "context_length_exceeded",
                    "param": "input",
                }
            }))
            .unwrap(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        match p.embed_documents(&[doc("x")]).unwrap_err() {
            ProviderError::InvalidInput(message) => {
                assert!(message.contains("http 400"));
                assert!(message.contains("invalid_request_error"));
                assert!(message.contains("context_length_exceeded"));
                // The free-form `message` (echoing request text) and the
                // `param` field never surface.
                assert!(!message.contains("CONFIDENTIAL-TEXT"));
                assert!(!message.contains("too long"));
                assert!(!message.contains("param"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    // ── Security level: size bound, depth bomb, breadth bomb ──────────────

    #[test]
    fn oversized_response_body_is_rejected_before_json_parsing() {
        // Bound for batch 1 × dimension 4 = 4×64 + 256 + 1024 = 1536 bytes.
        // The body is not even JSON: if the gate had tried to parse it, the
        // error would say so — instead the size bound fires first.
        let response = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: vec![b'x'; 2_048],
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        match p.embed_documents(&[doc("x")]).unwrap_err() {
            ProviderError::InvalidInput(message) => {
                assert!(
                    message.contains("exceeds the derived bound"),
                    "got: {message}"
                );
                assert!(message.contains("2048"));
                assert!(message.contains("1536"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        // A legitimate body under the bound passes untouched.
        let p = provider(Some(Arc::new(MockTransport::with_response(ok_response(
            vec![(0, vec![1.0, 0.0, 0.0, 0.0])],
        )))));
        assert!(p.embed_documents(&[doc("x")]).is_ok());
    }

    #[test]
    fn deeply_nested_json_is_rejected_by_the_depth_limit() {
        let response = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: "[".repeat(300).into_bytes(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert!(matches!(
            p.embed_documents(&[doc("x")]),
            Err(ProviderError::InvalidInput(message)) if message.contains("not valid JSON")
        ));
    }

    #[test]
    fn breadth_bomb_dies_at_the_count_gate_before_decoding_entries() {
        // 200 placeholder entries against a 1-item batch: each entry is a
        // bare null — proof that the count gate fires before any entry is
        // decoded (decoding would demand objects). The body deliberately
        // stays under the derived size bound so this pins the count gate
        // specifically; larger floods die at the size gate first.
        let bomb = format!(
            r#"{{"model":"{MODEL}","data":[{}]}}"#,
            "null,".repeat(200).trim_end_matches(',')
        );
        let response = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: bomb.into_bytes(),
        };
        let p = provider(Some(Arc::new(MockTransport::with_response(response))));
        assert_eq!(
            p.embed_documents(&[doc("x")]).unwrap_err(),
            ProviderError::InvalidInput(
                "openai-compatible response returned 200 vectors, expected 1".into()
            )
        );
    }

    // ── Security level: zero leakage on the validation path ───────────────

    #[test]
    fn validation_errors_never_carry_request_or_response_payloads() {
        const INPUT_MARKER: &str = "CONFIDENTIAL-INPUT-42";
        const RESPONSE_MARKER: &str = "RESPONSE-MARKER-7";

        let scenarios: Vec<HttpResponse> = vec![
            // 400 error envelope whose message echoes the input.
            HttpResponse {
                status: 400,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: serde_json::to_vec(&serde_json::json!({
                    "error": {"message": format!("bad input {INPUT_MARKER} {RESPONSE_MARKER}"),
                              "type": "invalid_request_error"}
                }))
                .unwrap(),
            },
            // 200 with a hostile extra field and a malformed entry.
            raw_json_response(serde_json::json!({
                "model": MODEL,
                "notes": RESPONSE_MARKER,
                "data": [{"index": 0, "embedding": [0.1, "oops", 0.3, 0.4]}],
            })),
            // Malformed body that itself contains the marker.
            HttpResponse {
                status: 200,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: format!("{{broken {RESPONSE_MARKER}").into_bytes(),
            },
        ];

        for response in scenarios {
            let transport = Arc::new(MockTransport::with_response(response));
            let p = provider(Some(transport));
            let err = p.embed_documents(&[doc(INPUT_MARKER)]).unwrap_err();
            let rendered = format!("{err:?}");
            assert!(
                !rendered.contains(INPUT_MARKER),
                "request text leaked into error: {rendered}"
            );
            assert!(
                !rendered.contains(RESPONSE_MARKER),
                "response body leaked into error: {rendered}"
            );
            assert!(
                !rendered.contains("sk-test-secret"),
                "api key leaked into error: {rendered}"
            );
        }
    }

    // ── "错误向量不缓存": validation failure ⇒ no ArtifactCache::put ───────

    #[test]
    fn error_vectors_never_reach_the_artifact_cache() {
        use crate::cache::{ArtifactCache, CacheRead};
        use crate::spec::DocumentEncodingSpec;
        use crate::types::{DocSpecDigest, InputDigest};

        static UNIQUE: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "cc-semantic-p7004-no-error-cache-{}-{}",
            std::process::id(),
            UNIQUE.fetch_add(1, Ordering::SeqCst)
        ));
        let cache = ArtifactCache::open(&root, "p7004-test".to_string()).expect("open cache");
        let spec =
            DocumentEncodingSpec::new(space(), None, 8_192, "fake-tokenizer").expect("valid spec");
        let spec_digest: DocSpecDigest = spec.digest().expect("spec digest");
        let input = InputDigest::of_input(b"payload").expect("valid input");

        let put_count = |cache: &ArtifactCache| -> usize {
            // Count persisted artifact payloads (.bin) under the cache root:
            // each `put` creates exactly one.
            let _ = cache;
            let mut count = 0;
            fn walk(dir: &std::path::Path, count: &mut usize) {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    return;
                };
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        walk(&entry.path(), count);
                    } else if entry.path().extension().is_some_and(|e| e == "bin") {
                        *count += 1;
                    }
                }
            }
            walk(&root, &mut count);
            count
        };

        // Control: a fully valid response goes embed → put and lands once.
        let mut scripted = VecDeque::new();
        scripted.push_back(Ok(ok_response(vec![(0, vec![1.0, 0.0, 0.0, 0.0])])));
        // Then every malformed-response class from the strong gate.
        for hostile in [
            raw_json_response(serde_json::json!({
                "model": "other/model-999",
                "data": [{"index": 0, "embedding": [1.0, 0.0, 0.0, 0.0]}],
            })),
            raw_json_response(serde_json::json!({
                "model": MODEL,
                "data": [{"index": 0, "embedding": [0.0, 0.0, 0.0, 0.0]}],
            })),
            raw_json_response(serde_json::json!({
                "model": MODEL,
                "data": [{"index": 0, "embedding": [0.1, 1e39, 0.3, 0.4]}],
            })),
        ] {
            scripted.push_back(Ok(hostile));
        }
        let mock = MockTransport {
            scripted: Mutex::new(scripted),
            requests: Mutex::new(Vec::new()),
        };
        let p = provider(Some(Arc::new(mock)));

        let vectors = p.embed_documents(&[doc("payload")]).expect("valid embed");
        assert_eq!(vectors.len(), 1);
        cache
            .put(&space(), &input, &spec_digest, &vectors[0], 1_000)
            .expect("control put");
        assert_eq!(put_count(&cache), 1, "control: exactly one artifact");

        for _ in 0..3 {
            // Every rejected batch stops at the gate: nothing is handed back,
            // so nothing can be published or cached.
            assert!(p.embed_documents(&[doc("payload")]).is_err());
            match cache.get(&space(), &input, &spec_digest).expect("get") {
                CacheRead::Hit(validated) => {
                    assert_eq!(validated.data, vec![1.0, 0.0, 0.0, 0.0]);
                }
                other => panic!("control hit must survive, got {other:?}"),
            }
        }
        assert_eq!(
            put_count(&cache),
            1,
            "error vectors must never reach the artifact cache"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── P7-006: bounded retry + circuit breaker ─────────────────────────

    use crate::admission::GateLimits;
    use crate::ports::{EmbeddingProvider, QueryInput};

    /// Scripted inner provider for the retry tests: pops one outcome per
    /// call (sticking on the last one), counts calls.
    struct ScriptedProvider {
        space: VectorSpace,
        outcomes: Mutex<VecDeque<Result<Vec<Vec<f32>>, ProviderError>>>,
        calls: AtomicUsize,
    }

    impl ScriptedProvider {
        fn new(outcomes: Vec<Result<Vec<Vec<f32>>, ProviderError>>) -> Self {
            Self {
                space: space(),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
            }
        }

        fn ok() -> Self {
            Self::new(vec![Ok(vec![vec![1.0_f32]])])
        }
    }

    impl EmbeddingProvider for ScriptedProvider {
        fn space(&self) -> &VectorSpace {
            &self.space
        }

        fn embed_documents(
            &self,
            _batch: &[DocumentInput],
        ) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("script exhausted"))
        }

        fn embed_queries(&self, _batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("script exhausted"))
        }
    }

    fn retrying(
        inner: Arc<ScriptedProvider>,
        policy: RetryPolicy,
        gate: Option<Arc<ProviderGate>>,
        clock: Arc<MockRetryClock>,
    ) -> RetryingProvider {
        RetryingProvider::with_clock(
            inner,
            policy,
            Arc::new(CircuitBreaker::disabled()),
            gate,
            clock,
        )
    }

    fn policy(max_attempts: u32) -> RetryPolicy {
        RetryPolicy::validated(
            max_attempts,
            Duration::from_millis(10),
            Duration::from_millis(10_000),
            true,
            Duration::from_millis(60_000),
            None,
        )
        .expect("valid policy")
    }

    fn one_doc() -> Vec<DocumentInput> {
        vec![doc("payload")]
    }

    #[test]
    fn retryable_server_errors_are_retried_within_the_bound() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(1_000));
        let p = retrying(inner.clone(), policy(3), None, clock.clone());
        let out = p.embed_documents(&one_doc()).expect("eventual success");
        assert_eq!(out, vec![vec![1.0_f32]]);
        assert_eq!(inner.calls.load(Ordering::SeqCst), 3);
        // Two waits between three attempts, exponential from 10ms, jitter
        // only ever shrinks.
        let sleeps = clock.recorded_sleeps();
        assert_eq!(sleeps.len(), 2);
        assert!(sleeps[0] <= Duration::from_millis(10));
        assert!(sleeps[1] <= Duration::from_millis(20));
        assert!(sleeps[1] >= Duration::from_millis(15));
    }

    #[test]
    fn non_retryable_errors_pass_through_without_a_second_call() {
        for terminal in [
            ProviderError::InvalidInput("zero vector".into()),
            ProviderError::Cancelled,
        ] {
            let inner = Arc::new(ScriptedProvider::new(vec![
                Err(terminal.clone()),
                Ok(vec![vec![1.0_f32]]),
            ]));
            let p = retrying(
                inner.clone(),
                policy(5),
                None,
                Arc::new(MockRetryClock::new(0)),
            );
            let err = p.embed_documents(&one_doc()).unwrap_err();
            assert_eq!(err, terminal);
            assert_eq!(inner.calls.load(Ordering::SeqCst), 1, "{terminal:?}");
        }
    }

    #[test]
    fn auth_error_is_not_retried_and_trips_the_long_open_window() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::AuthError),
            Ok(vec![vec![1.0_f32]]),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let breaker = Arc::new(CircuitBreaker::new(
            BreakerLimits::validated(2, Duration::from_millis(100)).unwrap(),
        ));
        let p = RetryingProvider::with_clock(
            inner.clone(),
            policy(5),
            breaker.clone(),
            None,
            clock.clone(),
        );
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::AuthError)
        ));
        assert_eq!(
            inner.calls.load(Ordering::SeqCst),
            1,
            "auth is never retried"
        );
        // The long window: 10 × the configured one.
        match breaker.state(&*clock) {
            CircuitState::Open { remaining } => {
                assert!(
                    remaining > Duration::from_millis(500),
                    "long window, got {remaining:?}"
                );
                assert!(remaining <= Duration::from_secs(1));
            }
            other => panic!("expected open, got {other:?}"),
        }
        // The next call fails fast WITHOUT provider contact.
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::ServerError)
        ));
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn retry_budget_exhaustion_returns_the_last_error() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::Timeout),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let p = retrying(inner.clone(), policy(3), None, clock);
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::ServerError)
        ));
        assert_eq!(
            inner.calls.load(Ordering::SeqCst),
            3,
            "bounded at max_attempts"
        );
    }

    #[test]
    fn total_deadline_ends_the_sequence_before_the_attempt_bound() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let short_deadline = RetryPolicy::validated(
            5,
            Duration::from_millis(100),
            Duration::from_millis(10_000),
            true,
            Duration::from_millis(175),
            None,
        )
        .unwrap();
        let p = retrying(inner.clone(), short_deadline, None, clock.clone());
        assert!(p.embed_documents(&one_doc()).is_err());
        // Attempt 1 fails and waits 100ms shrunk by ≤25% jitter (75–100ms,
        // virtual clock: exact); attempt 2 fails; the next wait (200ms
        // shrunk to ≥150ms) would push past the 175ms deadline from any
        // ≥25ms position → give up after 2 calls, jitter-independently.
        assert_eq!(inner.calls.load(Ordering::SeqCst), 2);
        let sleeps = clock.recorded_sleeps();
        assert_eq!(sleeps.len(), 1);
        assert!(sleeps[0] >= Duration::from_millis(75) && sleeps[0] <= Duration::from_millis(100));
    }

    #[test]
    fn cost_cap_ends_the_sequence() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let capped = RetryPolicy::validated(
            5,
            Duration::from_millis(10),
            Duration::from_millis(10_000),
            true,
            Duration::from_millis(60_000),
            Some(2),
        )
        .unwrap();
        let p = retrying(inner.clone(), capped, None, clock);
        assert!(p.embed_documents(&one_doc()).is_err());
        assert_eq!(
            inner.calls.load(Ordering::SeqCst),
            2,
            "cost cap = 2 units, 1 per attempt"
        );
    }

    #[test]
    fn rate_limited_waits_retry_after_feeds_the_gate_and_resumes_after_expiry() {
        // The gate cooldown is REAL time (ProviderGate owns `Instant`), so
        // this test uses the system clock with a short Retry-After: the
        // wait IS the cooldown, and the loop resumes right after it.
        let gate = Arc::new(ProviderGate::new(GateLimits::permissive()));
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::RateLimited {
                retry_after: Duration::from_millis(30),
            }),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let p = RetryingProvider::with_clock(
            inner.clone(),
            policy(2),
            Arc::new(CircuitBreaker::disabled()),
            Some(gate.clone()),
            Arc::new(SystemRetryClock),
        );
        assert!(p.embed_documents(&one_doc()).is_ok());
        assert_eq!(
            inner.calls.load(Ordering::SeqCst),
            2,
            "resumed after the cooldown"
        );
        assert!(
            gate.snapshot().suspended_for.is_none(),
            "cooldown expired by itself"
        );
    }

    #[test]
    fn a_suspended_gate_stops_the_loop_instead_of_parking() {
        let gate = Arc::new(ProviderGate::new(GateLimits::permissive()));
        // A foreign, much longer cooldown is already active.
        gate.note_rate_limited(Duration::from_secs(3_600));
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::RateLimited {
                retry_after: Duration::from_secs(1),
            }),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let p = retrying(inner.clone(), policy(3), Some(gate.clone()), clock.clone());
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::RateLimited { .. })
        ));
        // Exactly ONE provider call and ONE wait; the signal was consumed
        // right after the wait instead of parking against 3_600s.
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(clock.recorded_sleeps(), vec![Duration::from_secs(1)]);
        assert!(gate.snapshot().suspended_for.is_some());
    }

    #[test]
    fn without_respecting_retry_after_the_backoff_is_exponential() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::RateLimited {
                retry_after: Duration::from_secs(5),
            }),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let p = RetryPolicy {
            respect_retry_after: false,
            ..policy(3)
        };
        let p = retrying(inner.clone(), p, None, clock.clone());
        assert!(p.embed_documents(&one_doc()).is_ok());
        let sleeps = clock.recorded_sleeps();
        assert_eq!(sleeps.len(), 2);
        // The 5s Retry-After was ignored; waits follow base/2×base bounds.
        assert!(sleeps[0] <= Duration::from_millis(10));
        assert!(sleeps[1] <= Duration::from_millis(20) && sleeps[1] >= Duration::from_millis(15));
    }

    #[test]
    fn backoff_is_exponential_capped_and_jitter_only_shrinks() {
        let policy = RetryPolicy::validated(
            8,
            Duration::from_millis(100),
            Duration::from_millis(800),
            true,
            Duration::from_secs(60),
            None,
        )
        .unwrap();
        let no_jitter: Vec<u128> = (0..5u32)
            .map(|a| policy.backoff_delay(a, 0.0).as_millis())
            .collect();
        assert_eq!(
            no_jitter,
            vec![100, 200, 400, 800, 800],
            "pure exponential, capped"
        );
        let full_jitter: Vec<u128> = (0..5u32)
            .map(|a| policy.backoff_delay(a, 1.0).as_millis())
            .collect();
        assert_eq!(
            full_jitter,
            vec![75, 150, 300, 600, 600],
            "jitter shaves ≤ 25%, never adds"
        );
        // Rolls outside [0,1] clamp, they do not explode.
        assert_eq!(policy.backoff_delay(0, 42.0), Duration::from_millis(75));
        assert_eq!(policy.backoff_delay(0, -3.0), Duration::from_millis(100));
    }

    #[test]
    fn each_call_gets_a_fresh_sequence_budget_layering() {
        // Layering (call layer): every port call opens a NEW sequence with
        // the FULL budget — nothing leaks between calls.
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let p = retrying(inner.clone(), policy(2), None, clock);
        assert!(p.embed_documents(&one_doc()).is_ok());
        assert!(p.embed_documents(&one_doc()).is_ok());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn disabled_policy_makes_exactly_one_attempt() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let p = retrying(
            inner.clone(),
            RetryPolicy::disabled(),
            None,
            Arc::new(MockRetryClock::new(0)),
        );
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::ServerError)
        ));
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn empty_batch_is_a_no_op_without_attempts_or_breaker_contact() {
        let inner = Arc::new(ScriptedProvider::ok());
        let clock = Arc::new(MockRetryClock::new(0));
        let breaker = Arc::new(CircuitBreaker::new(
            BreakerLimits::validated(1, Duration::from_millis(10)).unwrap(),
        ));
        // A pre-tripped breaker would reject any real attempt.
        breaker.record_failure(&*clock);
        let p = RetryingProvider::with_clock(inner.clone(), policy(3), breaker, None, clock);
        assert_eq!(
            p.embed_documents(&[]).expect("empty is ok"),
            Vec::<Vec<f32>>::new()
        );
        assert_eq!(
            p.embed_queries(&[]).expect("empty is ok"),
            Vec::<Vec<f32>>::new()
        );
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn invalid_policy_combinations_are_rejected_with_named_reasons() {
        assert!(RetryPolicy::validated(
            0,
            Duration::from_millis(1),
            Duration::from_millis(1),
            true,
            Duration::from_millis(1),
            None
        )
        .is_err());
        assert!(RetryPolicy::validated(
            1,
            Duration::ZERO,
            Duration::from_millis(1),
            true,
            Duration::from_millis(1),
            None
        )
        .is_err());
        assert!(RetryPolicy::validated(
            1,
            Duration::from_millis(10),
            Duration::from_millis(5),
            true,
            Duration::from_millis(1),
            None
        )
        .is_err());
        assert!(RetryPolicy::validated(
            1,
            Duration::from_millis(1),
            Duration::from_millis(1),
            true,
            Duration::ZERO,
            None
        )
        .is_err());
        assert!(RetryPolicy::validated(
            1,
            Duration::from_millis(1),
            Duration::from_millis(1),
            true,
            Duration::from_millis(1),
            Some(0)
        )
        .is_err());
        assert!(BreakerLimits::validated(1, Duration::ZERO).is_err());
        assert!(
            BreakerLimits::validated(0, Duration::ZERO).is_ok(),
            "0 threshold = disabled"
        );
        // Config mapping: 0 attempts = disabled, timing keys validated.
        let mut cfg = cc_model::config::SemanticProviderConfig::default();
        assert_eq!(
            RetryPolicy::from_provider_config(&cfg).unwrap(),
            RetryPolicy::disabled()
        );
        cfg.retry_max_attempts = 3;
        cfg.retry_base_backoff_ms = 10_000;
        cfg.retry_max_backoff_ms = 100;
        assert!(RetryPolicy::from_provider_config(&cfg).is_err());
    }

    #[test]
    fn consecutive_failures_open_the_circuit_which_fails_fast_without_provider_contact() {
        let breaker =
            CircuitBreaker::new(BreakerLimits::validated(2, Duration::from_millis(100)).unwrap());
        let clock = MockRetryClock::new(0);
        assert_eq!(breaker.state(&clock), CircuitState::Closed);
        breaker.record_failure(&clock);
        assert_eq!(breaker.consecutive_failures(), 1);
        assert_eq!(
            breaker.state(&clock),
            CircuitState::Closed,
            "below threshold"
        );
        breaker.record_failure(&clock);
        match breaker.state(&clock) {
            CircuitState::Open { remaining } => assert!(remaining <= Duration::from_millis(100)),
            other => panic!("expected open, got {other:?}"),
        }
        assert!(matches!(
            breaker.admit(&clock),
            BreakerAdmission::Rejected { .. }
        ));
    }

    #[test]
    fn success_resets_the_consecutive_failure_count() {
        let breaker =
            CircuitBreaker::new(BreakerLimits::validated(2, Duration::from_millis(100)).unwrap());
        let clock = MockRetryClock::new(0);
        breaker.record_failure(&clock);
        breaker.record_success();
        assert_eq!(breaker.consecutive_failures(), 0);
        breaker.record_failure(&clock);
        assert_eq!(breaker.state(&clock), CircuitState::Closed, "1 < 2");
    }

    #[test]
    fn open_window_elapsed_lazily_becomes_half_open_with_a_single_probe_slot() {
        let breaker =
            CircuitBreaker::new(BreakerLimits::validated(1, Duration::from_millis(50)).unwrap());
        let clock = MockRetryClock::new(0);
        breaker.record_failure(&clock);
        assert!(matches!(breaker.state(&clock), CircuitState::Open { .. }));
        clock.advance(Duration::from_millis(50));
        assert_eq!(
            breaker.state(&clock),
            CircuitState::HalfOpen,
            "lazy transition, no timer"
        );
        // Exactly ONE probe at a time.
        assert_eq!(breaker.admit(&clock), BreakerAdmission::Admitted);
        assert!(matches!(
            breaker.admit(&clock),
            BreakerAdmission::Rejected { .. }
        ));
        // Probe success closes and reopens admission for everyone.
        breaker.record_success();
        assert_eq!(breaker.state(&clock), CircuitState::Closed);
        assert_eq!(breaker.admit(&clock), BreakerAdmission::Admitted);
    }

    #[test]
    fn half_open_probe_failure_reopens_for_a_fresh_window() {
        let breaker =
            CircuitBreaker::new(BreakerLimits::validated(1, Duration::from_millis(50)).unwrap());
        let clock = MockRetryClock::new(0);
        breaker.record_failure(&clock);
        clock.advance(Duration::from_millis(50));
        assert_eq!(breaker.admit(&clock), BreakerAdmission::Admitted);
        clock.advance(Duration::from_millis(10));
        breaker.record_failure(&clock);
        match breaker.state(&clock) {
            CircuitState::Open { remaining } => {
                assert!(
                    remaining <= Duration::from_millis(50),
                    "fresh window, got {remaining:?}"
                );
            }
            other => panic!("expected reopened, got {other:?}"),
        }
    }

    #[test]
    fn disabled_breaker_never_opens_and_neutral_outcomes_change_nothing() {
        let breaker = CircuitBreaker::disabled();
        let clock = MockRetryClock::new(0);
        for _ in 0..100 {
            breaker.record_failure(&clock);
        }
        assert_eq!(breaker.state(&clock), CircuitState::Closed);
        assert_eq!(breaker.admit(&clock), BreakerAdmission::Admitted);
        // Neutral outcomes (429 / invalid input / cancelled) never trip.
        let breaker =
            CircuitBreaker::new(BreakerLimits::validated(1, Duration::from_millis(10)).unwrap());
        breaker.record_neutral();
        breaker.record_neutral();
        assert_eq!(breaker.state(&clock), CircuitState::Closed);
    }

    #[test]
    fn half_open_probe_meeting_a_budget_refusal_does_not_retain_the_probe_slot() {
        // P7-014 batch-2 review finding: `run` admits through the breaker
        // BEFORE the cost-budget check, and in half-open that admission
        // takes the single probe slot. A budget refusal then returned
        // WITHOUT any `record_*` call — the admitted probe stranded the
        // slot and every later attempt was rejected forever (the breaker
        // contract: an admitted caller owes the breaker exactly one
        // record). The refusal path must settle its debt as neutral: the
        // call never reached the provider, so it is neither a success nor
        // a failure signal.
        use crate::admission::{CostBudget, ReceiptLedger};
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::ServerError),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let breaker = Arc::new(CircuitBreaker::new(
            BreakerLimits::validated(2, Duration::from_millis(100)).unwrap(),
        ));
        let ledger = Arc::new(ReceiptLedger::new(32).expect("ledger"));
        // Cost cap 2 = exactly the two tripping attempts; everything after
        // is refused before the call.
        let p = RetryingProvider::with_clock(
            inner.clone(),
            policy(1),
            breaker.clone(),
            None,
            clock.clone(),
        )
        .with_receipts(ledger.clone(), CostBudget::new(Some(2)));
        // Two failing attempts trip the breaker open (one attempt per call).
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::ServerError)
        ));
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::ServerError)
        ));
        assert!(matches!(breaker.state(&*clock), CircuitState::Open { .. }));
        clock.advance(Duration::from_millis(100));
        assert_eq!(breaker.state(&*clock), CircuitState::HalfOpen);
        // The probe is admitted, then the exhausted budget refuses the call
        // before the provider is reached.
        assert!(matches!(
            p.embed_documents(&one_doc()),
            Err(ProviderError::ServerError)
        ));
        assert_eq!(
            inner.calls.load(Ordering::SeqCst),
            2,
            "a budget refusal never reaches the provider"
        );
        // The probe slot is free again: the next admission can take it.
        assert_eq!(
            breaker.admit(&*clock),
            BreakerAdmission::Admitted,
            "a budget-refused probe must not strand the half-open slot"
        );
        // The refusal left its receipt (2 failed attempts + 1 budget refusal).
        assert_eq!(
            ledger.retained_count(),
            3,
            "one receipt per attempt or refusal"
        );
        assert_eq!(
            ledger.retained_receipts()[2].outcome,
            AttemptOutcome::RejectedByBudget,
            "the stranded-probe refusal is billed-certain and recorded"
        );
    }

    #[test]
    fn breaker_and_retrying_provider_are_send_sync_shareable() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CircuitBreaker>();
        assert_send_sync::<RetryingProvider>();
        assert_send_sync::<Arc<CircuitBreaker>>();
    }

    // ── P7-008 cost & uncertain-attempt receipts ──────────────────────────

    use crate::admission::{
        AttemptOutcome, BudgetAdmission, CostBudget, ReceiptLedger, ReceiptPath, UncertainReason,
        UsageReceipt, PROVIDER_ATTEMPT_COST_UNITS,
    };

    fn retrying_receipted(
        inner: Arc<ScriptedProvider>,
        policy: RetryPolicy,
        clock: Arc<MockRetryClock>,
        ledger: Arc<ReceiptLedger>,
        budget: CostBudget,
    ) -> RetryingProvider {
        RetryingProvider::with_clock(
            inner,
            policy,
            Arc::new(CircuitBreaker::disabled()),
            None,
            clock,
        )
        .with_receipts(ledger, budget)
    }

    /// A provider that takes `per_call` of the shared mock clock per call —
    /// makes attempt durations observable without real time.
    struct TimingProvider {
        space: VectorSpace,
        clock: Arc<MockRetryClock>,
        per_call: Duration,
    }

    impl EmbeddingProvider for TimingProvider {
        fn space(&self) -> &VectorSpace {
            &self.space
        }

        fn embed_documents(
            &self,
            _batch: &[DocumentInput],
        ) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.clock.advance(self.per_call);
            Ok(vec![vec![1.0_f32]])
        }

        fn embed_queries(&self, _batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.clock.advance(self.per_call);
            Ok(vec![vec![1.0_f32]])
        }
    }

    #[test]
    fn every_attempt_of_a_retry_sequence_leaves_a_complete_receipt() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::ServerError),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(1_000));
        let ledger = Arc::new(ReceiptLedger::new(32).expect("ledger"));
        let p = retrying_receipted(
            inner.clone(),
            policy(3),
            clock,
            ledger.clone(),
            CostBudget::new(None),
        );
        p.embed_documents(&one_doc()).expect("eventual success");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 3);

        let receipts = ledger.retained_receipts();
        assert_eq!(
            receipts.len(),
            3,
            "one receipt per attempt, retries included"
        );
        let attempts: Vec<u64> = receipts.iter().map(|r| r.attempt).collect();
        assert_eq!(attempts, vec![1, 2, 3]);
        let outcomes: Vec<AttemptOutcome> = receipts.iter().map(|r| r.outcome).collect();
        assert_eq!(
            outcomes,
            vec![
                AttemptOutcome::Failed,
                AttemptOutcome::Failed,
                AttemptOutcome::Succeeded
            ]
        );
        for r in &receipts {
            assert_eq!(r.space_model, MODEL);
            assert_eq!(r.path, ReceiptPath::Documents);
            assert_eq!(r.batch_items, 1);
            // P7-003 estimator over the exact input bytes: "payload" is
            // 7 UTF-8 bytes ⇒ ceil(7/4) = 2 estimated tokens. Reported
            // usage is None (the frozen port does not surface usage):
            // unknown, never zero-filled.
            assert_eq!(
                r.usage,
                UsageReceipt {
                    reported: None,
                    estimated: Some(2),
                    cache_reuse: false
                }
            );
            assert_eq!(r.cost_units, PROVIDER_ATTEMPT_COST_UNITS);
            assert_eq!(r.uncertain, None, "ServerError is not an uncertain outcome");
        }
        assert_eq!(ledger.total_cost_units(), 3 * PROVIDER_ATTEMPT_COST_UNITS);
        let agg = ledger.aggregate(None);
        assert_eq!(agg.succeeded, 1);
        assert_eq!(agg.failed, 2);
        assert_eq!(agg.uncertain_attempts, 0);
    }

    #[test]
    fn receipt_duration_is_measured_on_the_injected_clock() {
        let clock = Arc::new(MockRetryClock::new(5_000));
        let inner = Arc::new(TimingProvider {
            space: space(),
            clock: clock.clone(),
            per_call: Duration::from_millis(7),
        });
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = RetryingProvider::with_clock(
            inner,
            policy(1),
            Arc::new(CircuitBreaker::disabled()),
            None,
            clock,
        )
        .with_receipts(ledger.clone(), CostBudget::new(None));
        p.embed_documents(&one_doc()).expect("ok");
        let receipts = ledger.retained_receipts();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].duration_ms, 7);
    }

    #[test]
    fn breaker_rejection_records_an_uncertain_receipt_without_provider_contact() {
        // Threshold 1: the first failed sequence opens the circuit.
        let breaker = Arc::new(CircuitBreaker::new(
            BreakerLimits::validated(1, Duration::from_secs(60)).expect("limits"),
        ));
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            // Would be consumed if the breaker let the second call through —
            // it must stay untouched.
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = RetryingProvider::with_clock(inner.clone(), policy(1), breaker, None, clock)
            .with_receipts(ledger.clone(), CostBudget::new(None));

        let first = p.embed_documents(&one_doc());
        assert!(first.is_err(), "first sequence fails and trips the breaker");
        let second = p.embed_documents(&one_doc());
        assert!(second.is_err(), "open breaker refuses the second sequence");
        // The refusal never reached the provider.
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);

        let receipts = ledger.retained_receipts();
        assert_eq!(receipts.len(), 2);
        let refusal = &receipts[1];
        assert_eq!(refusal.outcome, AttemptOutcome::RejectedByBreaker);
        assert_eq!(refusal.uncertain, Some(UncertainReason::BreakerOpen));
        assert_eq!(refusal.cost_units, 0, "a refused call charges nothing");
        assert_eq!(refusal.attempt, 1, "each sequence numbers its own attempts");
        assert_eq!(ledger.total_cost_units(), PROVIDER_ATTEMPT_COST_UNITS);
        let agg = ledger.aggregate(None);
        assert_eq!(agg.rejected_by_breaker, 1);
        assert_eq!(agg.uncertain_breaker_open, 1);
    }

    #[test]
    fn timeout_attempts_carry_the_uncertain_marker_and_duplicate_risk() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::Timeout),
            Err(ProviderError::Timeout),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = retrying_receipted(
            inner.clone(),
            policy(2),
            clock,
            ledger.clone(),
            CostBudget::new(None),
        );
        assert!(p.embed_documents(&one_doc()).is_err());

        let receipts = ledger.retained_receipts();
        assert_eq!(receipts.len(), 2);
        for r in &receipts {
            assert_eq!(r.outcome, AttemptOutcome::Failed);
            assert_eq!(
                r.uncertain,
                Some(UncertainReason::TimeoutIndeterminate),
                "after a timeout, server-side processing/billing is undecidable"
            );
        }
        let agg = ledger.aggregate(None);
        assert_eq!(agg.uncertain_timeout, 2);
        // The retry (attempt 2) has unknown reported usage: the observable
        // duplicate-billing risk ("重启重试能看见费用不确定性").
        assert_eq!(agg.unknown_duplicate_risk, 1);
    }

    #[test]
    fn cost_budget_refuses_before_the_call_and_records_the_refusal() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Err(ProviderError::ServerError),
            Err(ProviderError::ServerError),
            // Third call must never happen: the budget stops the sequence
            // before it.
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = retrying_receipted(
            inner.clone(),
            policy(5),
            clock,
            ledger.clone(),
            CostBudget::new(Some(2)),
        );
        let err = p.embed_documents(&one_doc()).unwrap_err();
        assert_eq!(err, ProviderError::ServerError);
        assert_eq!(
            inner.calls.load(Ordering::SeqCst),
            2,
            "refused before the call"
        );

        let receipts = ledger.retained_receipts();
        assert_eq!(receipts.len(), 3);
        assert_eq!(receipts[0].outcome, AttemptOutcome::Failed);
        assert_eq!(receipts[1].outcome, AttemptOutcome::Failed);
        let refusal = &receipts[2];
        assert_eq!(refusal.outcome, AttemptOutcome::RejectedByBudget);
        assert_eq!(refusal.cost_units, 0);
        assert_eq!(refusal.uncertain, None, "a pre-call refusal made no call");
        assert_eq!(ledger.total_cost_units(), 2 * PROVIDER_ATTEMPT_COST_UNITS);
        assert_eq!(ledger.budget_refusals(), 1);
        let agg = ledger.aggregate(None);
        assert_eq!(agg.rejected_by_budget, 1);
    }

    #[test]
    fn receipts_never_contain_input_text_or_credentials() {
        let inner = Arc::new(ScriptedProvider::new(vec![
            Ok(vec![vec![1.0_f32]]),
            Ok(vec![vec![1.0_f32]]),
        ]));
        let clock = Arc::new(MockRetryClock::new(0));
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = retrying_receipted(
            inner,
            policy(1),
            clock,
            ledger.clone(),
            CostBudget::new(None),
        );
        let secret_doc = "SECRET-CORPUS-PAYLOAD 🔑 internal source code snippet";
        let secret_query = "sk-live-abc123do-not-leak";
        p.embed_documents(&[doc(secret_doc)]).expect("ok");
        p.embed_queries(&[query(secret_query)]).expect("ok");

        let receipts = ledger.retained_receipts();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].path, ReceiptPath::Documents);
        assert_eq!(receipts[1].path, ReceiptPath::Queries);
        // The full observable surface — receipts and aggregates — carries
        // counters and labels only.
        let rendered = format!("{:?}\n{:?}", receipts, ledger.aggregate(None));
        assert!(!rendered.contains("SECRET-CORPUS-PAYLOAD"));
        assert!(!rendered.contains("source code snippet"));
        assert!(!rendered.contains("sk-live-abc123"));
        assert!(!rendered.contains("🔑"));
    }

    #[test]
    fn empty_batch_records_no_receipt() {
        let inner = Arc::new(ScriptedProvider::ok());
        let clock = Arc::new(MockRetryClock::new(0));
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = retrying_receipted(
            inner,
            policy(1),
            clock,
            ledger.clone(),
            CostBudget::new(None),
        );
        assert!(p.embed_documents(&[]).unwrap().is_empty());
        assert_eq!(
            ledger.retained_receipts().len(),
            0,
            "no attempt, no receipt"
        );
        assert_eq!(ledger.total_cost_units(), 0);
    }

    #[test]
    fn budget_admission_boundary_is_visible_in_the_receipt_chain() {
        // Cap 1: the very first call is refused before touching the
        // provider (the degrade-precedent shutdown threshold).
        let inner = Arc::new(ScriptedProvider::ok());
        let clock = Arc::new(MockRetryClock::new(0));
        let ledger = Arc::new(ReceiptLedger::new(8).expect("ledger"));
        let p = retrying_receipted(
            inner.clone(),
            policy(1),
            clock,
            ledger.clone(),
            CostBudget::new(Some(0)),
        );
        assert!(p.embed_documents(&one_doc()).is_err());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            ledger.retained_receipts()[0].outcome,
            AttemptOutcome::RejectedByBudget
        );
        assert_eq!(ledger.budget_refusals(), 1);
        // The admission primitive itself stays a pure check.
        assert_eq!(CostBudget::new(Some(1)).admit(0), BudgetAdmission::Allowed);
    }
}
