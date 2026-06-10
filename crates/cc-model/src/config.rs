use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level project configuration (loaded from .codecortex.json).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default = "IndexingConfig::default")]
    pub indexing: IndexingConfig,
    #[serde(default = "SearchConfig::default")]
    pub search: SearchConfig,
    #[serde(default)]
    pub ranking: RankingConfig,
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
    #[serde(default = "default_include_patterns")]
    pub include: Vec<String>,
    #[serde(default = "default_ignore_patterns")]
    pub ignore: Vec<String>,
    #[serde(default = "default_max_file_bytes")]
    pub max_file_bytes: u64,
    #[serde(default = "default_chunk_line_budget")]
    pub chunk_line_budget: u32,
    #[serde(default)]
    pub parse_timeout_micros: Option<u64>,
    #[serde(default)]
    pub parallelism: Parallelism,
    /// SQLite read connection pool size. `None` means derive from repo size tier.
    #[serde(default)]
    pub db_read_pool_size: Option<u32>,
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

fn default_include_patterns() -> Vec<String> {
    vec![
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
    ]
}

fn default_max_file_bytes() -> u64 {
    512_000
}

fn default_chunk_line_budget() -> u32 {
    80
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            include: default_include_patterns(),
            ignore: default_ignore_patterns(),
            max_file_bytes: default_max_file_bytes(),
            chunk_line_budget: default_chunk_line_budget(),
            parse_timeout_micros: None,
            parallelism: Parallelism::Auto,
            db_read_pool_size: None,
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
    #[serde(default = "default_lexical_top_k")]
    pub lexical_top_k: usize,
    #[serde(default = "default_grep_top_k")]
    pub grep_top_k: usize,
    #[serde(default = "default_rrf_k")]
    pub rrf_k: usize,
    #[serde(default = "default_lexical_weight")]
    pub lexical_weight: f64,
    #[serde(default = "default_grep_weight")]
    pub grep_weight: f64,
    #[serde(default = "default_rerank_window")]
    pub rerank_window: usize,
    #[serde(default = "default_graph_weight")]
    pub graph_weight: f64,
    #[serde(default = "default_graph_top_k")]
    pub graph_top_k: usize,
}

fn default_lexical_top_k() -> usize {
    24
}
fn default_grep_top_k() -> usize {
    12
}
fn default_rrf_k() -> usize {
    50
}
fn default_lexical_weight() -> f64 {
    1.1
}
fn default_grep_weight() -> f64 {
    0.8
}
fn default_rerank_window() -> usize {
    40
}
fn default_graph_weight() -> f64 {
    0.6
}
fn default_graph_top_k() -> usize {
    12
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            lexical_top_k: default_lexical_top_k(),
            grep_top_k: default_grep_top_k(),
            rrf_k: default_rrf_k(),
            lexical_weight: default_lexical_weight(),
            grep_weight: default_grep_weight(),
            rerank_window: default_rerank_window(),
            graph_weight: default_graph_weight(),
            graph_top_k: default_graph_top_k(),
        }
    }
}

/// Ranking configuration for search result scoring.
///
/// Centralizes the tunable scoring weights used by cc-search (chunk rerank,
/// file preselection, and graph-lane seeding) so tuning happens in one
/// place instead of scattered literals.  A few structural constants (e.g.
/// the preselect graph-neighbor increment/cap) deliberately remain literals
/// at their use sites.  All defaults preserve the historical hard-coded
/// values exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    /// Weight of graph_score contribution to final rerank_score.
    /// Range: 0.0 (disabled) to 1.0 (maximum influence).
    #[serde(default = "default_graph_rerank_weight")]
    pub graph_rerank_weight: f64,

    // ── Chunk rerank bonuses (plan.rs::hit_from_chunk) ──────────────
    /// Weight of query-token/text overlap added to the fused score.
    #[serde(default = "default_overlap_weight")]
    pub overlap_weight: f64,
    /// Bonus when a query token exactly matches the chunk's symbol name.
    #[serde(default = "default_symbol_exact_bonus")]
    pub symbol_exact_bonus: f64,
    /// Bonus when the file path starts with the requested path prefix.
    #[serde(default = "default_path_prefix_bonus")]
    pub path_prefix_bonus: f64,
    /// Bonus for project documentation files (README, docs/, ADRs).
    #[serde(default = "default_doc_file_bonus")]
    pub doc_file_bonus: f64,
    /// Bonus for files in the caller's working set (boost_file_paths).
    #[serde(default = "default_working_set_boost")]
    pub working_set_boost: f64,
    /// Bonus for recently-edited files (recent_file_paths).
    #[serde(default = "default_recent_file_boost")]
    pub recent_file_boost: f64,
    /// Bonus for pinned context files (pinned_file_paths).
    #[serde(default = "default_pinned_context_boost")]
    pub pinned_context_boost: f64,
    /// Bonus for overlay/dirty-buffer files (overlay_file_paths).
    #[serde(default = "default_overlay_neighbor_boost")]
    pub overlay_neighbor_boost: f64,
    /// Multiplier mapping the stage-A (preselect) file score into rerank.
    #[serde(default = "default_stage_a_weight")]
    pub stage_a_weight: f64,
    /// Cap on the stage-A file-score contribution to rerank.
    #[serde(default = "default_stage_a_cap")]
    pub stage_a_cap: f64,
    /// Bonus when a `name:` DSL filter matches the hit's symbol name.
    #[serde(default = "default_dsl_name_bonus")]
    pub dsl_name_bonus: f64,

    // ── File preselection scores (preselect.rs) ─────────────────────
    /// Working-set layer: score is `max(floor, scale / rank)`.
    #[serde(default = "default_preselect_working_set_floor")]
    pub preselect_working_set_floor: f64,
    #[serde(default = "default_preselect_working_set_scale")]
    pub preselect_working_set_scale: f64,
    /// Recent-files layer: score is `max(floor, scale / rank)`.
    #[serde(default = "default_preselect_recent_floor")]
    pub preselect_recent_floor: f64,
    #[serde(default = "default_preselect_recent_scale")]
    pub preselect_recent_scale: f64,
    /// Pinned-files layer: score is `max(floor, scale / rank)`.
    #[serde(default = "default_preselect_pinned_floor")]
    pub preselect_pinned_floor: f64,
    #[serde(default = "default_preselect_pinned_scale")]
    pub preselect_pinned_scale: f64,
    /// Overlay (dirty-buffer) layer: score is `max(floor, scale / rank)`.
    #[serde(default = "default_preselect_overlay_floor")]
    pub preselect_overlay_floor: f64,
    #[serde(default = "default_preselect_overlay_scale")]
    pub preselect_overlay_scale: f64,
    /// FTS summary layer: score is `base + 1 / (1 + |bm25|)`.
    #[serde(default = "default_preselect_fts_base")]
    pub preselect_fts_base: f64,
    /// Per-token symbol-name match: exact name equality.
    #[serde(default = "default_preselect_symbol_exact_bonus")]
    pub preselect_symbol_exact_bonus: f64,
    /// Per-token symbol-name match: substring (fuzzy) match.
    #[serde(default = "default_preselect_symbol_fuzzy_bonus")]
    pub preselect_symbol_fuzzy_bonus: f64,
    /// Per-token path component match.
    #[serde(default = "default_preselect_path_token_bonus")]
    pub preselect_path_token_bonus: f64,
    /// Graph neighbor expansion: base score for 1-hop call-graph neighbors.
    #[serde(default = "default_preselect_graph_neighbor_base")]
    pub preselect_graph_neighbor_base: f64,
    /// Fallback layer: score for recently-indexed files when nothing matched.
    #[serde(default = "default_preselect_fallback_score")]
    pub preselect_fallback_score: f64,

    // ── Graph retrieval lane (lanes.rs) ─────────────────────────────
    /// Score decay per hop when expanding from a seed symbol to its
    /// call-graph neighbors.
    #[serde(default = "default_graph_neighbor_decay")]
    pub graph_neighbor_decay: f64,
    /// Seed relevance for an exact symbol-name match.
    #[serde(default = "default_graph_seed_exact_score")]
    pub graph_seed_exact_score: f64,
    /// Seed relevance for a substring symbol-name match.
    #[serde(default = "default_graph_seed_fuzzy_score")]
    pub graph_seed_fuzzy_score: f64,
}

fn default_graph_rerank_weight() -> f64 {
    0.3
}
fn default_overlap_weight() -> f64 {
    0.35
}
fn default_symbol_exact_bonus() -> f64 {
    0.18
}
fn default_path_prefix_bonus() -> f64 {
    0.05
}
fn default_doc_file_bonus() -> f64 {
    0.08
}
fn default_working_set_boost() -> f64 {
    0.22
}
fn default_recent_file_boost() -> f64 {
    0.12
}
fn default_pinned_context_boost() -> f64 {
    0.20
}
fn default_overlay_neighbor_boost() -> f64 {
    0.10
}
fn default_stage_a_weight() -> f64 {
    0.04
}
fn default_stage_a_cap() -> f64 {
    0.25
}
fn default_dsl_name_bonus() -> f64 {
    0.25
}
fn default_preselect_working_set_floor() -> f64 {
    2.0
}
fn default_preselect_working_set_scale() -> f64 {
    5.0
}
fn default_preselect_recent_floor() -> f64 {
    1.2
}
fn default_preselect_recent_scale() -> f64 {
    3.5
}
fn default_preselect_pinned_floor() -> f64 {
    2.2
}
fn default_preselect_pinned_scale() -> f64 {
    4.0
}
fn default_preselect_overlay_floor() -> f64 {
    1.5
}
fn default_preselect_overlay_scale() -> f64 {
    3.0
}
fn default_preselect_fts_base() -> f64 {
    1.4
}
fn default_preselect_symbol_exact_bonus() -> f64 {
    2.0
}
fn default_preselect_symbol_fuzzy_bonus() -> f64 {
    1.2
}
fn default_preselect_path_token_bonus() -> f64 {
    1.0
}
fn default_preselect_graph_neighbor_base() -> f64 {
    0.8
}
fn default_preselect_fallback_score() -> f64 {
    0.2
}
fn default_graph_neighbor_decay() -> f64 {
    0.5
}
fn default_graph_seed_exact_score() -> f64 {
    1.0
}
fn default_graph_seed_fuzzy_score() -> f64 {
    0.5
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            graph_rerank_weight: default_graph_rerank_weight(),
            overlap_weight: default_overlap_weight(),
            symbol_exact_bonus: default_symbol_exact_bonus(),
            path_prefix_bonus: default_path_prefix_bonus(),
            doc_file_bonus: default_doc_file_bonus(),
            working_set_boost: default_working_set_boost(),
            recent_file_boost: default_recent_file_boost(),
            pinned_context_boost: default_pinned_context_boost(),
            overlay_neighbor_boost: default_overlay_neighbor_boost(),
            stage_a_weight: default_stage_a_weight(),
            stage_a_cap: default_stage_a_cap(),
            dsl_name_bonus: default_dsl_name_bonus(),
            preselect_working_set_floor: default_preselect_working_set_floor(),
            preselect_working_set_scale: default_preselect_working_set_scale(),
            preselect_recent_floor: default_preselect_recent_floor(),
            preselect_recent_scale: default_preselect_recent_scale(),
            preselect_pinned_floor: default_preselect_pinned_floor(),
            preselect_pinned_scale: default_preselect_pinned_scale(),
            preselect_overlay_floor: default_preselect_overlay_floor(),
            preselect_overlay_scale: default_preselect_overlay_scale(),
            preselect_fts_base: default_preselect_fts_base(),
            preselect_symbol_exact_bonus: default_preselect_symbol_exact_bonus(),
            preselect_symbol_fuzzy_bonus: default_preselect_symbol_fuzzy_bonus(),
            preselect_path_token_bonus: default_preselect_path_token_bonus(),
            preselect_graph_neighbor_base: default_preselect_graph_neighbor_base(),
            preselect_fallback_score: default_preselect_fallback_score(),
            graph_neighbor_decay: default_graph_neighbor_decay(),
            graph_seed_exact_score: default_graph_seed_exact_score(),
            graph_seed_fuzzy_score: default_graph_seed_fuzzy_score(),
        }
    }
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

    /// Suggested SQLite read pool size for this repository tier.
    pub fn db_read_pool_size(&self) -> u32 {
        match self {
            Self::Tiny => 4,
            Self::Small => 6,
            Self::Medium => 8,
            Self::Large => 12,
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

/// Budget limits for graph enrichment in the `context` tool.
#[derive(Debug, Clone)]
pub struct GraphEnrichLimits {
    pub max_resolve: usize,
    pub callers_per_sym: usize,
    pub callees_per_sym: usize,
    pub max_tests: usize,
    pub max_routes: usize,
    pub graph_budget_pct: u32,
}

impl RepoSizeTier {
    pub fn graph_enrich_limits(&self) -> GraphEnrichLimits {
        match self {
            Self::Tiny => GraphEnrichLimits {
                max_resolve: 3,
                callers_per_sym: 2,
                callees_per_sym: 2,
                max_tests: 2,
                max_routes: 1,
                graph_budget_pct: 20,
            },
            Self::Small => GraphEnrichLimits {
                max_resolve: 5,
                callers_per_sym: 3,
                callees_per_sym: 3,
                max_tests: 3,
                max_routes: 2,
                graph_budget_pct: 25,
            },
            Self::Medium => GraphEnrichLimits {
                max_resolve: 7,
                callers_per_sym: 3,
                callees_per_sym: 3,
                max_tests: 4,
                max_routes: 2,
                graph_budget_pct: 25,
            },
            Self::Large => GraphEnrichLimits {
                max_resolve: 8,
                callers_per_sym: 4,
                callees_per_sym: 4,
                max_tests: 5,
                max_routes: 3,
                graph_budget_pct: 30,
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
        let workdir = std::env::var("CODECORTEX_CACHE_DIR")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(|base| PathBuf::from(base).join(project_cache_key(project_path)))
            .unwrap_or_else(|| project_path.join(".codecortex"));
        Self {
            project_path: project_path.to_path_buf(),
            workdir: workdir.clone(),
            index_db: workdir.join("index.sqlite3"),
            logs_dir: workdir.join("logs"),
        }
    }
}

fn project_cache_key(project_path: &Path) -> String {
    let raw = project_path.to_string_lossy();
    let hash = blake3::hash(raw.as_bytes()).to_hex().to_string();
    let name = project_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("{}-{}", name, &hash[..16])
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

fn canonical_start(start: Option<&Path>) -> PathBuf {
    start
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .canonicalize()
        .unwrap_or_default()
}

/// Find the project root by walking up from `start` looking for .git or .codecortex.json.
///
/// Returns `None` when no explicit project marker is found. This is safer for
/// MCP startup than blindly indexing the process working directory, which may
/// be the user's home directory depending on the client.
pub fn find_project_root_with_marker(start: Option<&Path>) -> Option<PathBuf> {
    let current = canonical_start(start);
    let mut candidate = current.as_path();
    loop {
        if candidate.join(".git").exists() || candidate.join(CONFIG_FILE_NAME).exists() {
            return Some(candidate.to_path_buf());
        }
        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => return None,
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
            "indexing": {
                "dirty_propagation": true,
                "dirty_propagation_max_files": 50,
                "memory_budget_fraction": 0.25,
                "use_direct_writer": false
            }
        }"#;
        std::fs::write(project_dir.join(CONFIG_FILE_NAME), config_json).unwrap();

        let keys = [
            "CODECORTEX_DIRTY_PROPAGATION",
            "CODECORTEX_DIRTY_PROPAGATION_MAX_FILES",
            "CODECORTEX_MEMORY_BUDGET_FRACTION",
            "CODECORTEX_USE_DIRECT_WRITER",
        ];
        let originals: Vec<(String, Option<String>)> = keys
            .iter()
            .map(|key| ((*key).to_string(), std::env::var(key).ok()))
            .collect();

        std::env::set_var("CODECORTEX_DIRTY_PROPAGATION", "false");
        std::env::set_var("CODECORTEX_DIRTY_PROPAGATION_MAX_FILES", "125");
        std::env::set_var("CODECORTEX_MEMORY_BUDGET_FRACTION", "0.75");
        std::env::set_var("CODECORTEX_USE_DIRECT_WRITER", "true");

        let config = load_project_config(&project_dir);
        assert!(!config.indexing.dirty_propagation);
        assert_eq!(config.indexing.dirty_propagation_max_files, 125);
        assert_eq!(config.indexing.memory_budget_fraction, 0.75);
        assert!(config.indexing.use_direct_writer);

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

    #[test]
    fn partial_config_fills_missing_fields_with_defaults() {
        // Contract: every config field is optional. A partial .codecortex.json
        // that overrides one field must deserialize and fall back to defaults
        // for the rest (previously failed with `missing field chunk_line_budget`).
        let json = r#"{
            "indexing": { "max_file_bytes": 1024 },
            "search": { "lexical_top_k": 8 },
            "ranking": { "overlap_weight": 0.5 }
        }"#;
        let config: ProjectConfig = serde_json::from_str(json).expect("partial config must parse");

        assert_eq!(config.indexing.max_file_bytes, 1024);
        assert_eq!(
            config.indexing.chunk_line_budget,
            default_chunk_line_budget()
        );
        assert_eq!(config.indexing.include, default_include_patterns());
        assert_eq!(config.search.lexical_top_k, 8);
        assert_eq!(config.search.grep_top_k, default_grep_top_k());
        assert_eq!(config.search.rrf_k, default_rrf_k());
        assert_eq!(config.ranking.overlap_weight, 0.5);
        assert_eq!(
            config.ranking.graph_rerank_weight,
            default_graph_rerank_weight()
        );
        assert_eq!(config.ranking.symbol_exact_bonus, 0.18);
        assert_eq!(config.ranking.preselect_working_set_scale, 5.0);
    }

    #[test]
    fn index_paths_can_use_external_cache_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("CODECORTEX_CACHE_DIR").ok();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_dir = std::env::temp_dir().join(format!("codecortex-path-test-{}", unique));
        let cache_dir = std::env::temp_dir().join(format!("codecortex-cache-test-{}", unique));
        std::fs::create_dir_all(&project_dir).unwrap();

        std::env::set_var("CODECORTEX_CACHE_DIR", &cache_dir);
        let paths = IndexPaths::new(&project_dir);
        assert!(paths.workdir.starts_with(&cache_dir));
        assert_eq!(paths.index_db, paths.workdir.join("index.sqlite3"));
        assert_eq!(paths.logs_dir, paths.workdir.join("logs"));

        if let Some(value) = original {
            std::env::set_var("CODECORTEX_CACHE_DIR", value);
        } else {
            std::env::remove_var("CODECORTEX_CACHE_DIR");
        }
        let _ = std::fs::remove_dir_all(&project_dir);
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn find_project_root_with_marker_returns_marker_dir() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_dir = std::env::temp_dir().join(format!("codecortex-root-test-{}", unique));
        let nested = project_dir.join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(project_dir.join(".git")).unwrap();

        let found = find_project_root_with_marker(Some(&nested)).unwrap();
        assert_eq!(found, project_dir.canonicalize().unwrap());

        let _ = std::fs::remove_dir_all(&project_dir);
    }

    #[test]
    fn find_project_root_with_marker_returns_none_without_marker() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_dir = std::env::temp_dir().join(format!("codecortex-no-root-test-{}", unique));
        std::fs::create_dir_all(&project_dir).unwrap();

        assert!(find_project_root_with_marker(Some(&project_dir)).is_none());

        let _ = std::fs::remove_dir_all(&project_dir);
    }
}
