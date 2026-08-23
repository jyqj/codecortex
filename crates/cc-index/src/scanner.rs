//! File scanner — walks the project directory respecting ignore patterns.

use cc_model::{config::IndexingConfig, Language};
use cc_parsers::detect_language;
use std::collections::HashSet;
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

/// Event-path ignore filter aligned with the scan walk, for event-driven
/// callers (the file watcher) that judge one path at a time instead of
/// walking directories.
///
/// Mirrors the two configurable walk filters per path:
/// - the root `.gitignore` (only when the project is a git repository, same
///   as the walker's `require_git` gate). The walker additionally honors
///   nested `.gitignore` files; a path only a nested file would ignore can
///   pass here — harmless, because every admitted path still goes through
///   the scan walk, which drops it.
/// - `.codecortex.json` `indexing.ignore` patterns, built with exactly the
///   negated-override construction [`Scanner`]'s walker uses, checked
///   against the path and its ancestor directories (the walker prunes
///   ignored directories during descent).
///
/// So: `is_ignored == true` is authoritative (the walk would filter it);
/// `false` is advisory (the walk may still filter it for other reasons).
pub struct IgnoreRules {
    gitignore: Option<ignore::gitignore::Gitignore>,
    overrides: Option<ignore::overrides::Override>,
}

impl IgnoreRules {
    pub fn load(project_path: &Path, config: &IndexingConfig) -> Self {
        // The walker applies .gitignore only inside a git repository
        // (`require_git` default); match that so non-git projects are not
        // over-filtered by a stray .gitignore file.
        let gitignore = if project_path.join(".git").exists() {
            let mut builder = ignore::gitignore::GitignoreBuilder::new(project_path);
            builder.add(project_path.join(".gitignore"));
            builder.build().ok()
        } else {
            None
        };

        let mut overrides = ignore::overrides::OverrideBuilder::new(project_path);
        for pattern in &config.ignore {
            let neg = format!("!{}", pattern);
            if let Err(e) = overrides.add(&neg) {
                tracing::warn!(pattern = %pattern, err = %e, "skipping invalid ignore pattern");
            }
        }
        let overrides = overrides.build().ok().filter(|o| !o.is_empty());

        Self {
            gitignore,
            overrides,
        }
    }

    /// Whether the scan walk would filter this relative file path via
    /// gitignore or configured ignore patterns.
    pub fn is_ignored(&self, rel_path: &str) -> bool {
        if let Some(gitignore) = &self.gitignore {
            if gitignore
                .matched_path_or_any_parents(rel_path, false)
                .is_ignore()
            {
                return true;
            }
        }
        if let Some(overrides) = &self.overrides {
            if overrides.matched(rel_path, false).is_ignore() {
                return true;
            }
            // Directory patterns ("gen/", "vendored/**"): the walker prunes
            // the directory itself, so check the path's ancestors as dirs.
            let mut ancestor = rel_path;
            while let Some(pos) = ancestor.rfind('/') {
                ancestor = &ancestor[..pos];
                if overrides.matched(ancestor, true).is_ignore() {
                    return true;
                }
            }
        }
        false
    }
}

impl Scanner {
    pub fn new(project_path: &Path, config: &IndexingConfig) -> Self {
        Self {
            project_path: project_path.to_path_buf(),
            config: config.clone(),
        }
    }

    /// The configured walker shared by [`Scanner::scan`] and
    /// [`Scanner::scan_paths`]: gitignore-aware, hidden-file-skipping, with
    /// the `.codecortex.json` ignore patterns applied as negated overrides.
    fn walk_builder(&self) -> ignore::WalkBuilder {
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
        builder
    }

    /// Per-entry admission shared by both scan flavors: size cap, language
    /// detection, and the include-pattern rescue for unknown languages.
    /// Returns `None` for entries that must not be indexed.
    fn admit(&self, path: &Path) -> Option<ScannedFile> {
        if !path.is_file() {
            return None;
        }

        // Check file size
        let metadata = path.metadata().ok()?;
        if metadata.len() > self.config.max_file_bytes {
            return None;
        }

        // Get relative path
        let rel_path = path
            .strip_prefix(&self.project_path)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");

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
                return None;
            }
        }

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        Some(ScannedFile {
            rel_path,
            abs_path: path.to_path_buf(),
            language,
            size: metadata.len(),
            mtime,
        })
    }

    /// Scan the project directory and return all indexable files.
    pub fn scan(&self) -> Vec<ScannedFile> {
        let mut files = Vec::new();
        for entry in self.walk_builder().build().flatten() {
            if let Some(file) = self.admit(entry.path()) {
                files.push(file);
            }
        }
        files
    }

    /// Scoped scan: return the indexable subset of `rel_paths` with exactly
    /// the same admission rules as [`Scanner::scan`].
    ///
    /// Fidelity is guaranteed by construction: the same `WalkBuilder`
    /// (gitignore chain, hidden filter, config overrides) drives the
    /// traversal, restricted via `filter_entry` to the candidate paths and
    /// their ancestor directories. A path yielded here is a path the full
    /// scan would also yield; a path the full scan would filter (gitignored,
    /// hidden, oversized, unknown language without include rescue, vanished)
    /// is filtered here too. Cost scales with the candidate set's directory
    /// spine, not the tree size.
    pub fn scan_paths(&self, rel_paths: &HashSet<String>) -> Vec<ScannedFile> {
        if rel_paths.is_empty() {
            return Vec::new();
        }

        // Candidate files plus every ancestor directory (the walker must be
        // allowed to descend to them). The project root is included so the
        // walk's root entry always passes the filter.
        let mut allowed: HashSet<PathBuf> = HashSet::new();
        allowed.insert(self.project_path.clone());
        for rel in rel_paths {
            let abs = self.project_path.join(rel);
            for ancestor in abs.ancestors() {
                if !allowed.insert(ancestor.to_path_buf()) {
                    break;
                }
                if ancestor == self.project_path {
                    break;
                }
            }
        }

        let mut builder = self.walk_builder();
        builder.filter_entry(move |entry| allowed.contains(entry.path()));

        let mut files = Vec::new();
        for entry in builder.build().flatten() {
            // Ancestor directories pass the filter but are not files;
            // `admit` drops them.
            if let Some(file) = self.admit(entry.path()) {
                files.push(file);
            }
        }
        files
    }
}

#[cfg(test)]
mod scoped_scan_tests {
    use super::*;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn rel_set(files: &[ScannedFile]) -> Vec<&str> {
        let mut v: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        v.sort();
        v
    }

    /// The scoped scan must admit exactly the full scan's verdict for every
    /// candidate path: plain files pass, gitignored / config-ignored /
    /// vanished ones do not.
    #[test]
    fn scan_paths_matches_full_scan_admission() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, ".gitignore", "gen/\n");
        write(root, "src/a.py", "def a():\n    return 1\n");
        write(root, "src/deep/b.py", "def b():\n    return 2\n");
        write(root, "gen/generated.py", "def g():\n    return 3\n");
        write(root, "vendored/c.py", "def c():\n    return 4\n");
        // git context so the ignore crate honors .gitignore
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap();

        let config = IndexingConfig {
            ignore: vec!["vendored/**".to_string()],
            ..IndexingConfig::default()
        };
        let scanner = Scanner::new(root, &config);

        let full = scanner.scan();
        let full_rels = rel_set(&full);
        assert!(full_rels.contains(&"src/a.py"));
        assert!(!full_rels.contains(&"gen/generated.py"), "gitignored");
        assert!(!full_rels.contains(&"vendored/c.py"), "config-ignored");

        let scope: HashSet<String> = [
            "src/a.py",
            "src/deep/b.py",
            "gen/generated.py",
            "vendored/c.py",
            "src/missing.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let scoped = scanner.scan_paths(&scope);
        assert_eq!(
            rel_set(&scoped),
            vec!["src/a.py", "src/deep/b.py"],
            "scoped admission must match the full scan verdict per path"
        );

        // Every scoped hit must be byte-identical to its full-scan twin.
        for file in &scoped {
            let twin = full
                .iter()
                .find(|f| f.rel_path == file.rel_path)
                .expect("scoped hit exists in full scan");
            assert_eq!(file.size, twin.size);
            assert_eq!(file.language, twin.language);
        }
    }

    #[test]
    fn scan_paths_empty_scope_scans_nothing() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "src/a.py", "def a():\n    return 1\n");
        let scanner = Scanner::new(tmp.path(), &IndexingConfig::default());
        assert!(scanner.scan_paths(&HashSet::new()).is_empty());
    }

    /// Alignment contract for [`IgnoreRules`] (the watcher's event filter):
    /// `is_ignored == true` must imply the scan walk filters the path too —
    /// dropping such an event can never cause index staleness — and paths
    /// the walk admits must never be reported as ignored.
    #[test]
    fn ignore_rules_align_with_scan_walk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, ".gitignore", "gen/\n*.log.py\n");
        write(root, "src/a.py", "def a():\n    return 1\n");
        write(root, "gen/generated.py", "def g():\n    return 3\n");
        write(root, "src/trace.log.py", "def t():\n    return 4\n");
        write(root, "vendored/c.py", "def c():\n    return 5\n");
        write(root, "vendored/deep/d.py", "def d():\n    return 6\n");
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap();

        let config = IndexingConfig {
            ignore: vec!["vendored/**".to_string()],
            ..IndexingConfig::default()
        };
        let rules = IgnoreRules::load(root, &config);
        let scanned: HashSet<String> = Scanner::new(root, &config)
            .scan()
            .into_iter()
            .map(|f| f.rel_path)
            .collect();

        for path in [
            "src/a.py",
            "gen/generated.py",
            "src/trace.log.py",
            "vendored/c.py",
            "vendored/deep/d.py",
        ] {
            if rules.is_ignored(path) {
                assert!(
                    !scanned.contains(path),
                    "{path}: is_ignored must be authoritative — the walk admitted it"
                );
            }
            if scanned.contains(path) {
                assert!(
                    !rules.is_ignored(path),
                    "{path}: walk-admitted paths must not be reported ignored"
                );
            }
        }
        // The interesting verdicts, pinned explicitly.
        assert!(!rules.is_ignored("src/a.py"));
        assert!(rules.is_ignored("gen/generated.py"), "gitignored dir");
        assert!(rules.is_ignored("src/trace.log.py"), "gitignored glob");
        assert!(rules.is_ignored("vendored/c.py"), "config-ignored");
        assert!(
            rules.is_ignored("vendored/deep/d.py"),
            "config-ignored via ancestor dir"
        );
    }

    /// Without a git repository the walker does not honor .gitignore, and
    /// neither must the rules (over-filtering would starve the watcher).
    #[test]
    fn ignore_rules_skip_gitignore_outside_git_repos() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".gitignore", "gen/\n");
        write(tmp.path(), "gen/generated.py", "def g():\n    return 1\n");
        let config = IndexingConfig::default();
        let rules = IgnoreRules::load(tmp.path(), &config);
        assert!(
            !rules.is_ignored("gen/generated.py"),
            "no git repo: .gitignore must not apply (walker parity)"
        );
        let scanned: Vec<String> = Scanner::new(tmp.path(), &config)
            .scan()
            .into_iter()
            .map(|f| f.rel_path)
            .collect();
        assert!(
            scanned.contains(&"gen/generated.py".to_string()),
            "walker must admit the file without a git repo"
        );
    }
}
