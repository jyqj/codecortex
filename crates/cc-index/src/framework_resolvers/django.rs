//! Django framework resolver.
//!
//! - `enrich_file`: extracts route definitions from Django URL patterns
//! - `resolve_cross_file`: resolves include() mounting across files

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::mount_resolution::{resolve_mounts, MountPoint, MountSpec, PrefixJoin, TargetLookup};
use super::{line_for_offset, FrameworkResolver, ProjectFrameworkContext};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Django path() and re_path() calls:
///   path("api/users/", views.user_list, name="user-list")
///   path("api/users/<int:pk>/", views.user_detail)
///   re_path(r"^api/(?P<pk>\d+)/$", views.detail)
///
/// Captures: (1) route pattern, (2) handler reference (e.g. views.user_list)
static URL_PATTERN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:re_)?path\(\s*(?:r)?["']([^"']+)["']\s*,\s*([\w.]+)"#)
        .expect("django url pattern re")
});

/// Django include() mounting:
///   path("api/", include("app.urls"))
///   path("api/", include(app_urls))
///
/// Captures: (1) prefix path, (2) included module or variable (quoted string or bare identifier)
static INCLUDE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:re_)?path\(\s*(?:r)?["']([^"']*)["']\s*,\s*include\(\s*(?:["']([\w.]+)["']|(\w+))\s*\)"#)
        .expect("django include re")
});

pub struct DjangoResolver;

impl DjangoResolver {}

impl FrameworkResolver for DjangoResolver {
    fn framework_key(&self) -> &str {
        "django"
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

        for cap in URL_PATTERN_RE.captures_iter(source) {
            let route_pattern = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let handler_ref = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // Skip entries that are actually include() calls (handled separately)
            if handler_ref == "include" {
                continue;
            }

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            // Normalize the route path: ensure leading /
            let route_path = if route_pattern.starts_with('/') || route_pattern.starts_with('^') {
                route_pattern.to_string()
            } else {
                format!("/{}", route_pattern)
            };

            // Extract handler name: last segment of dotted path (e.g. "views.user_list" -> "user_list")
            let handler_name = handler_ref.rsplit('.').next().map(|s| s.to_string());

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: None, // Django URL patterns don't specify HTTP methods
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: Some(handler_ref.to_string()),
                router_symbol_uid: None,
                framework: Some("django".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- include() mounting: path("api/", include("app.urls")) ---
        for cap in INCLUDE_RE.captures_iter(source) {
            let prefix_pattern = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            // Group 2 = quoted module string, Group 3 = bare variable name
            let included_ref = cap
                .get(2)
                .or_else(|| cap.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");

            if included_ref.is_empty() {
                continue;
            }

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            // Normalize the prefix path: ensure leading /
            let route_path = if prefix_pattern.is_empty() {
                "/".to_string()
            } else if prefix_pattern.starts_with('/') || prefix_pattern.starts_with('^') {
                prefix_pattern.to_string()
            } else {
                format!("/{}", prefix_pattern)
            };

            // For dotted module paths like "app.urls", use the last component as handler_name
            // For bare variables like app_urls, use as-is
            let handler_name = if included_ref.contains('.') {
                // Convert "app.urls" -> last segment "urls" for symbol lookup
                included_ref.rsplit('.').next().map(|s| s.to_string())
            } else {
                Some(included_ref.to_string())
            };

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: None,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: Some(included_ref.to_string()),
                router_symbol_uid: None,
                framework: Some("django".to_string()),
                route_kind: Some("router_mount".to_string()),
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
        // Resolve include() mounting across files.
        //
        // Pattern: path("api/", include("app.urls"))
        //   → find the file where `app.urls` urlpatterns are defined
        //   → prepend "/api/" to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for route_edges whose handler_name
        // is set but handler_symbol_uid is not.

        // Fallback for dotted module paths like "app.urls": try "urlpatterns"
        // as a common Django convention, then check the file path matches the
        // module hint (e.g. "app.urls" -> file should contain "app/urls").
        let module_fallback = |catalog: &crate::resolver::SymbolCatalog,
                               _outcomes: &[(String, ParseOutcome)],
                               mount: &MountPoint|
         -> Option<String> {
            if !mount.handler_expr.contains('.') {
                return None;
            }
            catalog
                .lookup_symbol("urlpatterns", &mount.mount_file)
                .and_then(|(_, file)| {
                    let module_path = mount.handler_expr.replace('.', "/");
                    if file.contains(&module_path) {
                        Some(file)
                    } else {
                        None
                    }
                })
        };

        resolve_mounts(
            catalog,
            outcomes,
            &MountSpec {
                mount_kinds: &["router_mount"],
                skip_root_prefix: true,
                framework: None,
                // Django: "/api/" + "/users/" -> "/api/users/"
                join: PrefixJoin::StripSubLeadingSlash,
                lookup: TargetLookup::DefaultWithFallback(&module_fallback),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_django(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        DjangoResolver.enrich_file(file_path, source, Language::Python, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_django_path_basic() {
        let source = r#"
from django.urls import path
from . import views

urlpatterns = [
    path("api/users/", views.user_list, name="user-list"),
    path("api/users/<int:pk>/", views.user_detail, name="user-detail"),
]
"#;
        let routes = run_django("myapp/urls.py", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        assert!(routes.iter().any(
            |r| r.route_path == "/api/users/" && r.handler_name.as_deref() == Some("user_list")
        ));
        assert!(routes.iter().any(|r| r.route_path == "/api/users/<int:pk>/"
            && r.handler_name.as_deref() == Some("user_detail")));

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("django")));
        // Django doesn't specify method at URL level
        assert!(routes.iter().all(|r| r.method.is_none()));
    }

    #[test]
    fn test_django_re_path() {
        let source = r#"
from django.urls import re_path
from . import views

urlpatterns = [
    re_path(r"^api/(?P<pk>\d+)/$", views.detail),
]
"#;
        let routes = run_django("myapp/urls.py", source);
        assert_eq!(routes.len(), 1, "expected 1 route, got {}", routes.len());
        assert!(routes[0].route_path.contains("api/"));
        assert_eq!(routes[0].handler_name.as_deref(), Some("detail"));
        assert_eq!(routes[0].handler_expr.as_deref(), Some("views.detail"));
    }

    #[test]
    fn test_django_ignores_non_py_files() {
        let source = r#"path("test/", views.test)"#;
        let routes = run_django("test.js", source);
        assert!(routes.is_empty(), "should ignore non-.py files");
    }

    #[test]
    fn test_django_path_leading_slash() {
        let source = r#"
from django.urls import path
from . import views

urlpatterns = [
    path("/absolute/path/", views.absolute_handler),
    path("relative/path/", views.relative_handler),
]
"#;
        let routes = run_django("myapp/urls.py", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        // Path starting with / should stay as-is
        assert!(routes.iter().any(|r| r.route_path == "/absolute/path/"
            && r.handler_name.as_deref() == Some("absolute_handler")));
        // Path without leading / should get one prepended
        assert!(routes.iter().any(|r| r.route_path == "/relative/path/"
            && r.handler_name.as_deref() == Some("relative_handler")));
    }
}
