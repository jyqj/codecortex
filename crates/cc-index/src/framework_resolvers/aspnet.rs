//! ASP.NET framework resolver.
//!
//! - `enrich_file`: extracts route definitions from ASP.NET attribute routing,
//!   Minimal API endpoints, and controller conventions
//! - `resolve_cross_file`: TODO

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

/// Attribute routing: [HttpGet("path")] or [HttpGet]
///
/// Matches:
///   [HttpGet("users")]
///   [HttpPost("api/items")]
///   [HttpDelete]
///
/// Captures: (1) HTTP method name (e.g. "HttpGet"), (2) optional route path
static HTTP_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\[(Http(?:Get|Post|Put|Delete|Patch|Head|Options))(?:\("([^"]*)"\))?\]"#)
        .expect("aspnet http attr re")
});

/// Class-level route prefix: [Route("api/[controller]")]
///
/// Captures: (1) route template
static ROUTE_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\[Route\("([^"]+)"\)\]"#).expect("aspnet route attr re"));

/// Minimal API: app.MapGet("/path", handler) / app.MapPost("/path", handler)
///
/// Matches:
///   app.MapGet("/users", GetUsers)
///   app.MapPost("/items", async (Item item) => { ... })
///
/// Captures: (1) HTTP method, (2) route path, (3) handler identifier (optional)
static MINIMAL_API_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)app\.Map(Get|Post|Put|Delete|Patch)\("([^"]+)"\s*,\s*(\w+)"#)
        .expect("aspnet minimal api re")
});

/// Controller class name: class FooController
///
/// Captures: (1) controller name without "Controller" suffix
static CONTROLLER_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)class\s+(\w+)Controller\b"#).expect("aspnet controller class re")
});

/// Strip the "Http" prefix from method attribute name.
fn attr_to_method(attr: &str) -> String {
    attr.strip_prefix("Http").unwrap_or(attr).to_uppercase()
}

pub struct AspNetResolver;

impl AspNetResolver {
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }
}

impl FrameworkResolver for AspNetResolver {
    fn framework_key(&self) -> &str {
        "aspnet"
    }

    fn languages(&self) -> &[Language] {
        &[Language::CSharp]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        if !file_path.ends_with(".cs") {
            return;
        }

        // --- Detect class-level route prefix ---
        // Find the first [Route("...")] that appears before or near a Controller class
        let class_route_prefix: Option<String> =
            ROUTE_ATTR_RE.captures_iter(source).next().map(|cap| {
                let template = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                // Replace [controller] placeholder with actual controller name if available
                if let Some(ctrl_cap) = CONTROLLER_CLASS_RE.captures(source) {
                    let ctrl_name = ctrl_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    template
                        .replace("[controller]", &ctrl_name.to_lowercase())
                        .replace("[Controller]", &ctrl_name.to_lowercase())
                } else {
                    template.to_string()
                }
            });

        // --- Attribute routing: [HttpGet("path")] ---
        for cap in HTTP_ATTR_RE.captures_iter(source) {
            let attr_name = cap.get(1).map(|m| m.as_str()).unwrap_or("HttpGet");
            let method = attr_to_method(attr_name);
            let action_path = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // Build full route path
            let route_path = match &class_route_prefix {
                Some(prefix) if !action_path.is_empty() => {
                    let p = prefix.trim_end_matches('/');
                    let a = action_path.trim_start_matches('/');
                    format!("{}/{}", p, a)
                }
                Some(prefix) => prefix.clone(),
                None if !action_path.is_empty() => action_path.to_string(),
                None => "/".to_string(),
            };

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name: None,
                method: Some(method),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("aspnet".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Minimal API: app.MapGet("/path", handler) ---
        for cap in MINIMAL_API_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("Get");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let handler_name = cap.get(3).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: Some(method_name.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("aspnet".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Resolve handler_symbol_uid for ASP.NET route_edges.
        //
        // For attribute-based controller routes, handler_name is typically None
        // (the route is attached to the method via the [Http*] attribute), so
        // UID resolution mainly benefits Minimal API routes where handler_name
        // is explicitly captured (e.g. app.MapGet("/users", GetAllUsers)).
        //
        // Cross-file mounting is uncommon in ASP.NET — the framework
        // auto-discovers controllers via conventions, so prefix resolution is
        // handled entirely in enrich_file via [Route] attributes.

        for (file_path, outcome) in outcomes.iter_mut() {
            let fp = file_path.clone();
            for edge in &mut outcome.route_edges {
                if edge.handler_symbol_uid.is_some() {
                    continue;
                }
                if let Some(ref handler_name) = edge.handler_name {
                    if let Some((uid, _)) = catalog.lookup_symbol(handler_name, &fp) {
                        if !uid.is_empty() {
                            edge.handler_symbol_uid = Some(uid);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_aspnet(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        AspNetResolver.enrich_file(file_path, source, Language::CSharp, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_aspnet_attribute_routing() {
        let source = r#"
[Route("api/[controller]")]
public class UsersController : ControllerBase
{
    [HttpGet("list")]
    public IActionResult GetAll() { }

    [HttpPost]
    public IActionResult Create() { }

    [HttpDelete("{id}")]
    public IActionResult Delete(int id) { }
}
"#;
        let routes = run_aspnet("Controllers/UsersController.cs", source);
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        // [HttpGet("list")] with class prefix api/users
        assert!(routes
            .iter()
            .any(|r| r.route_path == "api/users/list" && r.method == Some("GET".into())));
        // [HttpPost] with class prefix api/users (no action path)
        assert!(routes
            .iter()
            .any(|r| r.route_path == "api/users" && r.method == Some("POST".into())));
        // [HttpDelete("{id}")] with class prefix
        assert!(routes
            .iter()
            .any(|r| r.route_path == "api/users/{id}" && r.method == Some("DELETE".into())));

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("aspnet")));
    }

    #[test]
    fn test_aspnet_minimal_api() {
        let source = r#"
var app = builder.Build();

app.MapGet("/users", GetAllUsers);
app.MapPost("/users", CreateUser);
app.MapDelete("/users/{id}", DeleteUser);
"#;
        let routes = run_aspnet("Program.cs", source);
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("GetAllUsers")));
        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("CreateUser")));
        assert!(routes.iter().any(|r| r.route_path == "/users/{id}"
            && r.method == Some("DELETE".into())
            && r.handler_name.as_deref() == Some("DeleteUser")));
    }

    #[test]
    fn test_aspnet_ignores_non_cs_files() {
        let source = r#"[HttpGet("test")]"#;
        let routes = run_aspnet("test.py", source);
        assert!(routes.is_empty(), "should ignore non-C# files");
    }
}
