//! Framework-specific semantic enrichment (Phase 2.1 infrastructure).
//!
//! Two-phase design:
//! - `enrich_file`: per-file, runs after parse, can read source text
//! - `resolve_cross_file`: global, runs after SymbolCatalog is built
//!
//! Currently only defines the trait and registry. Actual resolver
//! implementations will be added in Phase 2.2+.

pub mod go_router;
pub mod react;
pub mod spring;

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
    registry
}
