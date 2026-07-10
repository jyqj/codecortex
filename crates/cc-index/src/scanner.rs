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

impl Scanner {
    pub fn new(project_path: &Path, config: &IndexingConfig) -> Self {
        Self {
            project_path: project_path.to_path_buf(),
            config: config.clone(),
        }
    }

    /// Scan the project directory and return all indexable files.
    pub fn scan(&self) -> Vec<ScannedFile> {
        let mut files = Vec::new();
        for entry in self.walk_builder().build().flatten() {
            if let Some(scanned) = self.process_path(entry.path()) {
                files.push(scanned);
            }
        }
        files
    }

    /// Scan only `rel_paths` (project-relative, forward-slash separated),
    /// applying the exact same ignore/type/size filters as [`Scanner::scan`].
    ///
    /// The walk is pruned to the target paths' ancestor directories via
    /// `filter_entry`, so the `.gitignore` files along each path still load
    /// and apply — a gitignored, oversized, or non-indexable target is
    /// simply absent from the result, exactly as it would be from a full
    /// walk. Cost is O(targets × depth + sibling directory entries) instead
    /// of O(tree).
    pub fn scan_paths(&self, rel_paths: &[String]) -> Vec<ScannedFile> {
        if rel_paths.is_empty() {
            return Vec::new();
        }

        let targets: HashSet<PathBuf> = rel_paths
            .iter()
            .map(|rel| self.project_path.join(rel))
            .collect();
        // Every ancestor directory of every target (up to and including the
        // project root) must stay on the walk so ignore files load; every
        // other directory is pruned without descending.
        let mut keep_dirs: HashSet<PathBuf> = HashSet::new();
        keep_dirs.insert(self.project_path.clone());
        for target in &targets {
            let mut current = target.as_path();
            while let Some(parent) = current.parent() {
                if !keep_dirs.insert(parent.to_path_buf()) || parent == self.project_path {
                    break;
                }
                current = parent;
            }
        }

        let mut builder = self.walk_builder();
        let target_files = targets.clone();
        builder.filter_entry(move |entry| {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                keep_dirs.contains(entry.path())
            } else {
                target_files.contains(entry.path())
            }
        });

        let mut files = Vec::new();
        for entry in builder.build().flatten() {
            if let Some(scanned) = self.process_path(entry.path()) {
                files.push(scanned);
            }
        }
        files
    }

    /// The shared walker configuration: hidden files skipped, `.gitignore`
    /// respected, config `ignore` patterns applied as negated overrides.
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

    /// Apply the per-file checks (regular file, size cap, language/include
    /// match) and build the [`ScannedFile`]. `None` when the path is not
    /// indexable.
    fn process_path(&self, path: &Path) -> Option<ScannedFile> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanner_for(dir: &Path) -> Scanner {
        Scanner::new(dir, &IndexingConfig::default())
    }

    /// `scan_paths` must apply the identical filter stack as the full walk:
    /// gitignore, hidden dirs, and language detection all hold, and missing
    /// paths yield nothing.
    #[test]
    fn scan_paths_matches_full_walk_filters() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("gen")).unwrap();
        std::fs::write(root.join("src/keep.rs"), "pub fn keep() {}\n").unwrap();
        std::fs::write(root.join("gen/skip.rs"), "pub fn skip() {}\n").unwrap();
        std::fs::write(root.join(".gitignore"), "gen/\n").unwrap();
        // Initialize a git repo so the ignore crate honors .gitignore.
        let _ = std::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output();

        let scanner = scanner_for(root);
        let full: HashSet<String> = scanner.scan().into_iter().map(|f| f.rel_path).collect();
        assert!(full.contains("src/keep.rs"));
        assert!(!full.contains("gen/skip.rs"), "gitignored in full walk");

        let targeted: Vec<String> = scanner
            .scan_paths(&[
                "src/keep.rs".to_string(),
                "gen/skip.rs".to_string(),
                "src/missing.rs".to_string(),
            ])
            .into_iter()
            .map(|f| f.rel_path)
            .collect();
        assert_eq!(
            targeted,
            vec!["src/keep.rs".to_string()],
            "targeted scan must apply gitignore and drop missing paths"
        );
    }

    /// A targeted scan yields the same `ScannedFile` payload (size, mtime,
    /// language) as the full walk for the same file.
    #[test]
    fn scan_paths_payload_matches_full_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("main.py"), "def main():\n    pass\n").unwrap();

        let scanner = scanner_for(root);
        let full = scanner.scan();
        let full_entry = full.iter().find(|f| f.rel_path == "main.py").unwrap();
        let targeted = scanner.scan_paths(&["main.py".to_string()]);
        assert_eq!(targeted.len(), 1);
        assert_eq!(targeted[0].rel_path, full_entry.rel_path);
        assert_eq!(targeted[0].language, full_entry.language);
        assert_eq!(targeted[0].size, full_entry.size);
        assert_eq!(targeted[0].mtime, full_entry.mtime);
    }

    #[test]
    fn scan_paths_empty_input_short_circuits() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let scanner = scanner_for(tmp.path());
        assert!(scanner.scan_paths(&[]).is_empty());
    }
}
