//! Route path normalization utilities.
//!
//! Normalizes URL/path patterns into a canonical form for cross-service
//! endpoint matching. Extracted to cc-model so both cc-index and cc-search
//! can use it without circular dependencies.

/// Normalize a URL or route path to a canonical form for matching.
///
/// Transformations:
/// 1. Strip scheme + host (e.g. `https://host/api` -> `/api`)
/// 2. Lowercase
/// 3. Strip query string and fragment
/// 4. Strip trailing slash (unless root `/`)
/// 5. Replace dynamic parameters with `*`:
///    - `:param` -> `*`
///    - `{param}` -> `*`
///    - `[param]` -> `*` (Next.js style)
///    - `<param>` -> `*` (Flask/FastAPI style)
///
/// # Examples
/// ```
/// use cc_model::route_normalize::normalize_route_path;
/// assert_eq!(normalize_route_path("/api/users/:id"), "/api/users/*");
/// assert_eq!(normalize_route_path("https://svc.local/v1/items/{itemId}?q=1"), "/v1/items/*");
/// ```
pub fn normalize_route_path(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return String::new();
    }

    // 1. Strip scheme + host
    let path_start = if let Some(idx) = input.find("://") {
        let after_scheme = &input[idx + 3..];
        after_scheme
            .find('/')
            .map(|i| &after_scheme[i..])
            .unwrap_or("/")
    } else {
        input
    };

    // 2. Strip query string and fragment
    let path = path_start
        .split('?')
        .next()
        .unwrap_or(path_start)
        .split('#')
        .next()
        .unwrap_or(path_start);

    // 3. Lowercase
    let lower = path.to_lowercase();

    // 4. Strip trailing slash (unless root)
    let trimmed = if lower.len() > 1 && lower.ends_with('/') {
        &lower[..lower.len() - 1]
    } else {
        &lower
    };

    // 5. Split into segments, replace dynamic parameters, rejoin
    let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return "/".to_string();
    }

    let mut result = String::with_capacity(trimmed.len());
    for seg in &segments {
        result.push('/');
        if is_param_segment(seg) {
            result.push('*');
        } else {
            result.push_str(seg);
        }
    }
    result
}

/// Check if a path segment is a dynamic parameter placeholder.
pub fn is_param_segment(seg: &str) -> bool {
    seg.starts_with(':')
        || (seg.starts_with('{') && seg.ends_with('}'))
        || (seg.starts_with('[') && seg.ends_with(']'))
        || (seg.starts_with('<') && seg.ends_with('>'))
        || seg == "*"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_basic_paths() {
        assert_eq!(normalize_route_path("/api/users"), "/api/users");
        assert_eq!(normalize_route_path("/api/users/"), "/api/users");
        assert_eq!(normalize_route_path("/"), "/");
        assert_eq!(normalize_route_path(""), "");
    }

    #[test]
    fn normalize_strips_scheme_and_host() {
        assert_eq!(
            normalize_route_path("https://svc.local/api/users"),
            "/api/users"
        );
        assert_eq!(
            normalize_route_path("http://localhost:3000/v1/items"),
            "/v1/items"
        );
    }

    #[test]
    fn normalize_strips_query_and_fragment() {
        assert_eq!(
            normalize_route_path("/api/users?page=1&size=10"),
            "/api/users"
        );
        assert_eq!(normalize_route_path("/api/users#section"), "/api/users");
    }

    #[test]
    fn normalize_replaces_params() {
        assert_eq!(normalize_route_path("/api/users/:id"), "/api/users/*");
        assert_eq!(normalize_route_path("/api/users/{userId}"), "/api/users/*");
        assert_eq!(normalize_route_path("/api/users/[id]"), "/api/users/*");
        assert_eq!(normalize_route_path("/api/<version>/items"), "/api/*/items");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_route_path("/API/Users"), "/api/users");
    }

    #[test]
    fn normalize_full_url_with_params() {
        assert_eq!(
            normalize_route_path("https://svc-b.internal/v2/orders/{orderId}?expand=items"),
            "/v2/orders/*"
        );
    }
}
