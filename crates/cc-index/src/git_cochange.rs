//! Git co-change analysis — extract file co-change correlations from git history.

use cc_model::edge::CoChangeEdgeRecord;
use cc_model::id::StableId;
use cc_model::CcResult;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

const MAX_FILES_PER_COMMIT: usize = 20;
const NOISE_PATTERNS: &[&str] = &[
    "node_modules/",
    ".git/",
    "vendor/",
    "__pycache__/",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "go.sum",
    "poetry.lock",
    "Pipfile.lock",
    ".min.js",
    ".min.css",
];

/// Analyze git history and return co-change edges.
///
/// Strategy: parse recent `git log --name-only --pretty=format:""` output to get
/// commit→files mappings, filter noisy files / mega-commits, then compute a
/// coupling coefficient for file pairs: `pair_count / min(total_a, total_b)`.
pub fn analyze_cochanges(
    project_root: &Path,
    min_cochanges: u32,
    min_confidence: f64,
    max_commits: usize,
) -> CcResult<Vec<CoChangeEdgeRecord>> {
    let output = Command::new("git")
        .args([
            "log",
            "--since=1.year.ago",
            "--name-only",
            "--diff-filter=AMR",
            "--pretty=format:",
            &format!("-{}", max_commits),
        ])
        .current_dir(project_root)
        .output()
        .map_err(cc_model::CcError::Io)?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut file_commit_count: HashMap<String, u32> = HashMap::new();
    let mut pair_count: HashMap<(String, String), u32> = HashMap::new();

    let mut current_files: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            process_commit(&current_files, &mut file_commit_count, &mut pair_count);
            current_files.clear();
            continue;
        }
        if let Some(path) = normalize_git_path(line) {
            if !current_files.iter().any(|existing| existing == &path) {
                current_files.push(path);
            }
        }
    }
    process_commit(&current_files, &mut file_commit_count, &mut pair_count);

    let mut edges = Vec::new();
    for ((file_a, file_b), count) in &pair_count {
        if *count < min_cochanges {
            continue;
        }
        let total_a = file_commit_count.get(file_a).copied().unwrap_or(1);
        let total_b = file_commit_count.get(file_b).copied().unwrap_or(1);
        let denom = total_a.min(total_b);
        let confidence = if denom > 0 {
            *count as f64 / denom as f64
        } else {
            0.0
        };

        if confidence < min_confidence {
            continue;
        }

        edges.push(CoChangeEdgeRecord {
            edge_id: StableId::cochange_edge_id(file_a, file_b),
            file_a: file_a.clone(),
            file_b: file_b.clone(),
            co_change_count: *count,
            total_commits_a: total_a,
            total_commits_b: total_b,
            confidence,
        });
    }

    edges.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.co_change_count.cmp(&a.co_change_count))
    });
    Ok(edges)
}

fn normalize_git_path(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_start_matches("./");
    if trimmed.is_empty() || is_noise_path(trimmed) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve the current git HEAD sha for `project_path`. Returns `None` when
/// git is unavailable or the directory is not a git repository, in which
/// case callers must fall back to running analysis unconditionally (never
/// permanently skip just because HEAD could not be read).
pub(crate) fn current_git_head(project_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() {
        None
    } else {
        Some(head)
    }
}

fn is_noise_path(path: &str) -> bool {
    NOISE_PATTERNS
        .iter()
        .any(|pattern| path.contains(pattern) || path.ends_with(pattern))
}

/// Process a single commit's file list: update per-file counts and pair counts.
/// Skips mega-commits (> 20 files) as they add noise.
fn process_commit(
    files: &[String],
    file_commit_count: &mut HashMap<String, u32>,
    pair_count: &mut HashMap<(String, String), u32>,
) {
    if files.is_empty() || files.len() > MAX_FILES_PER_COMMIT {
        return;
    }

    for f in files {
        *file_commit_count.entry(f.clone()).or_insert(0) += 1;
    }
    if files.len() < 2 {
        return;
    }
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            let a = &files[i];
            let b = &files[j];
            let key = if a < b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            };
            *pair_count.entry(key).or_insert(0) += 1;
        }
    }
}
