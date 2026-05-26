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

/// Tokenize text in a code-aware way (for overlap scoring).
pub fn tokenize_codeish(text: &str) -> Vec<String> {
    FTS_TOKEN_RE
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

/// Replace non-alphanumeric characters with `_`, lowercase, and collapse runs of `_`.
pub fn slugify_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else {
            // Collapse consecutive underscores
            if !result.ends_with('_') {
                result.push('_');
            }
        }
    }
    // Trim leading/trailing underscores
    result.trim_matches('_').to_string()
}

/// Blake3 hash, returning the first 8 bytes.
pub fn blake_hash(data: &[u8]) -> [u8; 8] {
    let hash = blake3::hash(data);
    hash.as_bytes()[..8].try_into().unwrap()
}

/// Read an environment variable and return true for "1", "true", or "yes" (case-insensitive).
pub fn env_bool(key: &str) -> bool {
    match std::env::var(key) {
        Ok(val) => matches!(val.to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

/// Normalize a function signature: collapse whitespace variations, trim.
pub fn normalize_signature(sig: &str) -> String {
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
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
    fn slugify_path_basic() {
        assert_eq!(slugify_path("src/main.rs"), "src_main_rs");
        assert_eq!(slugify_path("/foo/bar/baz.py"), "foo_bar_baz_py");
        assert_eq!(slugify_path("hello---world"), "hello_world");
    }

    #[test]
    fn expand_query_snake_case() {
        let expanded = expand_query_text("get_user_name");
        assert!(expanded.contains("get"));
        assert!(expanded.contains("user"));
        assert!(expanded.contains("name"));
    }

    #[test]
    fn env_bool_values() {
        std::env::set_var("_TEST_ENV_BOOL_TRUE", "true");
        std::env::set_var("_TEST_ENV_BOOL_ONE", "1");
        std::env::set_var("_TEST_ENV_BOOL_YES", "YES");
        std::env::set_var("_TEST_ENV_BOOL_NO", "no");
        std::env::remove_var("_TEST_ENV_BOOL_MISSING");

        assert!(env_bool("_TEST_ENV_BOOL_TRUE"));
        assert!(env_bool("_TEST_ENV_BOOL_ONE"));
        assert!(env_bool("_TEST_ENV_BOOL_YES"));
        assert!(!env_bool("_TEST_ENV_BOOL_NO"));
        assert!(!env_bool("_TEST_ENV_BOOL_MISSING"));
    }
}
