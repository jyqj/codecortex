//! Review regressions for falsely-known interfaces. Expectations are authored
//! from language syntax, not copied from the extractor's output.
use cc_model::{
    public_surface::{PublicSurface, SurfaceKnowledge},
    Language,
};
use cc_parsers::ParserRegistry;
fn surface(path: &str, text: &str, language: Language) -> PublicSurface {
    let value = ParserRegistry::new()
        .parse(path, text, language)
        .unwrap()
        .public_surface;
    value.validate().unwrap();
    value
}
fn ts(text: &str) -> PublicSurface {
    surface("api.ts", text, Language::TypeScript)
}
fn py(text: &str) -> PublicSurface {
    surface("api.py", text, Language::Python)
}
fn rs(text: &str) -> PublicSurface {
    surface("api.rs", text, Language::Rust)
}
fn known(value: &PublicSurface) -> String {
    value
        .fingerprint()
        .unwrap_or_else(|| panic!("{:?}", value.reasons))
}
fn unknown(value: PublicSurface) {
    assert_eq!(
        value.knowledge,
        SurfaceKnowledge::Unknown,
        "falsely known: {value:?}"
    );
    assert!(value.fingerprint().is_none());
}
#[test]
fn exported_binding_mutability_changes_fingerprint() {
    for (a, b) in [
        ("export const value = 1;", "export let value = 1;"),
        (
            "const value = 1; export { value };",
            "let value = 1; export { value };",
        ),
        (
            "const value = 1; export { value };",
            "var value = 1; export { value };",
        ),
    ] {
        assert_ne!(
            known(&ts(a)),
            known(&ts(b)),
            "binding qualifier lost: {a} vs {b}"
        );
    }
}
#[test]
fn ambient_global_augmentation_is_not_known_empty() {
    unknown(ts(
        "export {}; declare global { interface Window { token: string; } }",
    ));
    unknown(ts(
        "export {}; declare module 'external' { export const token: string; }",
    ));
}
#[test]
fn dynamic_default_parameter_is_not_skipped_with_function_body() {
    unknown(ts("function defaultValue(): number { return 1; } export function api(x = defaultValue()): number { return x; }"));
}
#[test]
fn commonjs_detached_exports_alias_is_not_certified() {
    unknown(surface(
        "api.js",
        "module.exports = {}; exports.extra = 1;",
        Language::JavaScript,
    ));
    // Direct writes through module.exports after replacing it still target the
    // live object and remain in the explicitly supported static subset.
    assert!(surface(
        "api.js",
        "module.exports = {}; module.exports.extra = 1;",
        Language::JavaScript
    )
    .fingerprint()
    .is_some());
}
#[test]
fn rust_associated_const_functions_require_evaluation() {
    unknown(rs("pub struct S; impl S { pub const fn size() -> usize { 1 } } pub type Bytes = [u8; S::size()];"));
}
#[test]
fn p2a_closeout_nested_rust_attributes_are_not_certified() {
    for source in [
        "pub struct S; impl S { #[transform] pub fn api(&self) -> i32 { 1 } }",
        "pub trait T { #[cfg_attr(feature = \"x\", transform)] fn api(&self) -> i32 { 1 } }",
    ] {
        unknown(rs(source));
    }
    // Inert built-in attributes keep an ordinary, explicitly typed method known.
    known(&rs(
        "pub struct S; impl S { #[inline] pub fn api(&self) -> i32 { 1 } }",
    ));
}
#[test]
fn p2a_closeout_es_computed_properties_are_not_certified() {
    for source in [
        "export const api = { [globalThis.key]: 1 };",
        "export class Api { [globalThis.key](): number { return 1; } }",
    ] {
        unknown(ts(source));
    }
    known(&ts("export const api = { key: 1 };"));
    known(&ts("export class Api { key(): number { return 1; } }"));
}
#[test]
fn python_all_requires_existing_static_sequence_bindings() {
    unknown(py("__all__ = ['missing']\n"));
    unknown(py("__all__ = {'value'}\nvalue = 1\n"));
    assert!(py("__all__ = ['value']\nvalue = 1\n")
        .fingerprint()
        .is_some());
    assert!(py("__all__ = ('value',)\nfrom api_impl import value\n")
        .fingerprint()
        .is_some());
}
#[test]
fn python_generator_shape_changes_without_hashing_ordinary_body() {
    let plain = py("def api():\n    return 1\n");
    let generator = py("def api():\n    yield 1\n");
    assert!(
        generator.changed_from(Some(&plain)),
        "callable/generator distinction lost"
    );
    assert_eq!(
        known(&plain),
        known(&py(
            "def api():\n    def nested():\n        yield 1\n    return 2\n"
        )),
        "nested generator is not the outer callable shape"
    );
}
