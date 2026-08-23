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

    pub(crate) fn project_path(&self) -> &Path {
        &self.project_path
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
    ///
    /// The filesystem half (readdir + stat + gitignore evaluation) runs on
    /// the `ignore` crate's parallel walker; the entries are then sorted by
    /// rel path and classified sequentially with the exact single-walk
    /// semantics. Path order guarantees a directory sorts before everything
    /// in its subtree (a proper path prefix orders first), so the override
    /// directory-prune state is complete before any contained file is
    /// classified — same invariant the sequential walk relied on.
    pub fn scan_with_manifest(&self) -> (Vec<ScannedFile>, WalkManifest) {
        // ── Parallel filesystem half ─────────────────────────────────────
        struct RawEntry {
            rel_path: String,
            abs_path: PathBuf,
            is_dir: bool,
            size: u64,
            mtime: f64,
            mtime_secs: u64,
        }

        let mut builder = ignore::WalkBuilder::new(&self.project_path);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .threads(
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
                    .min(12),
            );

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

        let (tx, rx) = std::sync::mpsc::channel::<RawEntry>();
        let project_path = self.project_path.clone();
        builder.build_parallel().run(|| {
            let tx = tx.clone();
            let project_path = project_path.clone();
            Box::new(move |result| {
                let Ok(entry) = result else {
                    return ignore::WalkState::Continue;
                };
                let path = entry.path();
                let rel_path = match path.strip_prefix(&project_path) {
                    Ok(r) => r.to_string_lossy().replace('\\', "/"),
                    Err(_) => return ignore::WalkState::Continue,
                };
                if rel_path.is_empty() {
                    return ignore::WalkState::Continue;
                }

                // Symlinks resolve through follow-metadata, matching the
                // sequential walk's `path.is_dir()` / `path.is_file()`;
                // regular entries trust the dirent type and pay one stat
                // (files only, for size/mtime).
                let file_type = entry.file_type();
                let needs_follow = file_type.is_none_or(|t| t.is_symlink());
                let (is_dir, metadata) = if needs_follow {
                    match std::fs::metadata(path) {
                        Ok(m) if m.is_dir() => (true, None),
                        Ok(m) if m.is_file() => (false, Some(m)),
                        _ => return ignore::WalkState::Continue,
                    }
                } else if file_type.is_some_and(|t| t.is_dir()) {
                    (true, None)
                } else {
                    match path.metadata() {
                        Ok(m) if m.is_file() => (false, Some(m)),
                        _ => return ignore::WalkState::Continue,
                    }
                };

                let (size, mtime, mtime_secs) = match &metadata {
                    Some(m) => {
                        let mtime_duration = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                        (
                            m.len(),
                            mtime_duration.map(|d| d.as_secs_f64()).unwrap_or(0.0),
                            mtime_duration.map(|d| d.as_secs()).unwrap_or(0),
                        )
                    }
                    None => (0, 0.0, 0),
                };
                let _ = tx.send(RawEntry {
                    rel_path,
                    abs_path: path.to_path_buf(),
                    is_dir,
                    size,
                    mtime,
                    mtime_secs,
                });
                ignore::WalkState::Continue
            })
        });
        drop(tx);
        let mut entries: Vec<RawEntry> = rx.try_iter().collect();
        entries.sort_unstable_by(|a, b| a.rel_path.cmp(&b.rel_path));

        // ── Sequential classification half (order-dependent state) ──────
        let overrides = self.build_overrides();
        // Directory prefixes (with trailing '/') matched by an ignore
        // override: their whole subtree is excluded from the indexable set,
        // mirroring walker-level override pruning.
        let mut ignored_dir_prefixes: Vec<String> = Vec::new();

        let mut indexable = Vec::new();
        let mut walk_files = Vec::new();
        for entry in entries {
            let rel_path = entry.rel_path;

            if entry.is_dir {
                if let Some(ovr) = &overrides {
                    if ovr.matched(&rel_path, true).is_ignore() {
                        ignored_dir_prefixes.push(format!("{rel_path}/"));
                    }
                }
                continue;
            }

            let hidden = rel_path.split('/').any(|c| c.starts_with('.'));
            let depth = rel_path.split('/').count();

            walk_files.push(WalkedFile {
                rel_path: rel_path.clone(),
                size: entry.size,
                mtime: entry.mtime,
                mtime_secs: entry.mtime_secs,
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
            if entry.size > self.config.max_file_bytes {
                continue;
            }

            // Detect language and check if included
            let language = detect_language(&rel_path);
            if language == Language::Unknown {
                // Check if extension matches any include pattern
                let ext = entry
                    .abs_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let file_name = entry
                    .abs_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
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
                abs_path: entry.abs_path,
                language,
                size: entry.size,
                mtime: entry.mtime,
            });
        }

        (indexable, WalkManifest { files: walk_files })
    }

    /// Event-scoped scan: stat only the given rel paths (plus the contents
    /// of any that are directories), applying the exact same admission rules
    /// as the full tree walk. Implemented as ONE root-anchored walk whose
    /// `filter_entry` only descends the ancestor directories of the requested
    /// paths — so every gitignore rule (root, nested, `.git/info/exclude`) is
    /// evaluated at the same directory level as the full walk, including
    /// rules that prune a requested path's ancestor. Cost is O(entries of
    /// traversed directories), not O(tree). Paths missing on disk or failing
    /// admission are simply absent from the result (the caller treats them
    /// as removals against the DB state).
    pub fn scan_paths(&self, rel_paths: &[String]) -> Vec<ScannedFile> {
        let overrides = self.build_overrides();

        // Admission pre-filter shared with the tree walk: hidden components
        // and user-ignore matches (file itself or any ancestor directory,
        // mirroring walker-level directory pruning).
        let admissible = |rel: &str| -> bool {
            if rel.is_empty() || rel.split('/').any(|c| c.starts_with('.')) {
                return false;
            }
            if let Some(ovr) = &overrides {
                if ovr.matched(rel, false).is_ignore() {
                    return false;
                }
                let mut prefix = String::new();
                let components: Vec<&str> = rel.split('/').collect();
                for dir in &components[..components.len().saturating_sub(1)] {
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(dir);
                    if ovr.matched(&prefix, true).is_ignore() {
                        return false;
                    }
                }
            }
            true
        };

        // Requested files, requested directory subtrees (dir-level watcher
        // events, e.g. a folder move), and the ancestor-dir chain the walker
        // needs to descend to reach them.
        let mut want_files: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut subtree_prefixes: Vec<String> = Vec::new();
        let mut descend_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for raw in rel_paths {
            let rel = raw.trim_end_matches('/');
            if !admissible(rel) {
                continue;
            }
            if self.project_path.join(rel).is_dir() {
                subtree_prefixes.push(format!("{rel}/"));
                descend_dirs.insert(rel.to_string());
            } else {
                want_files.insert(rel.to_string());
            }
            let components: Vec<&str> = rel.split('/').collect();
            let mut prefix = String::new();
            for dir in &components[..components.len().saturating_sub(1)] {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(dir);
                descend_dirs.insert(prefix.clone());
            }
        }
        if want_files.is_empty() && subtree_prefixes.is_empty() {
            return Vec::new();
        }

        let mut builder = ignore::WalkBuilder::new(&self.project_path);
        builder.hidden(true).git_ignore(true).git_global(false);

        let root = self.project_path.clone();
        let want_files_filter = want_files;
        let subtree_prefixes_filter = subtree_prefixes;
        let descend_dirs_filter = descend_dirs;
        builder.filter_entry(move |entry| {
            let rel = match entry.path().strip_prefix(&root) {
                Ok(r) => r,
                Err(_) => return false,
            };
            if rel.as_os_str().is_empty() {
                return true;
            }
            let rel = rel.to_string_lossy().replace('\\', "/");
            let under_subtree = subtree_prefixes_filter
                .iter()
                .any(|prefix| rel.starts_with(prefix.as_str()));
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                under_subtree || descend_dirs_filter.contains(&rel)
            } else {
                under_subtree || want_files_filter.contains(&rel)
            }
        });

        let mut out = Vec::new();
        for entry in builder.build().flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let rel = match path.strip_prefix(&self.project_path) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            // User-override dir-prune parity for files inside requested
            // subtrees (requested paths were pre-checked above).
            if !admissible(&rel) {
                continue;
            }
            self.admit_scanned(path, &mut out);
        }
        out
    }

    /// Final admission for one on-disk file: size cap + language/include
    /// filter (identical to the tree-walk indexable filters); pushes the
    /// resulting [`ScannedFile`].
    fn admit_scanned(&self, path: &Path, out: &mut Vec<ScannedFile>) {
        let rel_path = match path.strip_prefix(&self.project_path) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };
        let metadata = match path.metadata() {
            Ok(m) => m,
            Err(_) => return,
        };
        if metadata.len() > self.config.max_file_bytes {
            return;
        }
        let language = detect_language(&rel_path);
        if language == Language::Unknown {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let glob_match = self.config.include.iter().any(|p| {
                p.ends_with(&format!("*.{ext}"))
                    || p.ends_with(&format!("/{file_name}"))
                    || p.ends_with(file_name)
            });
            if !glob_match {
                return;
            }
        }
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        out.push(ScannedFile {
            rel_path,
            abs_path: path.to_path_buf(),
            language,
            size: metadata.len(),
            mtime,
        });
    }
}

#[cfg(test)]
mod scoped_scan_tests {
    use super::*;
    use cc_model::config::IndexingConfig;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// `scan_paths` must agree with the full tree walk on admission for every
    /// requested path: gitignore at any ancestor level (root and nested),
    /// user `indexing.ignore` directory pruning, the size cap, and the
    /// language filter. This is the membership contract event-scoped builds
    /// rely on — drift here would make a scoped build admit (or drop) a file
    /// the next full walk classifies differently.
    #[test]
    fn scoped_scan_matches_full_walk_admission() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // The ignore crate only applies gitignore rules inside a git repo.
        std::fs::create_dir(root.join(".git")).unwrap();
        write(root, ".gitignore", "logs/\n");
        write(root, "src/main.py", "def main():\n    return 1\n");
        write(root, "src/nested/.gitignore", "secret.py\n");
        write(root, "src/nested/secret.py", "def secret():\n    return 2\n");
        write(root, "src/nested/open.py", "def open_fn():\n    return 3\n");
        // Ancestor directory gitignored at the ROOT level: a walk rooted at
        // `logs/` would never see the pruning rule — the regression this
        // test pins.
        write(root, "logs/app.py", "def logged():\n    return 4\n");
        write(root, "vendor/lib.py", "def vendored():\n    return 5\n");
        write(root, "notes.xyz", "not a source language\n");
        write(root, "big.py", &"# padding\n".repeat(64));

        let mut config = IndexingConfig::default();
        config.ignore.push("vendor".to_string());
        config.max_file_bytes = 128;
        let scanner = Scanner::new(root, &config);

        let full: std::collections::HashSet<String> = scanner
            .scan()
            .into_iter()
            .map(|f| f.rel_path)
            .collect();

        let requested: Vec<String> = [
            "src/main.py",
            "src/nested/secret.py",
            "src/nested/open.py",
            "logs/app.py",
            "vendor/lib.py",
            "notes.xyz",
            "big.py",
            "missing.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let scoped: std::collections::HashSet<String> = scanner
            .scan_paths(&requested)
            .into_iter()
            .map(|f| f.rel_path)
            .collect();

        let expected: std::collections::HashSet<String> = requested
            .iter()
            .filter(|p| full.contains(p.as_str()))
            .cloned()
            .collect();
        assert_eq!(
            scoped, expected,
            "scoped admission must equal full-walk admission on the requested set"
        );
        assert!(scoped.contains("src/main.py"));
        assert!(scoped.contains("src/nested/open.py"));
        assert!(
            !scoped.contains("logs/app.py"),
            "root-level gitignore of an ancestor dir must prune the scoped scan"
        );
        assert!(
            !scoped.contains("src/nested/secret.py"),
            "nested .gitignore must apply to the scoped scan"
        );
        assert!(!scoped.contains("vendor/lib.py"), "user ignore dir prune");
        assert!(!scoped.contains("notes.xyz"), "language/include filter");
        assert!(!scoped.contains("big.py"), "size cap");
    }

    /// A requested directory expands to its admissible subtree, matching the
    /// full walk's membership under that prefix; stat fields agree with the
    /// full walk for the same file.
    #[test]
    fn scoped_scan_expands_directory_events() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        write(root, "src/a.py", "def a():\n    return 1\n");
        write(root, "src/sub/b.py", "def b():\n    return 2\n");
        write(root, "src/.gitignore", "skipped.py\n");
        write(root, "src/skipped.py", "def s():\n    return 3\n");
        write(root, "other/c.py", "def c():\n    return 4\n");

        let scanner = Scanner::new(root, &IndexingConfig::default());
        let full: Vec<ScannedFile> = scanner.scan();
        let full_under_src: std::collections::HashSet<String> = full
            .iter()
            .filter(|f| f.rel_path.starts_with("src/"))
            .map(|f| f.rel_path.clone())
            .collect();

        let scoped = scanner.scan_paths(&["src".to_string()]);
        let scoped_paths: std::collections::HashSet<String> =
            scoped.iter().map(|f| f.rel_path.clone()).collect();
        assert_eq!(
            scoped_paths, full_under_src,
            "directory event must expand to the full walk's membership under the prefix"
        );
        assert!(!scoped_paths.contains("other/c.py"));

        let scoped_a = scoped.iter().find(|f| f.rel_path == "src/a.py").unwrap();
        let full_a = full.iter().find(|f| f.rel_path == "src/a.py").unwrap();
        assert_eq!(scoped_a.size, full_a.size);
        assert_eq!(scoped_a.language, full_a.language);
        assert_eq!(scoped_a.mtime, full_a.mtime);
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
