//! Table-driven language specification framework.
//!
//! Inspired by codebase-memory-mcp's CBMLangSpec approach: each language
//! defines a static spec table mapping tree-sitter node types to semantic
//! roles (function, class, method, call, import, etc.).
//!
//! This allows adding new language support with minimal code — just a
//! LangSpec configuration and a tree-sitter grammar dependency.

use cc_model::Language;

/// Language specification — table-driven tree-sitter node type mapping
#[derive(Debug, Clone)]
pub struct LangSpec {
    /// Target language
    pub language: Language,

    /// tree-sitter grammar factory function
    /// (wrapped as string name for now; actual grammar linking happens at registration)
    pub grammar_name: &'static str,

    // ── Definition node types ──
    /// Node types that represent function definitions
    pub function_types: &'static [&'static str],
    /// Node types that represent class definitions
    pub class_types: &'static [&'static str],
    /// Node types that represent method definitions (inside classes)
    pub method_types: &'static [&'static str],
    /// Node types that represent field/property definitions
    pub field_types: &'static [&'static str],
    /// Node types that represent module/namespace definitions
    pub module_types: &'static [&'static str],

    // ── Reference node types ──
    /// Node types that represent function/method calls
    pub call_types: &'static [&'static str],
    /// Node types that represent import statements
    pub import_types: &'static [&'static str],

    // ── Name extraction rules ──
    /// Child field name for extracting definition name (e.g., "name", "declarator")
    pub name_field: &'static str,
    /// Child field name for parameters (e.g., "parameters", "formal_parameters")
    pub params_field: &'static str,
    /// Child field name for function/class body (e.g., "body", "block")
    pub body_field: &'static str,
    /// Child field name for return type annotation (if language supports it)
    pub return_type_field: Option<&'static str>,
    /// Child field name for superclass/base class
    pub superclass_field: Option<&'static str>,

    // ── Patterns ──
    /// File extensions for this language (without dot)
    pub extensions: &'static [&'static str],
    /// Whether the language has significant whitespace (like Python)
    pub indent_sensitive: bool,
    /// Separator for qualified names (e.g., ".", "::", "/")
    pub qname_separator: &'static str,
}

// ── Pre-defined specs for common languages ──

/// C# language spec
pub static CSHARP_SPEC: LangSpec = LangSpec {
    language: Language::CSharp,
    grammar_name: "c_sharp",
    function_types: &["method_declaration", "local_function_statement"],
    class_types: &[
        "class_declaration",
        "interface_declaration",
        "struct_declaration",
        "enum_declaration",
        "record_declaration",
    ],
    method_types: &["method_declaration", "constructor_declaration"],
    field_types: &["field_declaration", "property_declaration"],
    module_types: &["namespace_declaration"],
    call_types: &["invocation_expression", "object_creation_expression"],
    import_types: &["using_directive"],
    name_field: "name",
    params_field: "parameters",
    body_field: "body",
    return_type_field: Some("type"),
    superclass_field: Some("base_list"),
    extensions: &["cs"],
    indent_sensitive: false,
    qname_separator: ".",
};

/// PHP language spec
pub static PHP_SPEC: LangSpec = LangSpec {
    language: Language::Php,
    grammar_name: "php",
    function_types: &["function_definition"],
    class_types: &[
        "class_declaration",
        "interface_declaration",
        "trait_declaration",
        "enum_declaration",
    ],
    method_types: &["method_declaration"],
    field_types: &["property_declaration"],
    module_types: &["namespace_definition"],
    call_types: &[
        "function_call_expression",
        "method_call_expression",
        "static_call_expression",
    ],
    import_types: &[
        "use_declaration",
        "require_expression",
        "include_expression",
    ],
    name_field: "name",
    params_field: "parameters",
    body_field: "body",
    return_type_field: Some("return_type"),
    superclass_field: Some("base_clause"),
    extensions: &["php"],
    indent_sensitive: false,
    qname_separator: "\\",
};

/// Ruby language spec
pub static RUBY_SPEC: LangSpec = LangSpec {
    language: Language::Ruby,
    grammar_name: "ruby",
    function_types: &["method"],
    class_types: &["class", "module"],
    method_types: &["method", "singleton_method"],
    field_types: &[],
    module_types: &["module"],
    call_types: &["call", "method_call"],
    import_types: &["call"], // require/require_relative are method calls in Ruby
    name_field: "name",
    params_field: "parameters",
    body_field: "body",
    return_type_field: None,
    superclass_field: Some("superclass"),
    extensions: &["rb", "rake"],
    indent_sensitive: false,
    qname_separator: "::",
};

/// Swift language spec
pub static SWIFT_SPEC: LangSpec = LangSpec {
    language: Language::Swift,
    grammar_name: "swift",
    function_types: &["function_declaration"],
    class_types: &[
        "class_declaration",
        "struct_declaration",
        "protocol_declaration",
        "enum_declaration",
    ],
    method_types: &["function_declaration"], // methods are function_declarations inside class body
    field_types: &["property_declaration"],
    module_types: &[],
    call_types: &["call_expression"],
    import_types: &["import_declaration"],
    name_field: "name",
    params_field: "parameters",
    body_field: "body",
    return_type_field: Some("return_type"),
    superclass_field: Some("type_inheritance_clause"),
    extensions: &["swift"],
    indent_sensitive: false,
    qname_separator: ".",
};

/// Kotlin language spec
pub static KOTLIN_SPEC: LangSpec = LangSpec {
    language: Language::Kotlin,
    grammar_name: "kotlin",
    function_types: &["function_declaration"],
    class_types: &[
        "class_declaration",
        "object_declaration",
        "interface_declaration",
    ],
    method_types: &["function_declaration"],
    field_types: &["property_declaration"],
    module_types: &["package_header"],
    call_types: &["call_expression"],
    import_types: &["import_header"],
    name_field: "simple_identifier",
    params_field: "function_value_parameters",
    body_field: "function_body",
    return_type_field: Some("user_type"),
    superclass_field: Some("delegation_specifiers"),
    extensions: &["kt", "kts"],
    indent_sensitive: false,
    qname_separator: ".",
};

/// Dart language spec
pub static DART_SPEC: LangSpec = LangSpec {
    language: Language::Dart,
    grammar_name: "dart",
    function_types: &["function_signature", "method_signature"],
    class_types: &[
        "class_definition",
        "enum_declaration",
        "mixin_declaration",
        "extension_declaration",
    ],
    method_types: &["method_signature"],
    field_types: &["declaration"],
    module_types: &["library_name"],
    call_types: &["function_expression_body"],
    import_types: &["import_or_export"],
    name_field: "name",
    params_field: "formal_parameter_list",
    body_field: "class_body",
    return_type_field: Some("type_identifier"),
    superclass_field: Some("superclass"),
    extensions: &["dart"],
    indent_sensitive: false,
    qname_separator: ".",
};

/// Scala language spec
pub static SCALA_SPEC: LangSpec = LangSpec {
    language: Language::Scala,
    grammar_name: "scala",
    function_types: &["function_definition"],
    class_types: &[
        "class_definition",
        "object_definition",
        "trait_definition",
        "enum_definition",
    ],
    method_types: &["function_definition"],
    field_types: &["val_definition", "var_definition"],
    module_types: &["package_clause"],
    call_types: &["call_expression"],
    import_types: &["import_declaration"],
    name_field: "name",
    params_field: "parameters",
    body_field: "body",
    return_type_field: Some("type_identifier"),
    superclass_field: Some("extends_clause"),
    extensions: &["scala", "sc"],
    indent_sensitive: false,
    qname_separator: ".",
};

/// Lua language spec
pub static LUA_SPEC: LangSpec = LangSpec {
    language: Language::Lua,
    grammar_name: "lua",
    function_types: &[
        "function_declaration",
        "local_function_declaration_statement",
    ],
    class_types: &[],
    method_types: &["function_declaration"],
    field_types: &["assignment_statement", "local_declaration"],
    module_types: &[],
    call_types: &["function_call"],
    import_types: &["function_call"], // require() calls
    name_field: "name",
    params_field: "parameters",
    body_field: "body",
    return_type_field: None,
    superclass_field: None,
    extensions: &["lua", "luau"],
    indent_sensitive: false,
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
                !spec.function_types.is_empty(),
                "{:?} has no function types",
                spec.language
            );
            assert!(
                !spec.call_types.is_empty(),
                "{:?} has no call types",
                spec.language
            );
            assert!(
                !spec.extensions.is_empty(),
                "{:?} has no extensions",
                spec.language
            );
        }
    }
}
