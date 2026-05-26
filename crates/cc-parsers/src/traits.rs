use cc_model::{CcResult, Language, ParseOutcome, ParserTier};

/// Trait implemented by all language parsers.
pub trait FileParser: Send + Sync {
    /// Parse a single file, returning the full parse outcome.
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome>;

    /// Parse with an optional tree-sitter timeout in microseconds.
    ///
    /// Parsers that do not support timeouts can ignore the hint and delegate to
    /// `parse()`. Tree-sitter based parsers should enforce the timeout best-effort.
    fn parse_with_timeout(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        timeout_micros: Option<u64>,
    ) -> CcResult<ParseOutcome> {
        let _ = timeout_micros;
        self.parse(file_path, content, language)
    }

    /// Languages this parser can handle.
    fn supported_languages(&self) -> &[Language];

    /// The confidence tier of this parser.
    fn tier(&self) -> ParserTier;
}
