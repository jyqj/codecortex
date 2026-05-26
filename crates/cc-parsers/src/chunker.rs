//! Unified chunking logic — splits source code into indexable chunks.

use cc_model::chunk::ChunkRecord;
use cc_model::id::StableId;
use cc_model::symbol::SymbolRecord;
use cc_model::{approx_tokens, Language, ParserTier};

pub struct Chunker {
    pub line_budget: u32,
}

impl Chunker {
    pub fn new(line_budget: u32) -> Self {
        Self { line_budget }
    }

    /// Symbol-aware chunking: each top-level symbol becomes a chunk.
    pub fn chunk_with_symbols(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        symbols: &[SymbolRecord],
        parser_tier: ParserTier,
        parser_confidence: f64,
    ) -> Vec<ChunkRecord> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let mut spans: Vec<(u32, u32, Option<&str>, Option<cc_model::symbol::SymbolKind>)> =
            symbols
                .iter()
                .filter(|s| s.parent_symbol_id.is_none())
                .map(|s| {
                    (
                        s.start_line,
                        s.end_line,
                        Some(s.name.as_str()),
                        Some(s.kind),
                    )
                })
                .collect();

        if spans.is_empty() {
            return self.chunk_by_lines(
                file_path,
                content,
                language,
                parser_tier,
                parser_confidence,
            );
        }

        spans.sort_by_key(|&(start, _, _, _)| start);

        let mut chunks = Vec::new();
        let mut idx: u32 = 0;
        let mut covered: u32 = 0;

        for &(sl, el, sn, sk) in &spans {
            let s0 = sl.saturating_sub(1) as usize;
            let e0 = (el as usize).min(lines.len());

            if (covered as usize) < s0 {
                let gap = lines[covered as usize..s0].join("\n");
                if !gap.trim().is_empty() {
                    chunks.push(self.make_chunk(
                        file_path,
                        language,
                        idx,
                        covered + 1,
                        sl - 1,
                        "",
                        &gap,
                        None,
                        None,
                        parser_tier,
                        parser_confidence,
                    ));
                    idx += 1;
                }
            }

            let text = lines[s0..e0].join("\n");
            let nlines = e0 - s0;
            if nlines as u32 <= self.line_budget {
                chunks.push(self.make_chunk(
                    file_path,
                    language,
                    idx,
                    sl,
                    el,
                    sn.unwrap_or(""),
                    &text,
                    sn,
                    sk,
                    parser_tier,
                    parser_confidence,
                ));
                idx += 1;
            } else {
                let budget = self.line_budget as usize;
                let mut off = 0;
                while off < nlines {
                    let end = (off + budget).min(nlines);
                    let t = lines[s0 + off..s0 + end].join("\n");
                    chunks.push(self.make_chunk(
                        file_path,
                        language,
                        idx,
                        sl + off as u32,
                        sl + end as u32 - 1,
                        sn.unwrap_or(""),
                        &t,
                        sn,
                        sk,
                        parser_tier,
                        parser_confidence,
                    ));
                    idx += 1;
                    off = end;
                }
            }
            covered = el;
        }

        if (covered as usize) < lines.len() {
            let gap = lines[covered as usize..].join("\n");
            if !gap.trim().is_empty() {
                chunks.push(self.make_chunk(
                    file_path,
                    language,
                    idx,
                    covered + 1,
                    lines.len() as u32,
                    "",
                    &gap,
                    None,
                    None,
                    parser_tier,
                    parser_confidence,
                ));
            }
        }

        chunks
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
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }
        let budget = self.line_budget as usize;
        let mut chunks = Vec::new();
        let mut idx: u32 = 0;
        let mut off = 0;
        while off < lines.len() {
            let end = (off + budget).min(lines.len());
            let text = lines[off..end].join("\n");
            if !text.trim().is_empty() {
                chunks.push(self.make_chunk(
                    file_path,
                    language,
                    idx,
                    (off + 1) as u32,
                    end as u32,
                    "",
                    &text,
                    None,
                    None,
                    parser_tier,
                    parser_confidence,
                ));
                idx += 1;
            }
            off = end;
        }
        chunks
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
            embedding: Vec::new(),
            token_estimate: approx_tokens(text),
            parser_tier,
            parser_confidence,
        }
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(80)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_by_lines_basic() {
        let c = Chunker::new(5);
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
        let c = Chunker::new(10);
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
