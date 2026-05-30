pub mod architecture;
pub mod chunk;
pub mod config;
pub mod context;
pub mod diagnostic;
pub mod dispatch_site;
pub mod edge;
pub mod error;
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
