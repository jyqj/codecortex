//! Laravel framework resolver.
//!
//! - `enrich_file`: extracts route definitions from Laravel route files
//! - `resolve_cross_file`: resolves Route::prefix/group file-based grouping across files

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

/// New-style array syntax: Route::get("/path", [Controller::class, "method"])
///
/// Matches:
///   Route::get("/users", [UserController::class, "index"])
///   Route::post('/users', [UserController::class, 'store'])
///
/// Captures: (1) HTTP method, (2) route path, (3) controller name, (4) method name
static ROUTE_ARRAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)Route::(get|post|put|patch|delete|options)\(\s*["']([^"']+)["']\s*,\s*\[(\w+)::class\s*,\s*["'](\w+)["']\]"#,
    )
    .expect("laravel route array re")
});

/// Old-style string syntax: Route::get("/path", "Controller@method")
///
/// Matches:
///   Route::post("/users", "UserController@store")
///   Route::get('/users', 'UserController@index')
///
/// Captures: (1) HTTP method, (2) route path, (3) controller name, (4) method name
static ROUTE_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)Route::(get|post|put|patch|delete|options)\(\s*["']([^"']+)["']\s*,\s*["'](\w+)@(\w+)["']"#,
    )
    .expect("laravel route string re")
});

/// Resource route: Route::resource("/path", Controller::class)
///
/// Matches:
///   Route::resource("/users", UserController::class)
///   Route::resource('posts', PostController::class)
///
/// Captures: (1) resource path, (2) controller name
static ROUTE_RESOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)Route::resource\(\s*["']([^"']+)["']\s*,\s*(\w+)::class"#,
    )
    .expect("laravel route resource re")
});

/// Route::prefix("/api")->group(base_path("routes/api.php"))
/// or Route::prefix('/api')->group(base_path('routes/api.php'))
///
/// Captures: (1) prefix path, (2) file path inside base_path()
static ROUTE_PREFIX_GROUP_FILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)Route::prefix\(\s*["']([^"']+)["']\s*\)\s*->\s*group\(\s*base_path\(\s*["']([^"']+)["']\s*\)"#,
    )
    .expect("laravel route prefix group file re")
});

/// Route::group(['prefix' => '/api'], base_path("routes/api.php"))
/// or Route::group(["prefix" => "/api"], base_path('routes/api.php'))
///
/// Captures: (1) prefix path, (2) file path inside base_path()
static ROUTE_GROUP_ARRAY_FILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)Route::group\(\s*\[["']prefix["']\s*=>\s*["']([^"']+)["']\s*\]\s*,\s*base_path\(\s*["']([^"']+)["']\s*\)"#,
    )
    .expect("laravel route group array file re")
});

/// Route::prefix("/api")->group($variable) — variable-based grouping
///
/// Captures: (1) prefix path, (2) variable name (without $)
static ROUTE_PREFIX_GROUP_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)Route::prefix\(\s*["']([^"']+)["']\s*\)\s*->\s*group\(\s*\$(\w+)"#,
    )
    .expect("laravel route prefix group var re")
});

pub struct LaravelResolver;

impl LaravelResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Normalize resource path to ensure leading slash.
    fn normalize_path(path: &str) -> String {
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        }
    }

    /// Expand a `Route::resource` into standard RESTful route edges.
    fn expand_resource(
        resource_path: &str,
        controller_name: &str,
        file_path: &str,
        line: u32,
    ) -> Vec<RouteEdgeRecord> {
        let base_path = Self::normalize_path(resource_path);
        let member_path = format!("{}/{{id}}", base_path);

        let routes = [
            ("GET", base_path.clone(), format!("{}::index", controller_name)),
            ("POST", base_path.clone(), format!("{}::store", controller_name)),
            (
                "GET",
                format!("{}/create", base_path),
                format!("{}::create", controller_name),
            ),
            (
                "GET",
                member_path.clone(),
                format!("{}::show", controller_name),
            ),
            (
                "PUT",
                member_path.clone(),
                format!("{}::update", controller_name),
            ),
            (
                "DELETE",
                member_path.clone(),
                format!("{}::destroy", controller_name),
            ),
            (
                "GET",
                format!("{}/edit", member_path),
                format!("{}::edit", controller_name),
            ),
        ];

        routes
            .into_iter()
            .enumerate()
            .map(|(i, (method, path, handler))| RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, i as u32),
                file_path: file_path.to_string(),
                route_path: path,
                handler_name: Some(handler),
                method: Some(method.to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("laravel".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            })
            .collect()
    }
}

impl FrameworkResolver for LaravelResolver {
    fn framework_key(&self) -> &str {
        "laravel"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Php]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process PHP files
        if !file_path.ends_with(".php") {
            return;
        }

        // --- New-style array syntax: Route::get("/path", [Controller::class, "method"]) ---
        for cap in ROUTE_ARRAY_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let controller = cap.get(3).map(|m| m.as_str()).unwrap_or("");
            let action = cap.get(4).map(|m| m.as_str()).unwrap_or("");
            let handler = format!("{}::{}", controller, action);

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: Self::normalize_path(route_path),
                handler_name: Some(handler),
                method: Some(method_name.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("laravel".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Old-style string syntax: Route::get("/path", "Controller@method") ---
        for cap in ROUTE_STRING_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let controller = cap.get(3).map(|m| m.as_str()).unwrap_or("");
            let action = cap.get(4).map(|m| m.as_str()).unwrap_or("");
            let handler = format!("{}::{}", controller, action);

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: Self::normalize_path(route_path),
                handler_name: Some(handler),
                method: Some(method_name.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("laravel".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Resource route: Route::resource("/path", Controller::class) ---
        for cap in ROUTE_RESOURCE_RE.captures_iter(source) {
            let resource_path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let controller = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            let expanded = Self::expand_resource(resource_path, controller, file_path, line);
            outcome.route_edges.extend(expanded);
        }

        // --- Route::prefix("/api")->group(base_path("routes/api.php")) ---
        for cap in ROUTE_PREFIX_GROUP_FILE_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let target_file = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: Self::normalize_path(prefix),
                handler_name: target_file,
                method: Some("GROUP".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("laravel".to_string()),
                route_kind: Some("group_file".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Route::group(['prefix' => '/api'], base_path("routes/api.php")) ---
        for cap in ROUTE_GROUP_ARRAY_FILE_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let target_file = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: Self::normalize_path(prefix),
                handler_name: target_file,
                method: Some("GROUP".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("laravel".to_string()),
                route_kind: Some("group_file".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Route::prefix("/api")->group($variable) ---
        for cap in ROUTE_PREFIX_GROUP_VAR_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let var_name = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: Self::normalize_path(prefix),
                handler_name: var_name,
                method: Some("GROUP".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("laravel".to_string()),
                route_kind: Some("group_var".to_string()),
                confidence: 0.75,
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
        // Resolve Route::prefix/group file-based grouping across files.
        //
        // Pattern 1 (group_file): Route::prefix("/api")->group(base_path("routes/api.php"))
        //   → the handler_name is the file path (e.g. "routes/api.php")
        //   → find the matching file in outcomes by suffix match
        //   → prepend "/api" to all http route_edges in that file
        //
        // Pattern 2 (group_var): Route::prefix("/api")->group($apiRoutes)
        //   → look up the variable via catalog to find the target file
        //   → prepend the prefix to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for all route_edges.

        // Step 1: collect group entries
        struct GroupInfo {
            prefix: String,
            /// For group_file: the file path string (e.g. "routes/api.php")
            /// For group_var: the variable name
            target_ref: String,
            mount_file: String,
            is_file_ref: bool,
        }

        let mut groups: Vec<GroupInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                let is_file = edge.route_kind.as_deref() == Some("group_file");
                let is_var = edge.route_kind.as_deref() == Some("group_var");
                if !is_file && !is_var {
                    continue;
                }
                if let Some(ref handler) = edge.handler_name {
                    let prefix = &edge.route_path;
                    if prefix != "/" && !prefix.is_empty() {
                        groups.push(GroupInfo {
                            prefix: prefix.clone(),
                            target_ref: handler.clone(),
                            mount_file: file_path.clone(),
                            is_file_ref: is_file,
                        });
                    }
                }
            }
        }

        // Step 2: for each group, find the target file and prepend prefix
        for group in &groups {
            let target_file = if group.is_file_ref {
                // For file references like "routes/api.php", find by suffix match
                // in the outcomes list
                let suffix = &group.target_ref;
                outcomes
                    .iter()
                    .map(|(fp, _)| fp.clone())
                    .find(|fp| fp.ends_with(suffix) || fp.ends_with(&format!("/{}", suffix)))
                    .unwrap_or_default()
            } else {
                // For variable references, look up via catalog
                match catalog.lookup_symbol(&group.target_ref, &group.mount_file) {
                    Some((_, file)) if file != group.mount_file => file,
                    _ => continue,
                }
            };

            if target_file.is_empty() {
                continue;
            }

            // Prepend the group prefix to all http routes in the target file
            for (file_path, outcome) in outcomes.iter_mut() {
                if *file_path != target_file {
                    continue;
                }
                for edge in &mut outcome.route_edges {
                    if edge.route_kind.as_deref() != Some("http") {
                        continue;
                    }
                    // Only prepend once (avoid double-prefixing on repeated runs)
                    if !edge.route_path.starts_with(&group.prefix) {
                        let combined = if edge.route_path == "/" {
                            group.prefix.clone()
                        } else {
                            format!("{}{}", group.prefix, edge.route_path)
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

    fn run_laravel(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        LaravelResolver.enrich_file(file_path, source, Language::Php, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_laravel_array_syntax() {
        let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::get("/users", [UserController::class, "index"]);
Route::post("/users", [UserController::class, "store"]);
Route::delete("/users/{id}", [UserController::class, "destroy"]);
"#;
        let routes = run_laravel("routes/web.php", source);
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("UserController::index")));
        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("UserController::store")));
        assert!(routes.iter().any(|r| r.route_path == "/users/{id}"
            && r.method == Some("DELETE".into())
            && r.handler_name.as_deref() == Some("UserController::destroy")));

        // Verify framework annotation
        assert!(routes.iter().all(|r| r.framework.as_deref() == Some("laravel")));
    }

    #[test]
    fn test_laravel_string_syntax() {
        let source = r#"<?php
Route::get("/dashboard", "DashboardController@index");
Route::post("/login", "AuthController@login");
"#;
        let routes = run_laravel("routes/web.php", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        assert!(routes.iter().any(|r| r.route_path == "/dashboard"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("DashboardController::index")));
        assert!(routes.iter().any(|r| r.route_path == "/login"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("AuthController::login")));
    }

    #[test]
    fn test_laravel_resource_route() {
        let source = r#"<?php
Route::resource("/posts", PostController::class);
"#;
        let routes = run_laravel("routes/web.php", source);
        // resource expands to 7 standard RESTful routes
        assert_eq!(routes.len(), 7, "expected 7 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts" && r.method == Some("POST".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts/{id}" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts/{id}" && r.method == Some("PUT".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts/{id}" && r.method == Some("DELETE".into())));
    }

    #[test]
    fn test_laravel_prefix_group_file() {
        let source = r#"<?php
Route::prefix('/api')->group(base_path('routes/api.php'));
Route::prefix("/admin")->group(base_path("routes/admin.php"));
"#;
        let routes = run_laravel("routes/web.php", source);
        let groups: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("group_file"))
            .collect();
        assert_eq!(groups.len(), 2, "expected 2 group_file entries, got {}", groups.len());

        assert!(groups.iter().any(|r| r.route_path == "/api"
            && r.handler_name.as_deref() == Some("routes/api.php")
            && r.method == Some("GROUP".into())));
        assert!(groups.iter().any(|r| r.route_path == "/admin"
            && r.handler_name.as_deref() == Some("routes/admin.php")
            && r.method == Some("GROUP".into())));
    }

    #[test]
    fn test_laravel_group_array_file() {
        let source = r#"<?php
Route::group(['prefix' => '/api'], base_path('routes/api.php'));
"#;
        let routes = run_laravel("routes/web.php", source);
        let groups: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("group_file"))
            .collect();
        assert_eq!(groups.len(), 1, "expected 1 group_file entry, got {}", groups.len());

        assert_eq!(groups[0].route_path, "/api");
        assert_eq!(groups[0].handler_name.as_deref(), Some("routes/api.php"));
    }

    #[test]
    fn test_laravel_prefix_group_var() {
        let source = r#"<?php
Route::prefix('/api')->group($apiRoutes);
"#;
        let routes = run_laravel("routes/web.php", source);
        let groups: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("group_var"))
            .collect();
        assert_eq!(groups.len(), 1, "expected 1 group_var entry, got {}", groups.len());

        assert_eq!(groups[0].route_path, "/api");
        assert_eq!(groups[0].handler_name.as_deref(), Some("apiRoutes"));
    }
}
