//! Spring Framework resolver.
//!
//! Enrichment:
//! - `enrich_file`: extract @RequestMapping / @GetMapping / @PostMapping annotations → route_edges
//! - `resolve_cross_file`: no-op for v1

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::{FrameworkResolver, ProjectFrameworkContext};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Class-level @RequestMapping("/prefix") or @RequestMapping(value = "/prefix")
static CLASS_REQUEST_MAPPING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)@RequestMapping\s*\(\s*(?:value\s*=\s*)?"([^"]+)"\s*\)"#)
        .expect("class request mapping re")
});

/// Method-level HTTP verb annotations: @GetMapping("/path"), @PostMapping("/path"), etc.
/// Also matches bare @GetMapping without a path argument.
static METHOD_MAPPING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)@(Get|Post|Put|Delete|Patch)Mapping(?:\s*\(\s*(?:value\s*=\s*)?"([^"]*)"[^)]*\))?"#,
    )
    .expect("method mapping re")
});

/// Method-level @RequestMapping with method attribute:
/// @RequestMapping(value = "/path", method = RequestMethod.GET)
static METHOD_REQUEST_MAPPING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)@RequestMapping\s*\([^)]*value\s*=\s*"([^"]+)"[^)]*method\s*=\s*RequestMethod\.(\w+)[^)]*\)"#,
    )
    .expect("method request mapping re")
});

/// Extracts a method name from a Java method declaration line.
/// Matches: `public List<User> getUsers(` or `void create(`
static JAVA_METHOD_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)(?:public|protected|private|static|final|abstract|synchronized|default|\s)+(?:<[^>]+>\s+)?(?:[A-Za-z_][A-Za-z0-9_<>,\[\]\s?]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    )
    .expect("java method name re")
});

pub struct SpringResolver;

impl SpringResolver {
    /// Find the class-level @RequestMapping prefix, if any.
    fn find_class_prefix(source: &str) -> String {
        // Only take the first match (assuming one controller per file)
        CLASS_REQUEST_MAPPING_RE
            .captures(source)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    }

    /// Find the next Java method name declaration after a given byte offset.
    fn find_next_method_name(source: &str, after_offset: usize) -> Option<String> {
        let remaining = &source[after_offset..];
        JAVA_METHOD_NAME_RE
            .captures(remaining)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }
}

impl FrameworkResolver for SpringResolver {
    fn framework_key(&self) -> &str {
        "spring"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Java]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process .java files
        if !file_path.ends_with(".java") {
            return;
        }

        let class_prefix = Self::find_class_prefix(source);

        // Extract @GetMapping/@PostMapping/etc. annotations
        for cap in METHOD_MAPPING_RE.captures_iter(source) {
            let http_verb = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
            let method_path = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let annotation_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, annotation_offset);

            let handler_name = Self::find_next_method_name(source, annotation_offset);

            let route_path = if method_path.is_empty() {
                if class_prefix.is_empty() {
                    "/".to_string()
                } else {
                    class_prefix.clone()
                }
            } else {
                format!("{}{}", class_prefix, method_path)
            };

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: Some(http_verb.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("spring".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.88,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // Extract @RequestMapping with explicit method attribute
        for cap in METHOD_REQUEST_MAPPING_RE.captures_iter(source) {
            let method_path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let http_verb = cap.get(2).map(|m| m.as_str()).unwrap_or("GET");

            let annotation_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, annotation_offset);

            let handler_name = Self::find_next_method_name(source, annotation_offset);

            let route_path = format!("{}{}", class_prefix, method_path);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: Some(http_verb.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("spring".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }
    }

    fn resolve_cross_file(
        &self,
        _catalog: &crate::resolver::SymbolCatalog,
        _outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // No-op for v1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_spring(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        SpringResolver.enrich_file(file_path, source, Language::Java, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_spring_get_and_post_mapping() {
        let source = r#"
@RestController
@RequestMapping("/api/v1")
public class UserController {

    @GetMapping("/users")
    public List<User> getUsers() {
        return userService.findAll();
    }

    @PostMapping("/users")
    public User createUser(@RequestBody User user) {
        return userService.save(user);
    }
}
"#;
        let routes = run_spring("UserController.java", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        let get_route = routes.iter().find(|r| r.method == Some("GET".into()));
        assert!(get_route.is_some(), "should have a GET route");
        let get_route = get_route.unwrap();
        assert_eq!(get_route.route_path, "/api/v1/users");
        assert_eq!(get_route.handler_name.as_deref(), Some("getUsers"));
        assert_eq!(get_route.framework.as_deref(), Some("spring"));

        let post_route = routes.iter().find(|r| r.method == Some("POST".into()));
        assert!(post_route.is_some(), "should have a POST route");
        assert_eq!(post_route.unwrap().route_path, "/api/v1/users");
        assert_eq!(
            post_route.unwrap().handler_name.as_deref(),
            Some("createUser")
        );
    }

    #[test]
    fn test_spring_no_class_prefix() {
        let source = r#"
@RestController
public class SimpleController {

    @GetMapping("/health")
    public String health() {
        return "ok";
    }

    @DeleteMapping("/cache")
    public void clearCache() {}
}
"#;
        let routes = run_spring("SimpleController.java", source);
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/health" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/cache" && r.method == Some("DELETE".into())));
    }

    #[test]
    fn test_spring_bare_mapping() {
        let source = r#"
@RestController
@RequestMapping("/api")
public class RootController {

    @GetMapping
    public String index() { return "root"; }
}
"#;
        let routes = run_spring("RootController.java", source);
        assert_eq!(routes.len(), 1);
        // Bare @GetMapping with class prefix → just the prefix
        assert_eq!(routes[0].route_path, "/api");
        assert_eq!(routes[0].method, Some("GET".into()));
    }

    #[test]
    fn test_spring_request_mapping_with_method() {
        let source = r#"
@Controller
@RequestMapping("/legacy")
public class LegacyController {

    @RequestMapping(value = "/action", method = RequestMethod.POST)
    public String doAction() { return "done"; }
}
"#;
        let routes = run_spring("LegacyController.java", source);
        assert!(
            routes.iter().any(|r| r.route_path == "/legacy/action"
                && r.method == Some("POST".into())
                && r.handler_name.as_deref() == Some("doAction")),
            "should extract @RequestMapping with method attr, got: {:?}",
            routes
        );
    }

    #[test]
    fn test_spring_ignores_non_java() {
        let source = r#"@GetMapping("/foo") public void foo() {}"#;
        let routes = run_spring("Test.kt", source);
        assert!(routes.is_empty(), "should ignore non-.java files");
    }

    #[test]
    fn test_spring_all_http_verbs() {
        let source = r#"
@RestController
public class VerbController {
    @GetMapping("/a") public void a() {}
    @PostMapping("/b") public void b() {}
    @PutMapping("/c") public void c() {}
    @DeleteMapping("/d") public void d() {}
    @PatchMapping("/e") public void e() {}
}
"#;
        let routes = run_spring("VerbController.java", source);
        assert_eq!(routes.len(), 5, "should extract all 5 HTTP verb mappings");
        let methods: Vec<String> = routes.iter().filter_map(|r| r.method.clone()).collect();
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
        assert!(methods.contains(&"PUT".to_string()));
        assert!(methods.contains(&"DELETE".to_string()));
        assert!(methods.contains(&"PATCH".to_string()));
    }
}
