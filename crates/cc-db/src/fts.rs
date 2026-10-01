//! FTS5 query building helpers.

use std::sync::LazyLock;

static FTS_TOKEN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[A-Za-z0-9_]+|[\x{4e00}-\x{9fff}]{1,8}").unwrap());

/// Sanitize a user query into FTS5 match syntax.
/// Extracts alphanumeric tokens and joins with OR (max 12 tokens).
pub fn sanitize_fts_query(query: &str) -> String {
    let tokens: Vec<&str> = FTS_TOKEN_RE
        .find_iter(query)
        .map(|m| m.as_str())
        .take(12)
        .collect();
    if tokens.is_empty() {
        return r#""""#.to_string();
    }
    tokens.join(" OR ")
}

/// Expand a code query by splitting camelCase and snake_case tokens.
pub fn expand_query_text(query: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        tokens.push(word.to_string());
        // Split camelCase
        let camel_parts = split_camel_case(word);
        if camel_parts.len() > 1 {
            for part in &camel_parts {
                let lower = part.to_lowercase();
                if !tokens.iter().any(|t| t.eq_ignore_ascii_case(&lower)) {
                    tokens.push(lower);
                }
            }
        }
        // Split snake_case
        if word.contains('_') {
            for part in word.split('_') {
                if part.len() >= 2 {
                    let lower = part.to_lowercase();
                    if !tokens.iter().any(|t| t.eq_ignore_ascii_case(&lower)) {
                        tokens.push(lower);
                    }
                }
            }
        }
    }
    tokens.join(" ")
}

/// Split a camelCase or PascalCase identifier into parts.
fn split_camel_case(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..bytes.len() {
        if bytes[i].is_ascii_uppercase() && bytes[i - 1].is_ascii_lowercase() {
            parts.push(&s[start..i]);
            start = i;
        }
    }
    parts.push(&s[start..]);
    parts
}

/// A bounded FTS MATCH compilation. Literal source terms retain priority over
/// optional identifier expansions; omitted groups never become partial groups.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CompiledFtsQuery {
    pub query: String,
    pub source_tokens: usize,
    pub atom_count: usize,
    pub source_tokens_omitted: bool,
    pub identifier_groups_omitted: usize,
}

/// Compile original terms as OR groups. A compound identifier matches either
/// its entire literal or ALL distinct identifier components, not an arbitrary
/// generic component. This is lexical support, not exact-symbol authority.
///
/// FTS unicode61 can tokenize a quoted snake identifier as a phrase; the whole
/// branch preserves that behavior while the component branch permits reordering.
pub fn compile_expanded_fts_query(query: &str) -> CompiledFtsQuery {
    const MAX_ATOMS: usize = 12;
    let mut found = FTS_TOKEN_RE.find_iter(query);
    let originals: Vec<&str> = found.by_ref().take(MAX_ATOMS).map(|m| m.as_str()).collect();
    let source_tokens_omitted = found.next().is_some();
    let mut atom_count = originals.len();
    let mut identifier_groups_omitted = 0;
    let mut groups = Vec::new();
    for token in &originals {
        let whole = fts_literal(token);
        let parts = identifier_components(token);
        if parts.len() < 2 {
            groups.push(whole);
        } else if atom_count + parts.len() <= MAX_ATOMS {
            atom_count += parts.len();
            let all = parts
                .iter()
                .map(|p| fts_literal(p))
                .collect::<Vec<_>>()
                .join(" AND ");
            groups.push(format!("({whole} OR ({all}))"));
        } else {
            identifier_groups_omitted += 1;
            groups.push(whole);
        }
    }
    CompiledFtsQuery {
        query: if groups.is_empty() {
            r#""""#.into()
        } else {
            groups.join(" OR ")
        },
        source_tokens: originals.len(),
        atom_count,
        source_tokens_omitted,
        identifier_groups_omitted,
    }
}
fn fts_literal(token: &str) -> String {
    format!("\"{}\"", token.replace('"', "\"\""))
}
fn identifier_components(token: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for segment in token.split('_').filter(|s| !s.is_empty()) {
        let bytes = segment.as_bytes();
        let mut start = 0;
        for i in 1..bytes.len() {
            let prev = bytes[i - 1];
            let curr = bytes[i];
            let boundary = curr.is_ascii_uppercase()
                && (prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase()
                        && bytes.get(i + 1).is_some_and(u8::is_ascii_lowercase)));
            if boundary {
                let lower = segment[start..i].to_lowercase();
                if !parts.contains(&lower) {
                    parts.push(lower);
                }
                start = i;
            }
        }
        let lower = segment[start..].to_lowercase();
        if !parts.contains(&lower) {
            parts.push(lower);
        }
    }
    parts
}

/// Tokenize text in a code-aware way (for overlap scoring).
pub fn tokenize_codeish(text: &str) -> Vec<String> {
    FTS_TOKEN_RE
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_sanitize_basic() {
        assert_eq!(sanitize_fts_query("hello world"), "hello OR world");
        assert_eq!(sanitize_fts_query(""), r#""""#);
        assert_eq!(sanitize_fts_query("foo_bar"), "foo_bar");
    }

    #[test]
    fn camel_case_split() {
        assert_eq!(split_camel_case("handleRequest"), vec!["handle", "Request"]);
        assert_eq!(split_camel_case("foo"), vec!["foo"]);
    }

    #[test]
    fn expand_query() {
        let expanded = expand_query_text("handleRequest");
        assert!(expanded.contains("handle"));
        assert!(expanded.contains("request"));
    }

    #[test]
    fn expand_query_snake_case() {
        let expanded = expand_query_text("get_user_name");
        assert!(expanded.contains("get"));
        assert!(expanded.contains("user"));
        assert!(expanded.contains("name"));
    }
}
