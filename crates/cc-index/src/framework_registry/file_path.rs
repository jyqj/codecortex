//! File-path-convention detection signal.
//!
//! Owns the framework -> path-substring pattern table and the per-file path
//! match. Purely string-based: the only input is the file path itself.

use std::collections::HashMap;

use super::{detection_entry, FileFrameworkDetection, FrameworkSignalSpec, SignalContext};

pub(super) const SPEC: FrameworkSignalSpec = FrameworkSignalSpec {
    id: "file_path",
    detect: detect_file_path_patterns,
};

// ---------------------------------------------------------------------------
// File path pattern table  (framework_key -> path substrings)
// ---------------------------------------------------------------------------

fn file_path_pattern_table() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("nextjs", &["app/", "pages/", "next.config"]),
        ("django", &["urls.py", "views.py", "models.py", "admin.py"]),
        ("flask", &["templates/", "static/"]),
        ("angular", &[".component.ts", ".module.ts", ".service.ts"]),
        ("vue", &[".vue"]),
        (
            "spring",
            &[
                "Application.java",
                "Controller.java",
                "Service.java",
                "Repository.java",
            ],
        ),
        // --- Laravel ---
        (
            "laravel",
            &[
                "app/Http/Controllers",
                "routes/web.php",
                "routes/api.php",
                "app/Providers",
                "resources/views",
            ],
        ),
        // --- Rails ---
        (
            "rails",
            &[
                "app/controllers/",
                "config/routes.rb",
                "app/models/",
                "app/views/",
            ],
        ),
        // --- ASP.NET ---
        (
            "aspnet",
            &[
                "Controllers/",
                "Program.cs",
                "Startup.cs",
                "appsettings.json",
            ],
        ),
        // --- SvelteKit ---
        (
            "sveltekit",
            &[
                "src/routes/+page.svelte",
                "src/routes/+layout.svelte",
                "src/routes/+server.ts",
                "svelte.config",
            ],
        ),
        // --- Nuxt ---
        ("nuxt", &["nuxt.config", "pages/", "composables/"]),
        // --- Remix ---
        ("remix", &["app/routes/", "app/root.tsx", "app/root.jsx"]),
    ]
}

// ---------------------------------------------------------------------------
// Per-file scan
// ---------------------------------------------------------------------------

fn detect_file_path_patterns(
    ctx: &SignalContext,
    detections: &mut HashMap<String, FileFrameworkDetection>,
) {
    for &(fw_key, patterns) in file_path_pattern_table() {
        for pattern in patterns {
            if ctx.file_path.contains(pattern) {
                let det = detection_entry(detections, fw_key);
                let signal = format!("file_path:{}", pattern);
                if !det.signals.contains(&signal) {
                    det.confidence += super::WEIGHT_FILE_PATH;
                    det.signals.push(signal);
                }
                break;
            }
        }
    }
}
