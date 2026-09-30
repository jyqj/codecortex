//! Optional public OCE HTTP adapter. No OCE server implementation is embedded.
use super::Backend;
use crate::benchmark::{
    invalid,
    manifest::{source_bytes, FileRecord},
    readiness::{Readiness, State},
    schema::SearchInput,
    BenchError, Result,
};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf, time::Duration};
pub struct OceHttp {
    client: Client,
    base: Url,
    key: String,
    root: PathBuf,
    names: Vec<String>,
    timeout: Duration,
}
impl OceHttp {
    pub fn new(
        base: &str,
        key: String,
        root: PathBuf,
        timeout: Duration,
        allow_external: bool,
    ) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| invalid("invalid endpoint URL"))?;
        if base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
        {
            return Err(invalid(
                "endpoint cannot contain credentials/query/fragment",
            ));
        }
        let local = matches!(
            base.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
        );
        if !local && !allow_external {
            return Err(invalid(
                "external source transfer requires --allow-external",
            ));
        }
        if base.scheme() != "https" && !(local && base.scheme() == "http") {
            return Err(invalid("HTTPS required except explicit loopback"));
        }
        if key.is_empty() {
            return Err(invalid("empty API credential"));
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|_| BenchError::Protocol("HTTP client setup".into()))?;
        Ok(Self {
            client,
            base,
            key,
            root,
            names: vec![],
            timeout,
        })
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let url = self.base.join(path).map_err(|_| invalid("endpoint path"))?;
        let mut response = self
            .client
            .post(url)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    BenchError::Timeout("OCE HTTP".into())
                } else {
                    BenchError::Protocol("OCE transport failed".into())
                }
            })?;
        if !response.status().is_success() {
            return Err(BenchError::Protocol(format!(
                "OCE HTTP status {}",
                response.status().as_u16()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| BenchError::Protocol("HTTP body read".into()))?
        {
            if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                return Err(BenchError::Protocol("HTTP response over 8 MiB".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    async fn upload_batch(
        &self,
        batch: Vec<Value>,
        deadline: tokio::time::Instant,
        returned: &mut BTreeSet<String>,
    ) -> Result<()> {
        let v =
            tokio::time::timeout_at(deadline, self.post("/batch-upload", json!({"blobs":batch})))
                .await
                .map_err(|_| BenchError::Timeout("OCE upload".into()))??;
        for n in Self::names(&v, "blob_names", true)? {
            if !returned.insert(n) {
                return Err(BenchError::Protocol("duplicate uploaded blob".into()));
            }
        }
        Ok(())
    }
    fn names(v: &Value, key: &str, required: bool) -> Result<BTreeSet<String>> {
        let Some(x) = v.get(key) else {
            if required {
                return Err(BenchError::Protocol(format!("missing {key}")));
            }
            return Ok(BTreeSet::new());
        };
        let a = x
            .as_array()
            .ok_or_else(|| BenchError::Protocol(format!("invalid {key}")))?;
        let mut set = BTreeSet::new();
        for n in a {
            let s = n
                .as_str()
                .ok_or_else(|| BenchError::Protocol("nonstring blob name".into()))?;
            if !set.insert(s.to_string()) {
                return Err(BenchError::Protocol("duplicate blob status".into()));
            }
        }
        Ok(set)
    }
}
impl Backend for OceHttp {
    fn name(&self) -> &'static str {
        "oce-http"
    }
    async fn prepare(&mut self, files: &[FileRecord]) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        self.names.clear();
        // Byte bounds are enforced per admitted source; batches stay <=1.5 MB.
        let mut returned = BTreeSet::new();
        let mut batch = Vec::new();
        let mut size = 0;
        for f in files {
            let bytes = source_bytes(&self.root, &f.path)?;
            let text = String::from_utf8(bytes).map_err(|_| invalid("UTF8"))?;
            if !batch.is_empty() && (batch.len() == 32 || size + text.len() > 1_500_000) {
                self.upload_batch(std::mem::take(&mut batch), deadline, &mut returned)
                    .await?;
                size = 0;
            }
            let mut h = Sha256::new();
            h.update(f.path.as_bytes());
            h.update(text.as_bytes());
            self.names.push(format!("{:x}", h.finalize()));
            size += text.len();
            batch.push(json!({"path":f.path,"content":text}));
        }
        if !batch.is_empty() {
            self.upload_batch(batch, deadline, &mut returned).await?;
        }
        if returned != self.names.iter().cloned().collect() {
            return Err(BenchError::Protocol(
                "uploaded blob IDs differ from source manifest".into(),
            ));
        }
        loop {
            let state = tokio::time::timeout_at(deadline, self.readiness(files))
                .await
                .map_err(|_| BenchError::Timeout("OCE readiness".into()))??;
            if state.state == State::Ready {
                return Ok(json!({"uploaded":files.len(),"readiness":state}));
            }
            if state.failed > 0 || state.unknown > 0 {
                return Err(BenchError::Protocol(
                    "OCE failed/unknown inputs; measurement not ready".into(),
                ));
            }
            tokio::time::timeout_at(deadline, tokio::time::sleep(Duration::from_millis(100)))
                .await
                .map_err(|_| BenchError::Timeout("OCE readiness".into()))?;
        }
    }
    async fn readiness(&mut self, files: &[FileRecord]) -> Result<Readiness> {
        let v = self
            .post(
                "/agents/blob-status",
                json!({"blobs":{"checkpoint_id":null,"added_blobs":self.names,"deleted_blobs":[]}}),
            )
            .await?;
        let pending = Self::names(&v, "nonindexed_blob_names", true)?;
        let unknown = Self::names(&v, "unknown_blob_names", true)?;
        let mut failed = Self::names(&v, "failed_blob_names", false)?;
        failed.extend(Self::names(&v, "error_blob_names", false)?);
        let all: BTreeSet<String> = self.names.iter().cloned().collect();
        if pending
            .union(&unknown)
            .chain(failed.iter())
            .any(|n| !all.contains(n))
        {
            return Err(BenchError::Protocol("status includes foreign blob".into()));
        }
        let unknown_n = unknown.len();
        let failed_n = failed.difference(&unknown).count();
        let pending_n = pending
            .iter()
            .filter(|n| !unknown.contains(*n) && !failed.contains(*n))
            .count();
        let ready = files
            .len()
            .checked_sub(unknown_n + failed_n + pending_n)
            .ok_or_else(|| invalid("status counts"))?;
        Readiness::new(files.len(), ready, pending_n, failed_n, unknown_n)
    }
    async fn search(&mut self, input: &SearchInput) -> Result<Value> {
        if input.path_prefix.is_some() {
            return Err(invalid(
                "OCE public adapter does not support this path-prefix scope",
            ));
        }
        self.post("/agents/codebase-retrieval",json!({"information_request":input.query,"blobs":{"added_blobs":self.names,"deleted_blobs":[]}})).await
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}
