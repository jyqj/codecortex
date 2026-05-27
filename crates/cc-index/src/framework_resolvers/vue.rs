//! Vue framework resolver.
//!
//! - `enrich_file`: extracts vue-router route definitions, Nuxt file-based
//!   routes, and defineComponent / script-setup detection.
//! - `resolve_cross_file`: no-op

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

/// vue-router route definition: `{ path: "/path", component: ComponentName }` or
/// `{ path: "/path", name: "route-name" }`.
///
/// Captures: (1) route path, (2) component or route name
static VUE_ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"path:\s*["']([^"']+)["']\s*,\s*(?:component|name):\s*["']?(\w+)"#)
        .expect("vue route re")
});

/// `defineComponent({` — Vue Options API.
static DEFINE_COMPONENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)defineComponent\s*\("#).expect("vue defineComponent re")
});

/// `<script setup>` — Vue Composition API with script setup.
static SCRIPT_SETUP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<script\s+[^>]*setup[^>]*>"#).expect("vue script setup re")
});

pub struct VueResolver;

impl VueResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Detect Nuxt file-based routing from the file path.
    ///
    /// Patterns:
    /// - `pages/users/index.vue` → /users
    /// - `pages/users/[id].vue` → /users/:id
    /// - `pages/index.vue` → /
    fn detect_nuxt_file_route(file_path: &str) -> Option<String> {
        let normalized = file_path.replace('\\', "/");

        let rest = Self::strip_after_prefix(&normalized, "pages/")?;

        let clean = rest.strip_suffix(".vue")?;

        // Remove trailing /index
        let clean = clean.strip_suffix("/index").unwrap_or(clean);
        let clean = if clean == "index" { "" } else { clean };

        // Convert [param] to :param
        let route = if clean.is_empty() {
            "/".to_string()
        } else {
            let converted = clean
                .split('/')
                .map(|seg| {
                    if seg.starts_with('[') && seg.ends_with(']') {
                        format!(":{}", &seg[1..seg.len() - 1])
                    } else {
                        seg.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("/");
            format!("/{}", converted)
        };

        Some(route)
    }

    /// Find the suffix after a path prefix like `pages/` regardless of leading dirs.
    fn strip_after_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
        if let Some(rest) = path.strip_prefix(prefix) {
            return Some(rest);
        }
        if let Some(idx) = path.find(&format!("/{}", prefix)) {
            return Some(&path[idx + 1 + prefix.len()..]);
        }
        None
    }
}

impl FrameworkResolver for VueResolver {
    fn framework_key(&self) -> &str {
        "vue"
    }

    fn languages(&self) -> &[Language] {
        &[Language::TypeScript, Language::JavaScript, Language::Vue]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // --- 1. vue-router route definitions ---
        for cap in VUE_ROUTE_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let handler_name = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: Some("GET".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("vue_router".to_string()),
                route_kind: Some("vue_route".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- 2. Nuxt file-based routing ---
        if let Some(route) = Self::detect_nuxt_file_route(file_path) {
            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, 1, 0),
                file_path: file_path.to_string(),
                route_path: route,
                handler_name: None,
                method: Some("GET".to_string()),
                line: 1,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("nuxt".to_string()),
                route_kind: Some("nuxt_page".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- 3. defineComponent / script setup detection ---
        if DEFINE_COMPONENT_RE.is_match(source) {
            let offset = DEFINE_COMPONENT_RE.find(source).unwrap().start();
            let line = Self::line_for_offset(source, offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("component", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: String::new(),
                handler_name: None,
                method: None,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("vue".to_string()),
                route_kind: Some("vue_options_api".to_string()),
                confidence: 0.70,
                parser_tier: ParserTier::Heuristic,
            });
        }

        if SCRIPT_SETUP_RE.is_match(source) {
            let offset = SCRIPT_SETUP_RE.find(source).unwrap().start();
            let line = Self::line_for_offset(source, offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("component", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: String::new(),
                handler_name: None,
                method: None,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("vue".to_string()),
                route_kind: Some("vue_composition_api".to_string()),
                confidence: 0.70,
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
        // No-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_vue(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        VueResolver.enrich_file(file_path, source, Language::TypeScript, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_vue_router_routes() {
        let source = r#"
import { createRouter, createWebHistory } from "vue-router";
import Home from "./views/Home.vue";
import About from "./views/About.vue";

const router = createRouter({
    history: createWebHistory(),
    routes: [
        { path: "/", component: Home },
        { path: "/about", name: "about" },
        { path: "/users/:id", component: UserDetail },
    ],
});
"#;
        let routes = run_vue("src/router.ts", source);
        let vue_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("vue_route"))
            .collect();
        assert_eq!(
            vue_routes.len(),
            3,
            "expected 3 vue routes, got {}",
            vue_routes.len()
        );
        assert!(vue_routes
            .iter()
            .any(|r| r.route_path == "/" && r.handler_name.as_deref() == Some("Home")));
        assert!(vue_routes
            .iter()
            .any(|r| r.route_path == "/about" && r.handler_name.as_deref() == Some("about")));
        assert!(vue_routes.iter().any(
            |r| r.route_path == "/users/:id" && r.handler_name.as_deref() == Some("UserDetail")
        ));
    }

    #[test]
    fn test_nuxt_file_routing() {
        let source = r#"<template><div>Users</div></template>"#;

        // pages/users/index.vue → /users
        let routes = run_vue("pages/users/index.vue", source);
        let nuxt_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("nuxt_page"))
            .collect();
        assert_eq!(nuxt_routes.len(), 1);
        assert_eq!(nuxt_routes[0].route_path, "/users");

        // pages/users/[id].vue → /users/:id
        let routes = run_vue("pages/users/[id].vue", source);
        let nuxt_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("nuxt_page"))
            .collect();
        assert_eq!(nuxt_routes.len(), 1);
        assert_eq!(nuxt_routes[0].route_path, "/users/:id");

        // pages/index.vue → /
        let routes = run_vue("pages/index.vue", source);
        let nuxt_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("nuxt_page"))
            .collect();
        assert_eq!(nuxt_routes.len(), 1);
        assert_eq!(nuxt_routes[0].route_path, "/");
    }

    #[test]
    fn test_vue_component_detection() {
        // Options API
        let options_source = r#"
import { defineComponent } from "vue";

export default defineComponent({
    name: "MyComponent",
    data() { return { count: 0 } },
});
"#;
        let routes = run_vue("src/components/MyComponent.vue", options_source);
        assert!(routes
            .iter()
            .any(|r| r.route_kind.as_deref() == Some("vue_options_api")));

        // Composition API with script setup
        let setup_source = r#"
<script setup lang="ts">
import { ref } from "vue";
const count = ref(0);
</script>

<template>
    <button @click="count++">{{ count }}</button>
</template>
"#;
        let routes = run_vue("src/components/Counter.vue", setup_source);
        assert!(routes
            .iter()
            .any(|r| r.route_kind.as_deref() == Some("vue_composition_api")));
    }
}
