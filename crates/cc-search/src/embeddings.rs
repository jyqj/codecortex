//! Embedding providers — hash-based (default) and API-based.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Trait for text → vector embedding.
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Vec<f32>;
    fn dimensions(&self) -> usize;
}

/// Blake3 hash → stable embedding vector (default, zero network dependency).
pub struct HashingEmbedder {
    dimensions: usize,
}

impl HashingEmbedder {
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions: dimensions.max(1),
        }
    }
}

impl Embedder for HashingEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; self.dimensions];
        let tokens = cc_db::fts::tokenize_codeish(text);
        if tokens.is_empty() {
            return vector;
        }

        // Append first 120 chars of stripped text, matching Python's
        // `joined = tokens + [text.strip()[:120]]`
        let suffix: String = text.trim().chars().take(120).collect();
        let mut joined: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
        joined.push(&suffix);

        for token in &joined {
            if token.is_empty() {
                continue;
            }

            let base = blake3_hash_u64(token.as_bytes());
            let index = (base as usize) % self.dimensions;
            let sign: f32 = if ((base >> 8) & 1) == 0 { 1.0 } else { -1.0 };
            // Dynamic weight: 1.0 + min(len, 20) / 20.0  (range 1.0–2.0)
            let weight: f32 = 1.0 + (token.len().min(20) as f32) / 20.0;
            vector[index] += sign * weight;

            // Character-level trigrams on first 12 characters
            let trigram_span: String = token.chars().take(12).collect();
            let trigram_bytes = trigram_span.as_bytes();
            if trigram_bytes.len() >= 3 {
                for tri_idx in 0..trigram_bytes.len() - 2 {
                    let trigram = &trigram_bytes[tri_idx..tri_idx + 3];
                    let h = blake3_hash_u64(trigram);
                    let t_index = (h as usize) % self.dimensions;
                    let t_sign: f32 = if ((h >> 10) & 1) == 0 { 1.0 } else { -1.0 };
                    vector[t_index] += t_sign * 0.25;
                }
            }
        }

        // L2 normalize
        let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vector {
                *v /= norm;
            }
        }

        vector
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

impl Default for HashingEmbedder {
    fn default() -> Self {
        Self::new(256)
    }
}

/// OpenAI-compatible embedding API client.
pub struct ApiEmbedder {
    client: reqwest::blocking::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
    dimensions: usize,
    fallback: HashingEmbedder,
}

impl ApiEmbedder {
    pub fn new(
        base_url: impl AsRef<str>,
        model: impl Into<String>,
        api_key: Option<String>,
        dimensions: usize,
        timeout_seconds: u64,
    ) -> Result<Self, reqwest::Error> {
        let dimensions = dimensions.max(1);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()?;
        Ok(Self {
            client,
            endpoint: embedding_endpoint(base_url.as_ref()),
            model: model.into(),
            api_key: api_key.and_then(non_empty_string),
            dimensions,
            fallback: HashingEmbedder::new(dimensions),
        })
    }

    fn request_embedding(&self, text: &str) -> Result<Vec<f32>, String> {
        let payload = EmbeddingRequest {
            model: &self.model,
            input: text,
        };
        let mut request = self.client.post(&self.endpoint).json(&payload);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().map_err(|e| e.to_string())?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(format!("{} {}", status, body.trim()));
        }

        let parsed: EmbeddingResponse = response.json().map_err(|e| e.to_string())?;
        parsed
            .data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .filter(|embedding| !embedding.is_empty())
            .ok_or_else(|| "embedding response missing data[0].embedding".to_string())
    }
}

impl Embedder for ApiEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        if text.trim().is_empty() {
            return vec![0.0; self.dimensions];
        }

        match self.request_embedding(text) {
            Ok(embedding) => normalize_embedding_dimensions(embedding, self.dimensions),
            Err(error) => {
                tracing::warn!(
                    endpoint = %self.endpoint,
                    model = %self.model,
                    error = %error,
                    "embedding API request failed, falling back to hash"
                );
                self.fallback.embed(text)
            }
        }
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Unpack a blob of f32 LE bytes into a Vec<f32>.
pub fn unpack_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Pack a Vec<f32> into LE bytes.
pub fn pack_vector(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn embedding_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/embeddings") {
        trimmed.to_string()
    } else {
        format!("{}/embeddings", trimmed)
    }
}

/// Hash bytes with blake3 and return the first 8 bytes as a u64 (LE).
/// Mirrors the Python `blake_hash` which returns an integer from digest bytes.
fn blake3_hash_u64(data: &[u8]) -> u64 {
    let hash = blake3::hash(data);
    let bytes = hash.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn l2_normalize(vec: &mut [f32]) {
    let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in vec {
            *v /= norm;
        }
    }
}

fn normalize_embedding_dimensions(mut embedding: Vec<f32>, dimensions: usize) -> Vec<f32> {
    let dimensions = dimensions.max(1);
    if embedding.is_empty() {
        return vec![0.0; dimensions];
    }

    if embedding.len() == dimensions {
        l2_normalize(&mut embedding);
        return embedding;
    }

    if embedding.len() < dimensions {
        embedding.resize(dimensions, 0.0);
        l2_normalize(&mut embedding);
        return embedding;
    }

    let mut reduced = vec![0.0f32; dimensions];
    for (idx, value) in embedding.into_iter().enumerate() {
        reduced[idx % dimensions] += value;
    }
    l2_normalize(&mut reduced);
    reduced
}

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingResponseItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponseItem {
    embedding: Vec<f32>,
}

/// Null embedder — returns empty vectors. Used when no provider is configured.
pub struct NullEmbedder;

impl Embedder for NullEmbedder {
    fn embed(&self, _text: &str) -> Vec<f32> {
        Vec::new()
    }
    fn dimensions(&self) -> usize {
        0
    }
}

/// Create an embedder from config. Returns `NullEmbedder` when provider is `None`.
pub fn get_embedder(config: &cc_model::config::EmbeddingsConfig) -> Box<dyn Embedder> {
    match config.provider {
        cc_model::config::EmbeddingProvider::None => Box::new(NullEmbedder),
        cc_model::config::EmbeddingProvider::Hash => {
            Box::new(HashingEmbedder::new(config.dimensions))
        }
        cc_model::config::EmbeddingProvider::OpenAICompatible => {
            let fallback =
                || Box::new(HashingEmbedder::new(config.dimensions)) as Box<dyn Embedder>;
            let base_url = match config.base_url.as_deref().map(str::trim) {
                Some(url) if !url.is_empty() => url,
                _ => {
                    tracing::warn!(
                        "OpenAI-compatible embeddings selected but base_url is missing; falling back to hash"
                    );
                    return fallback();
                }
            };
            let model = match config.model.as_deref().map(str::trim) {
                Some(model) if !model.is_empty() => model,
                _ => {
                    tracing::warn!(
                        "OpenAI-compatible embeddings selected but model is missing; falling back to hash"
                    );
                    return fallback();
                }
            };

            match ApiEmbedder::new(
                base_url,
                model,
                config.api_key.clone(),
                config.dimensions,
                config.timeout_seconds,
            ) {
                Ok(embedder) => Box::new(embedder),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "failed to initialize embedding API client; falling back to hash"
                    );
                    fallback()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn hashing_embedder_produces_correct_dims() {
        let e = HashingEmbedder::new(128);
        let v = e.embed("hello world foo bar");
        assert_eq!(v.len(), 128);
    }

    #[test]
    fn hashing_embedder_is_deterministic() {
        let e = HashingEmbedder::new(64);
        let a = e.embed("test input");
        let b = e.embed("test input");
        assert_eq!(a, b);
    }

    #[test]
    fn cosine_self_is_one() {
        let e = HashingEmbedder::new(64);
        let v = e.embed("some code here");
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let original = vec![1.0f32, -2.5, 0.0, std::f32::consts::PI];
        let packed = pack_vector(&original);
        let unpacked = unpack_vector(&packed);
        assert_eq!(original, unpacked);
    }

    #[test]
    fn normalize_embedding_dimensions_resizes_and_normalizes() {
        let reduced = normalize_embedding_dimensions(vec![1.0, 2.0, 3.0, 4.0], 2);
        assert_eq!(reduced.len(), 2);
        assert!((cosine_similarity(&reduced, &reduced) - 1.0).abs() < 1e-6);

        let expanded = normalize_embedding_dimensions(vec![3.0, 4.0], 4);
        assert_eq!(expanded.len(), 4);
        assert!((cosine_similarity(&expanded, &expanded) - 1.0).abs() < 1e-6);
    }

    #[test]
    #[ignore] // Pre-existing failure: reqwest blocking client cannot connect to local mock on this platform
    fn api_embedder_calls_openai_compatible_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .ok();
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            // Read headers + body (for small payloads the entire request fits in one read)
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        request.extend_from_slice(&buf[..n]);
                        // Once we have the header/body separator, check if full body received
                        if let Some(pos) = request
                            .windows(4)
                            .position(|w| w == b"\r\n\r\n")
                        {
                            let header_end = pos + 4;
                            // Try to find Content-Length
                            let header_str =
                                String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_str
                                .lines()
                                .find_map(|line| {
                                    let lower = line.to_ascii_lowercase();
                                    lower
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                });
                            match content_length {
                                Some(cl) if request.len() >= header_end + cl => break,
                                None => break,
                                _ => {} // keep reading
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            tx.send(String::from_utf8_lossy(&request).to_string())
                .unwrap();

            let body = r#"{"data":[{"embedding":[1.0,2.0,3.0,4.0]}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        let embedder = ApiEmbedder::new(
            format!("http://{}/v1", addr),
            "demo-embedding-model",
            Some("secret-token".into()),
            4,
            5,
        )
        .unwrap();
        let embedding = embedder.embed("hello world");

        let request = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(request.starts_with("POST /v1/embeddings HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer secret-token"));
        assert_eq!(
            embedding,
            normalize_embedding_dimensions(vec![1.0, 2.0, 3.0, 4.0], 4)
        );

        handle.join().unwrap();
    }
}
