//! Framework detection — identifies frameworks from package files and imports.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkSignal {
    pub framework_key: String,
    pub confidence: f64,
    pub evidence: String,
}

/// Detect frameworks from project-level manifest files.
pub fn detect_repo_frameworks(project_path: &Path) -> Vec<FrameworkSignal> {
    let mut signals = Vec::new();

    // package.json (Node.js)
    if let Ok(content) = std::fs::read_to_string(project_path.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            let deps = merge_deps(&pkg);
            let checks: &[(&str, &str, f64)] = &[
                ("express", "express", 0.9),
                ("koa", "koa", 0.9),
                ("fastify", "fastify", 0.9),
                ("react", "react", 0.85),
                ("next", "next", 0.9),
                ("vue", "vue", 0.85),
                ("nuxt", "nuxt", 0.9),
                ("angular", "@angular/core", 0.85),
                ("nestjs", "@nestjs/core", 0.9),
                ("hono", "hono", 0.85),
            ];
            for (key, dep, conf) in checks {
                if deps.contains(&dep.to_string()) {
                    signals.push(FrameworkSignal {
                        framework_key: key.to_string(),
                        confidence: *conf,
                        evidence: format!("package.json dependency: {}", dep),
                    });
                }
            }
        }
    }

    // requirements.txt / pyproject.toml (Python)
    let py_files = ["requirements.txt", "pyproject.toml", "setup.py"];
    for fname in &py_files {
        if let Ok(content) = std::fs::read_to_string(project_path.join(fname)) {
            let lower = content.to_lowercase();
            let checks: &[(&str, &str, f64)] = &[
                ("django", "django", 0.9),
                ("flask", "flask", 0.85),
                ("fastapi", "fastapi", 0.9),
                ("pytest", "pytest", 0.8),
                ("sqlalchemy", "sqlalchemy", 0.8),
            ];
            for (key, dep, conf) in checks {
                if lower.contains(dep) {
                    signals.push(FrameworkSignal {
                        framework_key: key.to_string(),
                        confidence: *conf,
                        evidence: format!("{}: {}", fname, dep),
                    });
                }
            }
        }
    }

    // go.mod
    if let Ok(content) = std::fs::read_to_string(project_path.join("go.mod")) {
        let checks: &[(&str, &str, f64)] = &[
            ("gin", "github.com/gin-gonic/gin", 0.9),
            ("echo", "github.com/labstack/echo", 0.9),
            ("fiber", "github.com/gofiber/fiber", 0.9),
            ("chi", "github.com/go-chi/chi", 0.9),
            ("gorilla", "github.com/gorilla/mux", 0.9),
        ];
        for (key, dep, conf) in checks {
            if content.contains(dep) {
                signals.push(FrameworkSignal {
                    framework_key: key.to_string(),
                    confidence: *conf,
                    evidence: format!("go.mod: {}", dep),
                });
            }
        }
    }

    // pom.xml / build.gradle (Java / Spring)
    for fname in &["pom.xml", "build.gradle", "build.gradle.kts"] {
        if let Ok(content) = std::fs::read_to_string(project_path.join(fname)) {
            let checks: &[(&str, &str, f64)] = &[
                ("spring", "org.springframework", 0.9),
                ("spring", "spring-boot", 0.9),
            ];
            for (key, dep, conf) in checks {
                if content.contains(dep) {
                    signals.push(FrameworkSignal {
                        framework_key: key.to_string(),
                        confidence: *conf,
                        evidence: format!("{}: {}", fname, dep),
                    });
                }
            }
        }
    }

    // Cargo.toml (Rust)
    if let Ok(content) = std::fs::read_to_string(project_path.join("Cargo.toml")) {
        let checks: &[(&str, &str, f64)] = &[
            ("actix", "actix-web", 0.9),
            ("axum", "axum", 0.9),
            ("rocket", "rocket", 0.9),
            ("tokio", "tokio", 0.7),
        ];
        for (key, dep, conf) in checks {
            if content.contains(dep) {
                signals.push(FrameworkSignal {
                    framework_key: key.to_string(),
                    confidence: *conf,
                    evidence: format!("Cargo.toml: {}", dep),
                });
            }
        }
    }

    signals
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_node_framework() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"dependencies":{"express":"^4.18"}}"#,
        )
        .unwrap();
        let signals = detect_repo_frameworks(tmp.path());
        assert!(signals.iter().any(|s| s.framework_key == "express"));
    }

    #[test]
    fn detect_python_framework() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("requirements.txt"),
            "fastapi>=0.100\nuvicorn\n",
        )
        .unwrap();
        let signals = detect_repo_frameworks(tmp.path());
        assert!(signals.iter().any(|s| s.framework_key == "fastapi"));
    }
}
