pub mod alloc_stats;
pub mod broker_patterns;
pub mod c_cpp;
pub mod chunker;
pub mod framework;
pub mod generic;
pub mod go;
pub mod http_call_helpers;
pub mod import_resolver;
pub mod java;
pub mod jsts;
pub mod lang_spec;
pub mod python;
pub mod rust;
pub mod sfc;
pub mod spec_driven;
pub mod traits;

use cc_model::{CcResult, Language, ParseOutcome};
use spec_driven::SpecDrivenParser;
use traits::FileParser;

/// Registry of all available parsers. Dispatches to the best parser for a given language.
pub struct ParserRegistry {
    generic: generic::GenericParser,
    python: python::PythonParser,
    jsts: jsts::JsTsParser,
    rust: rust::RustParser,
    go: go::GoParser,
    java: java::JavaParser,
    c_parser: c_cpp::CCppParser,
    cpp_parser: c_cpp::CCppParser,
    // Spec-driven parsers for languages with LangSpec but no tree-sitter grammar.
    spec_csharp: SpecDrivenParser,
    spec_php: SpecDrivenParser,
    spec_ruby: SpecDrivenParser,
    spec_swift: SpecDrivenParser,
    spec_kotlin: SpecDrivenParser,
    spec_dart: SpecDrivenParser,
    spec_scala: SpecDrivenParser,
    spec_lua: SpecDrivenParser,
}

impl ParserRegistry {
    pub fn new() -> Self {
        alloc_stats::enable_tracking();
        Self {
            generic: generic::GenericParser::new(),
            python: python::PythonParser::new(),
            jsts: jsts::JsTsParser::new(),
            rust: rust::RustParser::new(),
            go: go::GoParser::new(),
            java: java::JavaParser::new(),
            c_parser: c_cpp::CCppParser::new(Language::C),
            cpp_parser: c_cpp::CCppParser::new(Language::Cpp),
            spec_csharp: SpecDrivenParser::new(&lang_spec::CSHARP_SPEC),
            spec_php: SpecDrivenParser::new(&lang_spec::PHP_SPEC),
            spec_ruby: SpecDrivenParser::new(&lang_spec::RUBY_SPEC),
            spec_swift: SpecDrivenParser::new(&lang_spec::SWIFT_SPEC),
            spec_kotlin: SpecDrivenParser::new(&lang_spec::KOTLIN_SPEC),
            spec_dart: SpecDrivenParser::new(&lang_spec::DART_SPEC),
            spec_scala: SpecDrivenParser::new(&lang_spec::SCALA_SPEC),
            spec_lua: SpecDrivenParser::new(&lang_spec::LUA_SPEC),
        }
    }

    /// Call after each file parse to reclaim tree-sitter memory.
    /// Should be called by the indexer in the parse loop.
    pub fn per_file_reclaim(&self) {
        alloc_stats::per_file_reclaim();
    }

    /// Get session-level allocation summary
    pub fn alloc_summary(&self) -> alloc_stats::AllocStats {
        alloc_stats::session_summary()
    }

    /// Parse a file, dispatching to the best available parser for its language.
    pub fn parse(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
    ) -> CcResult<ParseOutcome> {
        self.parse_with_timeout(file_path, content, language, None)
    }

    /// Parse a file with an optional timeout hint.
    pub fn parse_with_timeout(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        timeout_micros: Option<u64>,
    ) -> CcResult<ParseOutcome> {
        match language {
            Language::Python => {
                self.python
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => self
                .jsts
                .parse_with_timeout(file_path, content, language, timeout_micros),
            Language::Vue | Language::Svelte => {
                sfc::parse_sfc(&self.jsts, file_path, content, language, timeout_micros)
            }
            Language::Rust => {
                self.rust
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Go => {
                self.go
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Java => {
                self.java
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::C => {
                self.c_parser
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Cpp => {
                self.cpp_parser
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            // Spec-driven parsers: Heuristic tier, better than Generic fallback.
            Language::CSharp => {
                self.spec_csharp
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Php => {
                self.spec_php
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Ruby => {
                self.spec_ruby
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Swift => {
                self.spec_swift
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Kotlin => {
                self.spec_kotlin
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Dart => {
                self.spec_dart
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Scala => {
                self.spec_scala
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            Language::Lua => {
                self.spec_lua
                    .parse_with_timeout(file_path, content, language, timeout_micros)
            }
            // SQL, YAML, TOML, Dockerfile — use generic line-based chunking.
            Language::Sql | Language::Yaml | Language::Toml | Language::Dockerfile => self
                .generic
                .parse_with_timeout(file_path, content, language, timeout_micros),
            // Generic fallback for everything else.
            _ => self
                .generic
                .parse_with_timeout(file_path, content, language, timeout_micros),
        }
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect language from file extension or filename.
pub fn detect_language(path: &str) -> Language {
    // Check filename-based detection first (for files like Dockerfile).
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if file_name == "Dockerfile"
        || file_name.starts_with("Dockerfile.")
        || file_name == "dockerfile"
    {
        return Language::Dockerfile;
    }

    let ext = path.rsplit('.').next().unwrap_or("");
    Language::from_extension(ext)
}
