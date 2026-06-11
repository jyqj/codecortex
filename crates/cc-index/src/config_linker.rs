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

/// 配置文件中提取出的原始候选 token（未经 catalog 过滤）。
/// 扫描（walk + 读文件 + 逐行切词）与解析（对照符号/文件 catalog 过滤）解耦：
/// 原始 token 只依赖配置文件内容，可按配置文件集签名缓存；catalog 变化时
/// 只需对缓存的 token 重新 resolve，而不必重扫整个项目。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawConfigToken {
    pub config_file: String,
    pub config_key: String,
    pub value: String,
    pub line: u32,
    pub kind: RawTokenKind,
}

/// 原始 token 的提取通道，决定 resolve 阶段使用哪种置信度判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RawTokenKind {
    /// `extract_dotted_paths` 产出（候选 ModulePath）。
    Dotted,
    /// `extract_file_like_tokens` 产出（候选 FilePath）。
    FileLike,
    /// `extract_dependency_specs` 产出，仅依赖清单文件（候选 DependencyImport）。
    DependencySpec,
}

/// 扫描半程：walk 项目、读取每个配置文件并逐行提取候选 token。
/// 与 catalog 无关，输出顺序与原 `extract_config_links` 的链接产出顺序一致
/// （逐文件 → 逐行 → dotted → file-like → dependency）。
pub fn scan_config_tokens(project_root: &Path) -> CcResult<Vec<RawConfigToken>> {
    let mut tokens = Vec::new();

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
                tokens.push(RawConfigToken {
                    config_file: rel_string.clone(),
                    config_key: config_key.clone(),
                    value: word,
                    line: line_no,
                    kind: RawTokenKind::Dotted,
                });
            }

            for word in extract_file_like_tokens(line) {
                tokens.push(RawConfigToken {
                    config_file: rel_string.clone(),
                    config_key: config_key.clone(),
                    value: word,
                    line: line_no,
                    kind: RawTokenKind::FileLike,
                });
            }

            if is_dependency_manifest {
                for dep in extract_dependency_specs(line) {
                    tokens.push(RawConfigToken {
                        config_file: rel_string.clone(),
                        config_key: config_key.clone(),
                        value: dep,
                        line: line_no,
                        kind: RawTokenKind::DependencySpec,
                    });
                }
            }
        }
    }

    Ok(tokens)
}

/// 解析半程：对照当前符号/文件 catalog 过滤原始 token，产出与原
/// `extract_config_links` 完全一致的链接（含去重与置信度）。
pub fn resolve_config_links(
    tokens: &[RawConfigToken],
    known_symbols: &HashSet<String>,
    known_files: &HashSet<String>,
) -> Vec<ConfigLink> {
    let symbol_suffixes = build_symbol_suffix_index(known_symbols);
    let file_basenames = build_file_basename_index(known_files);
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    for token in tokens {
        match token.kind {
            RawTokenKind::Dotted => {
                if let Some(confidence) =
                    module_confidence(&token.value, known_symbols, &symbol_suffixes)
                {
                    push_link(
                        &mut links,
                        &mut seen,
                        ConfigLink {
                            config_file: token.config_file.clone(),
                            config_key: token.config_key.clone(),
                            referenced_value: token.value.clone(),
                            line: token.line,
                            link_kind: ConfigLinkKind::ModulePath,
                            confidence,
                        },
                    );
                }
            }
            RawTokenKind::FileLike => {
                if let Some((resolved, confidence)) =
                    resolve_known_file_token(&token.value, known_files, &file_basenames)
                {
                    push_link(
                        &mut links,
                        &mut seen,
                        ConfigLink {
                            config_file: token.config_file.clone(),
                            config_key: token.config_key.clone(),
                            referenced_value: resolved,
                            line: token.line,
                            link_kind: ConfigLinkKind::FilePath,
                            confidence,
                        },
                    );
                }
            }
            RawTokenKind::DependencySpec => {
                if let Some(confidence) = dependency_confidence(
                    &token.value,
                    known_symbols,
                    &symbol_suffixes,
                    &file_basenames,
                ) {
                    push_link(
                        &mut links,
                        &mut seen,
                        ConfigLink {
                            config_file: token.config_file.clone(),
                            config_key: token.config_key.clone(),
                            referenced_value: token.value.clone(),
                            line: token.line,
                            link_kind: ConfigLinkKind::DependencyImport,
                            confidence,
                        },
                    );
                }
            }
        }
    }

    links
}

/// 配置文件集签名：与 `infra_pass::infra_signature` 同款契约 —— 候选路径 +
/// mtime + size 的稳定哈希。walker 参数与 `scan_config_tokens` 完全一致，
/// 任何配置文件的增删改都会改变签名并强制重扫。
pub fn config_files_signature(project_root: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut candidates: Vec<(String, std::path::PathBuf)> = Vec::new();
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
        let rel = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        candidates.push((rel, path.to_path_buf()));
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = DefaultHasher::new();
    candidates.len().hash(&mut hasher);
    for (rel_path, abs_path) in &candidates {
        rel_path.hash(&mut hasher);
        if let Ok(metadata) = std::fs::metadata(abs_path) {
            metadata.len().hash(&mut hasher);
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            mtime.hash(&mut hasher);
        }
    }
    hasher.finish()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// scan + resolve 两段式必须复现原单段提取的链接：模块路径、文件路径、
    /// 依赖清单三个通道都要命中，且 token 经 serde 往返后 resolve 结果不变
    /// （这正是 metadata 缓存路径的正确性前提）。
    #[test]
    fn scan_then_resolve_links_all_three_channels_and_survives_serde() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("app.yaml"),
            "handler: myapp.handlers.auth\nentry: src/handlers/auth.ts\n",
        )
        .unwrap();
        std::fs::write(root.join("requirements.txt"), "authlib==1.0\n").unwrap();

        let known_symbols: HashSet<String> =
            ["myapp.handlers.auth".to_string(), "authlib".to_string()]
                .into_iter()
                .collect();
        let known_files: HashSet<String> =
            ["src/handlers/auth.ts".to_string()].into_iter().collect();

        let tokens = scan_config_tokens(root).unwrap();
        let links = resolve_config_links(&tokens, &known_symbols, &known_files);

        let find = |kind: ConfigLinkKind, value: &str| {
            links
                .iter()
                .find(|l| l.link_kind == kind && l.referenced_value == value)
        };
        assert!(
            find(ConfigLinkKind::ModulePath, "myapp.handlers.auth").is_some(),
            "module path link missing; got {links:?}"
        );
        assert!(
            find(ConfigLinkKind::FilePath, "src/handlers/auth.ts").is_some(),
            "file path link missing; got {links:?}"
        );
        assert!(
            find(ConfigLinkKind::DependencyImport, "authlib").is_some(),
            "dependency link missing; got {links:?}"
        );

        // serde 往返（metadata 缓存格式）后 resolve 结果必须逐项一致。
        let json = serde_json::to_string(&tokens).unwrap();
        let restored: Vec<RawConfigToken> = serde_json::from_str(&json).unwrap();
        let relinked = resolve_config_links(&restored, &known_symbols, &known_files);
        assert_eq!(links.len(), relinked.len());
        for (a, b) in links.iter().zip(relinked.iter()) {
            assert_eq!(a.config_file, b.config_file);
            assert_eq!(a.referenced_value, b.referenced_value);
            assert_eq!(a.link_kind, b.link_kind);
            assert_eq!(a.line, b.line);
            assert_eq!(a.confidence, b.confidence);
        }
    }

    /// 配置文件集签名：内容不变则稳定；mtime/size 变化或文件增删都会改变。
    #[test]
    fn config_files_signature_tracks_set_and_stat_changes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("app.yaml"), "a: 1\n").unwrap();

        let sig1 = config_files_signature(root);
        assert_eq!(sig1, config_files_signature(root), "signature is stable");

        // 大小变化必然改变签名（不依赖秒级 mtime 分辨率）。
        std::fs::write(root.join("app.yaml"), "a: 1\nb: 2\n").unwrap();
        let sig2 = config_files_signature(root);
        assert_ne!(sig1, sig2, "size change must change the signature");

        // 新增配置文件改变签名。
        std::fs::write(root.join("extra.toml"), "x = 1\n").unwrap();
        let sig3 = config_files_signature(root);
        assert_ne!(sig2, sig3, "added config file must change the signature");

        // 非配置文件不影响签名。
        std::fs::write(root.join("notes.txt"), "hello\n").unwrap();
        assert_eq!(
            sig3,
            config_files_signature(root),
            "non-config files must not affect the signature"
        );
    }
}
