//! 框架分类的单一声明源（canonical key + 别名 + tier）。
//!
//! `FrameworkTaxon{canonical, aliases, tier}` 是纯数据，无 cc-index / cc-parsers
//! 依赖。历史上同一份分类表散落在 cc-index 多处（resolver 注册表、
//! `resolver_tier_for_key` 的并行 match、`ProjectFrameworkContext::has_framework`
//! 的别名展开、indexer 的 `go_router_keys`）与 cc-parsers 的检测 emit 之间，彼此
//! 必须一致却无机制强制。收口到 cc-model 后，cc-index 与 cc-parsers 都从这里
//! 派生：canonical = 对应 resolver 的 `framework_key()`；aliases = detection 侧
//! 产生的同义 key；tier = resolver 的 `resolver_tier()`。检测模式（import 正则、
//! AST 启发式、路由词表）仍留各自 crate —— 本模块只解决「key/aliases/tier 三者
//! 的单一声明」。
//!
//! 注意：这**不是**「新增框架只在此处登记即可」。新增框架仍需 cc-index 侧的
//! resolver module + framework_registry 检测规则 + default_registry 注册 + 测试。

/// 框架分类条目：canonical key + 别名 + 覆盖 tier。
///
/// `canonical` 必须等于对应 resolver 的 `framework_key()` 返回值（由 cc-index 侧
/// 一致性测试 `default_registry_keys_covered_by_taxonomy` 强制）。`aliases` 是
/// detection 侧产生的同义 key（不作为 resolver 的 `framework_key()`）。`tier` 与
/// resolver 的 `resolver_tier()` 保持一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameworkTaxon {
    /// 权威 key，等于对应 resolver 的 `framework_key()`。
    pub canonical: &'static str,
    /// detection 侧产生的同义 key 集合（不含 canonical 自身）。
    pub aliases: &'static [&'static str],
    /// 覆盖 tier，与 resolver 的 `resolver_tier()` 一致。
    pub tier: &'static str,
}

/// 框架分类表的单一声明源。返回静态切片，顺序与 cc-index `default_registry` 的
/// 注册顺序对齐，便于一致性测试核对。
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
    framework_taxonomy()
        .iter()
        .find(|taxon| taxon.canonical == key || taxon.aliases.iter().any(|alias| *alias == key))
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

/// 迭代某 taxon 的 canonical key 后接其全部别名 —— 即归属该 taxon 的完整 key 集
/// （detection / resolution 侧所有同义 key）。供「取该框架的全部 key」场景使用
/// （如 indexer 的 go_router 激活集合），替代每处手写
/// `std::iter::once(canonical).chain(aliases)`。
pub fn canonical_aliases(taxon: &FrameworkTaxon) -> impl Iterator<Item = &'static str> {
    std::iter::once(taxon.canonical).chain(taxon.aliases.iter().copied())
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

    /// canonical_aliases 收敛了 canonical + 全部别名，替代手写 once().chain()。
    #[test]
    fn canonical_aliases_covers_canonical_then_aliases() {
        let gin = taxon_for_key("gin").expect("gin taxon exists");
        let keys: Vec<&str> = canonical_aliases(gin).collect();
        assert_eq!(keys, vec!["gin", "echo", "fiber", "chi", "gorilla", "net_http"]);

        let spring = taxon_for_key("spring").expect("spring taxon exists");
        assert_eq!(canonical_aliases(spring).collect::<Vec<_>>(), vec!["spring"]);
    }
}
