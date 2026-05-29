//! Framework detection, persistence, and query layer.
//!
//! Scans the index database (imports, file paths, route_edges) to detect which
//! frameworks are used in the repo and per-file.  Results are persisted to
//! `repo_frameworks` and `file_frameworks` tables so the runtime doesn't
//! need to re-detect on every context preparation.
//!
//! Multi-signal scoring:
//!   - import marker:     +0.40
//!   - file path:         +0.30
//!   - route framework:   +0.25
//!   - symbol pattern:    +0.20
//!   - package marker:    +0.60  (package.json / pyproject / go.mod / Cargo.toml)
//!
//! A framework is considered detected when its score exceeds `DETECTION_THRESHOLD`.

use std::collections::HashMap;
use std::path::Path;

use cc_db::index_db::{
    read_chunk_text_with_encoding, FileFrameworkRecord, FileFrameworkSignal, IndexDb,
    RepoFrameworkRecord,
};
use cc_model::{CcError, CcResult};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DETECTION_THRESHOLD: f64 = 0.35;

// ---------------------------------------------------------------------------
// Signal weights
// ---------------------------------------------------------------------------

const WEIGHT_IMPORT: f64 = 0.40;
const WEIGHT_FILE_PATH: f64 = 0.30;
const WEIGHT_ROUTE_FRAMEWORK: f64 = 0.25;
const WEIGHT_SYMBOL_PATTERN: f64 = 0.20;
const WEIGHT_PACKAGE_MARKER: f64 = 0.60;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Result of framework detection for a single file or the repo.
#[derive(Debug, Clone)]
pub struct FileFrameworkDetection {
    pub framework_key: String,
    pub confidence: f64,
    pub signals: Vec<String>,
}

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
// Activation literals table (for compute_active_frameworks task text matching)
// ---------------------------------------------------------------------------

fn activation_literals_table() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        (
            "express",
            &[
                "express",
                "router",
                "middleware",
                "req",
                "res",
                "app.get",
                "app.post",
            ],
        ),
        (
            "fastify",
            &["fastify", "reply", "request.body", "server.listen"],
        ),
        (
            "react",
            &[
                "useState",
                "useEffect",
                "component",
                "jsx",
                "props",
                "render",
            ],
        ),
        (
            "nextjs",
            &[
                "nextjs",
                "next",
                "getServerSideProps",
                "getStaticProps",
                "NextRequest",
                "NextResponse",
                "app router",
            ],
        ),
        (
            "nestjs",
            &[
                "nestjs",
                "@Controller",
                "@Injectable",
                "@Module",
                "@Get",
                "@Post",
                "dependency injection",
            ],
        ),
        (
            "fastapi",
            &[
                "fastapi",
                "uvicorn",
                "Depends",
                "HTTPException",
                "pydantic",
                "BaseModel",
            ],
        ),
        (
            "django",
            &[
                "django",
                "urlpatterns",
                "HttpResponse",
                "models.Model",
                "admin.site",
            ],
        ),
        (
            "spring",
            &[
                "spring",
                "SpringBootApplication",
                "RestController",
                "RequestMapping",
                "GetMapping",
                "PostMapping",
                "Autowired",
            ],
        ),
        ("chi", &["chi", "go-chi", "chi.NewRouter", "chi.Router"]),
        (
            "gorilla",
            &["gorilla", "mux.NewRouter", "mux.Router", "gorilla/mux"],
        ),
        (
            "gin",
            &[
                "gin",
                "gin.Default",
                "gin.New",
                "gin.Context",
                "r.GET",
                "r.POST",
            ],
        ),
        (
            "echo",
            &["echo", "echo.New", "echo.Context", "e.GET", "e.POST"],
        ),
        (
            "axum",
            &[
                "axum",
                "Router::new",
                ".route(",
                "axum::extract",
                "axum::response",
            ],
        ),
        (
            "actix",
            &[
                "actix",
                "actix-web",
                "HttpServer",
                "web::get",
                "web::post",
                "web::resource",
            ],
        ),
        (
            "rocket",
            &[
                "rocket",
                "#[get",
                "#[post",
                "rocket::launch",
                "rocket::routes",
            ],
        ),
        (
            "laravel",
            &[
                "laravel",
                "Eloquent",
                "Illuminate",
                "artisan",
                "Route::get",
                "Route::post",
            ],
        ),
        (
            "rails",
            &[
                "rails",
                "ActiveRecord",
                "ActionController",
                "routes.rb",
                "resources",
            ],
        ),
        (
            "aspnet",
            &[
                "asp.net",
                "aspnet",
                "ApiController",
                "HttpGet",
                "HttpPost",
                "IActionResult",
            ],
        ),
        (
            "sveltekit",
            &[
                "sveltekit",
                "svelte",
                "+page.svelte",
                "+server",
                "load function",
            ],
        ),
        (
            "nuxt",
            &[
                "nuxt",
                "nuxt3",
                "useFetch",
                "defineNuxtConfig",
                "useAsyncData",
            ],
        ),
        (
            "remix",
            &["remix", "@remix-run", "loader", "action", "useLoaderData"],
        ),
        (
            "vue_router",
            &[
                "vue-router",
                "router",
                "useRoute",
                "useRouter",
                "createRouter",
            ],
        ),
        (
            "hono",
            &["hono", "Hono", "app.get", "app.post", "c.json", "c.text"],
        ),
    ]
}

// ---------------------------------------------------------------------------
// Route framework normalization
// ---------------------------------------------------------------------------

fn normalize_route_framework(fw: &str) -> Option<&'static str> {
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
// Per-file detection from index data
// ---------------------------------------------------------------------------

/// Detect frameworks for a single file by querying its imports, file path,
/// route_edges, and symbol patterns from the index database.
pub fn detect_file_frameworks(db: &IndexDb, file_path: &str) -> Vec<FileFrameworkDetection> {
    let conn = match db.read_conn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut detections: HashMap<String, FileFrameworkDetection> = HashMap::new();

    // --- 1. Import markers ---
    let import_strings: Vec<String> = conn
        .prepare("SELECT import_string FROM imports WHERE file_path = ?1")
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
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
                let det = detections.entry(fw_key.to_string()).or_insert_with(|| {
                    FileFrameworkDetection {
                        framework_key: fw_key.to_string(),
                        confidence: 0.0,
                        signals: Vec::new(),
                    }
                });
                det.confidence += WEIGHT_IMPORT;
                det.signals.push(format!("import:{}", marker));
                break; // one marker match per framework is enough
            }
        }
    }

    // --- 1b. CommonJS require() fallback: scan first 3 chunks for require('pkg') ---
    let already_detected: Vec<String> = detections.keys().cloned().collect();
    let chunk_text: String = conn
        .prepare(
            "SELECT text, text_encoding FROM chunks WHERE file_path = ?1 ORDER BY chunk_index LIMIT 3",
        )
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![file_path], |row| {
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
                    let det = detections.entry(fw_key.to_string()).or_insert_with(|| {
                        FileFrameworkDetection {
                            framework_key: fw_key.to_string(),
                            confidence: 0.0,
                            signals: Vec::new(),
                        }
                    });
                    det.confidence += WEIGHT_IMPORT;
                    det.signals.push(format!("require:{}", marker));
                    break;
                }
            }
        }
    }

    // --- 2. File path conventions ---
    for &(fw_key, patterns) in file_path_pattern_table() {
        for pattern in patterns {
            if file_path.contains(pattern) {
                let det = detections.entry(fw_key.to_string()).or_insert_with(|| {
                    FileFrameworkDetection {
                        framework_key: fw_key.to_string(),
                        confidence: 0.0,
                        signals: Vec::new(),
                    }
                });
                let signal = format!("file_path:{}", pattern);
                if !det.signals.contains(&signal) {
                    det.confidence += WEIGHT_FILE_PATH;
                    det.signals.push(signal);
                }
                break;
            }
        }
    }

    // --- 3. Route framework signal from route_edges ---
    if let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT framework FROM route_edges WHERE file_path = ?1 AND framework IS NOT NULL AND framework != ''",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
        {
            for row in rows.flatten() {
                if let Some(fw_key) = normalize_route_framework(&row) {
                    let det = detections
                        .entry(fw_key.to_string())
                        .or_insert_with(|| FileFrameworkDetection {
                            framework_key: fw_key.to_string(),
                            confidence: 0.0,
                            signals: Vec::new(),
                        });
                    let signal = format!("route_framework:{}", row);
                    if !det.signals.contains(&signal) {
                        det.confidence += WEIGHT_ROUTE_FRAMEWORK;
                        det.signals.push(signal);
                    }
                }
            }
        }
    }

    // --- 4. Symbol pattern detection (framework_role from parser) ---
    if let Ok(mut stmt) = conn.prepare(
        "SELECT framework_role FROM symbols WHERE file_path = ?1 AND framework_role IS NOT NULL",
    ) {
        if let Ok(rows) =
            stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
        {
            for role in rows.flatten() {
                for &(r, fw_key) in role_to_framework() {
                    if role == r {
                        let det = detections.entry(fw_key.to_string()).or_insert_with(|| {
                            FileFrameworkDetection {
                                framework_key: fw_key.to_string(),
                                confidence: 0.0,
                                signals: Vec::new(),
                            }
                        });
                        let signal = format!("symbol_pattern:{}", role);
                        if !det.signals.contains(&signal) {
                            det.confidence += WEIGHT_SYMBOL_PATTERN;
                            det.signals.push(signal);
                        }
                        break;
                    }
                }
            }
        }
    }

    detections
        .into_values()
        .filter(|d| d.confidence >= DETECTION_THRESHOLD)
        .collect()
}

// ---------------------------------------------------------------------------
// Package manifest checking
// ---------------------------------------------------------------------------

/// Check package manifest files for framework dependencies.
///
/// Returns `framework_key -> confidence` for each detected framework.
pub fn check_package_markers(project_path: &Path) -> HashMap<String, f64> {
    let mut results: HashMap<String, f64> = HashMap::new();

    // --- package.json (Node.js) ---
    if let Ok(content) = std::fs::read_to_string(project_path.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            let deps = merge_deps(&pkg);
            let checks: &[(&str, &str)] = &[
                ("express", "express"),
                ("fastify", "fastify"),
                ("react", "react"),
                ("nextjs", "next"),
                ("nestjs", "@nestjs/core"),
                ("nestjs", "@nestjs/common"),
                ("koa", "koa"),
                ("vue", "vue"),
                ("angular", "@angular/core"),
                ("hono", "hono"),
                ("sveltekit", "@sveltejs/kit"),
                ("vue_router", "vue-router"),
                ("nuxt", "nuxt"),
                ("remix", "@remix-run/react"),
                ("remix", "@remix-run/node"),
            ];
            for &(fw_key, dep) in checks {
                if deps.contains(&dep.to_string()) {
                    let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                    *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
                }
            }
        }
    }

    // --- pyproject.toml / requirements.txt / setup.py (Python) ---
    let py_files = ["pyproject.toml", "requirements.txt", "setup.py"];
    for fname in &py_files {
        if let Ok(content) = std::fs::read_to_string(project_path.join(fname)) {
            let lower = content.to_lowercase();
            let checks: &[(&str, &str)] = &[
                ("fastapi", "fastapi"),
                ("django", "django"),
                ("flask", "flask"),
            ];
            for &(fw_key, dep) in checks {
                if lower.contains(dep) {
                    let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                    *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
                }
            }
        }
    }

    // --- go.mod (Go) ---
    if let Ok(content) = std::fs::read_to_string(project_path.join("go.mod")) {
        let checks: &[(&str, &str)] = &[
            ("gin", "github.com/gin-gonic/gin"),
            ("echo", "github.com/labstack/echo"),
            ("fiber", "github.com/gofiber/fiber"),
            ("chi", "github.com/go-chi/chi"),
            ("gorilla", "github.com/gorilla/mux"),
        ];
        for &(fw_key, dep) in checks {
            if content.contains(dep) {
                let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
            }
        }
    }

    // --- pom.xml / build.gradle (Java / Spring) ---
    for fname in &["pom.xml", "build.gradle", "build.gradle.kts"] {
        if let Ok(content) = std::fs::read_to_string(project_path.join(fname)) {
            let checks: &[(&str, &str)] =
                &[("spring", "org.springframework"), ("spring", "spring-boot")];
            for &(fw_key, dep) in checks {
                if content.contains(dep) {
                    let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                    *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
                }
            }
        }
    }

    // --- Cargo.toml (Rust) ---
    if let Ok(content) = std::fs::read_to_string(project_path.join("Cargo.toml")) {
        let checks: &[(&str, &str)] = &[
            ("actix", "actix-web"),
            ("axum", "axum"),
            ("rocket", "rocket"),
        ];
        for &(fw_key, dep) in checks {
            if content.contains(dep) {
                let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
            }
        }
    }

    // --- composer.json (PHP / Laravel) ---
    if let Ok(content) = std::fs::read_to_string(project_path.join("composer.json")) {
        let lower = content.to_lowercase();
        let checks: &[(&str, &str)] =
            &[("laravel", "laravel/framework"), ("laravel", "illuminate/")];
        for &(fw_key, dep) in checks {
            if lower.contains(dep) {
                let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
            }
        }
    }

    // --- Gemfile (Ruby / Rails) ---
    if let Ok(content) = std::fs::read_to_string(project_path.join("Gemfile")) {
        let checks: &[(&str, &str)] = &[("rails", "rails")];
        for &(fw_key, dep) in checks {
            if content.contains(dep) {
                let entry = results.entry(fw_key.to_string()).or_insert(0.0);
                *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
            }
        }
    }

    // --- *.csproj (ASP.NET / C#) ---
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("csproj") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if content.contains("Microsoft.AspNetCore")
                        || content.contains("Microsoft.NET.Sdk.Web")
                    {
                        let entry = results.entry("aspnet".to_string()).or_insert(0.0);
                        *entry = (*entry + WEIGHT_PACKAGE_MARKER).min(0.95);
                    }
                }
            }
        }
    }

    results
}

fn merge_deps(pkg: &serde_json::Value) -> Vec<String> {
    let mut deps = Vec::new();
    for key in &["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = pkg.get(key).and_then(|v| v.as_object()) {
            deps.extend(obj.keys().cloned());
        }
    }
    deps
}

// ---------------------------------------------------------------------------
// Repo-level detection
// ---------------------------------------------------------------------------

/// Aggregate per-file detections + repo-level signals into a repo framework set.
pub fn detect_repo_frameworks(db: &IndexDb, project_path: &Path) -> Vec<FileFrameworkDetection> {
    let conn = match db.read_conn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut repo_scores: HashMap<String, FileFrameworkDetection> = HashMap::new();

    // 1. Aggregate from file_frameworks already persisted
    if let Ok(mut stmt) = conn.prepare(
        "SELECT framework_key, COUNT(*) as cnt, MAX(confidence) as max_conf \
         FROM file_frameworks GROUP BY framework_key",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                let (fw_key, count, max_conf) = row;
                // Repo confidence: max file confidence + breadth bonus
                let breadth_bonus = (count as f64 * 0.05).min(0.25);
                let confidence = (max_conf + breadth_bonus).min(0.95);
                repo_scores.insert(
                    fw_key.clone(),
                    FileFrameworkDetection {
                        framework_key: fw_key,
                        confidence,
                        signals: vec![
                            format!("file_count:{}", count),
                            format!("max_file_conf:{:.2}", max_conf),
                        ],
                    },
                );
            }
        }
    }

    // 2. Route-edge framework signal
    if let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT framework FROM route_edges WHERE framework IS NOT NULL AND framework != ''",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for fw in rows.flatten() {
                if let Some(fw_key) = normalize_route_framework(&fw) {
                    let det = repo_scores
                        .entry(fw_key.to_string())
                        .or_insert_with(|| FileFrameworkDetection {
                            framework_key: fw_key.to_string(),
                            confidence: 0.0,
                            signals: Vec::new(),
                        });
                    det.confidence = (det.confidence + WEIGHT_ROUTE_FRAMEWORK).min(0.95);
                    det.signals.push(format!("route_framework:{}", fw));
                }
            }
        }
    }

    // 3. Package marker signal from filesystem
    let pkg_markers = check_package_markers(project_path);
    for (fw_key, pkg_conf) in pkg_markers {
        let det = repo_scores
            .entry(fw_key.clone())
            .or_insert_with(|| FileFrameworkDetection {
                framework_key: fw_key,
                confidence: 0.0,
                signals: Vec::new(),
            });
        det.confidence = (det.confidence + pkg_conf).min(0.95);
        det.signals.push("package_marker".to_string());
    }

    repo_scores
        .into_values()
        .filter(|d| d.confidence >= DETECTION_THRESHOLD)
        .collect()
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Run full framework detection and persist to `repo_frameworks` + `file_frameworks`.
///
/// Called after indexing completes.
pub fn detect_and_persist_frameworks(db: &IndexDb, project_path: &Path) -> CcResult<()> {
    // 1. Gather all file paths
    let file_paths: Vec<String> = {
        let conn = db.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT file_path FROM files")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // 2. Per-file detection
    let mut file_records: Vec<FileFrameworkRecord> = Vec::new();
    for fp in &file_paths {
        let detections = detect_file_frameworks(db, fp);
        if !detections.is_empty() {
            let signals: Vec<FileFrameworkSignal> = detections
                .into_iter()
                .map(|d| {
                    let evidence = d.signals.join(", ");
                    (d.framework_key, d.confidence, evidence)
                })
                .collect();
            file_records.push((fp.clone(), signals));
        }
    }

    // 3. Persist file frameworks
    db.replace_file_frameworks(&file_records)?;

    // 4. Repo-level aggregation (depends on file_frameworks being written)
    refresh_repo_frameworks(db, project_path)?;

    Ok(())
}

/// Recompute repo-level frameworks from persisted file detections and package markers.
pub fn refresh_repo_frameworks(db: &IndexDb, project_path: &Path) -> CcResult<()> {
    let repo_detections = detect_repo_frameworks(db, project_path);
    let repo_records: Vec<RepoFrameworkRecord> = repo_detections
        .into_iter()
        .map(|d| (d.framework_key, d.confidence, d.signals))
        .collect();

    db.replace_repo_frameworks(&repo_records)
}

/// Incremental framework detection: only re-scan changed files.
///
/// For < 20 changed files: only re-detect those files.
/// For >= 20 changed files: full repo rescan.
pub fn detect_and_persist_frameworks_incremental(
    db: &IndexDb,
    project_path: &Path,
    changed_files: &[&str],
) -> CcResult<()> {
    if changed_files.is_empty() {
        return Ok(());
    }

    if changed_files.len() >= 20 {
        // Large changeset: full rescan
        return detect_and_persist_frameworks(db, project_path);
    }

    // Small changeset: per-file incremental update
    let mut file_records: Vec<FileFrameworkRecord> = Vec::new();
    for &fp in changed_files {
        // Check the file still exists in the index
        let exists = {
            let conn = db.read_conn()?;
            conn.query_row(
                "SELECT 1 FROM files WHERE file_path = ?1",
                rusqlite::params![fp],
                |_| Ok(()),
            )
            .is_ok()
        };
        if !exists {
            continue;
        }

        let detections = detect_file_frameworks(db, fp);
        let signals: Vec<FileFrameworkSignal> = detections
            .into_iter()
            .map(|d| {
                let evidence = d.signals.join(", ");
                (d.framework_key, d.confidence, evidence)
            })
            .collect();
        file_records.push((fp.to_string(), signals));
    }

    db.replace_file_frameworks(&file_records)?;

    // Repo-level aggregation is cheap because it reads the persisted
    // file_frameworks table plus package markers. Keep it current even for
    // small increments; otherwise a fresh incremental build can leave
    // repo_frameworks empty.
    refresh_repo_frameworks(db, project_path)
}

// ---------------------------------------------------------------------------
// Query helpers (used by runtime)
// ---------------------------------------------------------------------------

/// Return all detected repo-level frameworks as `(framework_key, confidence)`.
pub fn get_repo_frameworks(db: &IndexDb) -> Vec<(String, f64)> {
    db.list_repo_frameworks().unwrap_or_default()
}

/// Return `{file_path: [(framework_key, confidence), ...]}` for a set of files.
pub fn get_frameworks_for_files(
    db: &IndexDb,
    file_paths: &[&str],
) -> HashMap<String, Vec<(String, f64)>> {
    if file_paths.is_empty() {
        return HashMap::new();
    }
    let conn = match db.read_conn() {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let placeholders: String = file_paths.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT file_path, framework_key, confidence FROM file_frameworks \
         WHERE file_path IN ({}) ORDER BY confidence DESC",
        placeholders
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
        .iter()
        .map(|p| p as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = match stmt.query_map(params.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };

    let mut result: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for row in rows.flatten() {
        let (fp, fw_key, conf) = row;
        result.entry(fp).or_default().push((fw_key, conf));
    }
    result
}

/// Return just the framework keys detected at repo level.
pub fn get_repo_framework_keys(db: &IndexDb) -> Vec<String> {
    get_repo_frameworks(db)
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

// ---------------------------------------------------------------------------
// Active framework computation
// ---------------------------------------------------------------------------

/// Compute a ranked list of active frameworks from multi-signal evidence.
///
/// Signals are bucketed into 3 tiers:
///   - Strong (0.4): framework detected in `working_files`
///   - Medium (0.1): framework detected in `evidence_files`
///   - Weak   (0.01): framework in repo but not in working/evidence files
///   - Task text activation: check activation_literals
///
/// Returns `(framework_key, score)` sorted by score descending.
pub fn compute_active_frameworks(
    db: &IndexDb,
    task_text: &str,
    working_files: &[&str],
    evidence_files: &[&str],
) -> Vec<(String, f64)> {
    struct Accum {
        strong: u32,
        medium: u32,
        weak: u32,
    }

    let mut accum: HashMap<String, Accum> = HashMap::new();

    let mut bump = |fw_key: &str, tier: &str| {
        let acc = accum.entry(fw_key.to_string()).or_insert(Accum {
            strong: 0,
            medium: 0,
            weak: 0,
        });
        match tier {
            "strong" => acc.strong += 1,
            "medium" => acc.medium += 1,
            _ => acc.weak += 1,
        }
    };

    // --- Strong: working_files ---
    if !working_files.is_empty() {
        let fw_map = get_frameworks_for_files(db, working_files);
        for fw_list in fw_map.values() {
            for (fw_key, _conf) in fw_list {
                bump(fw_key, "strong");
            }
        }
    }

    // --- Medium: evidence_files ---
    if !evidence_files.is_empty() {
        let fw_map = get_frameworks_for_files(db, evidence_files);
        for fw_list in fw_map.values() {
            for (fw_key, _conf) in fw_list {
                bump(fw_key, "medium");
            }
        }
    }

    // --- Medium: task text activation literals (needs >= 2 matches) ---
    if !task_text.is_empty() {
        let task_lower = task_text.to_lowercase();
        for &(fw_key, literals) in activation_literals_table() {
            let match_count = literals
                .iter()
                .filter(|lit| task_lower.contains(&lit.to_lowercase()))
                .count();
            if match_count >= 2 {
                bump(fw_key, "medium");
            }
        }
    }

    // --- Weak: repo baseline ---
    for fw_key in get_repo_framework_keys(db) {
        bump(&fw_key, "weak");
    }

    // --- Scoring & filtering ---
    let mut results: Vec<(String, f64)> = Vec::new();
    for (fw_key, acc) in &accum {
        if acc.strong == 0 && acc.medium == 0 {
            continue; // pure weak — not worth pushing
        }
        let score = acc.strong as f64 * 0.4 + acc.medium as f64 * 0.1 + acc.weak as f64 * 0.01;
        results.push((fw_key.clone(), score));
    }

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a minimal IndexDb with the schema.
    fn setup_test_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test_index.sqlite3");
        let db = IndexDb::open(&db_path).unwrap().0;
        (tmp, db)
    }

    /// Helper: insert a file + import records into the test DB for detection.
    fn insert_test_file(db: &IndexDb, file_path: &str, imports: &[&str]) {
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
             VALUES(?1, 'typescript', 'abc', 0.0, 100, '2025-01-01')",
            rusqlite::params![file_path],
        )
        .unwrap();
        for imp in imports {
            conn.execute(
                "INSERT INTO imports(file_path, import_string) VALUES(?1, ?2)",
                rusqlite::params![file_path, imp],
            )
            .unwrap();
        }
    }

    #[test]
    fn test_detect_file_frameworks_import_signal() {
        let (_tmp, db) = setup_test_db();
        insert_test_file(&db, "src/app.ts", &["express", "cors"]);

        let detections = detect_file_frameworks(&db, "src/app.ts");
        assert!(
            detections.iter().any(|d| d.framework_key == "express"),
            "should detect express from import"
        );
        let express_det = detections
            .iter()
            .find(|d| d.framework_key == "express")
            .unwrap();
        assert!(
            express_det.confidence >= WEIGHT_IMPORT - 0.001,
            "express confidence should be at least WEIGHT_IMPORT"
        );
        assert!(
            express_det.signals.iter().any(|s| s.starts_with("import:")),
            "should have an import signal"
        );
    }

    #[test]
    fn test_detect_file_frameworks_file_path_signal() {
        let (_tmp, db) = setup_test_db();
        // Next.js path pattern + next import
        insert_test_file(&db, "app/api/users/route.ts", &["next/server"]);

        let detections = detect_file_frameworks(&db, "app/api/users/route.ts");
        assert!(
            detections.iter().any(|d| d.framework_key == "nextjs"),
            "should detect nextjs from import + file path"
        );
        let nextjs_det = detections
            .iter()
            .find(|d| d.framework_key == "nextjs")
            .unwrap();
        // Should have both import and file_path signals
        assert!(
            nextjs_det.confidence >= WEIGHT_IMPORT + WEIGHT_FILE_PATH - 0.001,
            "nextjs confidence should combine import + file_path weights, got {}",
            nextjs_det.confidence
        );
    }

    #[test]
    fn test_check_package_markers_node() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"dependencies":{"express":"^4.18","cors":"^2.8"}}"#,
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("express"),
            "should detect express from package.json"
        );
        assert!(
            *markers.get("express").unwrap() >= WEIGHT_PACKAGE_MARKER - 0.001,
            "express package marker confidence should be at least WEIGHT_PACKAGE_MARKER"
        );
    }

    #[test]
    fn test_check_package_markers_python() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pyproject.toml"),
            "[project]\ndependencies = [\"fastapi>=0.100\", \"uvicorn\"]\n",
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("fastapi"),
            "should detect fastapi from pyproject.toml"
        );
    }

    #[test]
    fn test_check_package_markers_go() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("go.mod"),
            "module example.com/myapp\nrequire github.com/gin-gonic/gin v1.9.0\n",
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(markers.contains_key("gin"), "should detect gin from go.mod");
    }

    #[test]
    fn test_compute_active_frameworks_scoring() {
        let (_tmp, db) = setup_test_db();

        // Insert files with framework associations
        insert_test_file(&db, "src/app.ts", &["express"]);
        insert_test_file(&db, "src/utils.ts", &[]);
        insert_test_file(&db, "src/routes.ts", &["express"]);

        // Persist file frameworks first
        let detections1 = detect_file_frameworks(&db, "src/app.ts");
        let detections2 = detect_file_frameworks(&db, "src/routes.ts");
        let mut file_records: Vec<FileFrameworkRecord> = Vec::new();
        for (fp, dets) in [("src/app.ts", detections1), ("src/routes.ts", detections2)] {
            if !dets.is_empty() {
                let signals: Vec<FileFrameworkSignal> = dets
                    .into_iter()
                    .map(|d| (d.framework_key, d.confidence, d.signals.join(", ")))
                    .collect();
                file_records.push((fp.to_string(), signals));
            }
        }
        db.replace_file_frameworks(&file_records).unwrap();

        // Repo level
        let repo_records: Vec<RepoFrameworkRecord> =
            vec![("express".to_string(), 0.9, vec!["file_count:2".to_string()])];
        db.replace_repo_frameworks(&repo_records).unwrap();

        // Compute active frameworks
        let active = compute_active_frameworks(
            &db,
            "fix the express middleware bug",
            &["src/app.ts"],
            &["src/routes.ts"],
        );

        assert!(
            !active.is_empty(),
            "should find at least one active framework"
        );
        assert_eq!(
            active[0].0, "express",
            "express should be the top active framework"
        );
        // strong(1 file) + medium(1 evidence + task_text) + weak(repo baseline)
        assert!(
            active[0].1 > 0.1,
            "express score should be significant, got {}",
            active[0].1
        );
    }

    #[test]
    fn test_below_threshold_not_detected() {
        let (_tmp, db) = setup_test_db();
        // File with no matching imports
        insert_test_file(&db, "src/plain.ts", &["lodash", "uuid"]);

        let detections = detect_file_frameworks(&db, "src/plain.ts");
        assert!(
            detections.is_empty(),
            "no framework should be detected for generic utility imports"
        );
    }

    #[test]
    fn test_detect_new_frameworks_import_signal() {
        let (_tmp, db) = setup_test_db();

        // Laravel detection via import
        insert_test_file(
            &db,
            "app/Http/Controllers/UserController.php",
            &["Illuminate\\Http"],
        );
        let dets = detect_file_frameworks(&db, "app/Http/Controllers/UserController.php");
        assert!(
            dets.iter().any(|d| d.framework_key == "laravel"),
            "should detect laravel from Illuminate import + file path, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );

        // SvelteKit detection via import
        insert_test_file(&db, "src/routes/+page.svelte", &["@sveltejs/kit"]);
        let dets = detect_file_frameworks(&db, "src/routes/+page.svelte");
        assert!(
            dets.iter().any(|d| d.framework_key == "sveltekit"),
            "should detect sveltekit from import + file path, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );

        // Remix detection via import
        insert_test_file(&db, "app/routes/index.tsx", &["@remix-run/react"]);
        let dets = detect_file_frameworks(&db, "app/routes/index.tsx");
        assert!(
            dets.iter().any(|d| d.framework_key == "remix"),
            "should detect remix from import + file path, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_check_package_markers_laravel() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("composer.json"),
            r#"{"require":{"laravel/framework":"^10.0","php":"^8.1"}}"#,
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("laravel"),
            "should detect laravel from composer.json"
        );
    }

    #[test]
    fn test_check_package_markers_rails() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Gemfile"),
            "source 'https://rubygems.org'\ngem 'rails', '~> 7.0'\ngem 'puma'\n",
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("rails"),
            "should detect rails from Gemfile"
        );
    }

    #[test]
    fn test_check_package_markers_sveltekit() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"devDependencies":{"@sveltejs/kit":"^2.0","svelte":"^4.0"}}"#,
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("sveltekit"),
            "should detect sveltekit from package.json"
        );
    }

    #[test]
    fn test_normalize_route_framework_new_entries() {
        assert_eq!(normalize_route_framework("laravel"), Some("laravel"));
        assert_eq!(normalize_route_framework("rails"), Some("rails"));
        assert_eq!(normalize_route_framework("aspnet"), Some("aspnet"));
        assert_eq!(normalize_route_framework("asp.net"), Some("aspnet"));
        assert_eq!(normalize_route_framework("sveltekit"), Some("sveltekit"));
        assert_eq!(normalize_route_framework("nuxt"), Some("nuxt"));
        assert_eq!(normalize_route_framework("remix"), Some("remix"));
        assert_eq!(normalize_route_framework("hono"), Some("hono"));
        assert_eq!(normalize_route_framework("vue_router"), Some("vue_router"));
        assert_eq!(normalize_route_framework("vue-router"), Some("vue_router"));
    }
}
