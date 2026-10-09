//! Free helper functions used across the resolver module.

use cc_model::symbol::SymbolKind;

use super::types::*;

// ---------------------------------------------------------------------------
// Resolution heuristics (inspired by codebase-memory-mcp registry.c)
// ---------------------------------------------------------------------------

/// Penalize confidence when multiple candidates exist.
///
/// For 1–3 candidates: no penalty. For 4+: linear decay capped at 3/count.
/// This prevents over-confident resolution when many symbols share the same name.
pub(in crate::resolver) fn candidate_count_penalty(base: f64, count: usize) -> f64 {
    if count <= 3 {
        base
    } else {
        base * (3.0 / count as f64).min(1.0)
    }
}

/// Check whether a candidate symbol's file is reachable through the current
/// file's import chain.
///
/// Returns true if any import's source module is a dot-prefix of the
/// candidate's module path (or vice versa), indicating the candidate lives
/// in an imported module tree. Unreachable candidates get a 0.5× confidence
/// penalty.
///
/// Uses prefix matching (not substring) to avoid false positives like
/// `"a.b.cd"` matching `"a.b.c"`.
pub(in crate::resolver) fn is_import_reachable(
    candidate_file: &str,
    imports: &[ImportBinding],
) -> bool {
    if imports.is_empty() {
        return false;
    }
    let cand_mod = strip_ext_to_dotted(candidate_file);

    for imp in imports {
        let src = strip_ext_to_dotted(&imp.source_module);
        if dotted_prefix_match(&cand_mod, &src) {
            return true;
        }
    }
    false
}

/// Strip file extension and convert path separators to dots.
pub(in crate::resolver) fn strip_ext_to_dotted(path: &str) -> String {
    let stripped = path
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx")
        .trim_end_matches(".rs")
        .trim_end_matches(".go")
        .trim_end_matches(".java");
    stripped.replace('/', ".")
}

/// Check if `a` is a dot-prefix of `b` (or vice versa).
///
/// "src.utils" is a prefix of "src.utils.helpers" but NOT of "src.utilsXtra".
pub(in crate::resolver) fn dotted_prefix_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // a is prefix of b: b starts with a followed by '.' or end
    if b.len() > a.len() && b.starts_with(a) && b.as_bytes()[a.len()] == b'.' {
        return true;
    }
    // b is prefix of a
    if a.len() > b.len() && a.starts_with(b) && a.as_bytes()[b.len()] == b'.' {
        return true;
    }
    false
}

/// Count the number of common `/`-separated directory prefix segments between
/// two file paths. Extensions are stripped first so that `a.py` vs `a.ts`
/// in the same directory still count as co-located.
pub(in crate::resolver) fn common_path_prefix_len(a: &str, b: &str) -> usize {
    strip_ext(a)
        .split('/')
        .zip(strip_ext(b).split('/'))
        .take_while(|(x, y)| x == y)
        .count()
}

/// Strip common source-file extensions for path comparison.
pub(in crate::resolver) fn strip_ext(path: &str) -> &str {
    for ext in &[".py", ".ts", ".tsx", ".js", ".jsx", ".rs", ".go", ".java"] {
        if let Some(stripped) = path.strip_suffix(ext) {
            return stripped;
        }
    }
    path
}

/// Among multiple candidate indices, pick the one whose file shares the
/// longest common path prefix with `current_file`.
pub(in crate::resolver) fn best_by_import_distance(
    entries: &[CatalogEntry],
    candidates: &[usize],
    current_file: &str,
) -> Option<usize> {
    let best = import_distance_candidates(entries, candidates, current_file);
    (best.len() == 1).then(|| best[0])
}

/// Stable presentation order is never evidence of uniqueness.
pub(in crate::resolver) fn stable_candidates(
    entries: &[CatalogEntry],
    candidates: &[usize],
) -> Vec<usize> {
    let mut result = candidates.to_vec();
    result.sort_by(|&a, &b| {
        let a = &entries[a];
        let b = &entries[b];
        (
            &a.file_path,
            &a.qname,
            a.kind.as_str(),
            &a.symbol_uid,
            &a.symbol_id,
        )
            .cmp(&(
                &b.file_path,
                &b.qname,
                b.kind.as_str(),
                &b.symbol_uid,
                &b.symbol_id,
            ))
    });
    result.dedup_by(|a, b| {
        let a = &entries[*a];
        let b = &entries[*b];
        a.file_path == b.file_path && a.symbol_id == b.symbol_id && a.symbol_uid == b.symbol_uid
    });
    result
}
pub(in crate::resolver) fn import_distance_candidates(
    entries: &[CatalogEntry],
    candidates: &[usize],
    file: &str,
) -> Vec<usize> {
    let mut best = Vec::new();
    let mut longest = None;
    for &idx in candidates {
        let distance = common_path_prefix_len(&entries[idx].file_path, file);
        match longest {
            Some(max) if distance < max => {}
            Some(max) if distance == max => best.push(idx),
            _ => {
                longest = Some(distance);
                best.clear();
                best.push(idx);
            }
        }
    }
    // Preserve the original stable ordering, identity deduplication and ties.
    stable_candidates(entries, &best)
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Check whether a symbol kind represents a function/method/handler — the kinds
/// that can serve as route handlers.
pub(in crate::resolver) fn is_handler_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::RouteHandler
            | SymbolKind::Controller
            | SymbolKind::Middleware
            | SymbolKind::Hook
            | SymbolKind::Component
    )
}

/// Pick unique entry from candidates (deduplicated by symbol_id).
pub(in crate::resolver) fn pick_unique(
    entries: &[CatalogEntry],
    candidates: &[usize],
) -> Option<usize> {
    let unique = stable_candidates(entries, candidates);
    (unique.len() == 1).then(|| unique[0])
}

/// Deduplicate indices by symbol_id.
pub(in crate::resolver) fn dedup_by_id(entries: &[CatalogEntry], indices: &[usize]) -> Vec<usize> {
    stable_candidates(entries, indices)
}

// ---------------------------------------------------------------------------
// USES_TYPE helpers
// ---------------------------------------------------------------------------

/// Check whether a symbol kind represents a type definition.
pub(in crate::resolver) fn is_type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::TypeAlias
    )
}

/// Extract individual type names from a type expression string.
///
/// Handles generics, unions, and container types:
/// - `Promise<User>` -> `["Promise", "User"]`
/// - `Vec<Result<Foo, Err>>` -> `["Vec", "Result", "Foo"]`
/// - `A | B` -> `["A", "B"]`
/// - `Option<T>` -> `["Option"]`
/// - `*http.Client` -> `["http.Client"]`
/// - `&str` -> filtered out (primitive)
/// - `Dict[str, Any]` -> `["Dict"]` (str/Any are primitives)
///
/// Filters out:
/// - Built-in primitives (int, str, bool, float, void, any, None, etc.)
/// - Empty strings and ASCII syntax-only tokens (e.g. variadic tuple ellipsis)
/// - Single-char type params (T, K, V, etc.)
pub(in crate::resolver) fn type_atoms(raw: &str) -> Vec<String> {
    // Remove pointer/reference prefixes
    let s = raw.trim_start_matches(['*', '&']);

    // Split on delimiters: < > [ ] , | ( ) and whitespace
    let mut atoms = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            ch if ch.is_whitespace()
                || matches!(ch, '<' | '>' | '[' | ']' | ',' | '|' | '(' | ')') =>
            {
                let token = current.trim().to_string();
                if !token.is_empty() {
                    atoms.push(token);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let token = current.trim().to_string();
    if !token.is_empty() {
        atoms.push(token);
    }

    // Filter primitives and single-char type params
    static PRIMITIVES: &[&str] = &[
        "int",
        "str",
        "string",
        "bool",
        "boolean",
        "float",
        "f32",
        "f64",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "isize",
        "char",
        "void",
        "any",
        "none",
        "null",
        "undefined",
        "object",
        "number",
        "never",
        "unknown",
        "byte",
        "short",
        "long",
        "double",
        "self",
        "error",
    ];

    atoms
        .into_iter()
        .filter(|a| {
            let lower = a.to_lowercase();
            // Skip primitives
            if PRIMITIVES.contains(&lower.as_str()) {
                return false;
            }
            // Skip single-char type params (T, K, V, E, etc.)
            if a.len() == 1 && a.chars().next().unwrap().is_ascii_uppercase() {
                return false;
            }
            // Exclude ASCII syntax-only atoms, not the identifier domain:
            // '_' and '$' are identifier characters in supported languages,
            // and Unicode identifier starts need not be alphanumeric. Preserve
            // non-ASCII/unknown spellings; this is not a type grammar validator.
            a.chars()
                .any(|ch| !ch.is_ascii_punctuation() || matches!(ch, '_' | '$'))
        })
        .collect()
}

#[cfg(test)]
mod type_atom_regression_tests {
    use super::type_atoms;

    #[test]
    fn variadic_tuple_keeps_named_types_without_punctuation_edges() {
        assert_eq!(type_atoms("tuple[Widget, ...]"), ["tuple", "Widget"]);
        assert_eq!(type_atoms("tuple[int, ...] | None"), ["tuple"]);
        for raw in ["", " \t\n", "...", "::", "?", "&", "***"] {
            assert!(type_atoms(raw).is_empty(), "{raw:?}");
        }
        // '_' is a legal Python/TypeScript name, rather than syntax punctuation.
        assert_eq!(type_atoms("_"), ["_"]);
    }

    #[test]
    fn qualified_generic_union_and_unicode_types_remain_dependencies() {
        assert_eq!(
            type_atoms("Vec<Result<pkg::Widget, net.Error>> | 数据.类型"),
            ["Vec", "Result", "pkg::Widget", "net.Error", "数据.类型"]
        );
        assert_eq!(type_atoms("*http.Client"), ["http.Client"]);
        assert_eq!(type_atoms("Dict[str, Any]"), ["Dict"]);
        assert_eq!(type_atoms("Option<T>"), ["Option"]);
        assert_eq!(
            type_atoms("Pair<\tLeft,\nRight>"),
            ["Pair", "Left", "Right"]
        );
    }

    #[test]
    fn unknown_type_spellings_are_not_silently_discarded() {
        // Preserve identifiers even when this helper cannot validate the grammar.
        assert_eq!(
            type_atoms("_Private | Type? | pkg. | 123"),
            ["_Private", "Type?", "pkg.", "123"]
        );
    }
}

#[cfg(test)]
#[path = "type_atom_identifier_tests.rs"]
mod type_atom_identifier_tests;

#[cfg(test)]
mod import_distance_regression_tests {
    use super::*;

    fn legacy_prefix(a: &str, b: &str) -> usize {
        let a: Vec<_> = strip_ext(a).split('/').collect();
        let b: Vec<_> = strip_ext(b).split('/').collect();
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }

    fn legacy_candidates(
        entries: &[CatalogEntry],
        candidates: &[usize],
        file: &str,
    ) -> Vec<usize> {
        let max = candidates
            .iter()
            .map(|&idx| legacy_prefix(&entries[idx].file_path, file))
            .max();
        stable_candidates(
            entries,
            &candidates
                .iter()
                .copied()
                .filter(|&idx| Some(legacy_prefix(&entries[idx].file_path, file)) == max)
                .collect::<Vec<_>>(),
        )
    }

    fn entry(file: &str, id: &str, qname: &str) -> CatalogEntry {
        CatalogEntry {
            symbol_id: id.to_string(),
            symbol_uid: Some(format!("uid:{id}")),
            name: id.to_string(),
            file_path: file.to_string(),
            kind: SymbolKind::Function,
            container: None,
            qname: Some(qname.to_string()),
            is_default_export: false,
            start_line: 1,
            end_line: 1,
            scope_id: None,
        }
    }

    #[test]
    fn streamed_prefix_matches_original_path_segment_semantics() {
        let paths = [
            "",
            "/",
            "a.py",
            "a.ts",
            "a.tsx",
            "a.py.ts",
            "a.unknown",
            "a.PY",
            "src/a.py",
            "src/a/b.rs",
            "src//a.py",
            "./src/a.py",
            "/src/a.py",
            "src/a/",
            "数据/类型.py",
            "src\\a.py",
        ];
        for a in paths {
            for b in paths {
                assert_eq!(
                    common_path_prefix_len(a, b),
                    legacy_prefix(a, b),
                    "{a:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn one_pass_distance_preserves_original_ties_duplicates_and_order() {
        let entries = [
            entry("src/pkg/a.py", "a", "A"),
            entry("src/pkg/b.ts", "b", "B"),
            entry("src/other/c.rs", "c", "C"),
            entry("elsewhere/d.py", "d", "D"),
            entry("src/pkg/a.py", "a", "A"),
            entry("src/pkg/a.py", "a", "OtherQName"),
        ];
        for file in ["", "src/pkg/query.py", "src/pkg/a.ts", "elsewhere/x.py", "/"] {
            // Exhaust all input sequences of length 0..=4, including repeated
            // indices, different identities, duplicate rows and tied distances.
            for len in 0..=4 {
                for mut code in 0..entries.len().pow(len) {
                    let mut candidates = Vec::new();
                    for _ in 0..len {
                        candidates.push(code % entries.len());
                        code /= entries.len();
                    }
                    let expected = legacy_candidates(&entries, &candidates, file);
                    assert_eq!(
                        import_distance_candidates(&entries, &candidates, file),
                        expected,
                        "{file:?} {candidates:?}"
                    );
                    assert_eq!(
                        best_by_import_distance(&entries, &candidates, file),
                        (expected.len() == 1).then(|| expected[0])
                    );
                }
            }
        }
    }
}
