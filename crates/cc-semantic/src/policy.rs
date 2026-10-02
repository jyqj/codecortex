//! Code-egress and credential policy (P7-007, execution-mechanism leg).
//!
//! Boundary authority: the provider adapter
//! ([`crate::providers::openai_compatible`], P7-001) and its transport seam.
//! This module implements the *mechanisms* of the outbound policy; the
//! policy *text* lives in `docs/CONFIGURATION.md`. Per the user decision
//! D1/D2 (2026-10-02) the live leg — real provider calls, a production
//! transport, and the sensitive-file egress classification matrix — is
//! **conditional blocked** this round; nothing here performs network I/O and
//! nothing here claims an audit/compliance certification.
//!
//! ## Default no-network + explicit opt-in
//!
//! The [`EgressPolicy`] gates provider/transport assembly:
//!
//! - `network_opt_in` (config `semantic.network_opt_in`, default `false`):
//!   assembling a transport behind the adapter without this flag is a
//!   startup-refusing config error. The default is *no network*.
//! - `allow_http` (config `semantic.allow_http`, default `false`): plaintext
//!   `http://` endpoints are rejected unless this flag is set; the default
//!   is *https only*.
//!
//! [`gate_transport_assembly`] is the sanctioned composition-root check and
//! [`GuardedTransport`] is the belt-and-suspenders runtime wrapper: every
//! request is re-checked (scheme admission, positive timeout) before the
//! inner transport is touched. Redirects are never followed — the adapter
//! gate rejects any 3xx as non-retryable `InvalidInput`, and the
//! [`EmbeddingHttpTransport`] contract (clause 2 of the trait docs) binds
//! every implementation to never auto-follow and to strip the
//! `Authorization` header before any redirect hop if its underlying client
//! cannot disable auto-follow.
//!
//! ## Declared egress surface (audit mechanism)
//!
//! Exactly what one `/embeddings` request sends, item by item:
//!
//! 1. **input text bytes** — the batch texts (rendered document chunks or
//!    query text; these MAY contain source code, that is the declared
//!    content of an embedding call);
//! 2. **model name** — the frozen `space.model_id()`;
//! 3. **`encoding_format: "float"`** — the only admitted format;
//! 4. **`dimensions`** — *probe requests only*, and only in
//!    `dimensions_mode = "configurable"` (adapter embed requests never send
//!    it).
//!
//! Headers: exactly `Content-Type`, `Authorization` (the credential's only
//! travel path) and `Accept`. Nothing else leaves the process: no telemetry,
//! no extra fields, no tracing. [`audit_egress`] mechanically verifies one
//! request against this declaration (exact body key set, exact header name
//! set, https scheme, positive timeout, key material nowhere except the
//! `Authorization` header).
//!
//! ## Credentials: external references only
//!
//! Configuration stores a *reference* (`semantic.api_key_ref`), never the
//! secret. [`resolve_key_reference`] resolves `env:NAME` or `file:PATH`
//! forms into an [`EmbeddingApiKey`] at the call site; errors name the
//! reference, never the resolved value. The key is held in memory for the
//! shortest practical lifetime (no statics, no caching, no persistence;
//! `Debug` is fixed-redacted and there is no `Display`). Explicit
//! zero-on-drop is *not* implemented this round (it would require a new
//! dependency under the offline closure) — stated honestly here and in the
//! policy text rather than claimed.

use std::sync::Arc;

use cc_model::{CcError, CcResult};

use crate::providers::openai_compatible::{
    EmbeddingApiKey, EmbeddingHttpTransport, HttpRequest, TransportError,
};

/// Outbound-network policy switches (P7-007). Defaults are the conservative
/// closed state: no network, https only (`bool`'s `false` default IS the
/// closed state, by construction).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EgressPolicy {
    /// Explicit operator opt-in for building any provider transport at all
    /// (config `semantic.network_opt_in`). `false` (the default) = the
    /// no-network default: transport assembly is refused.
    pub network_opt_in: bool,
    /// Explicit operator opt-in for plaintext `http://` endpoints (config
    /// `semantic.allow_http`). `false` (the default) = https only.
    pub allow_http: bool,
}

impl EgressPolicy {
    /// Reads the policy switches from the `semantic.*` config section.
    pub fn from_provider_config(config: &cc_model::config::SemanticProviderConfig) -> Self {
        Self {
            network_opt_in: config.network_opt_in,
            allow_http: config.allow_http,
        }
    }

    /// Endpoint-scheme admission: `https://` always; `http://` only under the
    /// explicit `allow_http` opt-in. Anything else is a config error.
    pub fn validate_endpoint(&self, endpoint: &str) -> CcResult<()> {
        let trimmed = endpoint.trim_end_matches('/');
        if trimmed.starts_with("https://") {
            return Ok(());
        }
        if trimmed.starts_with("http://") {
            if self.allow_http {
                return Ok(());
            }
            return Err(CcError::Config(
                "semantic.allow_http is false: plaintext http endpoints are rejected by the \
                 egress policy (default https-only); use an https endpoint or set \
                 semantic.allow_http = true explicitly"
                    .into(),
            ));
        }
        Err(CcError::Config(format!(
            "endpoint must be an http(s) URL, got {endpoint:?}"
        )))
    }
}

/// Sanctioned composition-root gate for provider/transport assembly
/// (P7-007 "未 opt-in 不构造 transport"). Returns the transport to inject,
/// wrapped in the [`GuardedTransport`] policy guard, or a startup-refusing
/// config error:
///
/// - transport present but `network_opt_in` false → error (the no-network
///   default must not be bypassed by wiring a transport anyway);
/// - opted in but no transport → error (an opt-in with no transport is a
///   half-configured state, not a silent disabled);
/// - endpoint scheme rejected by [`EgressPolicy::validate_endpoint`].
pub fn gate_transport_assembly(
    policy: &EgressPolicy,
    endpoint: &str,
    transport: Option<Arc<dyn EmbeddingHttpTransport>>,
) -> CcResult<Option<Arc<dyn EmbeddingHttpTransport>>> {
    match (&transport, policy.network_opt_in) {
        (Some(_), false) => {
            return Err(CcError::Config(
                "semantic.network_opt_in is false (default no-network): refusing to assemble a \
                 provider transport; set semantic.network_opt_in = true to opt in explicitly"
                    .into(),
            ));
        }
        (None, true) => {
            return Err(CcError::Config(
                "semantic.network_opt_in is true but no transport is configured: supply a \
                 transport at the composition root or set network_opt_in back to false"
                    .into(),
            ));
        }
        _ => {}
    }
    policy.validate_endpoint(endpoint)?;
    Ok(transport.map(|inner| {
        Arc::new(GuardedTransport::new(inner, policy.clone()))
            as Arc<dyn EmbeddingHttpTransport>
    }))
}

/// Runtime policy guard around any [`EmbeddingHttpTransport`]. Enforces, per
/// request, the invariants the egress policy promises — regardless of what
/// the wrapped transport would do on its own:
///
/// 1. **scheme**: the request URL must pass [`EgressPolicy::validate_endpoint`];
/// 2. **timeout mandatory**: a zero timeout is refused (the adapter always
///    sets one; a zero would mean "no deadline" for a real transport).
///
/// Redirect handling is deliberately NOT re-intercepted here: the adapter
/// gate maps any 3xx to a non-retryable `InvalidInput` (retrying a redirect
/// burns budget on a config error), and the no-follow obligation on real
/// transports is clause 2 of the [`EmbeddingHttpTransport`] contract.
///
/// Error messages carry no URL and no header material.
pub struct GuardedTransport {
    inner: Arc<dyn EmbeddingHttpTransport>,
    policy: EgressPolicy,
}

impl GuardedTransport {
    pub fn new(inner: Arc<dyn EmbeddingHttpTransport>, policy: EgressPolicy) -> Self {
        Self { inner, policy }
    }
}

impl EmbeddingHttpTransport for GuardedTransport {
    fn post_json(&self, request: HttpRequest) -> Result<crate::providers::openai_compatible::HttpResponse, TransportError> {
        if request.timeout.is_zero() {
            return Err(TransportError::Io(
                "egress policy rejected the request: timeout is mandatory (zero timeout refused)"
                    .into(),
            ));
        }
        if self.policy.validate_endpoint(&request.url).is_err() {
            return Err(TransportError::Io(
                "egress policy rejected the request: endpoint scheme not permitted by the \
                 outbound policy"
                    .into(),
            ));
        }
        self.inner.post_json(request)
    }
}

/// The declared outbound surface of one request — what the policy text
/// promises may leave the process. [`audit_egress`] checks a concrete
/// request against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDeclaration {
    /// The exact destination URL of the request.
    pub destination: String,
    /// The exact set of JSON body fields (no more, no fewer).
    pub body_fields: Vec<String>,
    /// The exact set of header names (case-insensitive, no more, no fewer).
    pub header_names: Vec<String>,
}

impl EgressDeclaration {
    /// The declared surface of an adapter `/embeddings` embed request:
    /// body fields exactly `model`, `input`, `encoding_format`; headers
    /// exactly `Content-Type`, `Authorization`, `Accept`. `endpoint_url` is
    /// the full request URL (`endpoint + "/embeddings"`).
    pub fn embeddings_request(endpoint_url: impl Into<String>) -> Self {
        Self {
            destination: endpoint_url.into(),
            body_fields: vec![
                "model".to_owned(),
                "input".to_owned(),
                "encoding_format".to_owned(),
            ],
            header_names: vec![
                "content-type".to_owned(),
                "authorization".to_owned(),
                "accept".to_owned(),
            ],
        }
    }

    /// The declared surface of a capability-probe request (P7-002): the
    /// embed surface plus the explicit `dimensions` field when the probe
    /// runs in `dimensions_mode = "configurable"` (the `with_dimensions`
    /// flag — `false` under `fixed` mode, where the field is declared away).
    pub fn probe_request(endpoint_url: impl Into<String>, with_dimensions: bool) -> Self {
        let mut declaration = Self::embeddings_request(endpoint_url);
        if with_dimensions {
            declaration.body_fields.push("dimensions".to_owned());
        }
        declaration
    }
}

/// Mechanically audits one request against its declared egress surface.
/// Every deviation is a config-level error naming the deviation (never the
/// credential, never the payload):
///
/// - destination URL must match the declaration and pass the scheme policy;
/// - the JSON body must be an object whose key set is *exactly*
///   `declaration.body_fields` (missing and unexpected fields both fail);
/// - the header name set must be exactly `declaration.header_names`;
/// - the timeout must be positive (timeout mandatory);
/// - the credential material must appear **only** in the `Authorization`
///   header (never in the body, never in any other header).
pub fn audit_egress(
    request: &HttpRequest,
    declaration: &EgressDeclaration,
    api_key: &EmbeddingApiKey,
    policy: &EgressPolicy,
) -> CcResult<()> {
    policy.validate_endpoint(&request.url)?;
    if request.url != declaration.destination {
        return Err(CcError::Config(
            "egress audit: request URL does not match the declared destination".into(),
        ));
    }
    if request.timeout.is_zero() {
        return Err(CcError::Config(
            "egress audit: timeout is mandatory, got a zero per-request timeout".into(),
        ));
    }

    let body: serde_json::Value = serde_json::from_slice(&request.body).map_err(|_| {
        CcError::Config("egress audit: request body is not valid JSON".into())
    })?;
    let object = body.as_object().ok_or_else(|| {
        CcError::Config("egress audit: request body is not a JSON object".into())
    })?;
    let mut actual: Vec<String> = object.keys().cloned().collect();
    let mut expected = declaration.body_fields.clone();
    actual.sort();
    expected.sort();
    let unexpected: Vec<&String> = actual.iter().filter(|k| !expected.contains(k)).collect();
    if !unexpected.is_empty() {
        let names: Vec<&str> = unexpected.iter().map(|s| s.as_str()).collect();
        return Err(CcError::Config(format!(
            "egress audit: request body carries fields outside the declared surface: {names:?}"
        )));
    }
    let missing: Vec<&String> = expected.iter().filter(|k| !actual.contains(k)).collect();
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
        return Err(CcError::Config(format!(
            "egress audit: request body is missing declared fields: {names:?}"
        )));
    }

    let mut header_names: Vec<String> = request
        .headers
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    header_names.sort();
    let mut expected_headers = declaration.header_names.clone();
    expected_headers.sort();
    if header_names != expected_headers {
        return Err(CcError::Config(format!(
            "egress audit: header set does not match the declared surface (expected \
             {expected_headers:?}, got {header_names:?})"
        )));
    }

    let secret = api_key.expose_secret();
    if !secret.is_empty() {
        let header_ok = request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.as_str() == format!("Bearer {secret}")
        });
        if !header_ok {
            return Err(CcError::Config(
                "egress audit: the Authorization header does not carry the configured \
                 bearer credential"
                    .into(),
            ));
        }
        let body_text = String::from_utf8_lossy(&request.body);
        if body_text.contains(secret) {
            return Err(CcError::Config(
                "egress audit: credential material found in the request body (the key may \
                 only travel in the Authorization header)"
                    .into(),
            ));
        }
        for (name, value) in &request.headers {
            if !name.eq_ignore_ascii_case("authorization") && value.contains(secret) {
                return Err(CcError::Config(
                    "egress audit: credential material found outside the Authorization header"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

/// Resolves a credential *reference* (config `semantic.api_key_ref`) into
/// the in-memory [`EmbeddingApiKey`]. Supported forms:
///
/// - `env:NAME` — read the environment variable `NAME`;
/// - `file:PATH` — read the file at `PATH` (trailing whitespace trimmed).
///
/// Errors name the reference (and, for `env:`, the variable name) but never
/// echo a resolved value; a missing or empty resolution is a config error,
/// never a silent fallback. The returned key has the shortest practical
/// lifetime: no statics, no caching, no persistence — the caller drops it
/// after provider assembly. Explicit zero-on-drop is not implemented this
/// round (see the module docs).
pub fn resolve_key_reference(api_key_ref: &str) -> CcResult<EmbeddingApiKey> {
    resolve_key_reference_with(
        api_key_ref,
        |name| std::env::var(name).ok(),
        |path| std::fs::read_to_string(path).map_err(|error| error.to_string()),
    )
}

/// Deterministic core of [`resolve_key_reference`] with injected
/// environment and filesystem accessors (the public function delegates with
/// the real `std` implementations; tests inject fakes).
pub fn resolve_key_reference_with(
    api_key_ref: &str,
    env_lookup: impl Fn(&str) -> Option<String>,
    read_file: impl Fn(&str) -> Result<String, String>,
) -> CcResult<EmbeddingApiKey> {
    let reference = api_key_ref.trim();
    if reference.is_empty() {
        return Err(CcError::Config(
            "semantic.api_key_ref is empty: provide an external reference like `env:NAME` \
             or `file:PATH` (the secret itself never enters the configuration)"
                .into(),
        ));
    }
    if let Some(name) = reference.strip_prefix("env:") {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(CcError::Config(format!(
                "semantic.api_key_ref has an invalid env reference {reference:?}: the \
                 variable name must be non-empty whitespace-free"
            )));
        }
        let value = env_lookup(name).unwrap_or_default();
        if value.trim().is_empty() {
            return Err(CcError::Config(format!(
                "semantic.api_key_ref points at environment variable {name:?} which is \
                 unset or empty"
            )));
        }
        return Ok(EmbeddingApiKey::new(value));
    }
    if let Some(path) = reference.strip_prefix("file:") {
        let path = path.trim();
        if path.is_empty() {
            return Err(CcError::Config(
                "semantic.api_key_ref has an invalid file reference: the path is empty".into(),
            ));
        }
        let value = read_file(path)
            .map_err(|error| {
                CcError::Config(format!(
                    "semantic.api_key_ref file reference {path:?} could not be read: {error}"
                ))
            })?;
        let value = value.trim();
        if value.is_empty() {
            return Err(CcError::Config(format!(
                "semantic.api_key_ref file reference {path:?} is empty"
            )));
        }
        return Ok(EmbeddingApiKey::new(value.to_owned()));
    }
    Err(CcError::Config(format!(
        "semantic.api_key_ref uses an unsupported form ({schema}): only `env:NAME` and \
         `file:PATH` external references are supported — an inline secret value is rejected \
         and never echoed",
        schema = match reference.split_once(':') {
            Some((prefix, _)) => format!("prefix {prefix:?}"),
            None => "an inline value was supplied".to_owned(),
        }
    )))
}

/// Redacts bearer credentials from a diagnostic string before it may reach
/// a log: every `Bearer <token>` run (case-insensitive scheme match, token
/// at least [`MIN_REDACTED_TOKEN_CHARS`] characters) has its token replaced
/// with `[REDACTED]`. Adversarial inputs — error text that somehow embedded
/// an `Authorization` header value — come out clean. The length floor keeps
/// ordinary prose ("the bearer of the token ring") untouched; real bearer
/// tokens are far longer than the floor.
pub fn redact_for_log(input: &str) -> String {
    let needle = "bearer";
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut scan = lower.as_str();
    while let Some(position) = scan.find(needle) {
        let after = &scan[position + needle.len()..];
        // Scheme must be followed by whitespace and then a credential-length
        // token of non-whitespace characters.
        let trimmed = after.trim_start();
        let whitespace_len = after.len() - trimmed.len();
        let token_chars = trimmed.chars().take_while(|ch| !ch.is_whitespace()).count();
        if whitespace_len == 0 || trimmed.is_empty() || token_chars < MIN_REDACTED_TOKEN_CHARS {
            out.push_str(&rest[..position + needle.len()]);
            rest = &rest[position + needle.len()..];
            scan = &scan[position + needle.len()..];
            continue;
        }
        let token_len = trimmed
            .chars()
            .take_while(|ch| !ch.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        let token_start_in_rest = position + needle.len() + whitespace_len;
        out.push_str(&rest[..token_start_in_rest]);
        out.push_str("[REDACTED]");
        rest = &rest[token_start_in_rest + token_len..];
        scan = &scan[token_start_in_rest + token_len..];
    }
    out.push_str(rest);
    out
}

/// Minimum token length [`redact_for_log`] treats as a credential (real
/// bearer tokens are much longer; the floor keeps prose intact).
const MIN_REDACTED_TOKEN_CHARS: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::EmbeddingProvider;
    use crate::providers::openai_compatible::{
        OpenAiCompatibleConfig, OpenAiCompatibleProvider,
    };
    use crate::types::VectorSpace;
    use std::sync::Mutex;
    use std::time::Duration;

    const MODEL: &str = "text-embedding-test-001";
    const KEY: &str = "sk-test-secret-material";

    fn space() -> VectorSpace {
        VectorSpace::new(MODEL, 4).expect("valid space")
    }

    fn adapter(endpoint: &str, policy: EgressPolicy) -> OpenAiCompatibleProvider {
        let mut config = OpenAiCompatibleConfig::new(
            space(),
            endpoint,
            EmbeddingApiKey::new(KEY),
        );
        config.egress = policy;
        OpenAiCompatibleProvider::new(config)
    }

    fn https_adapter() -> OpenAiCompatibleProvider {
        adapter(
            "https://provider.invalid/v1",
            EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
    }

    fn audit_adapter_request(
        provider: &OpenAiCompatibleProvider,
        texts: &[&str],
    ) -> CcResult<()> {
        let request = provider.build_request(&texts.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        let declaration = EgressDeclaration::embeddings_request(request.url.clone());
        audit_egress(
            &request,
            &declaration,
            &EmbeddingApiKey::new(KEY),
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
    }

    // ── Egress-surface audit (outbound-surface inventory) ─────────────────

    #[test]
    fn embed_request_egress_surface_is_exactly_the_declaration() {
        // P7-001 pinned the request shape; this is the policy-level audit
        // point: the serialized body carries EXACTLY the declared fields
        // (input text bytes, model name, encoding_format), the header set is
        // exactly {Content-Type, Authorization, Accept}, and the audit passes.
        let provider = https_adapter();
        audit_adapter_request(&provider, &["alpha chunk", "beta chunk"])
            .expect("the embed request matches its declared egress surface");
    }

    #[test]
    fn an_extra_body_field_fails_the_audit_and_names_the_field() {
        let provider = https_adapter();
        let mut request = provider.build_request(&vec!["alpha".to_owned()]);
        let mut body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        body["telemetry_session"] = serde_json::json!("abc");
        request.body = serde_json::to_vec(&body).unwrap();
        let error = audit_egress(
            &request,
            &EgressDeclaration::embeddings_request(request.url.clone()),
            &EmbeddingApiKey::new(KEY),
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("telemetry_session"), "got: {message}");
        assert!(message.contains("outside the declared surface"), "got: {message}");
    }

    #[test]
    fn a_missing_body_field_fails_the_audit() {
        let provider = https_adapter();
        let mut request = provider.build_request(&vec!["alpha".to_owned()]);
        let mut body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        body.as_object_mut().unwrap().remove("encoding_format");
        request.body = serde_json::to_vec(&body).unwrap();
        let error = audit_egress(
            &request,
            &EgressDeclaration::embeddings_request(request.url.clone()),
            &EmbeddingApiKey::new(KEY),
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing declared fields"));
    }

    #[test]
    fn credential_material_may_only_travel_in_the_authorization_header() {
        let provider = https_adapter();
        let mut request = provider.build_request(&vec!["alpha".to_owned()]);
        let mut body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        // Adversarial: a code path accidentally folded the key into the body.
        body["input"] = serde_json::json!([format!("alpha {KEY}")]);
        request.body = serde_json::to_vec(&body).unwrap();
        let error = audit_egress(
            &request,
            &EgressDeclaration::embeddings_request(request.url.clone()),
            &EmbeddingApiKey::new(KEY),
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("credential material found in the request body")
        );
    }

    #[test]
    fn audit_rejects_http_destinations_unless_http_is_opted_in() {
        // Build the request from an https provider, then swap the URL — the
        // default-closed policy (constructor-legal, no transport) must reject
        // the plaintext destination.
        let provider = https_adapter();
        let mut request = provider.build_request(&vec!["alpha".to_owned()]);
        let closed = EgressPolicy {
            network_opt_in: true,
            allow_http: false,
        };
        let plaintext = EgressPolicy {
            network_opt_in: true,
            allow_http: true,
        };
        let declaration = EgressDeclaration::embeddings_request(request.url.clone());
        let https_request = request.clone();

        request.url = "http://provider.invalid/v1/embeddings".to_owned();
        assert!(
            audit_egress(
                &request,
                &EgressDeclaration::embeddings_request(request.url.clone()),
                &EmbeddingApiKey::new(KEY),
                &closed,
            )
            .is_err(),
            "plaintext destinations fail the closed policy"
        );
        audit_egress(
            &request,
            &EgressDeclaration::embeddings_request(request.url.clone()),
            &EmbeddingApiKey::new(KEY),
            &plaintext,
        )
        .expect("allow_http opt-in admits the plaintext endpoint");
        audit_egress(
            &https_request,
            &declaration,
            &EmbeddingApiKey::new(KEY),
            &closed,
        )
        .expect("https needs no extra opt-in");
    }

    #[test]
    fn audit_rejects_a_zero_timeout() {
        let provider = https_adapter();
        let mut request = provider.build_request(&vec!["alpha".to_owned()]);
        request.timeout = Duration::ZERO;
        let error = audit_egress(
            &request,
            &EgressDeclaration::embeddings_request(request.url.clone()),
            &EmbeddingApiKey::new(KEY),
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("timeout is mandatory"));
    }

    #[test]
    fn the_header_surface_is_audited() {
        let provider = https_adapter();
        let mut request = provider.build_request(&vec!["alpha".to_owned()]);
        request
            .headers
            .push(("X-Extra-Trace".to_owned(), "1".to_owned()));
        let error = audit_egress(
            &request,
            &EgressDeclaration::embeddings_request(request.url.clone()),
            &EmbeddingApiKey::new(KEY),
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("header set does not match"));
    }

    // ── Credential references ─────────────────────────────────────────────

    fn env_of<'a>(fixed: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name: &str| {
            fixed
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn env_reference_resolves_from_the_lookup_and_debug_stays_redacted() {
        let key = resolve_key_reference_with(
            "env:CODECORTEX_TEST_KEY",
            env_of(&[("CODECORTEX_TEST_KEY", "sk-env-secret-value")]),
            |_| Err("not used".into()),
        )
        .expect("env reference resolves");
        assert_eq!(key.expose_secret(), "sk-env-secret-value");
        assert_eq!(format!("{key:?}"), "EmbeddingApiKey([REDACTED])");
    }

    #[test]
    fn file_reference_resolves_and_trims_trailing_newlines() {
        let key = resolve_key_reference_with(
            "file:/run/secrets/embedding_key",
            |_| None,
            |path| {
                assert_eq!(path, "/run/secrets/embedding_key");
                Ok("sk-file-secret-value\n".to_owned())
            },
        )
        .expect("file reference resolves");
        assert_eq!(key.expose_secret(), "sk-file-secret-value");
    }

    #[test]
    fn every_resolution_failure_names_the_reference_and_never_echoes_values() {
        // Full-path leak scan: Display AND Debug of every error path must
        // contain the reference (actionable) but never a resolved value.
        let cases: Vec<(&str, Box<dyn Fn() -> CcResult<EmbeddingApiKey>>)> = vec![
            (
                "missing env",
                Box::new(|| {
                    resolve_key_reference_with(
                        "env:CODECORTEX_TEST_KEY",
                        env_of(&[]),
                        |_| Err("no".into()),
                    )
                }),
            ),
            (
                "empty env value",
                Box::new(|| {
                    resolve_key_reference_with(
                        "env:CODECORTEX_TEST_KEY",
                        env_of(&[("CODECORTEX_TEST_KEY", "   ")]),
                        |_| Err("no".into()),
                    )
                }),
            ),
            (
                "missing file",
                Box::new(|| {
                    resolve_key_reference_with("file:/run/secrets/k", |_| None, |_| {
                        Err("No such file or directory (os error 2)".into())
                    })
                }),
            ),
            (
                "empty file",
                Box::new(|| {
                    resolve_key_reference_with("file:/run/secrets/k", |_| None, |_| {
                        Ok("\n \n".to_owned())
                    })
                }),
            ),
            (
                "inline secret rejected",
                Box::new(|| {
                    resolve_key_reference_with("sk-inline-secret-value", |_| None, |_| {
                        Err("no".into())
                    })
                }),
            ),
            (
                "empty reference",
                Box::new(|| resolve_key_reference_with("  ", |_| None, |_| Err("no".into()))),
            ),
            (
                "malformed env name",
                Box::new(|| {
                    resolve_key_reference_with("env:MY KEY", |_| None, |_| Err("no".into()))
                }),
            ),
        ];
        for (label, resolve) in cases {
            let error = resolve().expect_err(label);
            let rendered = format!("{error}");
            let debugged = format!("{error:?}");
            for surface in [&rendered, &debugged] {
                assert!(
                    !surface.contains("sk-"),
                    "{label}: error surface leaks secret-looking material: {surface}"
                );
            }
            // Actionability: at least the reference or the config key is named.
            assert!(
                rendered.contains("api_key_ref") || rendered.contains("env:") || rendered.contains("file:"),
                "{label}: error must name the reference/key: {rendered}"
            );
        }
    }

    #[test]
    fn errors_that_embed_a_key_value_are_redacted_by_redact_for_log() {
        let leaky = format!(
            "provider call failed: Authorization: Bearer {KEY} rejected with http 401 \
             (retry-after seen as Bearer sk-other-token-42)"
        );
        let redacted = redact_for_log(&leaky);
        assert!(!redacted.contains(KEY), "still leaks: {redacted}");
        assert!(!redacted.contains("sk-other-token-42"), "still leaks: {redacted}");
        assert_eq!(redacted.matches("[REDACTED]").count(), 2, "got: {redacted}");
        // Innocent text is untouched.
        assert_eq!(redact_for_log("plain failure, no credentials"), "plain failure, no credentials");
        // A bare "bearer" word with no token is left alone.
        assert_eq!(redact_for_log("the bearer of the token ring"), "the bearer of the token ring");
    }

    // ── Transport guard ───────────────────────────────────────────────────

    #[derive(Default)]
    struct CountingTransport {
        calls: Mutex<usize>,
        respond: Mutex<Option<crate::providers::openai_compatible::HttpResponse>>,
    }

    impl CountingTransport {
        fn with_response(status: u16) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(0),
                respond: Mutex::new(Some(crate::providers::openai_compatible::HttpResponse {
                    status,
                    headers: Vec::new(),
                    body: Vec::new(),
                })),
            })
        }

        fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    impl EmbeddingHttpTransport for CountingTransport {
        fn post_json(
            &self,
            _request: HttpRequest,
        ) -> Result<crate::providers::openai_compatible::HttpResponse, TransportError> {
            *self.calls.lock().unwrap() += 1;
            let response = self.respond.lock().unwrap().take().expect("one response");
            Ok(response)
        }
    }

    fn https_request() -> HttpRequest {
        HttpRequest {
            url: "https://provider.invalid/v1/embeddings".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn guard_rejects_http_requests_before_the_inner_transport_is_touched() {
        let inner = CountingTransport::with_response(200);
        let guard = GuardedTransport::new(inner.clone(), EgressPolicy::default());
        let error = guard
            .post_json(HttpRequest {
                url: "http://provider.invalid/v1/embeddings".to_owned(),
                ..https_request()
            })
            .unwrap_err();
        assert!(matches!(error, TransportError::Io(message) if message.contains("scheme not permitted")));
        assert_eq!(inner.calls(), 0);
    }

    #[test]
    fn guard_rejects_a_zero_timeout_before_the_inner_transport_is_touched() {
        let inner = CountingTransport::with_response(200);
        let guard = GuardedTransport::new(inner.clone(), EgressPolicy::default());
        let error = guard
            .post_json(HttpRequest {
                timeout: Duration::ZERO,
                ..https_request()
            })
            .unwrap_err();
        assert!(matches!(error, TransportError::Io(message) if message.contains("timeout is mandatory")));
        assert_eq!(inner.calls(), 0);
    }

    #[test]
    fn guard_passes_https_2xx_through_unchanged() {
        let inner = CountingTransport::with_response(200);
        let guard = GuardedTransport::new(inner.clone(), EgressPolicy::default());
        let response = guard.post_json(https_request()).expect("passes");
        assert_eq!(response.status, 200);
        assert_eq!(inner.calls(), 1);
    }

    // ── Composition-root gate ─────────────────────────────────────────────

    fn transport() -> Arc<dyn EmbeddingHttpTransport> {
        CountingTransport::with_response(200)
    }

    #[test]
    fn gate_refuses_transport_assembly_without_network_opt_in() {
        let error = gate_transport_assembly(
            &EgressPolicy::default(),
            "https://provider.invalid/v1",
            Some(transport()),
        )
        .err()
        .expect("gate refuses transport without opt-in");
        let message = error.to_string();
        assert!(message.contains("semantic.network_opt_in"), "got: {message}");
        assert!(message.contains("default no-network"), "got: {message}");
    }

    #[test]
    fn gate_refuses_opted_in_but_missing_transport() {
        let error = gate_transport_assembly(
            &EgressPolicy {
                network_opt_in: true,
                allow_http: false,
            },
            "https://provider.invalid/v1",
            None,
        )
        .err()
        .expect("gate refuses an opt-in with no transport");
        assert!(error.to_string().contains("no transport is configured"));
    }

    #[test]
    fn gate_rejects_http_endpoints_unless_allow_http_is_set() {
        let opted_in = EgressPolicy {
            network_opt_in: true,
            allow_http: false,
        };
        assert!(
            gate_transport_assembly(&opted_in, "http://provider.invalid/v1", Some(transport()))
                .is_err()
        );
        gate_transport_assembly(
            &EgressPolicy {
                network_opt_in: true,
                allow_http: true,
            },
            "http://provider.invalid/v1",
            Some(transport()),
        )
        .expect("explicit allow_http admits plaintext");
        gate_transport_assembly(&opted_in, "https://provider.invalid/v1", Some(transport()))
            .expect("https needs no extra opt-in");
    }

    #[test]
    fn gate_with_default_policy_and_no_transport_is_the_normal_disabled_path() {
        gate_transport_assembly(
            &EgressPolicy::default(),
            "https://provider.invalid/v1",
            None,
        )
        .expect("the default closed state assembles nothing and is not an error");
    }

    #[test]
    fn adapter_constructor_refuses_transport_without_network_opt_in() {
        // The fail-closed default (opt-in false) plus an injected transport is
        // a wiring contradiction: the constructor fails fast at composition.
        let mut config = OpenAiCompatibleConfig::new(space(), "https://provider.invalid/v1", EmbeddingApiKey::new(KEY));
        config.transport = Some(transport());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            OpenAiCompatibleProvider::new(config)
        }))
        .is_err());
    }

    #[test]
    fn disabled_default_provider_still_constructs_and_fails_closed() {
        // The default (no opt-in, no transport) keeps the P7-001 disabled
        // semantics exactly.
        let provider = adapter("https://provider.invalid/v1", EgressPolicy::default());
        let input = crate::ports::DocumentInput::from_bytes(b"alpha").expect("valid");
        let error = provider.embed_documents(&[input]).unwrap_err();
        assert!(matches!(error, crate::ports::ProviderError::InvalidInput(message) if message.contains("disabled")));
    }
}
