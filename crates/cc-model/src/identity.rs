//! Index-local document identities, immutable versions and provenance. No vector publication API.
use crate::{
    chunk::ChunkRecord, retrieval::EmbeddingInput, source::ChunkSource, CcError, CcResult,
};
use serde::{Deserialize, Serialize};
pub const DOCUMENT_VERSION: u32 = 1;
pub fn bytes_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
pub fn hash(value: &impl Serialize) -> CcResult<String> {
    Ok(blake3::hash(&serde_json::to_vec(value)?)
        .to_hex()
        .to_string())
}
fn error() -> CcError {
    CcError::InvalidParams("invalid document provenance/version/input".into())
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentRef {
    pub doc_key: String,
    pub doc_version: String,
    pub entity_key: Option<String>,
    pub encoding_key: Option<String>,
}
/// A current manifest record. Keys are local to an owning index, not globally
/// unique across repositories. Rename creates new keys; no inferred lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentRecord {
    pub reference: DocumentRef,
    pub file_path: String,
    pub chunk_id: String,
    pub kind: String,
    pub occurrence: u32,
    pub policy: String,
    pub encoding_spec: String,
    pub source: ChunkSource,
    pub input: Option<EmbeddingInput>,
    pub render_error: Option<String>,
}
impl DocumentRecord {
    pub fn new(
        chunk: &ChunkRecord,
        policy: &str,
        occurrence: u32,
        encoding_spec: &str,
        rendered: Result<EmbeddingInput, String>,
    ) -> CcResult<Self> {
        let source = chunk.source.clone().ok_or_else(error)?;
        let (input, render_error) = match rendered {
            Ok(i) => (Some(i), None),
            Err(e) => (None, Some(e)),
        };
        let mut record = Self {
            reference: DocumentRef {
                doc_key: String::new(),
                doc_version: String::new(),
                entity_key: None,
                encoding_key: None,
            },
            file_path: chunk.file_path.clone(),
            chunk_id: chunk.chunk_id.clone(),
            kind: "source_chunk".into(),
            occurrence,
            policy: policy.into(),
            encoding_spec: encoding_spec.into(),
            source,
            input,
            render_error,
        };
        // Conservative exact-byte anchor plus duplicate occurrence. Position is
        // versioned, not used as the only identity or publish guard.
        record.reference.doc_key = record.key()?;
        record.reference.entity_key = record.entity()?;
        record.reference.encoding_key = record.encoding()?;
        record.reference.doc_version = record.version()?;
        record.validate(&chunk.text)?;
        Ok(record)
    }
    fn key(&self) -> CcResult<String> {
        hash(&(
            "index-local-doc-v1",
            &self.file_path,
            &self.kind,
            &self.source.slice_digest,
            self.occurrence,
        ))
    }
    fn entity(&self) -> CcResult<Option<String>> {
        self.source
            .owner
            .map(|owner| {
                hash(&(
                    "index-local-entity-span-v1",
                    &self.file_path,
                    &self.source.source.snapshot_id,
                    owner,
                ))
            })
            .transpose()
    }
    fn encoding(&self) -> CcResult<Option<String>> {
        self.input
            .as_ref()
            .map(|i| hash(&("document-encoding-v1", &self.encoding_spec, &i.input_hash)))
            .transpose()
    }
    fn version(&self) -> CcResult<String> {
        hash(&(
            DOCUMENT_VERSION,
            &self.reference.doc_key,
            &self.reference.entity_key,
            &self.kind,
            &self.policy,
            &self.encoding_spec,
            &self.source,
            &self.input,
            &self.render_error,
        ))
    }
    pub fn validate(&self, original: &str) -> CcResult<()> {
        if !crate::repo_path::is_canonical_file(&self.file_path)
            || self.file_path.len() > 4096
            || self.chunk_id.len() > 8192
            || self.kind != "source_chunk"
            || self.policy.len() != 64
            || self.encoding_spec.len() > 4096
            || !self.source.validate(original)
            || self.reference.doc_key != self.key()?
            || self.reference.doc_version != self.version()?
            || self.reference.entity_key != self.entity()?
            || self.reference.encoding_key != self.encoding()?
            || self.input.is_some() == self.render_error.is_some()
        {
            return Err(error());
        }
        if let Some(input) = &self.input {
            if input.text.len() > 1048576
                || input.format_version != 1
                || input.input_hash != blake3::hash(input.text.as_bytes()).to_hex().as_str()
                || input.source_snapshot_id != self.source.source.snapshot_id
                || input.source_slice_digest != self.source.slice_digest
                || input
                    .text
                    .get(input.source_range.start..input.source_range.end)
                    != Some(original)
                || input.token_estimate != crate::approx_tokens(&input.text)
                || input.token_estimator != crate::chunk_policy::TOKEN_ESTIMATOR
            {
                return Err(error());
            }
        }
        if self
            .render_error
            .as_ref()
            .is_some_and(|e| e.is_empty() || e.len() > 1024)
        {
            return Err(error());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentDelta {
    pub upsert: Vec<DocumentRef>,
    pub removed: Vec<DocumentRef>,
    pub unchanged: usize,
    /// Equal encoding inputs are reusable candidates, not published vectors.
    pub reusable_inputs: usize,
    pub render_failed: usize,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentChanges {
    pub files_projected: usize,
    pub upserted: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub reusable_inputs: usize,
    pub render_failed: usize,
}
impl DocumentChanges {
    pub fn observe(&mut self, delta: &DocumentDelta) {
        self.files_projected += 1;
        self.upserted += delta.upsert.len();
        self.removed += delta.removed.len();
        self.unchanged += delta.unchanged;
        self.reusable_inputs += delta.reusable_inputs;
        self.render_failed += delta.render_failed;
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentBatch {
    pub records: Vec<DocumentRecord>,
    pub delta: DocumentDelta,
}
