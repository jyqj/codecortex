//! Closed language capability policy. A syntax parser is not proof that its
//! public interface extractor exists. No unsupported language is KnownEmpty.
use cc_model::{public_surface::PublicSurface, Language};
pub(crate) fn fallback(language: Language, path: &str) -> PublicSurface {
    let reason = match language {
        Language::Java => "java_classpath_surface_not_modeled",
        Language::C | Language::Cpp => "c_cpp_preprocessor_template_surface_not_modeled",
        Language::Vue | Language::Svelte => "component_surface_not_modeled",
        Language::Python
        | Language::JavaScript
        | Language::TypeScript
        | Language::Tsx
        | Language::Jsx
        | Language::Rust
        | Language::Go => "declared_surface_extractor_unavailable",
        Language::CSharp
        | Language::Php
        | Language::Ruby
        | Language::Swift
        | Language::Kotlin
        | Language::Dart
        | Language::Scala
        | Language::Lua => "heuristic_parser_surface_not_modeled",
        Language::Markdown
        | Language::Sql
        | Language::Yaml
        | Language::Toml
        | Language::Hcl
        | Language::Dockerfile
        | Language::Bash
        | Language::Protobuf
        | Language::GraphQL
        | Language::CMake
        | Language::Unknown => "generic_or_data_surface_not_modeled",
    };
    let mut value = PublicSurface::new(language.as_str(), path, "conservative-v1");
    value.mark_unknown(reason);
    value.normalize();
    value
}
