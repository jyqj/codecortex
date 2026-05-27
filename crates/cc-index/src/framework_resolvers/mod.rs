//! Framework-specific semantic enrichment resolvers.
//!
//! Two-phase design:
//! - `enrich_file`: per-file, runs after parse, can read source text
//! - `resolve_cross_file`: global, runs after SymbolCatalog is built
//!
//! Covers 15 frameworks: Actix, ASP.NET, Axum, Django, Express, FastAPI,
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
pub mod nestjs;
pub mod rails;
pub mod react;
pub mod spring;
pub mod svelte;
pub mod vue;

use cc_model::parse::ParseOutcome;
use cc_model::Language;

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
        self.repo_frameworks.iter().any(|(k, _)| k == key)
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
    let registry = default_registry();
    for resolver in registry.all_resolvers() {
        if resolver.framework_key() == framework_key {
            return resolver.resolver_tier();
        }
    }
    "unknown"
}
