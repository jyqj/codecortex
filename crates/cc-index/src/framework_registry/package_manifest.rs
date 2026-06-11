//! Package-manifest detection signal (repo level).
//!
//! Owns the manifest-file checks (`package.json`, `pyproject.toml`, `go.mod`,
//! `pom.xml`/Gradle, `Cargo.toml`, `composer.json`, `Gemfile`, `*.csproj`).
//! Unlike the per-file signals this scans the project filesystem, so it feeds
//! the repo-level aggregation directly rather than the per-file registry.

use std::collections::HashMap;
use std::path::Path;

use super::WEIGHT_PACKAGE_MARKER;

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
