use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level project configuration (loaded from .codecortex.json).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default = "IndexingConfig::default")]
    pub indexing: IndexingConfig,
    #[serde(default = "SearchConfig::default")]
    pub search: SearchConfig,
    #[serde(default = "PackConfig::default")]
    pub pack: PackConfig,
    #[serde(default = "EmbeddingsConfig::default")]
    pub embeddings: EmbeddingsConfig,
    #[serde(default)]
    pub parsers: ParsersConfig,
    #[serde(default)]
    pub auto_index: AutoIndexConfig,
}

/// Auto-indexing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoIndexConfig {
    /// Enable auto-indexing on first connect (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum file count to auto-index (default: 50000)
    #[serde(default = "default_auto_index_limit")]
    pub file_limit: usize,
    /// Idle timeout in seconds before evicting store (default: 60)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
}

fn default_true() -> bool {
    true
}
fn default_auto_index_limit() -> usize {
    50000
}
fn default_idle_timeout() -> u64 {
    60
}

impl Default for AutoIndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file_limit: 50000,
            idle_timeout_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingConfig {
    pub include: Vec<String>,
    pub ignore: Vec<String>,
    pub max_file_bytes: u64,
    pub chunk_line_budget: u32,
    pub parse_timeout_micros: Option<u64>,
    pub parallelism: Parallelism,
    /// 是否启用增量脏传播（检测导出变化后重新解析引用方）
    #[serde(default = "default_dirty_propagation")]
    pub dirty_propagation: bool,
    /// 脏传播最大影响文件数，超过此数量放弃传播建议全量重建
    #[serde(default = "default_dirty_propagation_max_files")]
    pub dirty_propagation_max_files: usize,
    /// RSS 内存预算占物理内存比例 (0.1-0.95)
    #[serde(default = "default_memory_budget_fraction")]
    pub memory_budget_fraction: f64,
    /// 最大并行 parse 线程数 (None = 使用 rayon 默认)
    #[serde(default)]
    pub max_concurrent_parse: Option<usize>,
    /// 实验性：全量重建时使用 direct SQLite writer（跳过 SQL 解析器）
    /// 完整实现，默认关闭；可通过 use_direct_writer: true 启用
    #[serde(default)]
    pub use_direct_writer: bool,
    /// 是否启用 dispatch synthesis（event emitter → handler 合成边）
    #[serde(default = "default_true")]
    pub dispatch_synthesis: bool,
    /// 单个 emit 站点匹配到的 on-handler 数量上限（先按 receiver/same-file 收窄）
    #[serde(default = "default_event_fanout_cap")]
    pub event_fanout_cap: usize,
    /// 自定义 event 拒绝列表（空表示使用内置默认列表）
    #[serde(default)]
    pub event_denylist: Vec<String>,
}

fn default_event_fanout_cap() -> usize {
    6
}

fn default_dirty_propagation() -> bool {
    true
}

fn default_dirty_propagation_max_files() -> usize {
    200
}

fn default_memory_budget_fraction() -> f64 {
    0.5
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            include: vec![
                "**/*.py".into(),
                "**/*.js".into(),
                "**/*.jsx".into(),
                "**/*.ts".into(),
                "**/*.tsx".into(),
                "**/*.vue".into(),
                "**/*.svelte".into(),
                "**/*.java".into(),
                "**/*.go".into(),
                "**/*.rs".into(),
                "**/*.md".into(),
                "**/*.cs".into(),
                "**/*.php".into(),
                "**/*.rb".into(),
                "**/*.swift".into(),
                "**/*.kt".into(),
                "**/*.kts".into(),
                "**/*.dart".into(),
                "**/*.scala".into(),
                "**/*.sc".into(),
                "**/*.lua".into(),
                "**/*.sql".into(),
                "**/*.yaml".into(),
                "**/*.yml".into(),
                "**/*.toml".into(),
                "**/Dockerfile".into(),
                "**/Dockerfile.*".into(),
            ],
            ignore: default_ignore_patterns(),
            max_file_bytes: 512_000,
            chunk_line_budget: 80,
            parse_timeout_micros: None,
            parallelism: Parallelism::Auto,
            dirty_propagation: true,
            dirty_propagation_max_files: 200,
            memory_budget_fraction: 0.5,
            max_concurrent_parse: None,
            use_direct_writer: false,
            dispatch_synthesis: true,
            event_fanout_cap: 6,
            event_denylist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Parallelism {
    #[default]
    Auto,
    Fixed(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub vector_top_k: usize,
    pub lexical_top_k: usize,
    pub grep_top_k: usize,
    pub rrf_k: usize,
    pub vector_weight: f64,
    pub lexical_weight: f64,
    pub grep_weight: f64,
    pub rerank_window: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            vector_top_k: 24,
            lexical_top_k: 24,
            grep_top_k: 12,
            rrf_k: 50,
            vector_weight: 1.0,
            lexical_weight: 1.1,
            grep_weight: 0.8,
            rerank_window: 40,
        }
    }
}

/// Legacy config section — kept for `.codecortex.json` backwards compatibility.
/// Budget values are now driven by [`RepoSizeTier`]; these fields are not read at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct PackConfig {
    pub default_token_budget: u32,
    pub neighbor_window: u32,
    pub import_fanout: u32,
}

impl Default for PackConfig {
    fn default() -> Self {
        Self {
            default_token_budget: 6000,
            neighbor_window: 1,
            import_fanout: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    pub provider: EmbeddingProvider,
    pub dimensions: usize,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub timeout_seconds: u64,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProvider::Hash,
            dimensions: 256,
            base_url: None,
            api_key: None,
            model: None,
            timeout_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProvider {
    #[default]
    Hash,
    #[serde(rename = "openai_compatible")]
    OpenAICompatible,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsersConfig {
    pub javascript: Option<ParserBackendConfig>,
    pub typescript: Option<ParserBackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserBackendConfig {
    pub backend: String,
}

/// Repo size tier — drives adaptive limits for explore, search, and budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSizeTier {
    Tiny,
    Small,
    Medium,
    Large,
}

impl RepoSizeTier {
    pub fn from_file_count(n: usize) -> Self {
        if n < 500 {
            Self::Tiny
        } else if n < 5000 {
            Self::Small
        } else if n < 25000 {
            Self::Medium
        } else {
            Self::Large
        }
    }

    pub fn default_token_budget(&self) -> u32 {
        match self {
            Self::Tiny => 4000,
            Self::Small => 6000,
            Self::Medium => 8000,
            Self::Large => 12000,
        }
    }

    pub fn explore_max_symbols(&self) -> usize {
        match self {
            Self::Tiny => 3,
            Self::Small => 5,
            Self::Medium => 8,
            Self::Large => 10,
        }
    }

    pub fn search_top_k(&self) -> usize {
        match self {
            Self::Tiny => 5,
            Self::Small => 10,
            Self::Medium => 15,
            Self::Large => 20,
        }
    }

    pub fn max_source_chars_per_symbol(&self) -> usize {
        match self {
            Self::Tiny => 2000,
            Self::Small => 3000,
            Self::Medium => 4000,
            Self::Large => 6000,
        }
    }

    pub fn max_output_chars(&self) -> usize {
        match self {
            Self::Tiny => 18000,
            Self::Small => 24000,
            Self::Medium => 32000,
            Self::Large => 38000,
        }
    }

    /// Return an adaptive output budget for the given handler name.
    pub fn output_budget(&self, handler: &str) -> OutputBudget {
        let base_chars = self.max_output_chars();
        let base_items = match handler {
            "graph_query" => match self {
                Self::Tiny => 15,
                Self::Small => 30,
                Self::Medium => 45,
                Self::Large => 60,
            },
            "trace_path" => match self {
                Self::Tiny => 10,
                Self::Small => 15,
                Self::Medium | Self::Large => 20,
            },
            "explore_flow" => match self {
                Self::Tiny => 15,
                Self::Small => 20,
                Self::Medium => 25,
                Self::Large => 30,
            },
            "dead_code" => match self {
                Self::Tiny => 20,
                Self::Small => 30,
                Self::Medium => 40,
                Self::Large => 50,
            },
            "circular_deps" => match self {
                Self::Tiny => 10,
                Self::Small => 15,
                Self::Medium | Self::Large => 20,
            },
            "relations" => match self {
                Self::Tiny => 20,
                Self::Small => 30,
                Self::Medium => 40,
                Self::Large => 50,
            },
            "impact" => match self {
                Self::Tiny => 20,
                Self::Small => 30,
                Self::Medium => 50,
                Self::Large => 80,
            },
            "architecture" => match self {
                Self::Tiny => 20,
                Self::Small => 30,
                Self::Medium => 40,
                Self::Large => 60,
            },
            "files" => match self {
                Self::Tiny => 500,
                Self::Small => 2000,
                Self::Medium => 5000,
                Self::Large => 10000,
            },
            _ => self.search_top_k(),
        };
        OutputBudget {
            max_output_chars: base_chars,
            max_items: base_items,
            max_snippet_chars: base_chars / 3,
            max_source_chars_per_symbol: self.max_source_chars_per_symbol(),
        }
    }
}

/// Adaptive output budget returned by [`RepoSizeTier::output_budget`].
#[derive(Debug, Clone)]
pub struct OutputBudget {
    pub max_output_chars: usize,
    pub max_items: usize,
    pub max_snippet_chars: usize,
    pub max_source_chars_per_symbol: usize,
}

/// Per-file explore output budget, scaled to project size.
#[derive(Debug, Clone)]
pub struct ExploreBudget {
    pub max_output_chars: usize,
    pub default_max_files: usize,
    pub max_chars_per_file: usize,
    pub gap_threshold: usize,
    pub include_relationships: bool,
    pub include_additional_files: bool,
}

impl RepoSizeTier {
    pub fn explore_budget(&self) -> ExploreBudget {
        match self {
            Self::Tiny => ExploreBudget {
                max_output_chars: 18000,
                default_max_files: 5,
                max_chars_per_file: 3800,
                gap_threshold: 3,
                include_relationships: false,
                include_additional_files: false,
            },
            Self::Small => ExploreBudget {
                max_output_chars: 28000,
                default_max_files: 8,
                max_chars_per_file: 6500,
                gap_threshold: 5,
                include_relationships: true,
                include_additional_files: true,
            },
            Self::Medium => ExploreBudget {
                max_output_chars: 35000,
                default_max_files: 12,
                max_chars_per_file: 7000,
                gap_threshold: 8,
                include_relationships: true,
                include_additional_files: true,
            },
            Self::Large => ExploreBudget {
                max_output_chars: 38000,
                default_max_files: 15,
                max_chars_per_file: 7000,
                gap_threshold: 10,
                include_relationships: true,
                include_additional_files: true,
            },
        }
    }
}

/// Resolved filesystem paths for a project.
#[derive(Debug, Clone)]
pub struct IndexPaths {
    pub project_path: PathBuf,
    pub workdir: PathBuf,
    pub index_db: PathBuf,
    pub logs_dir: PathBuf,
}

impl IndexPaths {
    pub fn new(project_path: &Path) -> Self {
        let workdir = project_path.join(".codecortex");
        Self {
            project_path: project_path.to_path_buf(),
            workdir: workdir.clone(),
            index_db: workdir.join("index.sqlite3"),
            logs_dir: workdir.join("logs"),
        }
    }
}

/// Project statistics from the index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectStats {
    pub project_path: String,
    pub indexed_files: usize,
    pub indexed_chunks: usize,
    pub indexed_symbols: usize,
    pub indexed_symbol_refs: usize,
    pub indexed_call_edges: usize,
    pub indexed_test_edges: usize,
    pub indexed_route_edges: usize,
    pub indexed_literals: usize,
    pub indexed_diagnostics: usize,
    pub last_indexed_at: Option<String>,
    pub index_version: Option<String>,
}

// ─── Config loading ─────────────────────────────────────────────────

const CONFIG_FILE_NAME: &str = ".codecortex.json";

/// Find the project root by walking up from `start` looking for .git or .codecortex.json.
pub fn find_project_root(start: Option<&Path>) -> PathBuf {
    let current = start
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .canonicalize()
        .unwrap_or_default();

    let mut candidate = current.as_path();
    loop {
        if candidate.join(".git").exists() || candidate.join(CONFIG_FILE_NAME).exists() {
            return candidate.to_path_buf();
        }
        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => return current,
        }
    }
}

/// Load project configuration from `.codecortex.json`, with defaults.
pub fn load_project_config(project_path: &Path) -> ProjectConfig {
    let config_path = project_path.join(CONFIG_FILE_NAME);
    let mut config = ProjectConfig::default();
    if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => match serde_json::from_str::<ProjectConfig>(&content) {
                Ok(parsed) => config = parsed,
                Err(e) => {
                    tracing::warn!(
                        path = %config_path.display(),
                        error = %e,
                        "Failed to parse project config; falling back to defaults"
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    path = %config_path.display(),
                    error = %e,
                    "Failed to read project config file; falling back to defaults"
                );
            }
        }
    }

    apply_env_overrides(&mut config);
    config
}

fn apply_env_overrides(config: &mut ProjectConfig) {
    if let Ok(provider) = std::env::var("CODECORTEX_EMBEDDINGS_PROVIDER") {
        match provider.trim() {
            "hash" => config.embeddings.provider = EmbeddingProvider::Hash,
            "openai_compatible" => {
                config.embeddings.provider = EmbeddingProvider::OpenAICompatible;
            }
            _ => {}
        }
    }
    if let Ok(dimensions) = std::env::var("CODECORTEX_EMBEDDINGS_DIMENSIONS") {
        if let Ok(parsed) = dimensions.trim().parse::<usize>() {
            if parsed > 0 {
                config.embeddings.dimensions = parsed;
            }
        }
    }
    if let Ok(timeout_seconds) = std::env::var("CODECORTEX_EMBEDDINGS_TIMEOUT_SECONDS") {
        if let Ok(parsed) = timeout_seconds.trim().parse::<u64>() {
            if parsed > 0 {
                config.embeddings.timeout_seconds = parsed;
            }
        }
    }
    if let Ok(url) = std::env::var("CODECORTEX_EMBEDDINGS_BASE_URL") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            config.embeddings.base_url = Some(trimmed.to_string());
        }
    }
    if let Ok(key) = std::env::var("CODECORTEX_EMBEDDINGS_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            config.embeddings.api_key = Some(trimmed.to_string());
        }
    }
    if let Ok(model) = std::env::var("CODECORTEX_EMBEDDINGS_MODEL") {
        let trimmed = model.trim();
        if !trimmed.is_empty() {
            config.embeddings.model = Some(trimmed.to_string());
        }
    }
    if let Ok(val) = std::env::var("CODECORTEX_DIRTY_PROPAGATION") {
        match val.trim().to_lowercase().as_str() {
            "0" | "false" | "off" | "no" => config.indexing.dirty_propagation = false,
            "1" | "true" | "on" | "yes" => config.indexing.dirty_propagation = true,
            _ => {}
        }
    }
    if let Ok(val) = std::env::var("CODECORTEX_DIRTY_PROPAGATION_MAX_FILES") {
        if let Ok(parsed) = val.trim().parse::<usize>() {
            if parsed > 0 {
                config.indexing.dirty_propagation_max_files = parsed;
            }
        }
    }
    if let Ok(val) = std::env::var("CODECORTEX_MEMORY_BUDGET_FRACTION") {
        if let Ok(parsed) = val.trim().parse::<f64>() {
            let clamped = parsed.clamp(0.1, 0.95);
            config.indexing.memory_budget_fraction = clamped;
        }
    }
    if let Ok(val) = std::env::var("CODECORTEX_MAX_CONCURRENT_PARSE") {
        if let Ok(parsed) = val.trim().parse::<usize>() {
            if parsed > 0 {
                config.indexing.max_concurrent_parse = Some(parsed);
            }
        }
    }
    if let Ok(val) = std::env::var("CODECORTEX_USE_DIRECT_WRITER") {
        match val.trim().to_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => config.indexing.use_direct_writer = true,
            "0" | "false" | "off" | "no" => config.indexing.use_direct_writer = false,
            _ => {}
        }
    }
}

fn default_ignore_patterns() -> Vec<String> {
    vec![
        "**/.git/**",
        "**/.hg/**",
        "**/.svn/**",
        "**/.venv/**",
        "**/venv/**",
        "**/__pycache__/**",
        "**/node_modules/**",
        "**/dist/**",
        "**/build/**",
        "**/coverage/**",
        "**/.next/**",
        "**/.idea/**",
        "**/.vscode/**",
        "**/.codecortex/**",
        "**/target/**", // Rust build
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn env_overrides_apply_on_top_of_file_config() {
        let _guard = ENV_LOCK.lock().unwrap();

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_dir = std::env::temp_dir().join(format!("codecortex-config-test-{}", unique));
        std::fs::create_dir_all(&project_dir).unwrap();

        let config_json = r#"{
            "embeddings": {
                "provider": "hash",
                "dimensions": 64,
                "base_url": "http://file.example/v1",
                "api_key": "file-key",
                "model": "file-model",
                "timeout_seconds": 10
            }
        }"#;
        std::fs::write(project_dir.join(CONFIG_FILE_NAME), config_json).unwrap();

        let keys = [
            "CODECORTEX_EMBEDDINGS_PROVIDER",
            "CODECORTEX_EMBEDDINGS_DIMENSIONS",
            "CODECORTEX_EMBEDDINGS_TIMEOUT_SECONDS",
            "CODECORTEX_EMBEDDINGS_BASE_URL",
            "CODECORTEX_EMBEDDINGS_API_KEY",
            "CODECORTEX_EMBEDDINGS_MODEL",
        ];
        let originals: Vec<(String, Option<String>)> = keys
            .iter()
            .map(|key| ((*key).to_string(), std::env::var(key).ok()))
            .collect();

        std::env::set_var("CODECORTEX_EMBEDDINGS_PROVIDER", "openai_compatible");
        std::env::set_var("CODECORTEX_EMBEDDINGS_DIMENSIONS", "384");
        std::env::set_var("CODECORTEX_EMBEDDINGS_TIMEOUT_SECONDS", "45");
        std::env::set_var("CODECORTEX_EMBEDDINGS_BASE_URL", "http://env.example/v1");
        std::env::set_var("CODECORTEX_EMBEDDINGS_API_KEY", "env-key");
        std::env::set_var("CODECORTEX_EMBEDDINGS_MODEL", "env-model");

        let config = load_project_config(&project_dir);
        assert!(matches!(
            config.embeddings.provider,
            EmbeddingProvider::OpenAICompatible
        ));
        assert_eq!(config.embeddings.dimensions, 384);
        assert_eq!(config.embeddings.timeout_seconds, 45);
        assert_eq!(
            config.embeddings.base_url.as_deref(),
            Some("http://env.example/v1")
        );
        assert_eq!(config.embeddings.api_key.as_deref(), Some("env-key"));
        assert_eq!(config.embeddings.model.as_deref(), Some("env-model"));

        for (key, value) in originals {
            if let Some(value) = value {
                std::env::set_var(&key, value);
            } else {
                std::env::remove_var(&key);
            }
        }
        let _ = std::fs::remove_file(project_dir.join(CONFIG_FILE_NAME));
        let _ = std::fs::remove_dir(&project_dir);
    }
}
