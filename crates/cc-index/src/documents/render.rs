//! Pure bounded projection of a verified chunk. No IO, tokenizer or provider.
use cc_model::{
    chunk::ChunkRecord,
    retrieval::EmbeddingInput,
    source::{ByteSpan, SourceSnapshot},
    CcError, CcResult,
};
use serde::{Deserialize, Serialize};
pub const FORMAT_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderOptions {
    pub max_bytes: usize,
    pub max_estimated_tokens: usize,
    pub metadata_bytes: usize,
    pub signature_bytes: usize,
    pub include_signature: bool,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            max_bytes: 32768,
            max_estimated_tokens: 8192,
            metadata_bytes: 2048,
            signature_bytes: 512,
            include_signature: true,
        }
    }
}
fn error(s: &str) -> CcError {
    CcError::Config(format!("embedding projection: {s}"))
}
fn prefix(text: &str, max: usize) -> (&str, bool) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], end < text.len())
}
/// Preserve the complete source slice or fail. Metadata may be truncated with
/// an explicit flag; source text is never silently shortened or reconstructed.
pub fn render(
    source: &SourceSnapshot<'_>,
    chunk: &ChunkRecord,
    options: RenderOptions,
) -> CcResult<EmbeddingInput> {
    if !(128..=1048576).contains(&options.max_bytes)
        || !(32..=262144).contains(&options.max_estimated_tokens)
        || !(128..=65536).contains(&options.metadata_bytes)
        || options.signature_bytes > 65536
    {
        return Err(error("invalid render budgets"));
    }
    if !cc_model::repo_path::is_canonical_file(&chunk.file_path) || chunk.file_path.len() > 4096 {
        return Err(error("invalid path"));
    }
    let proof = chunk
        .source
        .as_ref()
        .ok_or_else(|| error("verified source required"))?;
    if !proof.validate(&chunk.text)
        || proof.source != *source.identity()
        || source.slice(proof.span)? != chunk.text
        || source.lines(proof.span)? != (chunk.start_line, chunk.end_line)
    {
        return Err(error("source identity/span/text/line mismatch"));
    }
    if proof
        .signature
        .is_some_and(|s| proof.owner.is_none_or(|o| !o.contains(s)))
    {
        return Err(error("signature outside declared owner"));
    }
    let cap = options
        .max_bytes
        .min(options.max_estimated_tokens.saturating_mul(4));
    const PREFIX: &str = "CODECORTEX_EMBED_V1\nmetadata: ";
    const SEPARATOR: &str = "\nsource:\n";
    if chunk
        .text
        .len()
        .saturating_add(PREFIX.len() + SEPARATOR.len())
        > cap
    {
        return Err(error(
            "source exceeds input budget; rechunk instead of truncating",
        ));
    }
    let (label, label_cut) = prefix(&chunk.breadcrumb, 1024);
    let mut breadcrumb = label.to_owned();
    let mut truncated = label_cut;
    let mut signature = if options.include_signature {
        proof
            .signature
            .map(|s| source.slice(s))
            .transpose()?
            .map(|s| {
                let (t, cut) = prefix(s, options.signature_bytes);
                truncated |= cut;
                t.to_owned()
            })
    } else {
        None
    };
    let available = options
        .metadata_bytes
        .min(cap - chunk.text.len() - PREFIX.len() - SEPARATOR.len());
    let header = loop {
        let h = serde_json::to_string(
            &serde_json::json!({"path":chunk.file_path,"language":chunk.language.as_str(),"parent":breadcrumb,"signature":signature,"source_span":proof.span,"metadata_truncated":truncated}),
        )?;
        if h.len() <= available {
            break h;
        }
        truncated = true;
        if signature.take().is_some() {
            continue;
        }
        if !breadcrumb.is_empty() {
            breadcrumb = prefix(&breadcrumb, breadcrumb.len() / 2).0.to_owned();
            continue;
        }
        return Err(error("path and required metadata exceed input budget"));
    };
    let mut text =
        String::with_capacity(PREFIX.len() + header.len() + SEPARATOR.len() + chunk.text.len());
    text.push_str(PREFIX);
    text.push_str(&header);
    text.push_str(SEPARATOR);
    let start = text.len();
    text.push_str(&chunk.text);
    let input_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    let render_key = blake3::hash(&serde_json::to_vec(&(
        FORMAT_VERSION,
        options,
        &proof.source,
        &proof.slice_digest,
        &input_hash,
    ))?)
    .to_hex()
    .to_string();
    Ok(EmbeddingInput {
        format_version: FORMAT_VERSION,
        source_range: ByteSpan {
            start,
            end: text.len(),
        },
        source_snapshot_id: proof.source.snapshot_id.clone(),
        source_slice_digest: proof.slice_digest.clone(),
        metadata_truncated: truncated,
        token_estimate: cc_model::approx_tokens(&text),
        token_estimator: cc_model::chunk_policy::TOKEN_ESTIMATOR.into(),
        text,
        input_hash,
        render_key,
    })
}
