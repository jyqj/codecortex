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
    match fw.to_lowercase().as_str() {
        "express" => Some("express"),
        "fastify" => Some("fastify"),
        "django" => Some("django"),
        "fastapi" => Some("fastapi"),
        "nestjs" => Some("nestjs"),
        "koa" => Some("koa"),
        "gin" => Some("gin"),
        "echo" => Some("echo"),
        "fiber" => Some("fiber"),
        "axum" => Some("axum"),
        "actix" => Some("actix"),
        "rocket" => Some("rocket"),
        "spring" => Some("spring"),
        "chi" => Some("chi"),
        "gorilla" => Some("gorilla"),
        "net/http" | "net_http" => Some("net_http"),
        "laravel" => Some("laravel"),
        "rails" => Some("rails"),
        "aspnet" | "asp.net" => Some("aspnet"),
        "sveltekit" => Some("sveltekit"),
        "vue_router" | "vue-router" => Some("vue_router"),
        "nuxt" => Some("nuxt"),
        "remix" => Some("remix"),
        "hono" => Some("hono"),
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
    if let Ok(mut stmt) = ctx.conn.prepare_cached(
        "SELECT DISTINCT framework FROM routes WHERE file_path = ?1 AND framework IS NOT NULL AND framework != ''",
    ) {
        if let Ok(rows) =
            stmt.query_map(rusqlite::params![ctx.file_path], |row| row.get::<_, String>(0))
        {
            for row in rows.flatten() {
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
    }
}
