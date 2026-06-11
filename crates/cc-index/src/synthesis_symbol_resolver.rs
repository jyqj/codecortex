//! Batched name → symbol disambiguation for dispatch synthesis passes.
//!
//! Synthesis passes resolve free-form names (JSX tags, Vue child components,
//! event handler expressions) to symbol UIDs. Each pass prefetches every name
//! it needs with one batched `IN (...)` query and disambiguates in memory, so
//! a file with N listeners costs one SQL round-trip instead of N.
//!
//! All passes share the same core policy — prefer a same-file match,
//! otherwise accept only a truly unique global match, never "first of
//! several" — but differ in whether candidate rows without a `symbol_uid`
//! participate in disambiguation. That difference is expressed as two
//! methods ([`SynthesisSymbolResolver::resolve_strict`] /
//! [`SynthesisSymbolResolver::resolve_lenient`]) rather than silently
//! unified.

use std::collections::{HashMap, HashSet};

use cc_db::index_db::{IndexDb, SymbolRow};
use cc_model::CcResult;

/// How a name was disambiguated to a single symbol. Callers map this to a
/// pass-specific confidence value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionScope {
    /// Matched a symbol defined in the same file as the use site.
    SameFile,
    /// The name had exactly one candidate repo-wide.
    UniqueGlobal,
}

/// Per-pass symbol resolver: build with [`Self::prefetch`], then call a
/// `resolve_*` method per use site.
#[derive(Default)]
pub(crate) struct SynthesisSymbolResolver {
    by_name: HashMap<String, Vec<SymbolRow>>,
}

impl SynthesisSymbolResolver {
    /// Batch-fetch all `names` (deduplicated) restricted to `kinds` in one
    /// pass over the DB.
    pub(crate) fn prefetch(db: &IndexDb, names: &[&str], kinds: &[&str]) -> CcResult<Self> {
        let unique_names: Vec<&str> = names
            .iter()
            .copied()
            .collect::<HashSet<&str>>()
            .into_iter()
            .collect();
        Ok(Self {
            by_name: db
                .reads()
                .find_symbols_by_names_and_kinds(&unique_names, kinds)?,
        })
    }

    /// Strict semantics (event-emitter handlers, re-render chain children,
    /// Vue event handlers): rows without a `symbol_uid` still count toward
    /// ambiguity, and a same-file row missing its uid resolves to `None`
    /// without falling back to the unique-global rule.
    ///
    /// Note: the event-emitter pass historically checked unique-global before
    /// same-file; the two orderings are equivalent (a unique match in the
    /// current file is also the first same-file match), so this single
    /// ordering preserves all three call sites' behavior.
    pub(crate) fn resolve_strict(
        &self,
        name: &str,
        current_file: &str,
    ) -> Option<(String, ResolutionScope)> {
        let matches = self.by_name.get(name)?;
        if let Some(found) = matches.iter().find(|s| s.file_path == current_file) {
            return found
                .symbol_uid
                .clone()
                .map(|uid| (uid, ResolutionScope::SameFile));
        }
        if matches.len() == 1 {
            return matches[0]
                .symbol_uid
                .clone()
                .map(|uid| (uid, ResolutionScope::UniqueGlobal));
        }
        None
    }

    /// Lenient semantics (JSX tags, Vue child components): rows without a
    /// `symbol_uid` are dropped before disambiguation, so a single
    /// uid-bearing candidate still resolves even when uid-less rows share the
    /// name.
    pub(crate) fn resolve_lenient(
        &self,
        name: &str,
        current_file: &str,
    ) -> Option<(String, ResolutionScope)> {
        let matches = self.by_name.get(name)?;
        let candidates: Vec<&SymbolRow> =
            matches.iter().filter(|s| s.symbol_uid.is_some()).collect();
        if let Some(found) = candidates.iter().find(|s| s.file_path == current_file) {
            return found
                .symbol_uid
                .clone()
                .map(|uid| (uid, ResolutionScope::SameFile));
        }
        if candidates.len() == 1 {
            return candidates[0]
                .symbol_uid
                .clone()
                .map(|uid| (uid, ResolutionScope::UniqueGlobal));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, file_path: &str, uid: Option<&str>) -> SymbolRow {
        SymbolRow {
            symbol_id: format!("sym:{}:{}", file_path, name),
            symbol_uid: uid.map(|u| u.to_string()),
            name: name.to_string(),
            kind: "function".to_string(),
            file_path: file_path.to_string(),
            container: None,
            start_line: 1,
            end_line: 10,
            qname: None,
            signature: None,
        }
    }

    fn resolver(rows: Vec<SymbolRow>) -> SynthesisSymbolResolver {
        let mut by_name: HashMap<String, Vec<SymbolRow>> = HashMap::new();
        for r in rows {
            by_name.entry(r.name.clone()).or_default().push(r);
        }
        SynthesisSymbolResolver { by_name }
    }

    #[test]
    fn same_file_match_preferred_over_other_files() {
        let r = resolver(vec![
            row("handle", "src/a.ts", Some("uid_a")),
            row("handle", "src/b.ts", Some("uid_b")),
        ]);
        assert_eq!(
            r.resolve_strict("handle", "src/b.ts"),
            Some(("uid_b".to_string(), ResolutionScope::SameFile))
        );
        assert_eq!(
            r.resolve_lenient("handle", "src/b.ts"),
            Some(("uid_b".to_string(), ResolutionScope::SameFile))
        );
    }

    #[test]
    fn global_ambiguity_without_same_file_returns_none() {
        let r = resolver(vec![
            row("handle", "src/a.ts", Some("uid_a")),
            row("handle", "src/b.ts", Some("uid_b")),
        ]);
        assert_eq!(r.resolve_strict("handle", "src/c.ts"), None);
        assert_eq!(r.resolve_lenient("handle", "src/c.ts"), None);
    }

    #[test]
    fn unique_global_match_resolves() {
        let r = resolver(vec![row("handle", "src/a.ts", Some("uid_a"))]);
        assert_eq!(
            r.resolve_strict("handle", "src/c.ts"),
            Some(("uid_a".to_string(), ResolutionScope::UniqueGlobal))
        );
        assert_eq!(r.resolve_strict("missing", "src/c.ts"), None);
    }

    #[test]
    fn uid_less_rows_block_strict_but_not_lenient() {
        // One uid-less row plus one uid-bearing row in another file:
        // strict counts both toward ambiguity (None), lenient drops the
        // uid-less row and resolves the unique remaining candidate.
        let r = resolver(vec![
            row("Button", "src/a.tsx", None),
            row("Button", "src/b.tsx", Some("uid_b")),
        ]);
        assert_eq!(r.resolve_strict("Button", "src/c.tsx"), None);
        assert_eq!(
            r.resolve_lenient("Button", "src/c.tsx"),
            Some(("uid_b".to_string(), ResolutionScope::UniqueGlobal))
        );
        // A same-file row whose uid is missing resolves to None under strict
        // (no fallback), matching the original per-site behavior.
        assert_eq!(r.resolve_strict("Button", "src/a.tsx"), None);
    }
}
