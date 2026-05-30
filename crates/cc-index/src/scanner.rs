//! File scanner — walks the project directory respecting ignore patterns.

use cc_model::{config::IndexingConfig, Language};
use cc_parsers::detect_language;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub language: Language,
    pub size: u64,
    pub mtime: f64,
}

pub struct Scanner {
    project_path: PathBuf,
    config: IndexingConfig,
}

impl Scanner {
    pub fn new(project_path: &Path, config: &IndexingConfig) -> Self {
        Self {
            project_path: project_path.to_path_buf(),
            config: config.clone(),
        }
    }

    /// Scan the project directory and return all indexable files.
    pub fn scan(&self) -> Vec<ScannedFile> {
        let mut builder = ignore::WalkBuilder::new(&self.project_path);
        builder.hidden(true).git_ignore(true).git_global(false);

        // Add custom ignore patterns
        let mut overrides = ignore::overrides::OverrideBuilder::new(&self.project_path);
        for pattern in &self.config.ignore {
            let neg = format!("!{}", pattern);
            if let Err(e) = overrides.add(&neg) {
                tracing::warn!(pattern = %pattern, err = %e, "skipping invalid ignore pattern");
            }
        }
        if let Ok(ovr) = overrides.build() {
            builder.overrides(ovr);
        }

        let mut files = Vec::new();
        for entry in builder.build().flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Check file size
            let metadata = match path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.len() > self.config.max_file_bytes {
                continue;
            }

            // Get relative path
            let rel_path = match path.strip_prefix(&self.project_path) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            // Detect language and check if included
            let language = detect_language(&rel_path);
            if language == Language::Unknown {
                // Check if extension matches any include pattern
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let glob_match = self.config.include.iter().any(|p| {
                    p.ends_with(&format!("*.{}", ext))
                        || p.ends_with(&format!("/{}", file_name))
                        || p.ends_with(file_name)
                });
                if !glob_match {
                    continue;
                }
            }

            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            files.push(ScannedFile {
                rel_path,
                abs_path: path.to_path_buf(),
                language,
                size: metadata.len(),
                mtime,
            });
        }

        files
    }
}
