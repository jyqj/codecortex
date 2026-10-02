//! Lazy composition of the OpenAI-compatible provider.
//!
//! The caller owns the HTTP implementation. Both injected factories are
//! invoked only after explicit enablement, network opt-in and validation.
//! Construction performs no embedding request, probe, or credential logging.
//! Retry/gate decoration remains the runtime composition root's responsibility.

use std::sync::Arc;

use cc_model::config::SemanticProviderConfig;
use cc_model::{CcError, CcResult};
use cc_semantic::capability::resolve_provider;
use cc_semantic::policy::{resolve_key_reference, resolve_key_reference_with, EgressPolicy};
use cc_semantic::ports::EmbeddingProvider;
use cc_semantic::providers::openai_compatible::{
    EmbeddingApiKey, EmbeddingHttpTransport, OpenAiCompatibleConfig, OpenAiCompatibleProvider,
};

/// Assemble with the standard env:/file: credential resolver and a lazy
/// transport factory. A disabled/no-network configuration has zero I/O.
pub fn build_embedding_provider(
    config: &SemanticProviderConfig,
    build_transport: impl FnOnce() -> CcResult<Arc<dyn EmbeddingHttpTransport>>,
) -> CcResult<Option<Arc<dyn EmbeddingProvider>>> {
    build_embedding_provider_with(config, resolve_key_reference, build_transport)
}

/// Injectable composition seam for local HTTP or in-memory acceptance tests.
/// Inert configuration returns None before validation or either factory runs.
/// Enabled, opted-in invalid configuration returns an error before either
/// factory runs. Factories must not perform embedding requests at construction.
pub fn build_embedding_provider_with(
    config: &SemanticProviderConfig,
    resolve_key: impl FnOnce(&str) -> CcResult<EmbeddingApiKey>,
    build_transport: impl FnOnce() -> CcResult<Arc<dyn EmbeddingHttpTransport>>,
) -> CcResult<Option<Arc<dyn EmbeddingProvider>>> {
    if !config.enabled || !config.network_opt_in {
        return Ok(None);
    }
    let (space, _) = resolve_provider(config)?;
    let egress = EgressPolicy::from_provider_config(config);
    validate_endpoint(&config.endpoint)?;
    egress.validate_endpoint(&config.endpoint)?;
    let reference = config.api_key_ref.as_deref().ok_or_else(|| {
        CcError::Config("semantic.api_key_ref is required when network is enabled".into())
    })?;
    // Reuse the sanctioned reference parser with inert accessors: malformed
    // references are rejected before the real resolver/transport is touched.
    resolve_key_reference_with(
        reference,
        |_| Some("validation-only".into()),
        |_| Ok("validation-only".into()),
    )
    .map_err(|_| CcError::Config("semantic.api_key_ref must use env:NAME or file:PATH".into()))?;
    let key = resolve_key(reference)?;
    if key.expose_secret().trim().is_empty() || key.expose_secret().chars().any(char::is_control) {
        return Err(CcError::Config(
            "semantic.api_key_ref resolved an invalid HTTP credential".into(),
        ));
    }
    let transport = build_transport()?;
    let mut provider_config = OpenAiCompatibleConfig::new(space, config.endpoint.clone(), key);
    provider_config.egress = egress;
    provider_config.transport = Some(transport);
    Ok(Some(Arc::new(OpenAiCompatibleProvider::new(
        provider_config,
    ))))
}

// Endpoint admission supplements the existing scheme gate without reading
// credentials or reflecting a possibly credential-bearing URL into errors.
fn validate_endpoint(endpoint: &str) -> CcResult<()> {
    let invalid = || {
        CcError::Config("semantic.endpoint must be an absolute HTTP(S) URL without credentials, query or fragment".into())
    };
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://"))
        || !endpoint.is_ascii()
        || endpoint
            .chars()
            .any(|c| c.is_ascii_whitespace() || c.is_ascii_control())
        || endpoint.contains(['@', '?', '#', '\\'])
    {
        return Err(invalid());
    }
    let (_, rest) = endpoint.split_once("://").ok_or_else(invalid)?;
    let authority = rest.split('/').next().unwrap_or_default();
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']').ok_or_else(invalid)?;
        authority[1..end]
            .parse::<std::net::Ipv6Addr>()
            .map_err(|_| invalid())?;
        let suffix = &authority[end + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or_else(invalid)?)
        };
        (&authority[..=end], port)
    } else {
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if host.trim_end_matches('.').split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            return Err(invalid());
        }
        (host, port)
    };
    if host.is_empty() || port.is_some_and(|p| p.parse::<u16>().map_or(true, |p| p == 0)) {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_semantic::ports::{DocumentInput, ProviderError, QueryInput};
    use cc_semantic::providers::openai_compatible::{HttpRequest, HttpResponse, TransportError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockTransport {
        result: Result<HttpResponse, TransportError>,
        requests: Mutex<Vec<HttpRequest>>,
    }
    impl EmbeddingHttpTransport for MockTransport {
        fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.lock().unwrap().push(request);
            self.result.clone()
        }
    }
    fn config() -> SemanticProviderConfig {
        SemanticProviderConfig {
            enabled: true,
            network_opt_in: true,
            allow_http: true,
            model_id: "factory-test".into(),
            dimensions: Some(2),
            endpoint: "http://localhost:1234/v1".into(),
            api_key_ref: Some("env:FACTORY_TEST_KEY".into()),
            max_input_tokens: Some(512),
            max_batch_items: Some(8),
            ..Default::default()
        }
    }
    fn response(status: u16) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: br#"{"model":"factory-test","data":[{"index":0,"embedding":[1.0,0.0]}]}"#
                .to_vec(),
        }
    }
    fn make(
        config: &SemanticProviderConfig,
        result: Result<HttpResponse, TransportError>,
    ) -> (Arc<dyn EmbeddingProvider>, Arc<MockTransport>) {
        let transport = Arc::new(MockTransport {
            result,
            requests: Mutex::new(vec![]),
        });
        let provider = build_embedding_provider_with(
            config,
            |_| Ok(EmbeddingApiKey::new("mock-only-key")),
            || Ok(transport.clone()),
        )
        .unwrap()
        .unwrap();
        (provider, transport)
    }
    #[test]
    fn disabled_and_no_opt_in_are_inert_even_with_invalid_config() {
        for (enabled, opted_in) in [(false, false), (false, true), (true, false)] {
            let config = SemanticProviderConfig {
                enabled,
                network_opt_in: opted_in,
                ..Default::default()
            };
            assert!(build_embedding_provider_with(
                &config,
                |_| panic!("credential resolver touched"),
                || panic!("transport constructed")
            )
            .unwrap()
            .is_none());
            assert!(
                build_embedding_provider(&config, || panic!("transport constructed"))
                    .unwrap()
                    .is_none()
            );
        }
    }
    #[test]
    fn invalid_configuration_is_rejected_before_side_effects() {
        let mut cases = vec![];
        let mut c = config();
        c.dimensions = Some(0);
        cases.push(c);
        let mut c = config();
        c.metric = "euclidean".into();
        cases.push(c);
        let mut c = config();
        c.max_batch_items = Some(0);
        cases.push(c);
        let mut c = config();
        c.allow_http = false;
        cases.push(c);
        let mut c = config();
        c.api_key_ref = None;
        cases.push(c);
        let mut c = config();
        c.api_key_ref = Some("inline-secret".into());
        cases.push(c);
        for endpoint in [
            "https://",
            "https://user:secret@host/v1",
            "https://host/?secret=yes",
            "https://host/#secret",
            "https://host:bad/v1",
            "https://host:0/v1",
            "https://bad host/v1",
            "https://[invalid]/v1",
            "https://host\\evil/v1",
        ] {
            let mut c = config();
            c.endpoint = endpoint.into();
            cases.push(c);
        }
        for c in cases {
            assert!(build_embedding_provider_with(
                &c,
                |_| panic!("credential resolver touched"),
                || panic!("transport constructed")
            )
            .is_err());
        }
    }
    #[test]
    fn credential_errors_and_header_injection_never_construct_transport() {
        let config = config();
        assert!(build_embedding_provider_with(
            &config,
            |_| Err(CcError::Config("mock missing reference".into())),
            || panic!("transport constructed")
        )
        .is_err());
        for secret in ["", " ", "mock\r\nInjected: header"] {
            assert!(build_embedding_provider_with(
                &config,
                |_| Ok(EmbeddingApiKey::new(secret)),
                || panic!("transport constructed")
            )
            .is_err());
        }
    }
    #[test]
    fn construction_is_lazy_and_preserves_model_endpoint_and_timeout() {
        let calls = AtomicUsize::new(0);
        let (provider, transport) = make(&config(), Ok(response(200)));
        assert!(transport.requests.lock().unwrap().is_empty());
        assert_eq!(provider.space().model_id(), "factory-test");
        let vectors = provider
            .embed_documents(&[DocumentInput::from_bytes(b"mock input").unwrap()])
            .unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0]]);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "http://localhost:1234/v1/embeddings");
        assert!(!requests[0].timeout.is_zero());
        assert_eq!(requests[0].timeout, std::time::Duration::from_secs(30));
        drop(requests);
        assert!(build_embedding_provider_with(
            &config(),
            |_| Ok(EmbeddingApiKey::new("mock-only-key")),
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(CcError::Config("mock factory failure".into()))
            }
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn provider_errors_preserve_frozen_taxonomy_without_retrying() {
        for (result, expected) in [
            (Err(TransportError::Timeout), ProviderError::Timeout),
            (Err(TransportError::Cancelled), ProviderError::Cancelled),
            (
                Err(TransportError::Io("mock connection failure".into())),
                ProviderError::ServerError,
            ),
            (Ok(response(401)), ProviderError::AuthError),
            (Ok(response(503)), ProviderError::ServerError),
        ] {
            let (provider, transport) = make(&config(), result);
            assert_eq!(
                provider
                    .embed_queries(&[QueryInput::from_bytes(b"mock query").unwrap()])
                    .unwrap_err(),
                expected
            );
            assert_eq!(transport.requests.lock().unwrap().len(), 1);
        }
        for status in [302, 400] {
            let (provider, _) = make(&config(), Ok(response(status)));
            assert!(matches!(
                provider.embed_queries(&[QueryInput::from_bytes(b"mock query").unwrap()]),
                Err(ProviderError::InvalidInput(_))
            ));
        }
    }
    #[test]
    fn rate_limit_empty_batch_and_vector_gate_preserve_adapter_contract() {
        let mut limited = response(429);
        limited.headers.push(("Retry-After".into(), "7".into()));
        let (provider, transport) = make(&config(), Ok(limited));
        assert!(provider.embed_queries(&[]).unwrap().is_empty());
        assert!(transport.requests.lock().unwrap().is_empty());
        assert_eq!(
            provider
                .embed_queries(&[QueryInput::from_bytes(b"mock query").unwrap()])
                .unwrap_err(),
            ProviderError::RateLimited {
                retry_after: std::time::Duration::from_secs(7)
            }
        );
        let mut bad_vector = response(200);
        bad_vector.body =
            br#"{"model":"factory-test","data":[{"index":0,"embedding":[0.0,0.0]}]}"#.to_vec();
        let (provider, _) = make(&config(), Ok(bad_vector));
        assert!(matches!(
            provider.embed_documents(&[DocumentInput::from_bytes(b"mock input").unwrap()]),
            Err(ProviderError::InvalidInput(_))
        ));
    }

    #[test]
    fn invalid_config_diagnostics_do_not_reflect_credential_material() {
        for endpoint in [
            "https://secret-material@host/v1",
            "unsupported://host/secret-material",
        ] {
            let mut c = config();
            c.endpoint = endpoint.into();
            let error = build_embedding_provider_with(
                &c,
                |_| panic!("resolver touched"),
                || panic!("transport built"),
            )
            .err()
            .unwrap();
            assert!(!error.to_string().contains("secret-material"));
        }
        let mut c = config();
        c.api_key_ref = Some("secret-material:literal".into());
        let error = build_embedding_provider_with(
            &c,
            |_| panic!("resolver touched"),
            || panic!("transport built"),
        )
        .err()
        .unwrap();
        assert!(!error.to_string().contains("secret-material"));
    }

    #[test]
    fn ipv6_endpoint_is_accepted_and_malformed_response_rejected() {
        let mut config = config();
        config.endpoint = "http://[::1]:1234/v1".into();
        let mut malformed = response(200);
        malformed.body = b"{}".to_vec();
        let (provider, _) = make(&config, Ok(malformed));
        assert!(matches!(
            provider.embed_queries(&[QueryInput::from_bytes(b"mock query").unwrap()]),
            Err(ProviderError::InvalidInput(_))
        ));
    }
}
