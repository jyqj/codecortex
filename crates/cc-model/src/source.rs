//! Original-byte snapshots and owned syntax coordinates. No filesystem or parser handles.
use crate::{CcError, CcResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}
impl ByteSpan {
    pub fn new(start: usize, end: usize) -> CcResult<Self> {
        if start > end {
            return Err(CcError::InvalidParams("reversed byte span".into()));
        }
        Ok(Self { start, end })
    }
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
    pub fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEncoding {
    Utf8,
    Opaque,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub snapshot_id: String,
    pub content_digest: String,
    pub byte_len: usize,
    pub encoding: SourceEncoding,
}
/// Borrowed original input; raw identity exists even when UTF-8 decoding fails.
/// Parser APIs accept UTF-8. This type never silently substitutes replacement text.
pub struct SourceSnapshot<'a> {
    bytes: &'a [u8],
    text: Option<&'a str>,
    line_starts: Vec<usize>,
    identity: SourceIdentity,
}
impl<'a> SourceSnapshot<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        let text = std::str::from_utf8(bytes).ok();
        let encoding = if text.is_some() {
            SourceEncoding::Utf8
        } else {
            SourceEncoding::Opaque
        };
        let content_digest = blake3::hash(bytes).to_hex().to_string();
        let mut hash = blake3::Hasher::new();
        hash.update(b"codecortex-source-v1\0");
        hash.update(if text.is_some() {
            b"utf8\0"
        } else {
            b"opaque\0"
        });
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
        let mut line_starts = vec![0];
        line_starts.extend(
            bytes
                .iter()
                .enumerate()
                .filter_map(|(i, b)| (*b == b'\n').then_some(i + 1)),
        );
        Self {
            bytes,
            text,
            line_starts,
            identity: SourceIdentity {
                snapshot_id: hash.finalize().to_hex().to_string(),
                content_digest,
                byte_len: bytes.len(),
                encoding,
            },
        }
    }
    pub fn slice_digest(&self, span: ByteSpan) -> CcResult<String> {
        Ok(blake3::hash(self.raw_slice(span)?).to_hex().to_string())
    }
    pub fn identity(&self) -> &SourceIdentity {
        &self.identity
    }
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    pub fn text(&self) -> CcResult<&'a str> {
        self.text
            .ok_or_else(|| CcError::InvalidParams("source encoding is not UTF-8".into()))
    }
    pub fn whole(&self) -> ByteSpan {
        ByteSpan {
            start: 0,
            end: self.bytes.len(),
        }
    }
    /// Nonempty physical lines. A final LF has an EOF point, not an extra text line.
    pub fn line_count(&self) -> usize {
        if self.bytes.is_empty() {
            0
        } else {
            self.line_starts.len() - usize::from(self.bytes.last() == Some(&b'\n'))
        }
    }
    /// 1-based line, 0-based byte column. EOF immediately after LF is next-line column zero.
    pub fn point(&self, byte: usize) -> CcResult<(usize, usize)> {
        if byte > self.bytes.len() {
            return Err(CcError::InvalidParams("byte outside snapshot".into()));
        }
        let line = self
            .line_starts
            .partition_point(|start| *start <= byte)
            .saturating_sub(1);
        Ok((line + 1, byte - self.line_starts[line]))
    }
    pub fn line_start(&self, line: usize) -> Option<usize> {
        line.checked_sub(1)
            .and_then(|i| self.line_starts.get(i).copied())
    }
    pub fn line_end(&self, line: usize) -> Option<usize> {
        if line == 0 || line > self.line_count() {
            None
        } else {
            Some(
                self.line_starts
                    .get(line)
                    .copied()
                    .unwrap_or(self.bytes.len()),
            )
        }
    }
    pub fn line_range(&self, first: usize, last: usize) -> CcResult<ByteSpan> {
        if first == 0 || first > last || last > self.line_count() {
            return Err(CcError::InvalidParams("line range outside snapshot".into()));
        }
        ByteSpan::new(
            self.line_start(first).unwrap(),
            self.line_end(last).unwrap(),
        )
    }
    pub fn raw_slice(&self, span: ByteSpan) -> CcResult<&'a [u8]> {
        self.bytes
            .get(span.start..span.end)
            .ok_or_else(|| CcError::InvalidParams("span outside snapshot".into()))
    }
    pub fn slice(&self, span: ByteSpan) -> CcResult<&'a str> {
        self.text()?.get(span.start..span.end).ok_or_else(|| {
            CcError::InvalidParams("span outside snapshot or inside UTF-8 character".into())
        })
    }
    /// Inclusive display lines occupied by nonempty [start,end); never counts a phantom final line.
    pub fn lines(&self, span: ByteSpan) -> CcResult<(u32, u32)> {
        self.raw_slice(span)?;
        if span.is_empty() {
            return Err(CcError::InvalidParams(
                "empty span has no occupied lines".into(),
            ));
        }
        let first = self.point(span.start)?.0;
        let last = self.point(span.end - 1)?.0;
        Ok((
            u32::try_from(first)
                .map_err(|_| CcError::InvalidParams("line count overflow".into()))?,
            u32::try_from(last)
                .map_err(|_| CcError::InvalidParams("line count overflow".into()))?,
        ))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    Symbol,
    Block,
    /// An independent branch/loop/exception construct is a merge barrier.
    Control,
    Statement,
    Comment,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntaxBoundary {
    pub span: ByteSpan,
    pub parent: Option<u32>,
    pub kind: BoundaryKind,
    pub name: Option<String>,
    pub symbol_kind: Option<crate::symbol::SymbolKind>,
    pub signature: Option<ByteSpan>,
    pub leading_comment: Option<ByteSpan>,
    /// A statically recognized leading body docstring; never a dynamic string expression.
    #[serde(default)]
    pub documentation: Option<ByteSpan>,
}
impl SyntaxBoundary {
    /// A partition may retain adjacent whitespace, but never another
    /// declaration's body. Leading documentation is an explicit AST relation.
    pub fn owns_chunk(&self, proof: &ChunkSource, text: &str) -> bool {
        let permitted_start = self.leading_comment.map_or(self.span.start, |s| s.start);
        let permitted = ByteSpan {
            start: permitted_start,
            end: self.span.end,
        };
        if permitted.contains(proof.span) {
            return true;
        }
        if proof.span.end <= permitted.start || proof.span.start >= permitted.end {
            return false;
        }
        let whitespace = |span: ByteSpan| {
            text.get(span.start..span.end)
                .is_some_and(|s| s.chars().all(char::is_whitespace))
        };
        (proof.span.start >= permitted.start
            || whitespace(ByteSpan {
                start: 0,
                end: permitted.start - proof.span.start,
            }))
            && (proof.span.end <= permitted.end
                || whitespace(ByteSpan {
                    start: permitted.end - proof.span.start,
                    end: text.len(),
                }))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceStructure {
    pub source: SourceIdentity,
    pub capability: String,
    pub complete: bool,
    pub reasons: Vec<String>,
    pub visited_nodes: usize,
    pub boundaries: Vec<SyntaxBoundary>,
}
impl SourceStructure {
    pub fn validate(&self, source: &SourceSnapshot<'_>) -> bool {
        if self.source != *source.identity() || self.boundaries.len() > 50_000 {
            return false;
        }
        self.boundaries.iter().enumerate().all(|(i, b)| {
            source.slice(b.span).is_ok()
                && !b.span.is_empty()
                && b.parent.is_none_or(|p| {
                    (p as usize) < i && self.boundaries[p as usize].span.contains(b.span)
                })
                && b.signature
                    .is_none_or(|s| b.span.contains(s) && source.slice(s).is_ok())
                && b.leading_comment
                    .is_none_or(|s| s.end <= b.span.start && source.slice(s).is_ok())
                && b.documentation
                    .is_none_or(|s| b.span.contains(s) && source.slice(s).is_ok())
                && b.name.as_ref().is_none_or(|s| s.len() <= 4096)
        })
    }

    pub fn fallback(source: &SourceSnapshot<'_>, reason: &str) -> Self {
        Self {
            source: source.identity().clone(),
            capability: "line_fallback".into(),
            complete: false,
            reasons: vec![reason.into()],
            visited_nodes: 0,
            boundaries: vec![],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkSource {
    pub source: SourceIdentity,
    pub span: ByteSpan,
    pub slice_digest: String,
    pub boundary: String,
    pub owner: Option<ByteSpan>,
    pub signature: Option<ByteSpan>,
}
impl ChunkSource {
    pub fn validate(&self, text: &str) -> bool {
        let hash_ok = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
        let valid = |s: ByteSpan| s.start <= s.end && s.end <= self.source.byte_len;
        valid(self.span)
            && !self.span.is_empty()
            && self.span.len() == text.len()
            && self.source.encoding == SourceEncoding::Utf8
            && !self.boundary.is_empty()
            && hash_ok(&self.source.snapshot_id)
            && hash_ok(&self.source.content_digest)
            && blake3::hash(text.as_bytes()).to_hex().as_str() == self.slice_digest
            && self.owner.is_none_or(valid)
            && self.signature.is_none_or(valid)
    }
}
