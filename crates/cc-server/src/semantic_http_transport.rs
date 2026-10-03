//! Optional HTTPS transport (`semantic-http`); no default network path.
//!
//! Construct, call and drop this blocking client outside async execution
//! (the runtime owner uses `spawn_blocking`). Client construction performs no
//! HTTP request. Redirects, proxies and client retries are disabled.
//! Cancellation is checked before dispatch and between blocking operations;
//! an in-flight blocking read/send remains bounded by the request deadline.

use std::io::Read;
use std::sync::Arc;
use std::time::Instant;

use cc_model::config::SemanticProviderConfig;
use cc_model::{CcError, CcResult};
use cc_semantic::capability::resolve_provider;
use cc_semantic::policy::EgressPolicy;
use cc_semantic::providers::openai_compatible::{
    EmbeddingHttpTransport, HttpRequest, HttpResponse, TransportError,
};
use reqwest::blocking::Client;
use reqwest::Url;
use tokio_util::sync::CancellationToken;

/// Allocation ceiling for one wire response, including error responses.
/// Larger responses are transport failures, never unbounded allocations.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Called lazily by `build_embedding_provider`; refuses inert/no-network
/// configuration even when invoked directly. Must run in blocking context.
pub fn build_http_transport(
    config: &SemanticProviderConfig,
) -> CcResult<Arc<dyn EmbeddingHttpTransport>> {
    build_http_transport_with_cancellation(config, CancellationToken::new())
}

/// Additive runtime seam for a project/worker cancellation token. Cancellation
/// prevents future dispatch and is observed after pending blocking I/O returns;
/// it does not claim to interrupt a blocking operation ahead of its deadline.
pub fn build_http_transport_with_cancellation(
    config: &SemanticProviderConfig,
    cancellation: CancellationToken,
) -> CcResult<Arc<dyn EmbeddingHttpTransport>> {
    if !config.enabled || !config.network_opt_in {
        return Err(CcError::Config(
            "semantic HTTP transport requires enabled and explicit network_opt_in".into(),
        ));
    }
    let (space, capability) = resolve_provider(config)?;
    let policy = EgressPolicy::from_provider_config(config);
    let endpoint = Url::parse(&config.endpoint)
        .map_err(|_| CcError::Config("semantic.endpoint is not a valid HTTP(S) URL".into()))?;
    if !matches!(endpoint.scheme(), "https" | "http")
        || endpoint.host_str().is_none()
        || endpoint.port() == Some(0)
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(CcError::Config(
            "semantic.endpoint must be HTTP(S) without credentials, query or fragment".into(),
        ));
    }
    policy.validate_endpoint(endpoint.as_str())?;
    let target = Url::parse(&format!(
        "{}/embeddings",
        endpoint.as_str().trim_end_matches('/')
    ))
    .map_err(|_| CcError::Config("semantic.endpoint cannot form an embeddings URL".into()))?;
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .connection_verbose(false)
        .https_only(!config.allow_http)
        .http1_only()
        .build()
        .map_err(|_| CcError::Config("semantic HTTP client initialization failed".into()))?;
    Ok(Arc::new(HttpEmbeddingTransport {
        client,
        target,
        dimension: space.dimension() as usize,
        max_batch: capability.max_batch_items as usize,
        cancellation,
    }))
}

struct HttpEmbeddingTransport {
    client: Client,
    target: Url,
    dimension: usize,
    max_batch: usize,
    cancellation: CancellationToken,
}

impl HttpEmbeddingTransport {
    fn cancelled(&self) -> Result<(), TransportError> {
        if self.cancellation.is_cancelled() {
            Err(TransportError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn map_error(&self, error: reqwest::Error) -> TransportError {
        if self.cancellation.is_cancelled() {
            TransportError::Cancelled
        } else if error.is_timeout() {
            TransportError::Timeout
        } else {
            TransportError::Io("semantic HTTP request failed".into())
        }
    }

    fn body_bound(&self, request: &HttpRequest) -> Result<usize, TransportError> {
        // Same count/dimension bound as the provider's response gate, with
        // an additional absolute allocation ceiling for the HTTP reader.
        let body: serde_json::Value = serde_json::from_slice(&request.body)
            .map_err(|_| TransportError::Io("semantic HTTP request is not JSON".into()))?;
        let count = body
            .get("input")
            .and_then(|v| v.as_array())
            .map(Vec::len)
            .ok_or_else(|| {
                TransportError::Io("semantic HTTP request input must be a batch".into())
            })?;
        if count == 0 || count > self.max_batch {
            return Err(TransportError::Io(
                "semantic HTTP request batch is outside configured bounds".into(),
            ));
        }
        Ok(count
            .saturating_mul(self.dimension.saturating_mul(64).saturating_add(256))
            .saturating_add(1024)
            .min(MAX_RESPONSE_BYTES))
    }
}

impl EmbeddingHttpTransport for HttpEmbeddingTransport {
    fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.cancelled()?;
        let started = Instant::now();
        let deadline = started
            .checked_add(request.timeout)
            .filter(|_| !request.timeout.is_zero())
            .ok_or_else(|| {
                TransportError::Io("semantic HTTP timeout must be positive and bounded".into())
            })?;
        let url = Url::parse(&request.url)
            .map_err(|_| TransportError::Io("semantic HTTP request URL is invalid".into()))?;
        if url != self.target {
            return Err(TransportError::Io(
                "semantic HTTP request target differs from configured endpoint".into(),
            ));
        }
        let bound = self.body_bound(&request)?;
        let mut builder = self.client.post(url);
        for (name, value) in request.headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| TransportError::Io("semantic HTTP header name is invalid".into()))?;
            let mut value = reqwest::header::HeaderValue::from_str(&value)
                .map_err(|_| TransportError::Io("semantic HTTP header value is invalid".into()))?;
            if name == reqwest::header::AUTHORIZATION {
                value.set_sensitive(true);
            }
            builder = builder.header(name, value);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TransportError::Timeout);
        }
        self.cancelled()?;
        let mut response = builder
            .body(request.body)
            .timeout(remaining)
            .send()
            .map_err(|e| self.map_error(e))?;
        self.cancelled()?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), value.to_owned()))
            })
            .collect();
        if response
            .content_length()
            .is_some_and(|length| length > bound as u64)
        {
            return Err(TransportError::Io(
                "semantic HTTP response exceeds size bound".into(),
            ));
        }
        let mut body = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            self.cancelled()?;
            if Instant::now() >= deadline {
                return Err(TransportError::Timeout);
            }
            // Read at most one byte beyond the bound: unknown-length/chunked
            // bodies cannot evade allocation admission.
            let room = (bound + 1 - body.len()).min(buffer.len());
            let n = response.read(&mut buffer[..room]).map_err(|error| {
                if self.cancellation.is_cancelled() {
                    return TransportError::Cancelled;
                }
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error
                        .get_ref()
                        .and_then(|e| e.downcast_ref::<reqwest::Error>())
                        .is_some_and(reqwest::Error::is_timeout)
                {
                    TransportError::Timeout
                } else {
                    TransportError::Io("semantic HTTP response read failed".into())
                }
            })?;
            self.cancelled()?;
            if Instant::now() >= deadline {
                return Err(TransportError::Timeout);
            }
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..n]);
            if body.len() > bound {
                return Err(TransportError::Io(
                    "semantic HTTP response exceeds size bound".into(),
                ));
            }
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    fn config(endpoint: String) -> SemanticProviderConfig {
        SemanticProviderConfig {
            enabled: true,
            network_opt_in: true,
            allow_http: true,
            model_id: "transport-test".into(),
            dimensions: Some(2),
            endpoint,
            api_key_ref: Some("env:MOCK_ONLY".into()),
            max_input_tokens: Some(512),
            max_batch_items: Some(8),
            ..Default::default()
        }
    }
    fn request(endpoint: &str, timeout: Duration) -> HttpRequest {
        HttpRequest { url: format!("{endpoint}/embeddings"), timeout,
            headers: vec![("Content-Type".into(), "application/json".into()),
                ("Authorization".into(), "Bearer mock-only-value".into())],
            body: br#"{"model":"transport-test","input":["synthetic mock input"],"encoding_format":"float"}"#.to_vec() }
    }
    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 512];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]);
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    return bytes;
                }
            }
            assert!(bytes.len() < 16384);
        }
    }
    fn server(
        handler: impl FnOnce(TcpStream) + Send + 'static,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        read_request(&mut stream);
                        handler(stream);
                        return;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "loopback request never arrived");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("loopback accept: {e}"),
                }
            }
        });
        (endpoint, join)
    }
    fn reply(stream: &mut TcpStream, status: u16, body: &[u8]) {
        write!(stream, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        let _ = stream.write_all(body);
    }
    #[test]
    fn valid_response_and_http_error_status_pass_through_once() {
        for status in [200, 401, 429, 503] {
            let calls = Arc::new(AtomicUsize::new(0));
            let received = calls.clone();
            let (endpoint, join) = server(move |mut stream| {
                received.fetch_add(1, Ordering::SeqCst);
                reply(&mut stream, status, b"{}");
            });
            let transport = build_http_transport(&config(endpoint.clone())).unwrap();
            let response = transport
                .post_json(request(&endpoint, Duration::from_secs(1)))
                .unwrap();
            assert_eq!(response.status, status);
            assert_eq!(response.body, b"{}");
            join.join().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn pr13_factory_and_http_transport_compose_against_loopback_only() {
        use cc_semantic::ports::DocumentInput;
        use cc_semantic::providers::openai_compatible::EmbeddingApiKey;
        let (endpoint, join) = server(|mut stream| {
            reply(
                &mut stream,
                200,
                br#"{"model":"transport-test","data":[{"index":0,"embedding":[1.0,0.0]}]}"#,
            )
        });
        let config = config(endpoint);
        let provider = crate::semantic_provider_factory::build_embedding_provider_with(
            &config,
            |_| Ok(EmbeddingApiKey::new("mock-only-value")),
            || build_http_transport(&config),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            provider
                .embed_documents(&[DocumentInput::from_bytes(b"synthetic mock input").unwrap()])
                .unwrap(),
            vec![vec![1.0, 0.0]]
        );
        join.join().unwrap();
    }

    #[test]
    fn peer_disconnect_is_not_replayed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let received = calls.clone();
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_millis(250);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        read_request(&mut stream);
                        received.fetch_add(1, Ordering::SeqCst);
                        drop(stream);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("loopback accept: {e}"),
                }
            }
        });
        let transport = build_http_transport(&config(endpoint.clone())).unwrap();
        assert!(matches!(
            transport.post_json(request(&endpoint, Duration::from_secs(1))),
            Err(TransportError::Io(_))
        ));
        join.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn inert_and_invalid_config_refuse_construction() {
        for (enabled, opted_in) in [(false, false), (false, true), (true, false)] {
            let c = SemanticProviderConfig {
                enabled,
                network_opt_in: opted_in,
                ..Default::default()
            };
            assert!(build_http_transport(&c).is_err());
        }
        for endpoint in [
            "https://secret-material@host/v1",
            "http://localhost:1234?secret-material",
            "file:///secret-material",
        ] {
            let mut c = config(endpoint.into());
            c.allow_http = false;
            let error = build_http_transport(&c).err().unwrap().to_string();
            assert!(!error.contains("secret-material"));
        }
        let mut c = config("http://localhost:1234".into());
        c.allow_http = false;
        assert!(build_http_transport(&c).is_err());
    }
    #[test]
    fn construction_zero_deadline_and_wrong_target_make_no_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let transport = build_http_transport(&config(endpoint.clone())).unwrap();
        assert!(matches!(
            transport.post_json(request(&endpoint, Duration::ZERO)),
            Err(TransportError::Io(_))
        ));
        assert!(matches!(
            transport.post_json(request("http://127.0.0.1:9", Duration::from_secs(1))),
            Err(TransportError::Io(_))
        ));
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    }
    #[test]
    fn redirects_never_send_credentials_to_second_endpoint() {
        let sink = TcpListener::bind("127.0.0.1:0").unwrap();
        sink.set_nonblocking(true).unwrap();
        let sink_url = format!("http://{}/capture", sink.local_addr().unwrap());
        let (endpoint, join) = server(move |mut stream| {
            write!(stream, "HTTP/1.1 307 Redirect\r\nLocation: {sink_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let transport = build_http_transport(&config(endpoint.clone())).unwrap();
        let response = transport
            .post_json(request(&endpoint, Duration::from_secs(1)))
            .unwrap();
        assert_eq!(response.status, 307);
        join.join().unwrap();
        assert!(matches!(sink.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    }
    #[test]
    fn oversized_content_length_and_streamed_body_are_bounded() {
        for declared in [true, false] {
            let (endpoint, join) = server(move |mut stream| {
                if declared {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                } else {
                    write!(stream, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n").unwrap();
                    let _ = stream.write_all(&vec![b'x'; 8192]);
                }
            });
            let transport = build_http_transport(&config(endpoint.clone())).unwrap();
            assert_eq!(
                transport.post_json(request(&endpoint, Duration::from_secs(1))),
                Err(TransportError::Io(
                    "semantic HTTP response exceeds size bound".into()
                ))
            );
            join.join().unwrap();
        }
    }
    #[test]
    fn header_and_body_timeout_are_classified_and_never_retried() {
        for headers_first in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let received = calls.clone();
            let (endpoint, join) = server(move |mut stream| {
                received.fetch_add(1, Ordering::SeqCst);
                if headers_first {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                }
                thread::sleep(Duration::from_millis(200));
                let _ = stream.write_all(b"{}");
            });
            let transport = build_http_transport(&config(endpoint.clone())).unwrap();
            let start = Instant::now();
            assert_eq!(
                transport.post_json(request(&endpoint, Duration::from_millis(40))),
                Err(TransportError::Timeout)
            );
            assert!(start.elapsed() < Duration::from_millis(180));
            join.join().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn cancellation_before_dispatch_and_during_io_never_retries() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let cancel = CancellationToken::new();
        let transport =
            build_http_transport_with_cancellation(&config(endpoint.clone()), cancel.clone())
                .unwrap();
        cancel.cancel();
        assert_eq!(
            transport.post_json(request(&endpoint, Duration::from_secs(1))),
            Err(TransportError::Cancelled)
        );
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let (endpoint, join) = server(move |mut stream| {
            server_cancel.cancel();
            reply(&mut stream, 200, b"{}");
        });
        let transport =
            build_http_transport_with_cancellation(&config(endpoint.clone()), cancel).unwrap();
        assert_eq!(
            transport.post_json(request(&endpoint, Duration::from_secs(1))),
            Err(TransportError::Cancelled)
        );
        join.join().unwrap();
    }
    #[test]
    fn connection_failure_diagnostics_are_redacted() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let transport = build_http_transport(&config(endpoint.clone())).unwrap();
        let error = transport
            .post_json(request(&endpoint, Duration::from_secs(1)))
            .unwrap_err();
        let text = format!("{error:?}");
        assert!(!text.contains(&endpoint));
        assert!(!text.contains("mock-only-value"));
        assert!(!text.contains("synthetic mock input"));
    }
    #[test]
    fn blocking_runtime_composition_is_supported() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            tokio::task::spawn_blocking(|| {
                build_http_transport(&config("https://provider.invalid/v1".into())).unwrap();
            })
            .await
            .unwrap();
        });
    }
    #[test]
    fn proxy_environment_is_ignored_in_isolated_process() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "semantic_http_transport::tests::proxy_environment_child",
                "--ignored",
            ])
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "")
            .env("http_proxy", "http://127.0.0.1:9")
            .env("https_proxy", "http://127.0.0.1:9")
            .env("all_proxy", "http://127.0.0.1:9")
            .env("no_proxy", "")
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[test]
    #[ignore = "run by isolated proxy-environment parent test"]
    fn proxy_environment_child() {
        let (endpoint, join) = server(|mut stream| reply(&mut stream, 200, b"{}"));
        let transport = build_http_transport(&config(endpoint.clone())).unwrap();
        assert_eq!(
            transport
                .post_json(request(&endpoint, Duration::from_secs(1)))
                .unwrap()
                .status,
            200
        );
        join.join().unwrap();
    }
}
