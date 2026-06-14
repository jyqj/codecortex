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
        resolution_strategy: None,
        resolution_confidence: None,
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
        // 单向语义：`key` 是 resolver 视角的查询 key，`k` 是 detection 写入
        // `repo_frameworks` 的 normalized key。仅当 `key` 恰是某 taxon 的
        // canonical 时，才把该 taxon 的 canonical+aliases 作为 k 的命中集展开；
        // 别名（如 "actix"/"echo"）作为查询 key 时不展开，回退到 `k == key`。
        //
        // 这逐字恢复了原 `matches!((key, k), ("actix-web","actix") |
        // ("gin", echo|fiber|chi|gorilla|net_http))` 的单向语义：
        //   - 原命中对的 key（"actix-web"、"gin"）正是各自 taxon 的 canonical；
        //   - k 取别名族（"actix"、echo|fiber|...）即 detection 产出的 key。
        // 因此 key=canonical 时展开覆盖全部原命中对；key=别名 时回退 `k == key`，
        // 不会出现"repo 存 canonical、查询别名"的反向命中（原 matches! 同样不命中）。
        // `k == key` 还兼容 taxonomy 之外的 detection key（未注册框架名）。
        self.repo_frameworks.iter().any(|(k, _)| {
            k == key
                || taxon_for_key(key).is_some_and(|taxon| {
                    taxon.canonical == key
                        && (taxon.canonical == k.as_str()
                            || taxon.aliases.iter().any(|alias| *alias == k.as_str()))
                })
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

// ---------------------------------------------------------------------------
// Framework taxonomy — 单一声明源
// ---------------------------------------------------------------------------
//
// 同一份「框架分类表」历史上散落在三处并行声明，三者必须保持一致却没有任何机制强制：
//   1. `default_registry()`（哪些 resolver 注册）
//   2. `resolver_tier_for_key()`（key→tier 的并行 match）
//   3. `ProjectFrameworkContext::has_framework()`（detection_key→resolver_key 的别名映射）
//   4. `indexer.rs` 的 `go_router_keys`（go_router resolver 的别名激活集合）
//
// 这里收口为单一声明源 [`framework_taxonomy`]：每项给出 canonical key（=对应
// resolver 的 `framework_key()` 返回值）、别名切片（= detection 侧的同义 key）、
// 覆盖 tier。下方 `resolver_tier_for_key` / `has_framework` 均从它派生，
// `indexer` 的 go_router 激活集合也从它取 key。形态参考
// `cc-server/src/graph_read_model/bridge_spec.rs` 的封闭 registry + 一致性测试。
//
// 注意：这**不是**「新增框架只在此处注册即可」。新增框架仍需 resolver module +
// framework_registry 检测规则 + default_registry 注册 + 测试。这里只解决
// key / aliases / tier 三者的单一声明。

/// 框架分类条目：canonical key + 别名 + 覆盖 tier。
///
/// `canonical` 必须等于对应 resolver 的 `framework_key()` 返回值（由一致性测试
/// `default_registry_keys_covered_by_taxonomy` 强制）。`aliases` 是 detection 侧
/// 产生的同义 key（不作为 resolver 的 `framework_key()`）。`tier` 与 resolver 的
/// `resolver_tier()` 保持一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameworkTaxon {
    /// 权威 key，等于对应 resolver 的 `framework_key()`。
    pub canonical: &'static str,
    /// detection 侧产生的同义 key 集合（不含 canonical 自身）。
    pub aliases: &'static [&'static str],
    /// 覆盖 tier，与 resolver 的 `resolver_tier()` 一致。
    pub tier: &'static str,
}

/// 框架分类表的单一声明源。返回静态切片，顺序与 `default_registry` 的注册顺序
/// 对齐，便于一致性测试核对。
pub fn framework_taxonomy() -> &'static [FrameworkTaxon] {
    const TAXONOMY: &[FrameworkTaxon] = &[
        // canonical / aliases / tier —— 顺序与 default_registry() 对齐
        FrameworkTaxon {
            canonical: "spring",
            aliases: &[],
            tier: "full",
        },
        // Go router 族：detection 会对 echo/fiber/chi/gorilla/net_http 分别产 key，
        // go_router resolver 的 canonical 是 "gin"。这组别名同时被
        // `has_framework("gin", ...)` 与 indexer 的 `go_router_keys` 消费，因此
        // 集中在此一处。
        FrameworkTaxon {
            canonical: "gin",
            aliases: &["echo", "fiber", "chi", "gorilla", "net_http"],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "react",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "axum",
            aliases: &[],
            tier: "full",
        },
        // actix：detection 产 normalized key "actix"，resolver 的 canonical 是
        // "actix-web"（route metadata 用 actix-web）。保留为别名。
        FrameworkTaxon {
            canonical: "actix-web",
            aliases: &["actix"],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "express",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "nestjs",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "fastapi",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "django",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "flask",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "rails",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "laravel",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "vue",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "sveltekit",
            aliases: &[],
            tier: "full",
        },
        FrameworkTaxon {
            canonical: "aspnet",
            aliases: &[],
            // AspNetResolver 未 override resolver_tier()，沿用默认 "extraction"。
            tier: "extraction",
        },
        FrameworkTaxon {
            canonical: "hono",
            aliases: &[],
            tier: "full",
        },
    ];
    TAXONOMY
}

/// 查询 key 所属的 taxon：canonical 或任一 alias 命中即返回。
pub fn taxon_for_key(key: &str) -> Option<&'static FrameworkTaxon> {
    framework_taxonomy().iter().find(|taxon| {
        taxon.canonical == key || taxon.aliases.iter().any(|alias| *alias == key)
    })
}

/// Look up the resolver tier for a given `framework_key`.
///
/// Returns `"full"`, `"extraction"`, or `"experimental"`.
/// If the key is not found in the registry, returns `"unknown"`.
///
/// 实现从 [`framework_taxonomy`] 派生，不再维护并行 match。
pub fn resolver_tier_for_key(framework_key: &str) -> &'static str {
    match taxon_for_key(framework_key) {
        Some(taxon) => taxon.tier,
        None => "unknown",
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
        // 单向语义锁定：has_framework 只在「key=canonical、k=该 taxon 任一 key」
        // 时命中，不产生对称扩展。故 repo 存 canonical、用别名查询时不得命中。
        let ctx2 = ProjectFrameworkContext {
            repo_frameworks: vec![("actix-web".to_string(), 0.9), ("gin".to_string(), 0.8)],
            file_frameworks: Default::default(),
        };
        // key=别名("actix")：taxon 命中但 key≠canonical，回退 k==key；
        // k 是 "actix-web"，不等于 "actix"，故不命中（恢复原 matches! 单向）。
        assert!(!ctx2.has_framework("actix"));
        // key=别名("echo")：同上，回退 k==key；k 是 "gin"，不命中。
        assert!(!ctx2.has_framework("echo"));
        assert!(!ctx2.has_framework("net_http"));
        // 不同 taxon 之间不得误命中（修复早期 zip 过宽 bug 的回归锁）。
        let ctx3 = ProjectFrameworkContext {
            repo_frameworks: vec![("django".to_string(), 0.9)],
            file_frameworks: Default::default(),
        };
        assert!(!ctx3.has_framework("express"));
        assert!(!ctx3.has_framework("gin"));
        // taxonomy 之外的 detection key 仍走 `k == key` 回退（原行为保留）。
        let ctx4 = ProjectFrameworkContext {
            repo_frameworks: vec![("customfw".to_string(), 0.5)],
            file_frameworks: Default::default(),
        };
        assert!(ctx4.has_framework("customfw"));
    }

    /// 锁定 taxonomy 的封闭性 —— 形态参考 bridge_spec.rs 的
    /// `registry_pins_bridge_kinds`。
    #[test]
    fn taxonomy_is_closed_and_unique() {
        let taxonomy = framework_taxonomy();
        assert!(!taxonomy.is_empty());

        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for taxon in taxonomy {
            // canonical 非空且全局唯一。
            assert!(!taxon.canonical.is_empty());
            assert!(seen.insert(taxon.canonical), "duplicate canonical");
            // tier 取值受限于 resolver_tier 的对外契约。
            assert!(
                matches!(taxon.tier, "full" | "extraction" | "experimental"),
                "invalid tier {:?}",
                taxon.tier
            );
            // alias 非空、不等于 canonical、且全局唯一（不与任何 canonical/alias 重复）。
            for alias in taxon.aliases {
                assert!(!alias.is_empty());
                assert_ne!(*alias, taxon.canonical);
                assert!(seen.insert(*alias), "duplicate alias/key {}", alias);
            }
        }
    }

    /// default_registry() 注册的每个 resolver，其 framework_key() 必须作为某
    /// taxon 的 canonical 出现在 taxonomy 中 —— 这是「单一声明源」的核心约束：
    /// 新增 resolver 若忘记登记 taxonomy，此处立即失败。
    #[test]
    fn default_registry_keys_covered_by_taxonomy() {
        let registry = default_registry();
        let canonicals: std::collections::HashSet<&str> = framework_taxonomy()
            .iter()
            .map(|taxon| taxon.canonical)
            .collect();

        for resolver in registry.all_resolvers() {
            let key = resolver.framework_key();
            assert!(
                canonicals.contains(key),
                "resolver framework_key {:?} not declared as canonical in taxonomy",
                key
            );
            // tier 也必须与 resolver 自报的 resolver_tier() 一致 —— 防止 taxonomy
            // 与 resolver 实现的并行 tier 声明再次漂移。
            assert_eq!(
                resolver.resolver_tier(),
                resolver_tier_for_key(key),
                "tier mismatch for {:?}: resolver_tier() != taxonomy tier",
                key
            );
        }

        // 反向覆盖：每个 taxon 的 canonical 都应对应 default_registry 中的某个
        // resolver（taxonomy 不得声明 resolver 不存在的框架）。
        let resolver_keys: std::collections::HashSet<&str> = registry
            .all_resolvers()
            .iter()
            .map(|r| r.framework_key())
            .collect();
        for taxon in framework_taxonomy() {
            assert!(
                resolver_keys.contains(taxon.canonical),
                "taxonomy canonical {:?} has no resolver in default_registry",
                taxon.canonical
            );
        }
    }

    /// taxon_for_key 必须对 canonical 与每个 alias 都命中、对未知 key 返回 None。
    #[test]
    fn taxon_lookup_covers_canonical_and_aliases() {
        for taxon in framework_taxonomy() {
            assert_eq!(taxon_for_key(taxon.canonical), Some(taxon));
            for alias in taxon.aliases {
                assert_eq!(taxon_for_key(alias), Some(taxon));
            }
        }
        assert_eq!(taxon_for_key("unknown-fw"), None);
        assert_eq!(taxon_for_key(""), None);
    }
}
