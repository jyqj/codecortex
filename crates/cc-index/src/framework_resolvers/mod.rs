//! Framework-specific semantic enrichment resolvers.
//!
//! Two-phase design:
//! - `enrich_file`: per-file, runs after parse, can read source text
//! - `resolve_cross_file`: global, runs after SymbolCatalog is built
//!
//! Covers 16 frameworks: Actix, ASP.NET, Axum, Django, Express, FastAPI,
//! Flask, Go router, Hono, Laravel, NestJS, Rails, React, Spring, Svelte, and Vue.

pub mod actix;
pub mod aspnet;
pub mod axum;
pub mod django;
pub mod express;
pub mod fastapi;
pub mod flask;
pub mod go_router;
pub mod hono;
pub mod laravel;
pub(crate) mod mount_resolution;
pub mod nestjs;
pub(crate) mod python_patterns;
pub mod rails;
pub mod react;
pub mod spring;
pub mod svelte;
pub mod vue;

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};

/// Compute a 1-based line number for a byte offset in source text.
pub(crate) fn line_for_offset(source: &str, offset: usize) -> u32 {
    source[..offset].matches('\n').count() as u32 + 1
}

/// Python `def` / `async def` handler name. Shared by the FastAPI and Flask
/// resolvers, which pull the decorated handler name identically.
pub(crate) static PY_DEF_NAME_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?:async\s+)?def\s+(\w+)\s*\("#).expect("python def name re")
    });

/// Common route-edge fields used by framework resolvers.
pub(crate) struct RouteEdgeSpec {
    pub route_path: String,
    pub handler_name: Option<String>,
    pub method: Option<String>,
    pub framework: &'static str,
    pub route_kind: &'static str,
    pub confidence: f64,
    pub parser_tier: ParserTier,
}

/// Build a framework route edge with the common default metadata shape.
pub(crate) fn make_route_edge(
    file_path: &str,
    line: u32,
    ordinal: u32,
    spec: RouteEdgeSpec,
) -> RouteEdgeRecord {
    RouteEdgeRecord {
        edge_id: StableId::edge_id("route", file_path, line, ordinal),
        file_path: file_path.to_string(),
        route_path: spec.route_path,
        handler_name: spec.handler_name,
        method: spec.method,
        line,
        start_col: 0,
        end_line: None,
        end_col: 0,
        handler_symbol_id: None,
        handler_symbol_uid: None,
        handler_expr: None,
        router_symbol_uid: None,
        framework: Some(spec.framework.to_string()),
        route_kind: Some(spec.route_kind.to_string()),
        confidence: spec.confidence,
        parser_tier: spec.parser_tier,
    }
}

/// Push a common route edge into a parse outcome.
pub(crate) fn push_route_edge(
    outcome: &mut ParseOutcome,
    file_path: &str,
    line: u32,
    ordinal: u32,
    spec: RouteEdgeSpec,
) {
    outcome
        .route_edges
        .push(make_route_edge(file_path, line, ordinal, spec));
}

// ---------------------------------------------------------------------------
// ProjectFrameworkContext
// ---------------------------------------------------------------------------

/// Context about detected frameworks at the project level.
/// Built during post-processing from framework_registry signals.
pub struct ProjectFrameworkContext {
    /// Frameworks detected at repo level: vec of (framework_key, confidence)
    pub repo_frameworks: Vec<(String, f64)>,
    /// Per-file framework detections: file_path -> vec of (framework_key, confidence)
    pub file_frameworks: std::collections::HashMap<String, Vec<(String, f64)>>,
}

impl ProjectFrameworkContext {
    pub fn new() -> Self {
        Self {
            repo_frameworks: Vec::new(),
            file_frameworks: std::collections::HashMap::new(),
        }
    }

    pub fn has_framework(&self, key: &str) -> bool {
        self.repo_frameworks.iter().any(|(k, _)| {
            k == key
                || matches!(
                    (key, k.as_str()),
                    // Detection stores the normalized key `actix`, while the
                    // resolver emits/handles actix-web route metadata.
                    ("actix-web", "actix")
                        // The Go resolver handles the common router family.
                        | ("gin", "echo" | "fiber" | "chi" | "gorilla" | "net_http")
                )
        })
    }
}

impl Default for ProjectFrameworkContext {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FrameworkResolver trait
// ---------------------------------------------------------------------------

/// Trait for framework-specific semantic enrichment.
///
/// Two-phase design:
/// - `enrich_file`: per-file, runs after tree-sitter parse, before cross-file resolution
/// - `resolve_cross_file`: global, runs after Phase 4c (resolve call edges)
pub trait FrameworkResolver: Send + Sync {
    /// Unique identifier matching a framework_key in the detection tables.
    fn framework_key(&self) -> &str;

    /// Languages this resolver applies to.
    fn languages(&self) -> &[Language];

    /// Per-file enrichment: extract framework-specific semantics from source.
    /// Called after tree-sitter parse, before cross-file resolution.
    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        language: Language,
        outcome: &mut ParseOutcome,
        ctx: &ProjectFrameworkContext,
    );

    /// Cross-file resolution: resolve handler bindings using the full symbol catalog.
    /// Called after Phase 4c (resolve call edges).
    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        file_outcomes: &mut [(String, ParseOutcome)],
        ctx: &ProjectFrameworkContext,
    );

    /// Coverage tier of this resolver.
    ///
    /// - `"full"` — `resolve_cross_file` has real cross-file resolution logic
    ///   (prefix propagation, handler UID binding, etc.)
    /// - `"extraction"` — `enrich_file` works well, but `resolve_cross_file` is
    ///   no-op or only does trivial UID lookups
    /// - `"experimental"` — minimal / untested implementation
    fn resolver_tier(&self) -> &'static str {
        "extraction" // safe default
    }
}

// ---------------------------------------------------------------------------
// FrameworkResolverRegistry
// ---------------------------------------------------------------------------

/// Registry of all available framework resolvers.
pub struct FrameworkResolverRegistry {
    resolvers: Vec<Box<dyn FrameworkResolver>>,
}

impl FrameworkResolverRegistry {
    pub fn new() -> Self {
        Self {
            resolvers: Vec::new(),
        }
    }

    pub fn register(&mut self, resolver: Box<dyn FrameworkResolver>) {
        self.resolvers.push(resolver);
    }

    /// Get resolvers that are active for the detected frameworks.
    pub fn active_resolvers(&self, ctx: &ProjectFrameworkContext) -> Vec<&dyn FrameworkResolver> {
        self.resolvers
            .iter()
            .filter(|r| ctx.has_framework(r.framework_key()))
            .map(|r| r.as_ref())
            .collect()
    }

    pub fn all_resolvers(&self) -> &[Box<dyn FrameworkResolver>] {
        &self.resolvers
    }
}

impl Default for FrameworkResolverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a default registry with all known framework resolvers registered.
pub fn default_registry() -> FrameworkResolverRegistry {
    let mut registry = FrameworkResolverRegistry::new();
    registry.register(Box::new(spring::SpringResolver));
    registry.register(Box::new(go_router::GoRouterResolver));
    registry.register(Box::new(react::ReactComponentResolver));
    registry.register(Box::new(axum::AxumResolver));
    registry.register(Box::new(actix::ActixResolver));
    registry.register(Box::new(express::ExpressResolver));
    registry.register(Box::new(nestjs::NestJsResolver));
    registry.register(Box::new(fastapi::FastApiResolver));
    registry.register(Box::new(django::DjangoResolver));
    registry.register(Box::new(flask::FlaskResolver));
    registry.register(Box::new(rails::RailsResolver));
    registry.register(Box::new(laravel::LaravelResolver));
    registry.register(Box::new(vue::VueResolver));
    registry.register(Box::new(svelte::SvelteResolver));
    registry.register(Box::new(aspnet::AspNetResolver));
    registry.register(Box::new(hono::HonoResolver));
    registry
}

/// Look up the resolver tier for a given `framework_key`.
///
/// Returns `"full"`, `"extraction"`, or `"experimental"`.
/// If the key is not found in the registry, returns `"unknown"`.
pub fn resolver_tier_for_key(framework_key: &str) -> &'static str {
    match framework_key {
        "actix" | "actix-web" | "axum" | "django" | "express" | "fastapi" | "flask" | "gin"
        | "echo" | "fiber" | "chi" | "gorilla" | "net_http" | "hono" | "laravel" | "nestjs"
        | "rails" | "react" | "spring" | "sveltekit" | "vue" => "full",
        "aspnet" => "extraction",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_tier_lookup_uses_static_aliases() {
        assert_eq!(resolver_tier_for_key("express"), "full");
        assert_eq!(resolver_tier_for_key("actix"), "full");
        assert_eq!(resolver_tier_for_key("actix-web"), "full");
        assert_eq!(resolver_tier_for_key("echo"), "full");
        assert_eq!(resolver_tier_for_key("aspnet"), "extraction");
        assert_eq!(resolver_tier_for_key("unknown-fw"), "unknown");
    }

    #[test]
    fn project_context_recognizes_resolver_aliases() {
        let ctx = ProjectFrameworkContext {
            repo_frameworks: vec![("actix".to_string(), 0.9), ("echo".to_string(), 0.8)],
            file_frameworks: Default::default(),
        };
        assert!(ctx.has_framework("actix-web"));
        assert!(ctx.has_framework("gin"));
        assert!(!ctx.has_framework("express"));
    }
}
