//! Route path normalization and matching utilities.
//!
//! Normalizes URL/path patterns into a canonical form for cross-service
//! endpoint matching. Inspired by codebase-memory-mcp's httplink.c.

// Re-export normalization functions from cc-model (the canonical implementation).
pub use cc_model::route_normalize::normalize_route_path;

/// Compute a similarity score between two normalized route paths.
///
/// Returns a score in `[0.0, 1.0]`:
/// - 1.0 = exact match
/// - 0.75+ = suffix match (one is a suffix of the other)
/// - 0.0–0.75 = segment-level Jaccard similarity
pub fn route_match_score(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }

    let seg_a: Vec<&str> = a.split('/').filter(|s| !s.is_empty()).collect();
    let seg_b: Vec<&str> = b.split('/').filter(|s| !s.is_empty()).collect();

    if seg_a.is_empty() || seg_b.is_empty() {
        return 0.0;
    }

    // Suffix match: one path ends with the other
    let shorter = seg_a.len().min(seg_b.len());
    let suffix_match = seg_a[seg_a.len() - shorter..]
        .iter()
        .zip(seg_b[seg_b.len() - shorter..].iter())
        .all(|(a, b)| segments_match(a, b));
    if suffix_match && shorter > 0 {
        let coverage = shorter as f64 / seg_a.len().max(seg_b.len()) as f64;
        return 0.5 + 0.5 * coverage; // [0.5, 1.0]
    }

    // Segment-level matching (order-aware Jaccard-like)
    let mut matches = 0usize;
    let min_len = seg_a.len().min(seg_b.len());
    for i in 0..min_len {
        if segments_match(seg_a[i], seg_b[i]) {
            matches += 1;
        }
    }
    let union = seg_a.len().max(seg_b.len());
    if union == 0 {
        return 0.0;
    }
    0.5 * (matches as f64 / union as f64)
}

/// Check if two path segments match (considering wildcards).
fn segments_match(a: &str, b: &str) -> bool {
    a == b || a == "*" || b == "*"
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

    #[test]
    fn match_score_exact() {
        assert_eq!(route_match_score("/api/users", "/api/users"), 1.0);
    }

    #[test]
    fn match_score_wildcard() {
        let score = route_match_score("/api/users/*", "/api/users/123");
        // Not exact match, but suffix match with wildcards
        assert!(score > 0.0);
    }

    #[test]
    fn match_score_different() {
        let score = route_match_score("/api/users", "/api/orders");
        assert!(score < 0.5);
    }

    #[test]
    fn match_score_suffix() {
        let score = route_match_score("/api/users", "/v2/api/users");
        assert!(score >= 0.5); // suffix match
    }
}
