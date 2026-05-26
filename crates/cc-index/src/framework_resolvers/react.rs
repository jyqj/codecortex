//! React component resolver (stub).
//!
//! Planned enrichment:
//! - `enrich_file`: enrich component metadata, detect hooks patterns
//! - `resolve_cross_file`: resolve JSX component references to their defining symbols

use cc_model::parse::ParseOutcome;
use cc_model::Language;

use super::{FrameworkResolver, ProjectFrameworkContext};

pub struct ReactComponentResolver;

impl FrameworkResolver for ReactComponentResolver {
    fn framework_key(&self) -> &str {
        "react"
    }

    fn languages(&self) -> &[Language] {
        &[Language::TypeScript, Language::JavaScript]
    }

    fn enrich_file(
        &self,
        _file_path: &str,
        _source: &str,
        _lang: Language,
        _outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // TODO Phase 2.2: Enrich component metadata, detect hooks patterns
    }

    fn resolve_cross_file(
        &self,
        _catalog: &crate::resolver::SymbolCatalog,
        _outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // TODO Phase 2.2: Resolve JSX component references to their defining symbols
    }
}
