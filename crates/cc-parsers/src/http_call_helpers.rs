//! Shared helpers for detecting outbound HTTP client calls across languages.

use cc_model::{HttpCallEdgeRecord, ParserTier, StableId};
use regex::Regex;

/// A regex-driven outbound-HTTP-call extraction pattern.
///
/// Used by the regex-based extractors (Go/Java/Rust), where matching call nodes
/// in the AST would be far more code than a precise pattern plus the
/// [`looks_like_url_or_path`] guard.
pub struct HttpCallSpec<'a> {
    /// Pattern whose overall match identifies one outbound HTTP call.
    pub re: &'a Regex,
    /// Capture group holding the URL/path literal.
    pub url_group: usize,
    /// Capture group holding the HTTP method verb (upper-cased on use), if any.
    pub method_group: Option<usize>,
    /// Fixed method when `method_group` is `None` (e.g. `reqwest::get` -> GET).
    pub fixed_method: Option<&'static str>,
}

/// Extract outbound HTTP-call edges by scanning `content` with `specs`.
///
/// Each match's URL literal must pass [`looks_like_url_or_path`]; this is the
/// primary false-positive guard for languages whose verb method names
/// (`.get()`, `.post()`, `.put()`) collide with non-HTTP APIs such as map/cache
/// accessors — a non-URL argument like `"key"` is rejected.
pub fn extract_http_calls_with(
    content: &str,
    file_path: &str,
    specs: &[HttpCallSpec],
) -> Vec<HttpCallEdgeRecord> {
    let mut edges = Vec::new();
    for spec in specs {
        for cap in spec.re.captures_iter(content) {
            let Some(m) = cap.get(0) else { continue };
            let Some(url_match) = cap.get(spec.url_group) else {
                continue;
            };
            let raw_url = url_match.as_str().to_string();
            if !looks_like_url_or_path(&raw_url) {
                continue;
            }
            let method = spec
                .method_group
                .and_then(|g| cap.get(g))
                .map(|mm| mm.as_str().to_uppercase())
                .or_else(|| spec.fixed_method.map(|s| s.to_string()));
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            edges.push(HttpCallEdgeRecord {
                edge_id: StableId::edge_id("http_call", file_path, line, m.start() as u32),
                file_path: file_path.to_string(),
                caller_symbol_uid: None,
                url_or_path: raw_url.clone(),
                normalized_path: Some(cc_model::route_normalize::normalize_route_path(
                    &normalize_template_to_path(&raw_url),
                )),
                method,
                call_kind: "http".to_string(),
                line,
                confidence: 0.70,
                parser_tier: ParserTier::Heuristic,
                broker_type: None,
            });
        }
    }
    edges
}

/// Known HTTP client receiver objects / module names.
const HTTP_CLIENT_OBJECTS: &[&str] = &[
    "axios",
    "got",
    "ky",
    "superagent",
    "request",
    "requests",
    "httpx",
    "aiohttp",
    "urllib",
    "http",
    "fetch",
    "node-fetch",
    "$http",      // Angular
    "HttpClient", // Angular
    "reqwest",    // Rust
];

/// HTTP methods that can be inferred from callee names.
const INFERRABLE_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("delete", "DELETE"),
    ("patch", "PATCH"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
];

/// Check if a receiver/object name looks like an HTTP client.
pub fn is_http_client_object(name: &str) -> bool {
    let lower = name.to_lowercase();
    HTTP_CLIENT_OBJECTS
        .iter()
        .any(|&obj| lower == obj.to_lowercase())
}

/// Check if a standalone function name is a known HTTP call (e.g. `fetch`).
pub fn is_standalone_http_call(name: &str) -> bool {
    matches!(name, "fetch" | "$fetch" | "ofetch" | "useFetch")
}

/// Check if a method name on an HTTP client object is an HTTP verb method.
pub fn is_http_verb_method(method_name: &str) -> bool {
    let lower = method_name.to_lowercase();
    INFERRABLE_METHODS.iter().any(|&(m, _)| m == lower) || lower == "request" || lower == "send"
}

/// Infer the HTTP method from a callee method name.
/// Returns None if the method name doesn't map to a specific HTTP verb.
pub fn infer_http_method(method_name: &str) -> Option<&'static str> {
    let lower = method_name.to_lowercase();
    for &(name, method) in INFERRABLE_METHODS {
        if lower == name {
            return Some(method);
        }
    }
    None
}

/// Try to extract a URL-like string from a raw string literal value.
/// Returns the string if it looks like a URL or path, None otherwise.
///
/// Recognizes:
/// - Absolute URLs: `http://...`, `https://...`
/// - Path-like strings: `/api/...`, `/v1/...`
/// - Relative paths with API-like segments
pub fn looks_like_url_or_path(value: &str) -> bool {
    let trimmed = value.trim().trim_matches(|c| c == '\'' || c == '"');
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return false;
    }
    // Absolute URL
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return true;
    }
    // Path starting with /
    if trimmed.starts_with('/') && trimmed.len() > 1 {
        // Filter out regex-like patterns, file paths with extensions
        let suspicious = trimmed.contains(".*")
            || trimmed.contains("\\d")
            || trimmed.ends_with(".js")
            || trimmed.ends_with(".ts")
            || trimmed.ends_with(".py")
            || trimmed.ends_with(".html")
            || trimmed.ends_with(".css");
        return !suspicious;
    }
    false
}

/// Normalize a template string by replacing interpolation expressions with `*`.
///
/// Handles:
/// - JS/TS: `${expr}` -> `*`
/// - Python f-string: `{expr}` -> `*` (caller should strip the `f` prefix)
pub fn normalize_template_to_path(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            // JS/TS template literal interpolation
            chars.next(); // consume '{'
            let mut depth = 1;
            for c in chars.by_ref() {
                if c == '{' {
                    depth += 1;
                }
                if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            result.push('*');
        } else if ch == '{' {
            // Python f-string or generic `{param}`
            let mut depth = 1;
            let mut is_interpolation = false;
            let mut inner = String::new();
            for c in chars.by_ref() {
                if c == '{' {
                    depth += 1;
                }
                if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        is_interpolation = true;
                        break;
                    }
                }
                inner.push(c);
            }
            if is_interpolation {
                result.push('*');
            } else {
                result.push('{');
                result.push_str(&inner);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Strip string delimiters (single quotes, double quotes, backticks, f-string prefix).
pub fn strip_string_delimiters(raw: &str) -> &str {
    let s = raw.trim();
    // Python f-string: f"...", f'...'
    let s = s.strip_prefix('f').unwrap_or(s);
    let s = s.strip_prefix('b').unwrap_or(s);
    let s = s.strip_prefix('r').unwrap_or(s);
    // Strip delimiters
    s.strip_prefix("\"\"\"")
        .and_then(|s| s.strip_suffix("\"\"\""))
        .or_else(|| s.strip_prefix("'''").and_then(|s| s.strip_suffix("'''")))
        .or_else(|| s.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .or_else(|| s.strip_prefix('`').and_then(|s| s.strip_suffix('`')))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_http_client_object() {
        assert!(is_http_client_object("axios"));
        assert!(is_http_client_object("Axios"));
        assert!(is_http_client_object("requests"));
        assert!(is_http_client_object("httpx"));
        assert!(!is_http_client_object("console"));
        assert!(!is_http_client_object("Math"));
    }

    #[test]
    fn test_is_standalone_http_call() {
        assert!(is_standalone_http_call("fetch"));
        assert!(is_standalone_http_call("$fetch"));
        assert!(!is_standalone_http_call("get"));
        assert!(!is_standalone_http_call("axios"));
    }

    #[test]
    fn test_infer_http_method() {
        assert_eq!(infer_http_method("get"), Some("GET"));
        assert_eq!(infer_http_method("POST"), Some("POST"));
        assert_eq!(infer_http_method("Delete"), Some("DELETE"));
        assert_eq!(infer_http_method("request"), None);
        assert_eq!(infer_http_method("send"), None);
    }

    #[test]
    fn test_looks_like_url_or_path() {
        assert!(looks_like_url_or_path("https://api.example.com/users"));
        assert!(looks_like_url_or_path("http://localhost:3000/api"));
        assert!(looks_like_url_or_path("/api/users"));
        assert!(looks_like_url_or_path("/v1/orders/123"));
        assert!(!looks_like_url_or_path("/"));
        assert!(!looks_like_url_or_path(""));
        assert!(!looks_like_url_or_path("/path/to/file.js"));
        assert!(!looks_like_url_or_path("not a url"));
    }

    #[test]
    fn test_normalize_template_js() {
        assert_eq!(
            normalize_template_to_path("/api/users/${userId}/orders"),
            "/api/users/*/orders"
        );
        assert_eq!(
            normalize_template_to_path("/api/${version}/items/${id}"),
            "/api/*/items/*"
        );
    }

    #[test]
    fn test_normalize_template_python() {
        assert_eq!(
            normalize_template_to_path("/api/users/{user_id}/orders"),
            "/api/users/*/orders"
        );
    }

    #[test]
    fn test_strip_string_delimiters() {
        assert_eq!(strip_string_delimiters("\"hello\""), "hello");
        assert_eq!(strip_string_delimiters("'hello'"), "hello");
        assert_eq!(strip_string_delimiters("`hello`"), "hello");
        assert_eq!(strip_string_delimiters("f\"/api/{id}\""), "/api/{id}");
    }
}
