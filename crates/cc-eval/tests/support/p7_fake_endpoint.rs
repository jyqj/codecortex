//! Fixed synthetic provider shared by actual public-policy and isolated-source tests.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MODEL: &str = "p7-019-fake-constant-vector";
pub struct FakeEndpoint {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl FakeEndpoint {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let body: Value = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() <= 1_048_576);
                        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            assert!(headers.starts_with("POST /v1/embeddings HTTP/1.1\r\n"));
                            let length: usize = headers
                                .lines()
                                .find_map(|line| {
                                    let (key, value) = line.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse().unwrap())
                                })
                                .unwrap();
                            assert!(length <= 1_000_000);
                            if bytes.len() >= end + 4 + length {
                                break serde_json::from_slice(&bytes[end + 4..end + 4 + length])
                                    .unwrap();
                            }
                        }
                    }
                })
                .await
                .unwrap();
                assert_eq!(body["model"], MODEL);
                let inputs = body["input"].as_array().unwrap();
                assert!(!inputs.is_empty());
                observed.lock().unwrap().push(json!({"profile":"fake","model":body["model"],
                    "input_count":inputs.len(),"input_sha256":inputs.iter().map(|input|
                        format!("{:x}",Sha256::digest(input.as_str().unwrap().as_bytes()))).collect::<Vec<_>>() }));
                let data = inputs
                    .iter()
                    .enumerate()
                    .map(|(index, _)| json!({"index":index,"embedding":[1.0,0.0,0.0,0.0]}))
                    .collect::<Vec<_>>();
                let response = serde_json::to_vec(&json!({"model":MODEL,"data":data})).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len()).as_bytes()).await.unwrap();
                socket.write_all(&response).await.unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }
}
impl Drop for FakeEndpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}
