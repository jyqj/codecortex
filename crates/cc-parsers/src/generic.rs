//! Generic fallback parser — line-based chunking, no AST.

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};

pub struct GenericParser {
    chunker: Chunker,
}

impl GenericParser {
    pub fn new() -> Self {
        Self {
            chunker: Chunker::default(),
        }
    }
}

impl Default for GenericParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for GenericParser {
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome> {
        let tier = ParserTier::Generic;
        let confidence = tier.default_confidence();
        let line_count = content.lines().count();
        let summary = format!("{} ({} lines)", file_path, line_count);

        let chunks = self
            .chunker
            .chunk_by_lines(file_path, content, language, tier, confidence);

        Ok(ParseOutcome {
            summary,
            chunks,
            parser_tier: tier,
            parser_confidence: confidence,
            ..Default::default()
        })
    }

    fn parse_with_timeout(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        timeout_micros: Option<u64>,
    ) -> CcResult<ParseOutcome> {
        let started = std::time::Instant::now();
        let tier = ParserTier::Generic;
        let confidence = tier.default_confidence();
        let line_count = content.lines().count();
        let summary = format!("{} ({} lines)", file_path, line_count);

        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "generic parser timeout exceeded".to_string(),
            });
        }

        let chunks = self
            .chunker
            .chunk_by_lines(file_path, content, language, tier, confidence);

        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "generic parser timeout exceeded".to_string(),
            });
        }

        Ok(ParseOutcome {
            summary,
            chunks,
            parser_tier: tier,
            parser_confidence: confidence,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[
            Language::Unknown,
            Language::Markdown,
            Language::Java,
            Language::Go,
            Language::Rust,
        ]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Generic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_parse_produces_chunks() {
        let p = GenericParser::new();
        let content = "def foo():\n    pass\n\ndef bar():\n    return 1\n";
        let outcome = p.parse("test.py", content, Language::Python).unwrap();
        assert!(!outcome.chunks.is_empty());
        assert_eq!(outcome.parser_tier, ParserTier::Generic);
        assert!(outcome.summary.contains("test.py"));
    }
}
