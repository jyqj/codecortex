//! Source locks are checked before copying admitted inputs into an isolated project.
use super::{invalid, schema::*, validation, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub bytes: u64,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputManifest {
    pub files: Vec<FileRecord>,
    pub source_digest: String,
    pub query_digest: String,
    pub config_digest: String,
    pub commit: Option<String>,
    pub submodules: String,
}
pub struct LoadedSuite {
    pub suite: Suite,
    pub root: PathBuf,
    pub queries: Vec<Query>,
    pub input: InputManifest,
}
pub fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
pub fn file_digest(path: &Path) -> Result<String> {
    Ok(digest(&std::fs::read(path)?))
}
pub fn json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(s.trim_start_matches('\u{feff}'))?)
}
pub fn read_queries(path: &Path) -> Result<Vec<Query>> {
    let s = std::fs::read_to_string(path)?;
    s.trim_start_matches('\u{feff}')
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !o.status.success() {
        return Err(invalid(format!("git {} failed", args.join(" "))));
    }
    String::from_utf8(o.stdout)
        .map(|v| v.trim().to_string())
        .map_err(|_| invalid("non-UTF8 git output"))
}
pub fn git_lock(root: &Path, commit: Option<&str>) -> Result<String> {
    if let Some(expected) = commit {
        if expected.len() != 40 || !expected.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid("commit must be 40 hex digits"));
        }
        if git(root, &["rev-parse", "HEAD"])? != expected {
            return Err(invalid("repository HEAD drift"));
        }
        if !git(root, &["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
            return Err(invalid("dirty/untracked repository inputs"));
        }
        let modules = git(root, &["submodule", "status", "--recursive"])?;
        if !modules.is_empty() {
            return Err(invalid(
                "submodule corpus needs expanded source locks; not supported by P0",
            ));
        }
        Ok(modules)
    } else {
        if root.join(".git").exists() {
            return Err(invalid("Git corpus cannot use snapshot-only lock"));
        }
        Ok(String::new())
    }
}
pub fn source_bytes(root: &Path, path: &str) -> Result<Vec<u8>> {
    validation::relative_path(path)?;
    let mut p = root.to_path_buf();
    for part in path.split('/') {
        p.push(part);
        if std::fs::symlink_metadata(&p)?.file_type().is_symlink() {
            return Err(invalid("symlink input not admitted"));
        }
    }
    let meta = std::fs::metadata(&p)?;
    if !meta.is_file() || meta.len() > 1_000_000 {
        return Err(invalid("input must be a text file at most 1 MB"));
    }
    let bytes = std::fs::read(p)?;
    if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        return Err(invalid("non-UTF8 or binary source"));
    }
    if bytes.starts_with(b"version https://git-lfs.github.com/spec/") {
        return Err(invalid("unresolved LFS pointer"));
    }
    Ok(bytes)
}
pub fn inventory(root: &Path, paths: &[String]) -> Result<Vec<FileRecord>> {
    if paths.is_empty() {
        return Err(invalid("empty source manifest"));
    }
    let mut ordered = paths.to_vec();
    ordered.sort();
    let mut unique = BTreeSet::new();
    let mut files = Vec::new();
    for p in ordered {
        if !unique.insert(p.clone()) {
            return Err(invalid("duplicate admitted path"));
        }
        if p.split('/')
            .any(|c| matches!(c, ".git" | ".codecortex" | "node_modules" | "target"))
            || p == ".codecortex.json"
            || p.split('/').any(|c| c.starts_with(".env"))
        {
            return Err(invalid(format!("excluded input: {p}")));
        }
        let bytes = source_bytes(root, &p)?;
        files.push(FileRecord {
            path: p,
            bytes: bytes.len() as u64,
            digest: digest(&bytes),
        });
    }
    Ok(files)
}
fn metadata_checks(s: &Suite) -> Result<()> {
    if s.schema_version != SCHEMA_VERSION
        || s.name.is_empty()
        || !(1..=200).contains(&s.top_k)
        || !(1..=10000).contains(&s.repetitions)
        || s.warmup > 1000
        || !(1..=600000).contains(&s.timeout_ms)
    {
        return Err(invalid("suite version or bounds"));
    }
    if !s.engine_config.is_object()
        || s.engine_config
            .pointer("/auto_index/enabled")
            .and_then(|v| v.as_bool())
            != Some(false)
    {
        return Err(invalid("explicit auto_index.enabled=false required"));
    }
    let text = serde_json::to_string(&s.engine_config)?.to_lowercase();
    for key in ["\"api_key\"", "\"authorization\"", "\"password\""] {
        if text.contains(key) {
            return Err(invalid(
                "secret values are not allowed in exportable suite config",
            ));
        }
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<LoadedSuite> {
    let suite: Suite = json_file(path)?;
    metadata_checks(&suite)?;
    let base = path.parent().unwrap_or(Path::new("."));
    let root = base.join(&suite.source.root).canonicalize()?;
    let query_path = base.join(&suite.queries).canonicalize()?;
    let submodules = git_lock(&root, suite.source.commit.as_deref())?;
    let files = inventory(&root, &suite.source.files)?;
    if files.iter().any(|f| root.join(&f.path) == query_path) {
        return Err(invalid("gold corpus included in indexed source"));
    }
    let source_digest = digest(&serde_json::to_vec(&files)?);
    let query_digest = file_digest(&query_path)?;
    if source_digest != suite.source.digest || query_digest != suite.queries_digest {
        return Err(invalid("source or query content lock drift"));
    }
    let queries = read_queries(&query_path)?;
    validation::queries(&queries, suite.scoring)?;
    let paths: BTreeSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    for q in &queries {
        if q.split == "quarantine" {
            return Err(invalid("quarantined queries cannot be scored"));
        }
        for p in &q.expected_files {
            let n = files
                .iter()
                .filter(|f| super::metrics::path_matches(&f.path, p).unwrap_or(false))
                .count();
            if n == 0 {
                return Err(invalid(format!("gold pattern matches no input: {p}")));
            }
        }
        for f in &files {
            if q.expected_files
                .iter()
                .filter(|p| super::metrics::path_matches(&f.path, p).unwrap_or(false))
                .count()
                > 1
            {
                return Err(invalid("overlapping expected globs are ambiguous"));
            }
        }
        for a in q.answers.iter().flat_map(|g| &g.alternatives) {
            if !paths.contains(a.path.as_str()) {
                return Err(invalid("gold path outside admitted sources"));
            }
            if let Some(s) = &a.span {
                let b = source_bytes(&root, &a.path)?;
                let t = std::str::from_utf8(&b).map_err(|_| invalid("UTF8"))?;
                if s.end > b.len() as u64
                    || !t.is_char_boundary(s.start as usize)
                    || !t.is_char_boundary(s.end as usize)
                {
                    return Err(invalid("gold span outside source or UTF8 boundary"));
                }
            }
        }
    }
    let input = InputManifest {
        files,
        source_digest,
        query_digest,
        config_digest: digest(&serde_json::to_vec(&suite.engine_config)?),
        commit: suite.source.commit.clone(),
        submodules,
    };
    Ok(LoadedSuite {
        suite,
        root,
        queries,
        input,
    })
}
/// Explicit authoring operation; run/validate never rewrite a lock.
pub fn freeze(path: &Path) -> Result<Suite> {
    let mut s: Suite = json_file(path)?;
    metadata_checks(&s)?;
    let base = path.parent().unwrap_or(Path::new("."));
    let root = base.join(&s.source.root).canonicalize()?;
    git_lock(&root, s.source.commit.as_deref())?;
    s.source.digest = digest(&serde_json::to_vec(&inventory(&root, &s.source.files)?)?);
    s.queries_digest = file_digest(&base.join(&s.queries))?;
    validation::queries(&read_queries(&base.join(&s.queries))?, s.scoring)?;
    Ok(s)
}
pub fn materialize(s: &LoadedSuite) -> Result<tempfile::TempDir> {
    let dir = tempfile::Builder::new()
        .prefix("cc-eval-input-")
        .tempdir()?;
    for f in &s.input.files {
        let b = source_bytes(&s.root, &f.path)?;
        if digest(&b) != f.digest {
            return Err(invalid("source changed during materialization"));
        }
        let p = dir.path().join(&f.path);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, b)?;
    }
    std::fs::write(
        dir.path().join(".codecortex.json"),
        serde_json::to_vec_pretty(&s.suite.engine_config)?,
    )?;
    Ok(dir)
}
