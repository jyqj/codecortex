//! Versioned source chunk budgets. Token units are estimates, never tokenizer claims.
use crate::{CcError, CcResult};
use serde::{Deserialize, Serialize};
pub const CHUNK_POLICY_VERSION: &str = "source-chunks-v2";
pub const TOKEN_ESTIMATOR: &str = "utf8-bytes-div-ceil-4-v1";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkPolicy {
    pub lines: u32,
    pub bytes: u32,
    /// Unicode scalar values, not graphemes or display columns.
    pub chars: u32,
    pub estimated_tokens: u32,
    /// Merge adjacent non-symbol pieces within one structural domain when one
    /// is smaller than this threshold. Zero disables merging.
    pub merge_min_bytes: u32,
}
impl Default for ChunkPolicy {
    fn default() -> Self {
        Self {
            lines: 80,
            bytes: 16384,
            chars: 16384,
            estimated_tokens: 4096,
            merge_min_bytes: 256,
        }
    }
}
impl ChunkPolicy {
    pub fn validate(self) -> CcResult<()> {
        if !(1..=10000).contains(&self.lines)
            || !(4..=1048576).contains(&self.bytes)
            || !(2..=1048576).contains(&self.chars)
            || !(1..=262144).contains(&self.estimated_tokens)
            || self.merge_min_bytes > 1048576
        {
            return Err(CcError::Config("invalid chunk budgets: lines=1..10000, bytes=4..1048576, Unicode scalars=2..1048576, estimated tokens=1..262144, merge_min_bytes=0..1048576".into()));
        }
        Ok(())
    }
    /// Safety clamp for the legacy infallible Chunker API only. Registry/build
    /// entry points validate the original policy and reject invalid config.
    pub fn bounded(self) -> Self {
        Self {
            lines: self.lines.clamp(1, 10000),
            bytes: self.bytes.clamp(4, 1048576),
            chars: self.chars.clamp(2, 1048576),
            estimated_tokens: self.estimated_tokens.clamp(1, 262144),
            merge_min_bytes: self.merge_min_bytes.min(1048576),
        }
    }
    pub fn effective_bytes(self) -> usize {
        (self.bytes as usize).min((self.estimated_tokens as usize).saturating_mul(4))
    }
    pub fn fingerprint(self) -> String {
        let payload = serde_json::to_vec(&(CHUNK_POLICY_VERSION, TOKEN_ESTIMATOR, self))
            .expect("finite chunk policy");
        blake3::hash(&payload).to_hex().to_string()
    }
}
