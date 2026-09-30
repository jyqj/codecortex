//! Bounded disk verification for indexed coordinates. Never slice new bytes with an old basis.
use cc_db::index_db::IndexDb;
use cc_model::{search::SearchHit, CcError, CcResult};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
pub const FILE_LIMIT: usize = 16 * 1024 * 1024;
pub use crate::evidence_path::resolve_source_path;
pub const QUERY_LIMIT: usize = 64 * 1024 * 1024;
#[derive(Debug, Serialize)]
pub struct VerifiedSource {
    pub status: &'static str,
    pub indexed_digest: Option<String>,
    pub observed_digest: Option<String>,
    pub read_bytes_charged: usize,
    #[serde(skip)]
    pub text: Option<String>,
}
impl VerifiedSource {
    pub fn is_current(&self) -> bool {
        self.status == "current_verified"
    }
    pub fn require_text(self) -> CcResult<String> {
        self.text.ok_or_else(|| {
            CcError::Search(format!(
                "source_freshness:{}; reindex before reading indexed coordinates",
                self.status
            ))
        })
    }
}
pub fn read_verified(
    db: &IndexDb,
    root: &Path,
    path: &str,
    limit: usize,
) -> CcResult<VerifiedSource> {
    let normalized = cc_model::repo_path::normalize_relative(path)?;
    let path = normalized.as_str();
    if !cc_model::repo_path::is_canonical_file(path) {
        return Err(CcError::InvalidParams("invalid source path".into()));
    }
    let before = db.reads().read_generation()?;
    let hash = cc_db::document_store::indexed_source_digest(db, path)?;
    let mut result = VerifiedSource {
        status: "not_indexed",
        indexed_digest: hash,
        observed_digest: None,
        read_bytes_charged: 0,
        text: None,
    };
    if result.indexed_digest.is_none() {
        return Ok(result);
    }
    let resolved = match resolve_source_path(root, path) {
        Ok(p) => p,
        Err(e) => {
            result.status = if e.to_string().contains("does not exist") {
                "deleted"
            } else {
                "source_unavailable"
            };
            return Ok(result);
        }
    };
    let canonical_root = root.canonicalize()?;
    let relative = resolved
        .strip_prefix(&canonical_root)
        .map_err(|_| CcError::InvalidParams("source root changed".into()))?;
    let relative = cc_model::repo_path::from_native_relative(relative)?;
    match cc_model::input_file::read(&canonical_root, &relative, limit.min(FILE_LIMIT)) {
        Ok(None) => result.status = "deleted",
        Err(cc_model::input_file::ReadError::Size) => {
            result.status = "source_read_limit";
            result.read_bytes_charged = limit.min(FILE_LIMIT);
        }
        Err(cc_model::input_file::ReadError::Read) => {
            result.status = "source_unavailable";
            result.read_bytes_charged = limit.min(FILE_LIMIT);
        }
        Err(_) => result.status = "source_unavailable",
        Ok(Some(bytes)) => {
            result.read_bytes_charged = bytes.len();
            let observed = cc_model::identity::bytes_hash(&bytes);
            result.status = if result.indexed_digest.as_ref() == Some(&observed) {
                "current_verified"
            } else {
                "stale_with_disk_change"
            };
            result.observed_digest = Some(observed);
            if result.is_current() {
                match String::from_utf8(bytes) {
                    Ok(text) => result.text = Some(text),
                    Err(_) => result.status = "unsupported_encoding",
                };
            }
        }
    }
    if db.reads().read_generation()? != before {
        result.status = "index_changed_retry";
        result.text = None;
    }
    Ok(result)
}
/// Per-public-query cache only. Disk state is never put into the index result LRU.
pub struct SourceVerifier<'a> {
    db: &'a IndexDb,
    root: &'a Path,
    remaining: usize,
    files: BTreeMap<String, VerifiedSource>,
    omitted: BTreeMap<String, &'static str>,
    budget_exhausted: bool,
}
impl<'a> SourceVerifier<'a> {
    pub fn new(db: &'a IndexDb, root: &'a Path) -> Self {
        Self {
            db,
            root,
            remaining: QUERY_LIMIT,
            files: BTreeMap::new(),
            omitted: BTreeMap::new(),
            budget_exhausted: false,
        }
    }
    pub fn path_current(&mut self, path: &str) -> CcResult<bool> {
        if !self.files.contains_key(path) {
            if self.files.len() >= 128 || self.remaining == 0 {
                self.budget_exhausted = true;
                self.omitted.insert(path.into(), "query_source_budget");
                return Ok(false);
            }
            let evidence = read_verified(self.db, self.root, path, self.remaining)?;
            // Charge successful reads; unsuccessful bounded reads may consume the remaining cap.
            self.remaining = self.remaining.saturating_sub(evidence.read_bytes_charged);
            self.files.insert(path.into(), evidence);
        }
        let evidence = &self.files[path];
        if !evidence.is_current() {
            self.omitted.insert(path.into(), evidence.status);
            return Ok(false);
        }
        Ok(true)
    }
    pub fn hit_current(&mut self, hit: &SearchHit) -> CcResult<bool> {
        if !self.path_current(&hit.file_path)? {
            return Ok(false);
        }
        let evidence = &self.files[&hit.file_path];
        if let Some(raw) = hit.metadata.get("source_evidence") {
            let proof: cc_model::source::ChunkSource = serde_json::from_value(raw.clone())?;
            if Some(&proof.source.content_digest) != evidence.indexed_digest.as_ref()
                || !proof.validate(&hit.text)
                || evidence
                    .text
                    .as_deref()
                    .and_then(|s| s.get(proof.span.start..proof.span.end))
                    != Some(hit.text.as_str())
            {
                self.omitted
                    .insert(hit.file_path.clone(), "indexed_snapshot_mismatch");
                return Ok(false);
            }
        } else {
            self.omitted
                .insert(hit.file_path.clone(), "unverified_legacy_source");
            return Ok(false);
        }
        if let Some(raw) = hit.metadata.get("document") {
            let reference = serde_json::from_value(raw.clone())?;
            if !cc_db::document_store::is_current(self.db, &reference)? {
                self.omitted
                    .insert(hit.file_path.clone(), "document_version_changed");
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(crate) fn verified_text(&self, path: &str) -> Option<&str> {
        self.files
            .get(path)
            .filter(|e| e.is_current())
            .and_then(|e| e.text.as_deref())
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"partial":!self.omitted.is_empty(),"checked_files":self.files.len(),"omitted_files":self.omitted,"budget_exhausted":self.budget_exhausted,"read_budget_charged":QUERY_LIMIT-self.remaining,"scope":"bounded per-file disk verification; not an atomic filesystem or whole-query snapshot"})
    }
}
