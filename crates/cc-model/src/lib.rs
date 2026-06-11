pub mod architecture;
pub mod chunk;
pub mod config;
pub mod context;
pub mod diagnostic;
pub mod dispatch_site;
pub mod edge;
pub mod error;
pub mod graph_catalog;
pub mod graph_explain;
pub mod id;
pub mod impact;
pub mod infra;
pub mod parse;
pub mod route_normalize;
pub mod scope;
pub mod search;
pub mod symbol;
pub mod type_assign;

// Re-export top-level types for convenience
pub use config::ProjectConfig;
pub use error::{CcError, CcResult};

pub use chunk::ChunkRecord;
pub use context::{ContextEnvelope, ContextNode, ContextSpan, NodeType, Role};
pub use diagnostic::{DiagnosticRecord, LiteralRecord};
pub use dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
pub use edge::{
    CallEdgeRecord, CoChangeEdgeRecord, DataFlowEdgeRecord, DispatchKind, HttpCallEdgeRecord,
    ImportRecord, ResolutionKind, RouteEdgeRecord, RouteNodeRecord, SemanticEdgeRecord,
    SemanticRelation, TestEdgeRecord,
};
pub use graph_explain::{GraphExplain, GraphExplainCollector};
pub use id::StableId;
pub use impact::{
    CrossServiceImpact, HistoricalImpact, ImpactReport, ImpactedSymbol, RiskLevel, RiskSummary,
};
pub use infra::{InfraEdge, InfraEdgeKind, InfraKind, InfraNode};
pub use parse::ParseOutcome;
pub use search::SearchHit;
/// Core enums used across the codebase
pub use symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
pub use type_assign::{TypeAssignRecord, TypeAssignSource};

/// Language and parser tier enums
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Jsx,
    Java,
    Go,
    Rust,
    Vue,
    Svelte,
    Markdown,
    CSharp,
    Php,
    Ruby,
    Swift,
    Kotlin,
    C,
    Cpp,
    Dart,
    Scala,
    Lua,
    Sql,
    Yaml,
    Toml,
    Hcl,
    Dockerfile,
    Bash,
    Protobuf,
    GraphQL,
    CMake,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "py" => Self::Python,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "jsx" => Self::Jsx,
            "java" => Self::Java,
            "go" => Self::Go,
            "rs" => Self::Rust,
            "vue" => Self::Vue,
            "svelte" => Self::Svelte,
            "md" | "mdx" => Self::Markdown,
            "cs" => Self::CSharp,
            "php" => Self::Php,
            "rb" | "rake" => Self::Ruby,
            "swift" => Self::Swift,
            "kt" | "kts" => Self::Kotlin,
            "c" => Self::C,
            "cpp" | "cc" | "cxx" | "h" | "hpp" | "hxx" => Self::Cpp,
            "dart" => Self::Dart,
            "scala" | "sc" => Self::Scala,
            "lua" | "luau" => Self::Lua,
            "sql" => Self::Sql,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "tf" | "tfvars" => Self::Hcl,
            "sh" | "bash" | "zsh" => Self::Bash,
            "proto" => Self::Protobuf,
            "graphql" | "gql" => Self::GraphQL,
            "cmake" => Self::CMake,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Jsx => "jsx",
            Self::Java => "java",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::Vue => "vue",
            Self::Svelte => "svelte",
            Self::Markdown => "markdown",
            Self::CSharp => "csharp",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::Swift => "swift",
            Self::Kotlin => "kotlin",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Dart => "dart",
            Self::Scala => "scala",
            Self::Lua => "lua",
            Self::Sql => "sql",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Hcl => "hcl",
            Self::Dockerfile => "dockerfile",
            Self::Bash => "bash",
            Self::Protobuf => "protobuf",
            Self::GraphQL => "graphql",
            Self::CMake => "cmake",
            Self::Unknown => "unknown",
        }
    }
}

impl Language {
    /// Parse a language name string (case-insensitive).
    pub fn from_name(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "python" | "py" => Self::Python,
            "javascript" | "js" => Self::JavaScript,
            "typescript" | "ts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "jsx" => Self::Jsx,
            "java" => Self::Java,
            "go" | "golang" => Self::Go,
            "rust" | "rs" => Self::Rust,
            "vue" => Self::Vue,
            "svelte" => Self::Svelte,
            "markdown" | "md" => Self::Markdown,
            "csharp" | "c#" | "cs" => Self::CSharp,
            "php" => Self::Php,
            "ruby" | "rb" => Self::Ruby,
            "swift" => Self::Swift,
            "kotlin" | "kt" => Self::Kotlin,
            "c" => Self::C,
            "cpp" | "c++" | "cxx" => Self::Cpp,
            "dart" => Self::Dart,
            "scala" | "sc" => Self::Scala,
            "lua" | "luau" => Self::Lua,
            "sql" => Self::Sql,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "hcl" | "terraform" | "tf" => Self::Hcl,
            "dockerfile" | "docker" => Self::Dockerfile,
            "bash" | "sh" | "shell" | "zsh" => Self::Bash,
            "protobuf" | "proto" => Self::Protobuf,
            "graphql" | "gql" => Self::GraphQL,
            "cmake" => Self::CMake,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parser confidence tier
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ParserTier {
    #[default]
    Generic,
    Heuristic,
    TreeSitter,
    Semantic,
    Verified,
}

/// Kind of element a parser extracts, used to look up the parser-assigned
/// extraction confidence in [`ParserTier::element_confidence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementKind {
    Symbol,
    CallEdge,
    /// `SymbolRefRecord` with `ref_kind = "call"` (callee position).
    CallRef,
    /// `SymbolRefRecord` with `ref_kind = "identifier"` (bare identifier).
    IdentifierRef,
    SemanticEdge,
    /// `DataFlowEdgeRecord` with `flow_kind = "type_ref"`.
    TypeRef,
    /// `DataFlowEdgeRecord` with `flow_kind = "env_access"`.
    EnvAccess,
    Route,
    HttpCall,
    DispatchSite,
}

impl ParserTier {
    pub fn default_confidence(&self) -> f64 {
        match self {
            Self::Generic => 0.3,
            Self::Heuristic => 0.5,
            Self::TreeSitter => 0.7,
            Self::Semantic => 0.85,
            Self::Verified => 0.95,
        }
    }

    /// Single source of truth for parser-assigned extraction confidence per
    /// (tier, element kind). Resolution-time confidence (cc-index resolver) is
    /// a separate concept and never read from this matrix. Framework-specific
    /// calibrations (e.g. per-framework route detection) deviate from these
    /// baselines via named constants at the call site.
    pub fn element_confidence(&self, kind: ElementKind) -> f64 {
        use ElementKind::*;
        match (self, kind) {
            (Self::Semantic, Symbol) => 0.85,
            (Self::Semantic | Self::TreeSitter, CallEdge | CallRef) => 0.7,
            // Bare identifier refs are noisier than callee refs.
            (Self::Semantic | Self::TreeSitter, IdentifierRef) => 0.6,
            // Declared relationships (inherits/implements/decorates) are
            // syntactically explicit regardless of tier.
            (Self::Semantic | Self::TreeSitter, SemanticEdge) => 0.95,
            (Self::Semantic, TypeRef) => 0.85,
            (Self::Semantic, Route) => 0.85,
            (Self::Semantic, DispatchSite) => 0.85,
            (Self::TreeSitter, Symbol) => 0.7,
            (Self::TreeSitter, Route) => 0.8,
            // AST-detected HTTP client calls; regex-detected ones come in via
            // the Heuristic arm below.
            (Self::TreeSitter, HttpCall) => 0.8,
            (Self::Heuristic, HttpCall) => 0.7,
            // Env-var access regexes are distinctive enough to beat the
            // Heuristic default.
            (Self::Heuristic, EnvAccess) => 0.8,
            _ => self.default_confidence(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Heuristic => "heuristic",
            Self::TreeSitter => "tree_sitter",
            Self::Semantic => "semantic",
            Self::Verified => "verified",
        }
    }
}

impl std::fmt::Display for ParserTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Intent type for context planning
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Fix,
    Refactor,
    Locate,
    Trace,
    Test,
    Patch,
    Explain,
    #[default]
    Default,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fix => "fix",
            Self::Refactor => "refactor",
            Self::Locate => "locate",
            Self::Trace => "trace",
            Self::Test => "test",
            Self::Patch => "patch",
            Self::Explain => "explain",
            Self::Default => "default",
        }
    }
}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Intent {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fix" => Ok(Self::Fix),
            "refactor" => Ok(Self::Refactor),
            "locate" => Ok(Self::Locate),
            "trace" => Ok(Self::Trace),
            "test" => Ok(Self::Test),
            "patch" => Ok(Self::Patch),
            "explain" => Ok(Self::Explain),
            "default" => Ok(Self::Default),
            _ => Err(()),
        }
    }
}

/// Approximate token count (1 token ~ 4 bytes)
pub fn approx_tokens(text: &str) -> u32 {
    (text.len() as u32).div_ceil(4)
}

#[cfg(test)]
fn first_sentence(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return trimmed;
    }
    // Find the earliest sentence-ending boundary
    let end = trimmed
        .find(". ")
        .or_else(|| trimmed.find(".\n"))
        .map(|pos| pos + 1) // include the period
        .unwrap_or(trimmed.len());
    let mut limit = end.min(240);
    // Ensure we don't split a multi-byte character
    while !trimmed.is_char_boundary(limit) {
        limit = limit.saturating_sub(1);
    }
    &trimmed[..limit]
}

#[cfg(test)]
fn chunked<T>(items: &[T], chunk_size: usize) -> impl Iterator<Item = &[T]> {
    assert!(chunk_size > 0, "chunk_size must be > 0");
    items.chunks(chunk_size)
}

#[cfg(test)]
mod parser_tier_tests {
    use super::*;

    #[test]
    fn element_confidence_matrix_baselines() {
        use ElementKind::*;
        let sem = ParserTier::Semantic;
        let ts = ParserTier::TreeSitter;

        assert_eq!(sem.element_confidence(Symbol), 0.85);
        assert_eq!(ts.element_confidence(Symbol), 0.7);
        for tier in [sem, ts] {
            assert_eq!(tier.element_confidence(CallEdge), 0.7);
            assert_eq!(tier.element_confidence(CallRef), 0.7);
            assert_eq!(tier.element_confidence(IdentifierRef), 0.6);
            assert_eq!(tier.element_confidence(SemanticEdge), 0.95);
        }
        assert_eq!(sem.element_confidence(TypeRef), 0.85);
        assert_eq!(sem.element_confidence(Route), 0.85);
        assert_eq!(sem.element_confidence(DispatchSite), 0.85);
        assert_eq!(ts.element_confidence(Route), 0.8);
        assert_eq!(ts.element_confidence(HttpCall), 0.8);
        assert_eq!(ParserTier::Heuristic.element_confidence(HttpCall), 0.7);
        assert_eq!(ParserTier::Heuristic.element_confidence(EnvAccess), 0.8);
    }

    #[test]
    fn element_confidence_falls_back_to_tier_default() {
        for tier in [
            ParserTier::Generic,
            ParserTier::Heuristic,
            ParserTier::Verified,
        ] {
            assert_eq!(
                tier.element_confidence(ElementKind::Symbol),
                tier.default_confidence()
            );
        }
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn from_extension_new_languages() {
        assert_eq!(Language::from_extension("dart"), Language::Dart);
        assert_eq!(Language::from_extension("scala"), Language::Scala);
        assert_eq!(Language::from_extension("sc"), Language::Scala);
        assert_eq!(Language::from_extension("lua"), Language::Lua);
        assert_eq!(Language::from_extension("luau"), Language::Lua);
        assert_eq!(Language::from_extension("sql"), Language::Sql);
        assert_eq!(Language::from_extension("yaml"), Language::Yaml);
        assert_eq!(Language::from_extension("yml"), Language::Yaml);
        assert_eq!(Language::from_extension("toml"), Language::Toml);
    }

    #[test]
    fn from_name_new_languages() {
        assert_eq!(Language::from_name("dart"), Language::Dart);
        assert_eq!(Language::from_name("scala"), Language::Scala);
        assert_eq!(Language::from_name("lua"), Language::Lua);
        assert_eq!(Language::from_name("sql"), Language::Sql);
        assert_eq!(Language::from_name("yaml"), Language::Yaml);
        assert_eq!(Language::from_name("yml"), Language::Yaml);
        assert_eq!(Language::from_name("toml"), Language::Toml);
        assert_eq!(Language::from_name("dockerfile"), Language::Dockerfile);
        assert_eq!(Language::from_name("docker"), Language::Dockerfile);
    }

    #[test]
    fn as_str_new_languages() {
        assert_eq!(Language::Dart.as_str(), "dart");
        assert_eq!(Language::Scala.as_str(), "scala");
        assert_eq!(Language::Lua.as_str(), "lua");
        assert_eq!(Language::Sql.as_str(), "sql");
        assert_eq!(Language::Yaml.as_str(), "yaml");
        assert_eq!(Language::Toml.as_str(), "toml");
        assert_eq!(Language::Dockerfile.as_str(), "dockerfile");
    }
}

#[cfg(test)]
mod util_tests {
    use super::*;

    #[test]
    fn first_sentence_with_period() {
        assert_eq!(
            first_sentence("Hello world. More text here."),
            "Hello world."
        );
    }

    #[test]
    fn first_sentence_without_period() {
        assert_eq!(first_sentence("No period here"), "No period here");
    }

    #[test]
    fn first_sentence_empty() {
        assert_eq!(first_sentence(""), "");
        assert_eq!(first_sentence("  "), "");
    }

    #[test]
    fn first_sentence_with_newline() {
        assert_eq!(first_sentence("First line.\nSecond line."), "First line.");
    }

    #[test]
    fn chunked_basic() {
        let items = vec![1, 2, 3, 4, 5];
        let chunks: Vec<&[i32]> = chunked(&items, 2).collect();
        assert_eq!(chunks, vec![&[1, 2][..], &[3, 4][..], &[5][..]]);
    }
}
