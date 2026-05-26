//! Config linker — detect references to code symbols in configuration files.
//!
//! Scans .env, .yaml, .toml, .json, Dockerfile, docker-compose.yml etc.
//! for patterns that look like code references (module paths, class names,
//! file paths, and dependency-style imports).

use cc_model::CcResult;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A link from a config file to a code symbol.
#[derive(Debug, Clone)]
pub struct ConfigLink {
    pub config_file: String,
    pub config_key: String,
    pub referenced_value: String,
    pub line: u32,
    pub link_kind: ConfigLinkKind,
    pub confidence: f64,
}

/// The kind of reference detected in a config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLinkKind {
    /// e.g., "myapp.handlers.auth" in Django settings
    ModulePath,
    /// e.g., "./src/handlers/auth.ts" in tsconfig paths
    FilePath,
    /// e.g., package / dependency names that line up with imported modules
    DependencyImport,
}

impl ConfigLinkKind {
    pub fn strategy_name(self) -> &'static str {
        match self {
            Self::ModulePath => "config_module",
            Self::FilePath => "config_file",
            Self::DependencyImport => "config_dependency",
        }
    }
}

const DEP_MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "requirements.txt",
    "requirements-dev.txt",
    "Pipfile",
    "Pipfile.lock",
    "poetry.lock",
    "pyproject.toml",
    "go.mod",
];

/// Scan config files and extract links to known code symbols and files.
pub fn extract_config_links(
    project_root: &Path,
    known_symbols: &HashSet<String>,
    known_files: &HashSet<String>,
) -> CcResult<Vec<ConfigLink>> {
    let symbol_suffixes = build_symbol_suffix_index(known_symbols);
    let file_basenames = build_file_basename_index(known_files);
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    for entry in ignore::WalkBuilder::new(project_root)
        .hidden(false)
        .max_depth(Some(5))
        .build()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() || !is_config_path(path) {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel_path = path.strip_prefix(project_root).unwrap_or(path);
        let rel_string = rel_path.to_string_lossy().to_string();
        let is_dependency_manifest = is_dependency_manifest_path(path);

        for (line_num, line) in content.lines().enumerate() {
            let line_no = (line_num + 1) as u32;
            let config_key = extract_key_from_line(line);

            for word in extract_dotted_paths(line) {
                if let Some(confidence) = module_confidence(&word, known_symbols, &symbol_suffixes)
                {
                    push_link(
                        &mut links,
                        &mut seen,
                        ConfigLink {
                            config_file: rel_string.clone(),
                            config_key: config_key.clone(),
                            referenced_value: word,
                            line: line_no,
                            link_kind: ConfigLinkKind::ModulePath,
                            confidence,
                        },
                    );
                }
            }

            for word in extract_file_like_tokens(line) {
                if let Some((resolved, confidence)) =
                    resolve_known_file_token(&word, known_files, &file_basenames)
                {
                    push_link(
                        &mut links,
                        &mut seen,
                        ConfigLink {
                            config_file: rel_string.clone(),
                            config_key: config_key.clone(),
                            referenced_value: resolved,
                            line: line_no,
                            link_kind: ConfigLinkKind::FilePath,
                            confidence,
                        },
                    );
                }
            }

            if is_dependency_manifest {
                for dep in extract_dependency_specs(line) {
                    if let Some(confidence) = dependency_confidence(
                        &dep,
                        known_symbols,
                        &symbol_suffixes,
                        &file_basenames,
                    ) {
                        push_link(
                            &mut links,
                            &mut seen,
                            ConfigLink {
                                config_file: rel_string.clone(),
                                config_key: config_key.clone(),
                                referenced_value: dep,
                                line: line_no,
                                link_kind: ConfigLinkKind::DependencyImport,
                                confidence,
                            },
                        );
                    }
                }
            }
        }
    }

    Ok(links)
}

fn push_link(links: &mut Vec<ConfigLink>, seen: &mut HashSet<String>, link: ConfigLink) {
    let key = format!(
        "{}:{}:{}:{}",
        link.config_file,
        link.line,
        link.link_kind.strategy_name(),
        link.referenced_value
    );
    if seen.insert(key) {
        links.push(link);
    }
}

fn is_config_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(
        ext,
        "yaml" | "yml" | "toml" | "json" | "env" | "cfg" | "ini"
    ) || filename.starts_with("Dockerfile")
        || filename.starts_with("docker-compose")
        || filename.starts_with(".env")
        || DEP_MANIFESTS.contains(&filename)
}

fn is_dependency_manifest_path(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    DEP_MANIFESTS.contains(&filename)
}

fn build_symbol_suffix_index(known_symbols: &HashSet<String>) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for symbol in known_symbols {
        let parts: Vec<&str> = symbol.split('.').collect();
        for idx in 0..parts.len() {
            let suffix = parts[idx..].join(".");
            *out.entry(suffix).or_insert(0) += 1;
        }
    }
    out
}

fn build_file_basename_index(known_files: &HashSet<String>) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for file in known_files {
        if let Some(base) = Path::new(file).file_name().and_then(|n| n.to_str()) {
            out.entry(base.to_string()).or_default().push(file.clone());
        }
    }
    out
}

fn module_confidence(
    value: &str,
    known_symbols: &HashSet<String>,
    suffixes: &HashMap<String, usize>,
) -> Option<f64> {
    if known_symbols.contains(value) {
        return Some(0.85);
    }
    match suffixes.get(value) {
        Some(1) => Some(0.75),
        _ => None,
    }
}

fn resolve_known_file_token(
    value: &str,
    known_files: &HashSet<String>,
    basenames: &HashMap<String, Vec<String>>,
) -> Option<(String, f64)> {
    let normalized = normalize_file_token(value);
    if normalized.is_empty() {
        return None;
    }
    if known_files.contains(&normalized) {
        return Some((normalized, 0.90));
    }
    let base = Path::new(&normalized)
        .file_name()
        .and_then(|n| n.to_str())?;
    match basenames.get(base) {
        Some(paths) if paths.len() == 1 => Some((paths[0].clone(), 0.70)),
        _ => None,
    }
}

fn dependency_confidence(
    value: &str,
    known_symbols: &HashSet<String>,
    suffixes: &HashMap<String, usize>,
    file_basenames: &HashMap<String, Vec<String>>,
) -> Option<f64> {
    let normalized = normalize_dependency_token(value);
    if normalized.is_empty() {
        return None;
    }
    if known_symbols.contains(&normalized)
        || suffixes.get(&normalized).copied().unwrap_or(0) == 1
        || file_basenames.contains_key(&normalized)
    {
        return Some(0.95);
    }

    let dotted = normalized.replace('/', ".").replace('-', "_");
    if suffixes.get(&dotted).copied().unwrap_or(0) >= 1 {
        return Some(0.80);
    }

    let leaf = dotted.rsplit('.').next().unwrap_or(&dotted);
    if file_basenames.contains_key(leaf) || suffixes.get(leaf).copied().unwrap_or(0) >= 1 {
        return Some(0.80);
    }

    None
}

/// Extract dotted paths from a line (e.g., "myapp.handlers.auth").
fn extract_dotted_paths(line: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            let start = i;
            let mut has_dot = false;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                if chars[i] == '.' {
                    has_dot = true;
                }
                i += 1;
            }
            if has_dot {
                let path: String = chars[start..i].iter().collect();
                let path = path.trim_end_matches('.');
                if path.contains('.') {
                    paths.push(path.to_string());
                }
            }
        } else {
            i += 1;
        }
    }
    paths
}

/// Extract file-path-like strings from a line.
fn extract_file_like_tokens(line: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for word in line.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | ',' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
            )
    }) {
        let cleaned = normalize_file_token(word);
        if cleaned.contains('/') && !cleaned.is_empty() {
            paths.push(cleaned);
        }
    }
    paths
}

fn normalize_file_token(word: &str) -> String {
    word.trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`'))
        .trim_start_matches("./")
        .trim_end_matches([',', ';'])
        .to_string()
}

fn normalize_dependency_token(word: &str) -> String {
    let trimmed = word
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`'))
        .trim_end_matches(',')
        .trim_end_matches(';');
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return String::new();
    }
    let spec = trimmed
        .split(['=', '<', '>', '~', '^', ' '])
        .next()
        .unwrap_or(trimmed)
        .trim();
    spec.trim_start_matches("./").to_string()
}

fn extract_dependency_specs(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in line.split(|c: char| {
        c.is_whitespace() || matches!(c, ',' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}')
    }) {
        let spec = normalize_dependency_token(token);
        if spec.len() >= 2 && !spec.contains('/') && !spec.starts_with('{') {
            out.push(spec);
        }
    }
    out
}

/// Extract the key portion from a config line (before = or :).
fn extract_key_from_line(line: &str) -> String {
    let line = line.trim();
    if let Some(pos) = line.find('=') {
        line[..pos].trim().to_string()
    } else if let Some(pos) = line.find(':') {
        line[..pos].trim().to_string()
    } else {
        line.chars().take(40).collect()
    }
}
