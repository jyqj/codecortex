//! Language specification table for spec-driven (regex-heuristic) parsing.
//!
//! Languages without a dedicated tree-sitter grammar (C#, PHP, Ruby, Swift,
//! Kotlin, Dart, Scala, Lua) are parsed by [`crate::spec_driven`] using regex
//! heuristics rather than an AST. Each `LangSpec` supplies the minimal metadata
//! the spec-driven parser needs: the target language, a grammar-name tag, the
//! qualified-name separator, and the file extensions.

use cc_model::Language;

/// Language specification metadata consumed by the spec-driven parser.
#[derive(Debug, Clone)]
pub struct LangSpec {
    /// Target language
    pub language: Language,

    /// Grammar name tag (also used as the spec-driven parser tier label)
    pub grammar_name: &'static str,

    /// File extensions for this language (without dot)
    pub extensions: &'static [&'static str],

    /// Separator for qualified names (e.g., ".", "::", "\\")
    pub qname_separator: &'static str,
}

// ── Pre-defined specs for common languages ──

/// C# language spec
pub static CSHARP_SPEC: LangSpec = LangSpec {
    language: Language::CSharp,
    grammar_name: "c_sharp",
    extensions: &["cs"],
    qname_separator: ".",
};

/// PHP language spec
pub static PHP_SPEC: LangSpec = LangSpec {
    language: Language::Php,
    grammar_name: "php",
    extensions: &["php"],
    qname_separator: "\\",
};

/// Ruby language spec
pub static RUBY_SPEC: LangSpec = LangSpec {
    language: Language::Ruby,
    grammar_name: "ruby",
    extensions: &["rb", "rake"],
    qname_separator: "::",
};

/// Swift language spec
pub static SWIFT_SPEC: LangSpec = LangSpec {
    language: Language::Swift,
    grammar_name: "swift",
    extensions: &["swift"],
    qname_separator: ".",
};

/// Kotlin language spec
pub static KOTLIN_SPEC: LangSpec = LangSpec {
    language: Language::Kotlin,
    grammar_name: "kotlin",
    extensions: &["kt", "kts"],
    qname_separator: ".",
};

/// Dart language spec
pub static DART_SPEC: LangSpec = LangSpec {
    language: Language::Dart,
    grammar_name: "dart",
    extensions: &["dart"],
    qname_separator: ".",
};

/// Scala language spec
pub static SCALA_SPEC: LangSpec = LangSpec {
    language: Language::Scala,
    grammar_name: "scala",
    extensions: &["scala", "sc"],
    qname_separator: ".",
};

/// Lua language spec
pub static LUA_SPEC: LangSpec = LangSpec {
    language: Language::Lua,
    grammar_name: "lua",
    extensions: &["lua", "luau"],
    qname_separator: ".",
};

/// Get all predefined specs
pub fn all_specs() -> Vec<&'static LangSpec> {
    vec![
        &CSHARP_SPEC,
        &PHP_SPEC,
        &RUBY_SPEC,
        &SWIFT_SPEC,
        &KOTLIN_SPEC,
        &DART_SPEC,
        &SCALA_SPEC,
        &LUA_SPEC,
    ]
}

/// Find a spec by language
pub fn spec_for_language(lang: Language) -> Option<&'static LangSpec> {
    all_specs().into_iter().find(|s| s.language == lang)
}

/// Find a spec by file extension
pub fn spec_for_extension(ext: &str) -> Option<&'static LangSpec> {
    all_specs()
        .into_iter()
        .find(|s| s.extensions.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_have_correct_languages() {
        assert_eq!(CSHARP_SPEC.language, Language::CSharp);
        assert_eq!(PHP_SPEC.language, Language::Php);
        assert_eq!(RUBY_SPEC.language, Language::Ruby);
        assert_eq!(DART_SPEC.language, Language::Dart);
        assert_eq!(SCALA_SPEC.language, Language::Scala);
        assert_eq!(LUA_SPEC.language, Language::Lua);
    }

    #[test]
    fn spec_lookup_by_extension() {
        assert!(spec_for_extension("cs").is_some());
        assert!(spec_for_extension("php").is_some());
        assert!(spec_for_extension("unknown_ext").is_none());
    }

    #[test]
    fn all_specs_returns_expected_count() {
        assert_eq!(all_specs().len(), 8);
    }

    #[test]
    fn specs_have_required_fields() {
        for spec in all_specs() {
            assert!(
                !spec.extensions.is_empty(),
                "{:?} has no extensions",
                spec.language
            );
            assert!(
                !spec.grammar_name.is_empty(),
                "{:?} has no grammar name",
                spec.language
            );
        }
    }
}
