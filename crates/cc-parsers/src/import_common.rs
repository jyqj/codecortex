//! Shared import-extraction helpers used across tree-sitter language parsers.
//!
//! Each full-AST parser used to hand-roll its own import walk plus an
//! `ImportRecord` assembly that set the same constant tail
//! (`resolved_path: None`, `is_default: false`, `is_reexport: false`) and the
//! same "last path segment is the imported name" logic. This module is the
//! single source for those primitives: a language supplies its import *syntax*
//! (which node kinds carry imports, how to read the path, which separator names
//! the symbol) and the shared code does the traversal and record assembly.
//!
//! Mirrors the `http_call_helpers` / `dataflow_common` seams: `pub(crate)`
//! helpers, per-language config dispatched through verbatim so each migrated
//! language's `ImportRecord` output is bit-for-bit unchanged.
//!
//! Records whose tail is *not* constant stay hand-written at their call sites:
//! the JS/TS re-export records (`is_reexport: true`), the CommonJS
//! default-require record (`is_default: true`), and the ESM `extract_import`
//! record that infers `is_namespace`/`is_default` from the statement text.

use cc_model::edge::ImportRecord;
use tree_sitter::Node;

/// Assemble an [`ImportRecord`] with the constant tail shared by every
/// AST-based importer (`resolved_path: None`, `is_default: false`,
/// `is_reexport: false`). Only the fields that actually vary per language are
/// parameters.
pub(crate) fn make_import(
    file_path: &str,
    import_string: String,
    imported_name: Option<String>,
    alias: Option<String>,
    is_namespace: bool,
) -> ImportRecord {
    ImportRecord {
        file_path: file_path.to_string(),
        import_string,
        resolved_path: None,
        imported_name,
        alias,
        is_namespace,
        is_default: false,
        is_reexport: false,
    }
}

/// The trailing segment of a path, used as the imported symbol name
/// (Java splits on `.`, Go/C/C++ on `/`). Returns `None` only for an empty
/// path, matching `str::rsplit(..).next()` on each call site.
pub(crate) fn last_segment(path: &str, separator: char) -> Option<String> {
    path.rsplit(separator).next().map(String::from)
}

/// Walk the immediate children of `root` and map each node whose kind matches
/// `unit_kind` into zero or more [`ImportRecord`]s via `map_unit`.
///
/// This is the root-level driver shared by the languages whose import
/// statements are top-level declarations (Java `import_declaration`, Go
/// `import_declaration`, C/C++ `preproc_include`). It is intentionally *not*
/// recursive: those parsers only ever scanned `root.children()`, so keeping the
/// traversal flat preserves their exact output.
pub(crate) fn collect_root_imports(
    root: &Node,
    source: &[u8],
    file_path: &str,
    unit_kind: &str,
    map_unit: impl Fn(&Node, &[u8], &str, &mut Vec<ImportRecord>),
) -> Vec<ImportRecord> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == unit_kind {
            map_unit(&child, source, file_path, &mut imports);
        }
    }
    imports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_import_sets_constant_tail() {
        let rec = make_import(
            "a.rs",
            "std::fmt".to_string(),
            Some("fmt".to_string()),
            Some("f".to_string()),
            true,
        );
        assert_eq!(rec.file_path, "a.rs");
        assert_eq!(rec.import_string, "std::fmt");
        assert_eq!(rec.imported_name.as_deref(), Some("fmt"));
        assert_eq!(rec.alias.as_deref(), Some("f"));
        assert!(rec.is_namespace);
        assert!(rec.resolved_path.is_none());
        assert!(!rec.is_default);
        assert!(!rec.is_reexport);
    }

    #[test]
    fn last_segment_splits_on_separator() {
        assert_eq!(last_segment("java.util.List", '.').as_deref(), Some("List"));
        assert_eq!(last_segment("encoding/json", '/').as_deref(), Some("json"));
        // No separator: whole string is the last segment.
        assert_eq!(last_segment("fmt", '/').as_deref(), Some("fmt"));
        // Empty string still yields Some("") to match rsplit(..).next().
        assert_eq!(last_segment("", '.').as_deref(), Some(""));
    }
}
