//! Shared helpers for detecting outbound HTTP client calls across languages.

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
