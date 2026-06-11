//! Symbol-pattern detection signal.
//!
//! Owns the parser `framework_role` -> framework mapping plus the per-file
//! scan over the `symbols` table.

use std::collections::HashMap;

use super::{detection_entry, FileFrameworkDetection, FrameworkSignalSpec, SignalContext};

pub(super) const SPEC: FrameworkSignalSpec = FrameworkSignalSpec {
    id: "symbol_pattern",
    detect: detect_symbol_patterns,
};

// ---------------------------------------------------------------------------
// Symbol role -> framework key mapping
// ---------------------------------------------------------------------------

fn role_to_framework() -> &'static [(&'static str, &'static str)] {
    &[
        ("hook", "react"),
        ("component", "react"),
        ("controller", "nestjs"),
        ("route_handler", "nestjs"),
        ("service", "nestjs"),
        ("middleware", "express"),
        ("spring_controller", "spring"),
        ("spring_service", "spring"),
        ("spring_repository", "spring"),
        ("laravel_controller", "laravel"),
        ("rails_controller", "rails"),
        ("aspnet_controller", "aspnet"),
        ("gin_handler", "gin"),
        ("axum_handler", "axum"),
        ("actix_handler", "actix"),
        ("rocket_handler", "rocket"),
    ]
}

// ---------------------------------------------------------------------------
// Per-file scan
// ---------------------------------------------------------------------------

fn detect_symbol_patterns(
    ctx: &SignalContext,
    detections: &mut HashMap<String, FileFrameworkDetection>,
) {
    if let Ok(mut stmt) = ctx.conn.prepare_cached(
        "SELECT framework_role FROM symbols WHERE file_path = ?1 AND framework_role IS NOT NULL",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![ctx.file_path], |row| {
            row.get::<_, String>(0)
        }) {
            for role in rows.flatten() {
                for &(r, fw_key) in role_to_framework() {
                    if role == r {
                        let det = detection_entry(detections, fw_key);
                        let signal = format!("symbol_pattern:{}", role);
                        if !det.signals.contains(&signal) {
                            det.confidence += super::WEIGHT_SYMBOL_PATTERN;
                            det.signals.push(signal);
                        }
                        break;
                    }
                }
            }
        }
    }
}
