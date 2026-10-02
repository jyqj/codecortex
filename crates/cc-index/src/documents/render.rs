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

/// Rendered-input manifest entry for batch admission (P7-003): the exact
/// final bytes an admission planner will measure, plus the declared token
/// estimate stamped at render time. Pure projection — no IO, no re-render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedManifestEntry {
    /// Exact final input bytes (`EmbeddingInput.text`, UTF-8): what the
    /// provider would see, measured as-is — never re-derived from the chunk.
    pub bytes: Vec<u8>,
    /// Declared token estimate stamped by `render`.
    pub token_estimate: u32,
    /// Declared estimator identity (`utf8-bytes-div-ceil-4-v1`).
    pub token_estimator: &'static str,
    /// Explicit render-layer truncation flag (metadata only; source text is
    /// never truncated — render re-chunks or fails instead).
    pub metadata_truncated: bool,
}

/// Expose one rendered input as an admission manifest entry. Refuses to
/// launder a foreign estimator or a drifted estimate: both must match the
/// declared workspace estimator exactly, so the admission side's declared
///口径 and this side's stamped numbers can never disagree silently.
pub fn manifest(input: &EmbeddingInput) -> CcResult<RenderedManifestEntry> {
    if input.token_estimator != cc_model::chunk_policy::TOKEN_ESTIMATOR {
        return Err(error("manifest: foreign token estimator"));
    }
    if input.token_estimate != cc_model::approx_tokens(&input.text) {
        return Err(error(
            "manifest: token estimate drifted from the declared estimator",
        ));
    }
    Ok(RenderedManifestEntry {
        bytes: input.text.as_bytes().to_vec(),
        token_estimate: input.token_estimate,
        token_estimator: cc_model::chunk_policy::TOKEN_ESTIMATOR,
        metadata_truncated: input.metadata_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_model::{source::SourceSnapshot, Language};
    use cc_parsers::ParserRegistry;

    fn rendered_input(text: &str) -> EmbeddingInput {
        let out = ParserRegistry::new()
            .parse("a.py", text, Language::Python)
            .unwrap();
        let source = SourceSnapshot::new(text.as_bytes());
        render(&source, &out.chunks[0], RenderOptions::default()).unwrap()
    }

    #[test]
    fn manifest_reports_exact_final_bytes_and_the_declared_estimate() {
        let input = rendered_input("def sample():\n    return 1\n");
        let entry = manifest(&input).unwrap();
        assert_eq!(entry.bytes, input.text.as_bytes());
        assert_eq!(
            entry.token_estimator,
            cc_model::chunk_policy::TOKEN_ESTIMATOR
        );
        assert_eq!(entry.token_estimate, cc_model::approx_tokens(&input.text));
        assert_eq!(entry.metadata_truncated, input.metadata_truncated);
        // The bytes are the FINAL input (framing prefix + header + source),
        // not the raw chunk text.
        assert!(std::str::from_utf8(&entry.bytes)
            .unwrap()
            .starts_with("CODECORTEX_EMBED_V1\n"));
    }

    #[test]
    fn manifest_refuses_foreign_estimator_or_drifted_estimate() {
        let mut foreign = rendered_input("def sample():\n    return 1\n");
        foreign.token_estimator = "tiktoken-cl100k".into();
        assert!(manifest(&foreign).is_err());

        let mut drifted = rendered_input("def sample():\n    return 1\n");
        drifted.token_estimate += 1;
        assert!(manifest(&drifted).is_err());
    }
}
