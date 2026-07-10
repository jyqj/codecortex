//! Route-framework detection signal.
//!
//! Owns the route framework normalization mapping plus the per-file scan over
//! the `routes` table. `normalize_route_framework` is also consumed by the
//! repo-level aggregation in the module root.

use std::collections::HashMap;

use super::{detection_entry, FileFrameworkDetection, FrameworkSignalSpec, SignalContext};

pub(super) const SPEC: FrameworkSignalSpec = FrameworkSignalSpec {
    id: "route_framework",
    detect: detect_route_frameworks,
};

// ---------------------------------------------------------------------------
// Route framework normalization
// ---------------------------------------------------------------------------

pub(super) fn normalize_route_framework(fw: &str) -> Option<&'static str> {
    let owned = fw.to_lowercase();
    // Layer 2: 双拼写归一（taxonomy 不收录带 '/' 或 '.' 的原始串）。
    let lower: &str = match owned.as_str() {
        "net/http" => "net_http",
        "asp.net" => "aspnet",
        "vue-router" => "vue_router",
        other => other,
    };
    // Layer 1: taxonomy 成员（canonical 或 alias）—— 从 cc-model 单一声明源判定
    // 成员资格，返回该 key 自身（取自 taxonomy 的 &'static 字面量）。保留 detection
    // 粒度：echo 仍是 echo、actix 仍是 actix（不坍缩到 canonical gin/actix-web），
    // 取代此前手抄的并行 match 表；同时修复 actix-web 孤儿（resolver 产的 canonical
    // 旧表不收、被 `_ => None` 丢弃的 detection signal）。
    for taxon in cc_model::framework_taxonomy::framework_taxonomy() {
        if taxon.canonical == lower {
            return Some(taxon.canonical);
        }
        if let Some(alias) = taxon.aliases.iter().copied().find(|a| *a == lower) {
            return Some(alias);
        }
    }
    // Layer 3: 路由专属无 resolver 超集（cc-index detection 概念，不进 taxonomy）。
    match lower {
        "fastify" => Some("fastify"),
        "koa" => Some("koa"),
        "rocket" => Some("rocket"),
        "vue_router" => Some("vue_router"),
        "nuxt" => Some("nuxt"),
        "remix" => Some("remix"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Per-file scan
// ---------------------------------------------------------------------------

fn detect_route_frameworks(
    ctx: &SignalContext,
    detections: &mut HashMap<String, FileFrameworkDetection>,
) {
    for row in ctx.scan.file_route_frameworks(ctx.file_path) {
        if let Some(fw_key) = normalize_route_framework(&row) {
            let det = detection_entry(detections, fw_key);
            let signal = format!("route_framework:{}", row);
            if !det.signals.contains(&signal) {
                det.confidence += super::WEIGHT_ROUTE_FRAMEWORK;
                det.signals.push(signal);
            }
        }
    }
}
