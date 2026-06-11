//! Shared Python regex builders for decorator-based framework resolvers.

use regex::Regex;

/// Build a `@receiver.method("/path")` decorator matcher for the given HTTP
/// method alternation (e.g. `"get|post|put|delete|patch"`).
///
/// Captures: (1) HTTP method, (2) route path.
pub(crate) fn http_method_decorator_re(methods: &str) -> Regex {
    Regex::new(&format!(r#"@\w+\.({})\(\s*["']([^"']+)["']"#, methods))
        .expect("python http method decorator re")
}
