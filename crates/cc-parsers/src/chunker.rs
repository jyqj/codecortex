//! Exact source-slice chunking with hierarchy from the parser's existing AST.
pub mod boundaries;
mod budget;
mod fallback;
mod merge;
mod split;
use cc_model::source::*;

use cc_model::chunk::ChunkRecord;
use cc_model::id::StableId;
use cc_model::symbol::SymbolRecord;
use cc_model::{approx_tokens, chunk_policy::ChunkPolicy, CcResult, Language, ParserTier};

pub struct Chunker {
    pub(crate) policy: ChunkPolicy,
}

impl Chunker {
    /// Construct an independently usable chunker from the same complete policy
    /// consumed by `ParserRegistry`. Invalid limits are rejected; the removed
    /// line-only constructor must not silently create a second policy surface.
    pub fn from_policy(policy: ChunkPolicy) -> CcResult<Self> {
        policy.validate()?;
        Ok(Self { policy })
    }

    /// `ParserRegistry` retains the caller's original policy so its public parse
    /// boundary can return the validation error. No chunking occurs before that
    /// validation; this constructor performs no bounding or fallback.
    pub(crate) fn with_deferred_policy(policy: ChunkPolicy) -> Self {
        Self { policy }
    }

    /// Compatibility path for heuristic parsers without an AST; spans stay exact.
    pub fn chunk_with_symbols(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        symbols: &[SymbolRecord],
        parser_tier: ParserTier,
        parser_confidence: f64,
    ) -> Vec<ChunkRecord> {
        let source = SourceSnapshot::new(content.as_bytes());
        let mut structure = SourceStructure::fallback(&source, "heuristic_symbol_lines");
        let mut items: Vec<_> = symbols
            .iter()
            .filter_map(|s| {
                source
                    .line_range(
                        s.start_line as usize,
                        (s.end_line as usize).min(source.line_count()),
                    )
                    .ok()
                    .map(|span| (s, span))
            })
            .collect();
        items.sort_by_key(|(_, s)| (s.start, std::cmp::Reverse(s.end)));
        let mut parents: Vec<usize> = Vec::new();
        for (symbol, span) in items.into_iter().take(50_000) {
            while parents.last().is_some_and(|&i| {
                !structure.boundaries[i].span.contains(span) || structure.boundaries[i].span == span
            }) {
                parents.pop();
            }
            let parent = parents.last().map(|&i| i as u32);
            let i = structure.boundaries.len();
            structure.boundaries.push(SyntaxBoundary {
                span,
                parent,
                kind: BoundaryKind::Symbol,
                name: Some(symbol.name.clone()),
                symbol_kind: Some(symbol.kind),
                signature: None,
                leading_comment: None,
                documentation: None,
            });
            parents.push(i);
        }
        self.from_structure(
            file_path,
            &source,
            language,
            &structure,
            parser_tier,
            parser_confidence,
        )
    }

    /// Line-based chunking fallback.
    pub fn chunk_by_lines(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        parser_tier: ParserTier,
        parser_confidence: f64,
    ) -> Vec<ChunkRecord> {
        let source = SourceSnapshot::new(content.as_bytes());
        let structure = SourceStructure::fallback(&source, "no_ast_boundaries");
        self.from_structure(
            file_path,
            &source,
            language,
            &structure,
            parser_tier,
            parser_confidence,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn chunk_with_tree(
        &self,
        file: &str,
        content: &str,
        language: Language,
        symbols: &[SymbolRecord],
        tree: &tree_sitter::Tree,
        tier: ParserTier,
        confidence: f64,
    ) -> (Vec<ChunkRecord>, SourceStructure) {
        let source = SourceSnapshot::new(content.as_bytes());
        let mut structure = boundaries::extract(tree, &source, symbols);
        if !structure.validate(&source) {
            structure = SourceStructure::fallback(&source, "invalid_ast_boundaries");
        }
        let chunks = self.from_structure(file, &source, language, &structure, tier, confidence);
        (chunks, structure)
    }
    pub fn from_structure(
        &self,
        file: &str,
        source: &SourceSnapshot<'_>,
        language: Language,
        structure: &SourceStructure,
        tier: ParserTier,
        confidence: f64,
    ) -> Vec<ChunkRecord> {
        if source.text().is_err() {
            return Vec::new();
        }
        let fallback;
        let structure = if structure.validate(source) {
            structure
        } else {
            fallback = SourceStructure::fallback(source, "invalid_or_mismatched_boundaries");
            &fallback
        };
        let member_labels = merge::members(structure);
        let pieces = split::Partition::new(source, structure, self.policy).run();
        merge::coalesce(source, budget::Budget::new(self.policy), pieces)
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let text = source
                    .slice(p.span)
                    .expect("partition preserves UTF-8 boundaries");
                let (first, last) = source.lines(p.span).expect("nonempty partition");
                let owner = p.owner.and_then(|i| structure.boundaries.get(i));
                let breadcrumb = merge::breadcrumb(structure, p.owner, &member_labels);
                let mut chunk = self.make_chunk(
                    file,
                    language,
                    i as u32,
                    first,
                    last,
                    &breadcrumb,
                    text,
                    owner.and_then(|b| b.name.as_deref()),
                    owner.and_then(|b| b.symbol_kind),
                    tier,
                    confidence,
                );
                chunk.source = Some(ChunkSource {
                    source: source.identity().clone(),
                    span: p.span,
                    slice_digest: source.slice_digest(p.span).expect("validated span"),
                    boundary: p.boundary.into(),
                    owner: owner.map(|b| b.span),
                    signature: owner.and_then(|b| b.signature),
                });
                chunk
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn make_chunk(
        &self,
        file_path: &str,
        language: Language,
        chunk_index: u32,
        start_line: u32,
        end_line: u32,
        breadcrumb: &str,
        text: &str,
        symbol_name: Option<&str>,
        symbol_kind: Option<cc_model::symbol::SymbolKind>,
        parser_tier: ParserTier,
        parser_confidence: f64,
    ) -> ChunkRecord {
        ChunkRecord {
            source: None,
            chunk_id: StableId::chunk_id(file_path, chunk_index),
            file_path: file_path.to_string(),
            language,
            chunk_index,
            start_line,
            end_line,
            breadcrumb: breadcrumb.to_string(),
            text: text.to_string(),
            symbol_name: symbol_name.map(String::from),
            symbol_kind,
            token_estimate: approx_tokens(text),
            parser_tier,
            parser_confidence,
        }
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::from_policy(ChunkPolicy::default()).expect("default chunk policy is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_by_lines_basic() {
        let c = Chunker::from_policy(ChunkPolicy {
            lines: 5,
            ..Default::default()
        })
        .unwrap();
        let content = (1..=12)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = c.chunk_by_lines(
            "test.py",
            &content,
            Language::Python,
            ParserTier::Generic,
            0.3,
        );
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 5);
    }

    #[test]
    fn chunk_ids_stable() {
        let c = Chunker::from_policy(ChunkPolicy {
            lines: 10,
            ..Default::default()
        })
        .unwrap();
        let chunks = c.chunk_by_lines(
            "f.py",
            "a\nb\nc",
            Language::Python,
            ParserTier::Generic,
            0.3,
        );
        assert_eq!(chunks[0].chunk_id, "chunk:f.py:0");
    }
}
