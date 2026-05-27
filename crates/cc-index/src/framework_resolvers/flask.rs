//! Flask framework resolver.
//!
//! - `enrich_file`: extracts route definitions from Flask decorators
//! - `resolve_cross_file`: resolves blueprint prefix mounting across files

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

/// @app.route("/path", methods=["GET", "POST"]) or @blueprint.route("/path")
///
/// Captures: (1) route path, (2) optional methods list content (e.g. "GET", "POST")
static ROUTE_DECORATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@\w+\.route\(\s*["']([^"']+)["'](?:\s*,\s*methods\s*=\s*\[([^\]]+)\])?"#)
        .expect("flask route decorator re")
});

/// Flask 2.0+ shorthand: @app.get("/path"), @app.post("/path"), etc.
///
/// Captures: (1) HTTP method, (2) route path
static METHOD_DECORATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@\w+\.(get|post|put|delete|patch)\(\s*["']([^"']+)["']"#)
        .expect("flask method decorator re")
});

/// Function definition: `def handler_name(`
///
/// Captures: (1) function name
static DEF_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:async\s+)?def\s+(\w+)\s*\("#).expect("flask def name re"));

/// Extract individual method strings from a methods list: "GET", 'POST'
static METHOD_STR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["'](\w+)["']"#).expect("flask method string re"));

/// app.register_blueprint(bp, url_prefix="/auth") or app.register_blueprint(bp)
///
/// Captures: (1) blueprint variable name, (2) optional url_prefix value
static REGISTER_BLUEPRINT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)\w+\.register_blueprint\(\s*(\w+)(?:\s*,\s*url_prefix\s*=\s*["']([^"']+)["'])?\s*\)"#,
    )
    .expect("flask register_blueprint re")
});

pub struct FlaskResolver;

impl FlaskResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Parse a methods=["GET", "POST"] string into a list of uppercase method names.
    fn parse_methods(methods_str: &str) -> Vec<String> {
        METHOD_STR_RE
            .captures_iter(methods_str)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_uppercase()))
            .collect()
    }
}

impl FrameworkResolver for FlaskResolver {
    fn framework_key(&self) -> &str {
        "flask"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Python]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process Python files
        if !file_path.ends_with(".py") {
            return;
        }

        // --- @app.route("/path", methods=[...]) ---
        for cap in ROUTE_DECORATOR_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let methods_str = cap.get(2).map(|m| m.as_str());

            let decorator_offset = cap.get(0).unwrap().start();
            let decorator_end = cap.get(0).unwrap().end();
            let line = Self::line_for_offset(source, decorator_offset);

            let handler_name = DEF_NAME_RE
                .captures(&source[decorator_end..])
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            let methods = match methods_str {
                Some(s) => {
                    let parsed = Self::parse_methods(s);
                    if parsed.is_empty() {
                        vec!["GET".to_string()]
                    } else {
                        parsed
                    }
                }
                None => vec!["GET".to_string()],
            };

            for method in &methods {
                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("route", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: route_path.to_string(),
                    handler_name: handler_name.clone(),
                    method: Some(method.clone()),
                    line,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: None,
                    router_symbol_uid: None,
                    framework: Some("flask".to_string()),
                    route_kind: Some("http".to_string()),
                    confidence: 0.85,
                    parser_tier: ParserTier::Heuristic,
                });
            }
        }

        // --- Flask 2.0+ shorthand: @app.get("/path"), @app.post("/path") ---
        for cap in METHOD_DECORATOR_RE.captures_iter(source) {
            let http_method = cap.get(1).map(|m| m.as_str()).unwrap_or("get");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let decorator_offset = cap.get(0).unwrap().start();
            let decorator_end = cap.get(0).unwrap().end();
            let line = Self::line_for_offset(source, decorator_offset);

            // Skip if this was already captured by ROUTE_DECORATOR_RE
            // (route decorator regex won't match @app.get style, so no overlap)

            let handler_name = DEF_NAME_RE
                .captures(&source[decorator_end..])
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: Some(http_method.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("flask".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- app.register_blueprint(bp, url_prefix="/auth") ---
        for cap in REGISTER_BLUEPRINT_RE.captures_iter(source) {
            let bp_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let url_prefix = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            if bp_name.is_empty() {
                continue;
            }

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: url_prefix.to_string(),
                handler_name: Some(bp_name.to_string()),
                method: None,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("flask".to_string()),
                route_kind: Some("blueprint_mount".to_string()),
                confidence: 0.80,
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
        // Resolve blueprint prefix mounting across files.
        //
        // Pattern: app.register_blueprint(auth_bp, url_prefix="/auth")
        //   → find the file where `auth_bp` is defined
        //   → prepend "/auth" to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for route_edges whose handler_name
        // is set but handler_symbol_uid is not.

        // Step 1: collect mount points (prefix → blueprint variable name → mounting file)
        struct MountInfo {
            prefix: String,
            bp_name: String,
            mount_file: String,
        }

        let mut mounts: Vec<MountInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                if edge.route_kind.as_deref() == Some("blueprint_mount") {
                    if let Some(ref handler) = edge.handler_name {
                        let prefix = &edge.route_path;
                        if prefix != "/" && !prefix.is_empty() {
                            mounts.push(MountInfo {
                                prefix: prefix.clone(),
                                bp_name: handler.clone(),
                                mount_file: file_path.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Step 2: for each mount, find the target file via catalog and prepend prefix
        for mount in &mounts {
            // Look up where the blueprint variable is defined
            let target_file = match catalog.lookup_symbol(&mount.bp_name, &mount.mount_file) {
                Some((_, file)) if file != mount.mount_file => file,
                _ => continue,
            };

            // Prepend the mount prefix to all http routes in the target file
            for (file_path, outcome) in outcomes.iter_mut() {
                if *file_path != target_file {
                    continue;
                }
                for edge in &mut outcome.route_edges {
                    if edge.route_kind.as_deref() != Some("http") {
                        continue;
                    }
                    // Only prepend once (avoid double-prefixing on repeated runs)
                    if !edge.route_path.starts_with(&mount.prefix) {
                        let combined = if edge.route_path == "/" {
                            mount.prefix.clone()
                        } else {
                            format!("{}{}", mount.prefix, edge.route_path)
                        };
                        edge.route_path = combined;
                    }
                }
            }
        }

        // Step 3: resolve handler_symbol_uid for http route_edges
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

    fn run_flask(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        FlaskResolver.enrich_file(file_path, source, Language::Python, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_flask_route_with_methods() {
        let source = r#"
from flask import Flask

app = Flask(__name__)

@app.route("/users", methods=["GET", "POST"])
def users():
    pass

@app.route("/health")
def health():
    return "ok"
"#;
        let routes = run_flask("src/app.py", source);
        // /users -> GET + POST (2 entries), /health -> GET (1 entry)
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/users" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/users" && r.method == Some("POST".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/health" && r.method == Some("GET".into())));

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("flask")));
    }

    #[test]
    fn test_flask_2_0_shorthand() {
        let source = r#"
from flask import Flask

app = Flask(__name__)

@app.get("/items")
def list_items():
    return []

@app.post("/items")
def create_item():
    pass

@app.delete("/items/<int:item_id>")
def delete_item(item_id):
    pass
"#;
        let routes = run_flask("src/app.py", source);
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items" && r.method == Some("POST".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items/<int:item_id>" && r.method == Some("DELETE".into())));
    }

    #[test]
    fn test_flask_blueprint_route() {
        let source = r#"
from flask import Blueprint

bp = Blueprint("auth", __name__, url_prefix="/auth")

@bp.route("/login", methods=["POST"])
def login():
    pass

@bp.route("/logout")
def logout():
    pass
"#;
        let routes = run_flask("src/auth.py", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/login" && r.method == Some("POST".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/logout" && r.method == Some("GET".into())));
    }

    #[test]
    fn test_flask_ignores_non_py_files() {
        let source = r#"@app.route("/test")
def handler():
    pass"#;
        let routes = run_flask("test.ts", source);
        assert!(routes.is_empty(), "should ignore non-.py files");
    }
}
