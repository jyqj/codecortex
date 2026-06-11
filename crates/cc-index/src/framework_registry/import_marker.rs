//! Import-marker detection signal.
//!
//! Owns the framework -> import-string marker table and both scan paths that
//! consume it per file: declared imports from the `imports` table and the
//! CommonJS `require()` fallback over the file's first chunks. The table is
//! also the authoritative source for Phase 3.7 enrichment scoring in
//! `indexer.rs` (`score_import_markers`), re-exported from the module root.

use std::collections::HashMap;

use cc_db::index_db::read_chunk_text_with_encoding;

use super::{detection_entry, FileFrameworkDetection, FrameworkSignalSpec, SignalContext};

pub(super) const SPEC: FrameworkSignalSpec = FrameworkSignalSpec {
    id: "import_marker",
    detect: detect_import_markers,
};

// ---------------------------------------------------------------------------
// Import marker table  (framework_key -> list of import strings)
// ---------------------------------------------------------------------------

pub(crate) fn import_marker_table() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("express", &["express"]),
        ("fastify", &["fastify"]),
        ("react", &["react", "react-dom"]),
        (
            "nextjs",
            &["next", "next/server", "next/router", "next/navigation"],
        ),
        ("nestjs", &["@nestjs/common", "@nestjs/core"]),
        ("fastapi", &["fastapi", "starlette"]),
        ("django", &["django"]),
        ("flask", &["flask"]),
        ("koa", &["koa"]),
        ("vue", &["vue"]),
        ("angular", &["@angular/core"]),
        ("gin", &["github.com/gin-gonic/gin"]),
        ("echo", &["github.com/labstack/echo"]),
        ("fiber", &["github.com/gofiber/fiber"]),
        ("chi", &["github.com/go-chi/chi"]),
        ("gorilla", &["github.com/gorilla/mux"]),
        ("net_http", &["net/http"]),
        ("spring", &["org.springframework", "spring-boot"]),
        ("axum", &["axum"]),
        ("actix", &["actix-web", "actix_web"]),
        ("rocket", &["rocket"]),
        // --- Laravel (PHP) ---
        (
            "laravel",
            &[
                "Illuminate\\",
                "Illuminate\\Http",
                "Illuminate\\Routing",
                "Illuminate\\Support",
            ],
        ),
        // --- Rails (Ruby) ---
        (
            "rails",
            &["rails", "action_controller", "active_record", "action_view"],
        ),
        // --- ASP.NET (C#) ---
        (
            "aspnet",
            &[
                "Microsoft.AspNetCore",
                "Microsoft.AspNetCore.Mvc",
                "Microsoft.AspNetCore.Http",
            ],
        ),
        // --- SvelteKit ---
        ("sveltekit", &["@sveltejs/kit", "$app/navigation"]),
        // --- Vue Router ---
        ("vue_router", &["vue-router"]),
        // --- Nuxt ---
        ("nuxt", &["nuxt", "#app", "@nuxt/kit"]),
        // --- Remix ---
        (
            "remix",
            &[
                "@remix-run/react",
                "@remix-run/node",
                "@remix-run/cloudflare",
                "@remix-run/serve",
            ],
        ),
        // --- Hono ---
        ("hono", &["hono"]),
    ]
}

// ---------------------------------------------------------------------------
// Per-file scan
// ---------------------------------------------------------------------------

fn detect_import_markers(
    ctx: &SignalContext,
    detections: &mut HashMap<String, FileFrameworkDetection>,
) {
    // --- Declared imports from the imports table ---
    let import_strings: Vec<String> = ctx
        .conn
        .prepare_cached("SELECT import_string FROM imports WHERE file_path = ?1")
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![ctx.file_path], |row| {
                row.get::<_, String>(0)
            })
            .ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    let import_lower: Vec<String> = import_strings.iter().map(|s| s.to_lowercase()).collect();

    for &(fw_key, markers) in import_marker_table() {
        for marker in markers {
            let marker_lower = marker.to_lowercase();
            if import_lower
                .iter()
                .any(|imp| *imp == marker_lower || imp.starts_with(&format!("{}/", marker_lower)))
            {
                let det = detection_entry(detections, fw_key);
                det.confidence += super::WEIGHT_IMPORT;
                det.signals.push(format!("import:{}", marker));
                break; // one marker match per framework is enough
            }
        }
    }

    // --- CommonJS require() fallback: scan first 3 chunks for require('pkg').
    //     Runs while the shared map holds only this signal's hits (the signal
    //     registry executes import markers first), so the already-detected
    //     snapshot covers exactly the declared-import matches above. ---
    let already_detected: Vec<String> = detections.keys().cloned().collect();
    let chunk_text: String = ctx
        .conn
        .prepare_cached(
            "SELECT text, text_encoding FROM chunks WHERE file_path = ?1 ORDER BY chunk_index LIMIT 3",
        )
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![ctx.file_path], |row| {
                read_chunk_text_with_encoding(row, 0, 1)
            })
                .ok()
                .map(|rows| {
                    rows.filter_map(|r| r.ok())
                        .collect::<Vec<String>>()
                        .join(" ")
                })
        })
        .unwrap_or_default()
        .to_lowercase();

    if !chunk_text.is_empty() {
        for &(fw_key, markers) in import_marker_table() {
            if already_detected.contains(&fw_key.to_string()) {
                continue;
            }
            for marker in markers {
                let m = marker.to_lowercase();
                if chunk_text.contains(&format!("require('{}')", m))
                    || chunk_text.contains(&format!("require(\"{}\")", m))
                {
                    let det = detection_entry(detections, fw_key);
                    det.confidence += super::WEIGHT_IMPORT;
                    det.signals.push(format!("require:{}", marker));
                    break;
                }
            }
        }
    }
}
