//! Rails framework resolver.
//!
//! - `enrich_file`: extracts route definitions from Rails routes.rb DSL
//! - `resolve_cross_file`: resolves engine mount prefix propagation across files

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

/// Standard route methods: get "/path", to: "controller#action"
///
/// Matches:
///   get "/users", to: "users#index"
///   post '/sessions', to: 'sessions#create'
///   delete "/users/:id", to: "users#destroy"
///
/// Captures: (1) HTTP method, (2) route path, (3) controller#action
static ROUTE_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)(get|post|put|patch|delete)\s+["']([^"']+)["']\s*,\s*to:\s*["'](\w+#\w+)["']"#,
    )
    .expect("rails route method re")
});

/// resources :name — generates standard RESTful routes
///
/// Matches:
///   resources :users
///   resources :posts
///
/// Captures: (1) resource name
static RESOURCES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)resources\s+:(\w+)"#).expect("rails resources re"));

/// root "controller#action" — maps GET / to a controller action
///
/// Matches:
///   root "pages#home"
///   root 'welcome#index'
///
/// Captures: (1) controller#action
static ROOT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)root\s+["'](\w+#\w+)["']"#).expect("rails root re"));

/// mount Engine, at: "/path" — mounts a Rails engine at a prefix
///
/// Matches:
///   mount Blog::Engine, at: "/blog"
///   mount Api::Engine, at: '/api'
///   mount Sidekiq::Web, at: "/sidekiq"
///
/// Captures: (1) engine name (e.g. Blog::Engine), (2) mount path
static MOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)mount\s+([\w:]+)\s*,\s*at:\s*["']([^"']+)["']"#).expect("rails mount re")
});

pub struct RailsResolver;

impl RailsResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Expand a `resources :name` into standard RESTful route edges.
    fn expand_resources(resource_name: &str, file_path: &str, line: u32) -> Vec<RouteEdgeRecord> {
        let base_path = format!("/{}", resource_name);
        let member_path = format!("/{}/:id", resource_name);
        let controller = resource_name.to_string();

        let routes = [
            ("GET", base_path.clone(), format!("{}#index", controller)),
            ("POST", base_path.clone(), format!("{}#create", controller)),
            (
                "GET",
                format!("{}/new", base_path),
                format!("{}#new", controller),
            ),
            (
                "GET",
                format!("{}/edit", member_path),
                format!("{}#edit", controller),
            ),
            ("GET", member_path.clone(), format!("{}#show", controller)),
            ("PUT", member_path.clone(), format!("{}#update", controller)),
            (
                "DELETE",
                member_path.clone(),
                format!("{}#destroy", controller),
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
                framework: Some("rails".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            })
            .collect()
    }
}

impl FrameworkResolver for RailsResolver {
    fn framework_key(&self) -> &str {
        "rails"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Ruby]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process Ruby files
        if !(file_path.ends_with(".rb")) {
            return;
        }

        // --- Standard route methods: get "/path", to: "controller#action" ---
        for cap in ROUTE_METHOD_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let handler = cap.get(3).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name: handler,
                method: Some(method_name.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("rails".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- resources :name → expand RESTful routes ---
        for cap in RESOURCES_RE.captures_iter(source) {
            let resource_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            let expanded = Self::expand_resources(resource_name, file_path, line);
            outcome.route_edges.extend(expanded);
        }

        // --- root "controller#action" → GET / ---
        for cap in ROOT_RE.captures_iter(source) {
            let handler = cap.get(1).map(|m| m.as_str().to_string());
            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: "/".to_string(),
                handler_name: handler,
                method: Some("GET".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("rails".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- mount Engine, at: "/path" → engine mount ---
        for cap in MOUNT_RE.captures_iter(source) {
            let engine_name = cap.get(1).map(|m| m.as_str().to_string());
            let mount_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: mount_path.to_string(),
                handler_name: engine_name,
                method: Some("MOUNT".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("rails".to_string()),
                route_kind: Some("engine_mount".to_string()),
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
        // Resolve engine mount prefix propagation across files.
        //
        // Pattern: mount Blog::Engine, at: "/blog"
        //   → find the file where `Blog::Engine` (or its routes) is defined
        //   → prepend "/blog" to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for route_edges whose handler_name
        // is set but handler_symbol_uid is not.

        // Step 1: collect engine mount points (prefix → engine name → mounting file)
        struct MountInfo {
            prefix: String,
            engine_name: String,
            mount_file: String,
        }

        let mut mounts: Vec<MountInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                if edge.route_kind.as_deref() == Some("engine_mount") {
                    if let Some(ref handler) = edge.handler_name {
                        let prefix = &edge.route_path;
                        if prefix != "/" && !prefix.is_empty() {
                            mounts.push(MountInfo {
                                prefix: prefix.clone(),
                                engine_name: handler.clone(),
                                mount_file: file_path.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Step 2: for each mount, find the target file via catalog and prepend prefix
        for mount in &mounts {
            // Look up where the engine is defined
            let target_file = match catalog.lookup_symbol(&mount.engine_name, &mount.mount_file) {
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

    fn run_rails(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        RailsResolver.enrich_file(file_path, source, Language::Ruby, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_rails_explicit_routes() {
        let source = r#"
Rails.application.routes.draw do
  get "/users", to: "users#index"
  post "/users", to: "users#create"
  delete "/users/:id", to: "users#destroy"
end
"#;
        let routes = run_rails("config/routes.rb", source);
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("users#index")));
        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("users#create")));
        assert!(routes.iter().any(|r| r.route_path == "/users/:id"
            && r.method == Some("DELETE".into())
            && r.handler_name.as_deref() == Some("users#destroy")));

        // Verify framework annotation
        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("rails")));
    }

    #[test]
    fn test_rails_resources() {
        let source = r#"
Rails.application.routes.draw do
  resources :posts
end
"#;
        let routes = run_rails("config/routes.rb", source);
        // resources expands to 7 standard RESTful routes
        assert_eq!(routes.len(), 7, "expected 7 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts" && r.method == Some("POST".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts/:id" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts/:id" && r.method == Some("PUT".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/posts/:id" && r.method == Some("DELETE".into())));
    }

    #[test]
    fn test_rails_root_route() {
        let source = r#"
Rails.application.routes.draw do
  root "pages#home"
end
"#;
        let routes = run_rails("config/routes.rb", source);
        assert_eq!(
            routes.len(),
            1,
            "expected 1 root route, got {}",
            routes.len()
        );
        assert_eq!(routes[0].route_path, "/");
        assert_eq!(routes[0].method, Some("GET".into()));
        assert_eq!(routes[0].handler_name.as_deref(), Some("pages#home"));
    }

    #[test]
    fn test_rails_engine_mount() {
        let source = r#"
Rails.application.routes.draw do
  mount Blog::Engine, at: "/blog"
  mount Sidekiq::Web, at: '/sidekiq'
end
"#;
        let routes = run_rails("config/routes.rb", source);
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("engine_mount"))
            .collect();
        assert_eq!(
            mounts.len(),
            2,
            "expected 2 engine mounts, got {}",
            mounts.len()
        );

        assert!(mounts.iter().any(|r| r.route_path == "/blog"
            && r.handler_name.as_deref() == Some("Blog::Engine")
            && r.method == Some("MOUNT".into())));
        assert!(mounts.iter().any(|r| r.route_path == "/sidekiq"
            && r.handler_name.as_deref() == Some("Sidekiq::Web")
            && r.method == Some("MOUNT".into())));
    }
}
