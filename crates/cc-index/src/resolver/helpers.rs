//! Free helper functions used across the resolver module.

use std::collections::HashSet;

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
    let seg_a: Vec<&str> = strip_ext(a).split('/').collect();
    let seg_b: Vec<&str> = strip_ext(b).split('/').collect();
    seg_a
        .iter()
        .zip(seg_b.iter())
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
    let best = candidates
        .iter()
        .map(|&i| common_path_prefix_len(&entries[i].file_path, current_file))
        .max()?;
    let winners: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|&i| common_path_prefix_len(&entries[i].file_path, current_file) == best)
        .collect();
    pick_unique(entries, &winners)
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
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for &idx in candidates {
        if seen.insert(&entries[idx].symbol_id) {
            unique.push(idx);
        }
    }
    if unique.len() == 1 {
        Some(unique[0])
    } else {
        None
    }
}

/// Deduplicate indices by symbol_id.
pub(in crate::resolver) fn dedup_by_id(entries: &[CatalogEntry], indices: &[usize]) -> Vec<usize> {
    let mut seen = HashSet::new();
    indices
        .iter()
        .copied()
        .filter(|&i| seen.insert(entries[i].symbol_id.clone()))
        .collect()
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
/// - Empty strings
/// - Single-char type params (T, K, V, etc.)
pub(in crate::resolver) fn type_atoms(raw: &str) -> Vec<String> {
    // Remove pointer/reference prefixes
    let s = raw.trim_start_matches(['*', '&']);

    // Split on delimiters: < > [ ] , | ( ) and whitespace
    let mut atoms = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            '<' | '>' | '[' | ']' | ',' | '|' | '(' | ')' | ' ' => {
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
            !a.is_empty()
        })
        .collect()
}
