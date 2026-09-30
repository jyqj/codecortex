use cc_eval::benchmark::{
    adapters::{mcp_stdio::McpStdio, Backend},
    manifest, normalizer,
    readiness::State,
    schema::SearchInput,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.py"),
        "def renew_session():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join(".codecortex.json"),
        "{\"auto_index\":{\"enabled\":false}}",
    )
    .unwrap();
    d
}
#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY pointing at an explicitly built product"]
async fn real_stdio_index_search_scope_and_tool_error() {
    let binary = PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product binary required"),
    );
    let d = project();
    let files = manifest::inventory(d.path(), &["a.py".into()]).unwrap();
    let mut client = McpStdio::spawn(&binary, d.path(), Duration::from_secs(15))
        .await
        .unwrap();
    assert!(client.pid().is_some());
    client.prepare(&files).await.unwrap();
    assert_eq!(client.readiness(&files).await.unwrap().state, State::Ready);
    let v = client
        .search(&SearchInput {
            query: "renew_session".into(),
            top_k: 10,
            path_prefix: None,
        })
        .await
        .unwrap();
    let (h, _) = normalizer::mcp(&v).unwrap();
    assert!(h.iter().any(|h| h.path == "a.py"));
    let v = client
        .search(&SearchInput {
            query: "renew_session".into(),
            top_k: 10,
            path_prefix: Some("no-such-dir/".into()),
        })
        .await
        .unwrap();
    assert!(normalizer::mcp(&v).unwrap().0.is_empty());
    assert!(client.call("search", json!({"qurey":"bad"})).await.is_err());
    client.close().await.unwrap();
    assert!(client
        .search(&SearchInput {
            query: "x".into(),
            top_k: 1,
            path_prefix: None
        })
        .await
        .is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn exited_child_is_protocol_error_not_empty_search() {
    let d = project();
    assert!(McpStdio::spawn(
        std::path::Path::new("/usr/bin/true"),
        d.path(),
        Duration::from_millis(500)
    )
    .await
    .is_err());
}

#[cfg(feature = "eval-http")]
mod http {
    use super::*;
    use cc_eval::benchmark::adapters::oce_http::OceHttp;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    async fn stub(
        responses: Vec<(&'static str, u16, Value)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let job = tokio::spawn(async move {
            for (path, status, value) in responses {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = s.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    assert!(data.len() < 2_000_000);
                    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..pos]);
                        assert!(head.starts_with(&format!("POST {path} ")));
                        let size = head
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if data.len() >= pos + 4 + size {
                            let request: Value =
                                serde_json::from_slice(&data[pos + 4..pos + 4 + size]).unwrap();
                            assert!(request.get("expected_files").is_none());
                            assert!(request.get("answers").is_none());
                            break;
                        }
                    }
                }
                let body = serde_json::to_vec(&value).unwrap();
                let header=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                s.write_all(header.as_bytes()).await.unwrap();
                s.write_all(&body).await.unwrap();
                s.shutdown().await.unwrap();
            }
        });
        (format!("http://{address}"), job)
    }
    fn blob() -> String {
        let mut h = Sha256::new();
        h.update(b"a.py");
        h.update(b"def renew_session():\n    return 1\n");
        format!("{:x}", h.finalize())
    }
    #[tokio::test]
    async fn public_http_upload_ready_search_contract() {
        let d = project();
        let name = blob();
        let (endpoint, job) = stub(vec![
            ("/batch-upload", 200, json!({"blob_names":[name]})),
            (
                "/agents/blob-status",
                200,
                json!({"nonindexed_blob_names":[],"unknown_blob_names":[]}),
            ),
            (
                "/agents/codebase-retrieval",
                200,
                json!({"formatted_retrieval":"Path: a.py\nLine 1: def renew_session():"}),
            ),
        ])
        .await;
        let files = manifest::inventory(d.path(), &["a.py".into()]).unwrap();
        let mut client = OceHttp::new(
            &endpoint,
            "test-credential".into(),
            d.path().into(),
            Duration::from_secs(2),
            false,
        )
        .unwrap();
        client.prepare(&files).await.unwrap();
        let v = client
            .search(&SearchInput {
                query: "renew_session".into(),
                top_k: 10,
                path_prefix: None,
            })
            .await
            .unwrap();
        assert_eq!(normalizer::oce(&v).unwrap().0[0].path, "a.py");
        client.close().await.unwrap();
        job.await.unwrap();
    }
    #[tokio::test]
    async fn unknown_or_failed_inputs_do_not_become_ready() {
        for field in ["unknown_blob_names", "failed_blob_names"] {
            let d = project();
            let name = blob();
            let mut state = json!({"nonindexed_blob_names":[],"unknown_blob_names":[]});
            state[field] = json!([name]);
            let (endpoint, job) = stub(vec![
                ("/batch-upload", 200, json!({"blob_names":[name]})),
                ("/agents/blob-status", 200, state),
            ])
            .await;
            let files = manifest::inventory(d.path(), &["a.py".into()]).unwrap();
            let mut client = OceHttp::new(
                &endpoint,
                "test-credential".into(),
                d.path().into(),
                Duration::from_secs(2),
                false,
            )
            .unwrap();
            assert!(client.prepare(&files).await.is_err());
            job.await.unwrap();
        }
    }
    #[tokio::test]
    async fn errors_do_not_log_credentials_or_response_body() {
        let d = project();
        let (endpoint, job) = stub(vec![(
            "/agents/codebase-retrieval",
            500,
            json!({"secret":"do-not-log-this"}),
        )])
        .await;
        let mut client = OceHttp::new(
            &endpoint,
            "sensitive-token".into(),
            d.path().into(),
            Duration::from_secs(2),
            false,
        )
        .unwrap();
        let e = client
            .search(&SearchInput {
                query: "x".into(),
                top_k: 1,
                path_prefix: None,
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(!e.contains("sensitive-token"));
        assert!(!e.contains("do-not-log-this"));
        job.await.unwrap();
    }
    #[test]
    fn external_transfer_and_endpoint_validation() {
        let d = project();
        assert!(OceHttp::new(
            "https://example.invalid",
            "x".into(),
            d.path().into(),
            Duration::from_secs(1),
            false
        )
        .is_err());
        assert!(OceHttp::new(
            "https://user:password@example.invalid",
            "x".into(),
            d.path().into(),
            Duration::from_secs(1),
            true
        )
        .is_err());
        assert!(OceHttp::new(
            "http://example.invalid",
            "x".into(),
            d.path().into(),
            Duration::from_secs(1),
            true
        )
        .is_err());
    }
    #[tokio::test]
    async fn timeout_is_distinct_from_empty_results() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let job = tokio::spawn(async move {
            let (_s, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let d = project();
        let mut client = OceHttp::new(
            &endpoint,
            "x".into(),
            d.path().into(),
            Duration::from_millis(30),
            false,
        )
        .unwrap();
        let e = client
            .search(&SearchInput {
                query: "x".into(),
                top_k: 1,
                path_prefix: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(e, cc_eval::benchmark::BenchError::Timeout(_)));
        job.await.unwrap();
    }
}
