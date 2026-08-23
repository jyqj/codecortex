//! File scanner — walks the project directory respecting ignore patterns.
//!
//! One shared walk per build: [`Scanner::scan_with_manifest`] produces both
//! the indexable file set (the scanner's historical output) and a
//! [`WalkManifest`] — the superset of files the config-linker and infra
//! passes derive their candidates and stat-signatures from. This replaces
//! three separate full-tree walks per build (scanner, config signature,
//! infra signature).

use cc_model::{config::IndexingConfig, Language};
use cc_parsers::detect_language;
use std::path::{Path, PathBuf};

/// Depth budget for hidden paths in the shared walk, matching the
/// config-linker's historical `max_depth(5)` walk (the only consumer that
/// needs hidden files such as `.env`). Non-hidden paths are walked at any
/// depth for the scanner and infra passes.
pub const CONFIG_WALK_MAX_DEPTH: usize = 5;

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub language: Language,
    pub size: u64,
    pub mtime: f64,
}

/// One file observed by the shared project walk. Carries the stat data the
/// downstream signature computations need, so they never re-stat the tree.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// Relative path, `/`-normalized.
    pub rel_path: String,
    pub size: u64,
    pub mtime: f64,
    /// Seconds-precision mtime, the unit used by stat-signature hashing.
    pub mtime_secs: u64,
    /// Path depth below the project root (number of rel-path components).
    pub depth: usize,
    /// Whether any rel-path component starts with `.`.
    pub hidden: bool,
}

/// All files seen by one shared project walk: gitignore-filtered, hidden
/// paths included up to [`CONFIG_WALK_MAX_DEPTH`], non-hidden paths at any
/// depth, with no size cap and no user `indexing.ignore` filtering (those
/// are scanner-consumer concerns; the config/infra passes historically saw
/// the unfiltered tree).
#[derive(Debug, Default)]
pub struct WalkManifest {
    pub files: Vec<WalkedFile>,
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
        self.scan_with_manifest().0
    }

    /// Build the user-config override matcher (patterns from
    /// `indexing.ignore`, added as `!pattern` so a match means "ignore").
    fn build_overrides(&self) -> Option<ignore::overrides::Override> {
        let mut overrides = ignore::overrides::OverrideBuilder::new(&self.project_path);
        for pattern in &self.config.ignore {
            let neg = format!("!{}", pattern);
            if let Err(e) = overrides.add(&neg) {
                tracing::warn!(pattern = %pattern, err = %e, "skipping invalid ignore pattern");
            }
        }
        overrides.build().ok()
    }

    /// Single shared walk: returns the indexable files plus the walk manifest
    /// for the config/infra signature consumers.
    ///
    /// Walk semantics vs the historical three walks:
    /// - gitignore-aware (`git_ignore(true)`, `git_global(false)` — global
    ///   gitignore deliberately excluded everywhere for determinism);
    /// - hidden paths are descended/yielded up to [`CONFIG_WALK_MAX_DEPTH`]
    ///   (the config-linker's historical coverage) and excluded from the
    ///   indexable set and infra candidates (their historical `hidden(true)`);
    /// - user `indexing.ignore` overrides apply to the indexable set only,
    ///   including directory pruning semantics (a matched directory excludes
    ///   its whole subtree), but not to the manifest.
    pub fn scan_with_manifest(&self) -> (Vec<ScannedFile>, WalkManifest) {
        let mut builder = ignore::WalkBuilder::new(&self.project_path);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(false);

        // Prune hidden subtrees beyond the config-linker depth budget: only
        // the config pass consumes hidden paths, and it never looked deeper.
        let root = self.project_path.clone();
        builder.filter_entry(move |entry| {
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            let has_hidden = rel.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::Normal(name)
                        if name.to_str().is_some_and(|s| s.starts_with('.'))
                )
            });
            if !has_hidden {
                return true;
            }
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            if is_dir {
                entry.depth() < CONFIG_WALK_MAX_DEPTH
            } else {
                entry.depth() <= CONFIG_WALK_MAX_DEPTH
            }
        });

        let overrides = self.build_overrides();
        // Directory prefixes (with trailing '/') matched by an ignore
        // override: their whole subtree is excluded from the indexable set,
        // mirroring walker-level override pruning.
        let mut ignored_dir_prefixes: Vec<String> = Vec::new();

        let mut indexable = Vec::new();
        let mut walk_files = Vec::new();
        for entry in builder.build().flatten() {
            let path = entry.path();

            // Get relative path
            let rel_path = match path.strip_prefix(&self.project_path) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if rel_path.is_empty() {
                continue;
            }

            if path.is_dir() {
                if let Some(ovr) = &overrides {
                    if ovr.matched(&rel_path, true).is_ignore() {
                        ignored_dir_prefixes.push(format!("{}/", rel_path));
                    }
                }
                continue;
            }
            if !path.is_file() {
                continue;
            }

            let metadata = match path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime_duration = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            let mtime = mtime_duration.map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let mtime_secs = mtime_duration.map(|d| d.as_secs()).unwrap_or(0);

            let hidden = rel_path.split('/').any(|c| c.starts_with('.'));
            let depth = rel_path.split('/').count();

            walk_files.push(WalkedFile {
                rel_path: rel_path.clone(),
                size: metadata.len(),
                mtime,
                mtime_secs,
                depth,
                hidden,
            });

            // ── Indexable-set filters (scanner semantics) ────────────────
            if hidden {
                continue;
            }
            if let Some(ovr) = &overrides {
                if ovr.matched(&rel_path, false).is_ignore()
                    || ignored_dir_prefixes
                        .iter()
                        .any(|prefix| rel_path.starts_with(prefix.as_str()))
                {
                    continue;
                }
            }
            if metadata.len() > self.config.max_file_bytes {
                continue;
            }

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

            indexable.push(ScannedFile {
                rel_path,
                abs_path: path.to_path_buf(),
                language,
                size: metadata.len(),
                mtime,
            });
        }

        (indexable, WalkManifest { files: walk_files })
    }
}

#[cfg(test)]
mod shared_walk_tests {
    use super::*;
    use cc_model::config::IndexingConfig;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// The manifest-based config/infra signatures must be value-equal to the
    /// standalone walk fallbacks for the same tree — the gates compare values
    /// recorded by either path, so a mismatch would re-run every gated pass
    /// whenever a build switches between manifest-carrying and scoped modes.
    #[test]
    fn manifest_signatures_match_walk_fallbacks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "src/lib.py", "def handler():\n    return 1\n");
        write(root, "settings.ini", "script = src/lib.py\n");
        write(root, ".env", "APP_MODULE=src.lib\n");
        write(root, "deploy/Dockerfile", "FROM alpine\n");
        write(
            root,
            "deploy/app.yaml",
            "apiVersion: v1\nkind: Service\nmetadata:\n  name: app\n",
        );

        let scanner = Scanner::new(root, &IndexingConfig::default());
        let (_, manifest) = scanner.scan_with_manifest();

        assert_eq!(
            crate::config_linker::config_files_signature_from_manifest(&manifest),
            crate::config_linker::config_files_signature(root),
            "config signature: manifest vs walk fallback"
        );
        assert_eq!(
            crate::infra_pass::infra_signature_from_manifest(root, &manifest),
            crate::infra_pass::infra_signature(root),
            "infra signature: manifest vs walk fallback"
        );
    }

    /// The manifest covers hidden paths only up to the config depth budget,
    /// keeps non-hidden paths at any depth, and the indexable set still
    /// excludes hidden files and honors `indexing.ignore` directory pruning.
    #[test]
    fn manifest_coverage_and_indexable_filters() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, ".env", "A=1\n");
        write(root, "a/b/c/d/e/f/deep.py", "def deep():\n    return 1\n");
        write(root, ".hidden/x/y/z/deep/too_deep.yaml", "k: v\n");
        write(root, "vendor/lib.py", "def vendored():\n    return 1\n");
        write(root, "src/main.py", "def main():\n    return 1\n");

        let mut config = IndexingConfig::default();
        config.ignore.push("vendor".to_string());
        let scanner = Scanner::new(root, &config);
        let (indexable, manifest) = scanner.scan_with_manifest();

        let manifest_paths: Vec<&str> =
            manifest.files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(manifest_paths.contains(&".env"), "hidden file at depth 1");
        assert!(
            manifest_paths.contains(&"a/b/c/d/e/f/deep.py"),
            "non-hidden beyond the config depth budget stays walked"
        );
        assert!(
            !manifest_paths.contains(&".hidden/x/y/z/deep/too_deep.yaml"),
            "hidden path beyond the config depth budget is pruned"
        );
        assert!(
            manifest_paths.contains(&"vendor/lib.py"),
            "user ignore patterns do not filter the manifest"
        );

        let indexable_paths: Vec<&str> =
            indexable.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(indexable_paths.contains(&"src/main.py"));
        assert!(indexable_paths.contains(&"a/b/c/d/e/f/deep.py"));
        assert!(
            !indexable_paths.contains(&".env"),
            "hidden files are not indexable"
        );
        assert!(
            !indexable_paths.contains(&"vendor/lib.py"),
            "ignored directory subtree is excluded from the indexable set"
        );
    }
}
