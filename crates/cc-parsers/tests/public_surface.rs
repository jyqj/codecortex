//! Independently authored surface goldens: names, visibility and change classes
//! are asserted directly, not inferred from the extractor's own fingerprint.
use cc_model::{
    public_surface::{PublicSurface, SurfaceKnowledge, VisibilityDomain},
    Language,
};
use cc_parsers::ParserRegistry;

fn parse(path: &str, source: &str, language: Language) -> PublicSurface {
    let s = ParserRegistry::new()
        .parse(path, source, language)
        .unwrap()
        .public_surface;
    s.validate().unwrap();
    s
}
fn rs(s: &str) -> PublicSurface {
    parse("src/lib.rs", s, Language::Rust)
}
fn ts(s: &str) -> PublicSurface {
    parse("api.ts", s, Language::TypeScript)
}
fn py(s: &str) -> PublicSurface {
    parse("pkg/api.py", s, Language::Python)
}
fn fp(s: &PublicSurface) -> String {
    s.fingerprint()
        .unwrap_or_else(|| panic!("unexpected unknown: {:?}", s.reasons))
}

#[test]
fn rust_signature_changes_but_private_and_public_bodies_do_not() {
    let a = rs("pub fn api(x: i32) -> i32 { x + 1 }\nfn helper() { let local = 1; }");
    assert_eq!(
        a.entries
            .iter()
            .map(|e| e.qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["api", "helper"]
    );
    let b = rs("// moved\npub fn api(x: i32) -> i32 { x * 123 }\nfn helper() { let other = 987; }");
    assert_eq!(fp(&a), fp(&b));
    assert_ne!(
        fp(&a),
        fp(&rs("pub fn api(x: i64) -> i32 { 1 }\nfn helper() {}"))
    );
}
#[test]
fn rust_lexical_visibility_domains_and_cfg_are_preserved() {
    let a=rs("pub fn a() {}\npub(crate) fn b() {}\npub(super) fn c() {}\npub(in crate::m) fn d() {}\nfn e() {}");
    assert_eq!(a.entries[0].visibility, VisibilityDomain::Exported);
    assert_eq!(a.entries[1].visibility, VisibilityDomain::Crate);
    assert!(matches!(
        a.entries[2].visibility,
        VisibilityDomain::Restricted(_)
    ));
    assert!(matches!(
        a.entries[3].visibility,
        VisibilityDomain::Restricted(_)
    ));
    assert_eq!(a.entries[4].visibility, VisibilityDomain::Module);
    let x = rs("#[cfg(feature = \"a\")] pub fn api() {}");
    let y = rs("#[cfg(feature = \"b\")] pub fn api() {}");
    assert_ne!(fp(&x), fp(&y));
    assert!(!x.entries[0].conditions.is_empty());
}
#[test]
fn rust_type_members_trait_and_impl_signatures_are_in_surface() {
    for (a, b) in [
        ("pub struct S { pub a: i32 }", "pub struct S { pub a: i64 }"),
        ("pub enum E { A(i32) }", "pub enum E { A(String) }"),
        (
            "pub trait T { fn f(&self) -> u32; }",
            "pub trait T { fn f(&self) -> u64; }",
        ),
        (
            "impl S { pub fn f(&self) -> i32 { 1 } }",
            "impl S { pub fn f(&self) -> i64 { 1 } }",
        ),
    ] {
        assert_ne!(fp(&rs(a)), fp(&rs(b)), "{a}");
    }
    assert_eq!(
        fp(&rs("impl S { fn helper(&self) { one(); } }")),
        fp(&rs("impl S { fn helper(&self) { two(); } }"))
    );
}
#[test]
fn rust_modules_forwarding_and_unknown_macros() {
    let a = rs("pub mod inner { pub fn api() {} }\npub use crate::base::Foo as Bar;");
    assert!(a.entries.iter().any(|e| e.qualified_name == "inner::api"));
    assert!(!a.forwards.is_empty());
    assert_ne!(
        fp(&a),
        fp(&rs(
            "pub mod inner { pub fn api() {} }\npub use crate::base::Foo as Baz;"
        ))
    );
    assert_eq!(rs("make_api!();").knowledge, SurfaceKnowledge::Unknown);
    assert_eq!(rs("pub fn broken(").knowledge, SurfaceKnowledge::Unknown);
    assert_eq!(
        rs("// no declarations").knowledge,
        SurfaceKnowledge::KnownEmpty
    );
}
#[test]
fn python_module_bindings_defaults_and_local_body_exclusion() {
    let a = py(
        "def f(value: int, suffix='a b') -> str:\n    local = 7\n    return suffix\n_PRIVATE = 5\n",
    );
    assert!(a.entries.iter().any(|e| e.qualified_name == "f"));
    assert!(a.entries.iter().any(|e| e.qualified_name == "_PRIVATE"));
    assert!(!a.entries.iter().any(|e| e.qualified_name == "local"));
    let b=py("# comment\ndef f(value: int, suffix='a b') -> str:\n    different = 999\n    return suffix + '!'\n_PRIVATE = 5\n");
    assert_eq!(fp(&a), fp(&b));
    assert_ne!(
        fp(&a),
        fp(&py(
            "def f(value: int, suffix='ab') -> str:\n    return suffix\n_PRIVATE = 5\n"
        ))
    );
}
#[test]
fn python_all_never_erases_explicit_private_bindings() {
    let a = py("__all__ = ['api']\ndef api(x):\n    return x\ndef _internal(y):\n    return y\n");
    let i = a
        .entries
        .iter()
        .find(|e| e.qualified_name == "_internal")
        .unwrap();
    assert_eq!(i.visibility, VisibilityDomain::Module);
    let empty = py("__all__ = []\n");
    assert!(empty.fingerprint().is_some());
    let dynamic = py("__all__ = make_names()\n");
    assert_eq!(dynamic.knowledge, SurfaceKnowledge::Unknown);
    assert_eq!(dynamic.fingerprint(), None);
    assert_eq!(
        py("__all__ = []\n__all__ += ['x']\n").knowledge,
        SurfaceKnowledge::Unknown
    );
    assert_eq!(
        py("def __getattr__(name):\n    return runtime(name)\n").knowledge,
        SurfaceKnowledge::Unknown
    );
}
#[test]
fn python_alias_and_star_forwarding_are_explicit() {
    let parser = ParserRegistry::new();
    let o = parser
        .parse(
            "pkg/__init__.py",
            "from .api import make as build\n__all__ = ['build']\n",
            Language::Python,
        )
        .unwrap();
    assert!(o
        .public_surface
        .forwards
        .iter()
        .any(|f| f.imported_name == "make" && f.exported_name == "build"));
    assert!(o.imports.iter().any(|i| i.is_reexport), "{:?}", o.imports);
    let star = parser
        .parse("pkg/__init__.py", "from .api import *\n", Language::Python)
        .unwrap();
    assert_eq!(star.public_surface.knowledge, SurfaceKnowledge::Unknown);
    assert!(star.imports.iter().any(|i| i.is_reexport));
    assert_eq!(
        py("\"\"\"Module docs\"\"\"\n# comment\n").knowledge,
        SurfaceKnowledge::KnownEmpty
    );
}
#[test]
fn python_class_members_change_not_method_bodies() {
    let a = py("class A:\n    def f(self, x: int) -> int:\n        return x\n");
    let b = py("class A:\n    def f(self, x: int) -> int:\n        return x + 1\n");
    assert_eq!(fp(&a), fp(&b));
    assert_ne!(
        fp(&a),
        fp(&py(
            "class A:\n    def f(self, x: str) -> int:\n        return 1\n"
        ))
    );
    assert!(a.entries.iter().any(|e| e.qualified_name == "A.f"));
}
#[test]
fn typescript_named_default_type_exports_have_independent_names() {
    let a=ts("type Local = { x: string };\nfunction f(x: number): number { return x; }\nexport type { Local as Public };\nexport { f as named };\nexport default f;\n");
    let mut names: Vec<_> = a.entries.iter().map(|e| e.exported_name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["Public", "default", "named"]);
    assert!(a.fingerprint().is_some());
    assert!(a
        .entries
        .iter()
        .any(|e| e.exported_name == "Public" && e.kind.starts_with("type_export:")));
}
#[test]
fn typescript_shapes_and_returns_vs_bodies() {
    let a = ts("export function f(x: number): string { return 'a'; }\nfunction helper() { a(); }");
    let b=ts("// moved\nexport function f(x: number): string { return 'b'; }\nfunction helper() { b(); }");
    assert_eq!(fp(&a), fp(&b));
    for (a, b) in [
        (
            "export type T = { a: string };",
            "export type T = { a: number };",
        ),
        (
            "export interface I { f(x: string): number; }",
            "export interface I { f(x: number): number; }",
        ),
        (
            "export class A { f(x: string): number { return 1; } }",
            "export class A { f(x: number): number { return 1; } }",
        ),
    ] {
        assert_ne!(fp(&ts(a)), fp(&ts(b)), "{a}");
    }
    assert_eq!(
        ts("export function inferred() { return 42; }").knowledge,
        SurfaceKnowledge::Unknown
    );
}
#[test]
fn typescript_direct_and_two_step_forwarding() {
    let a=ts("export type { Foo as Bar } from './a';\nexport * from './b';\nexport * as ns from './c';\nimport { value as local } from './d';\nexport { local as alias };\n");
    assert_eq!(a.forwards.len(), 4);
    assert!(a.fingerprint().is_some());
    assert!(a
        .forwards
        .iter()
        .any(|f| f.source == "./a" && f.exported_name == "Bar" && f.type_only));
    assert!(a
        .forwards
        .iter()
        .any(|f| f.source == "./c" && f.exported_name == "ns" && f.imported_name == "*"));
    assert!(a
        .forwards
        .iter()
        .any(|f| f.source == "./d" && f.exported_name == "alias" && f.imported_name == "value"));
    assert_eq!(ts("export {};").knowledge, SurfaceKnowledge::KnownEmpty);
}
#[test]
fn commonjs_static_object_and_forwarding() {
    let parser = ParserRegistry::new();
    let a = parser
        .parse(
            "api.js",
            "const token = 7; module.exports = { token };",
            Language::JavaScript,
        )
        .unwrap();
    assert_eq!(a.public_surface.knowledge, SurfaceKnowledge::Known);
    assert_eq!(a.public_surface.entries[0].exported_name, "token");
    let b = parser
        .parse(
            "api.js",
            "const { value: local } = require('./base'); module.exports = { alias: local };",
            Language::JavaScript,
        )
        .unwrap();
    assert!(b
        .public_surface
        .forwards
        .iter()
        .any(|f| f.source == "./base" && f.imported_name == "value" && f.exported_name == "alias"));
    assert!(b.imports.iter().any(|i| i.is_reexport), "{:?}", b.imports);
}
#[test]
fn dynamic_commonjs_cannot_masquerade_as_known_empty() {
    for source in [
        "exports[name] = value;",
        "if (flag) { exports.x = 1; }",
        "Object.assign(exports, source);",
        "module.exports = { ...other };",
        "const exports = {}; exports.a = 1;",
        "exports.a=1;exports.a=2;",
    ] {
        let s = parse("api.js", source, Language::JavaScript);
        assert_eq!(s.knowledge, SurfaceKnowledge::Unknown, "{source}: {s:?}");
        assert!(s.fingerprint().is_none());
    }
}
#[test]
fn generic_legacy_and_sfc_are_unknown_not_empty() {
    assert_eq!(
        parse("a.txt", "plain text", Language::Unknown).knowledge,
        SurfaceKnowledge::Unknown
    );
    assert_eq!(
        parse(
            "A.vue",
            "<script>export const a=1;</script><template>x</template>",
            Language::Vue
        )
        .knowledge,
        SurfaceKnowledge::Unknown
    );
    let mut value = serde_json::to_value(cc_model::ParseOutcome::default()).unwrap();
    value.as_object_mut().unwrap().remove("public_surface");
    let o: cc_model::ParseOutcome = serde_json::from_value(value).unwrap();
    assert_eq!(o.public_surface.knowledge, SurfaceKnowledge::Unknown);
}
#[test]
fn local_type_dependencies_cannot_hide_interface_changes() {
    let a = ts("type Local = { x: string }; export function f(x: Local): Local { return x; }");
    let b = ts("type Local = { x: number }; export function f(x: Local): Local { return x; }");
    assert!(
        b.changed_from(Some(&a)),
        "local alias changed but dependent exported API did not"
    );
}
#[test]
fn computed_commonjs_keys_with_constant_values_are_unknown() {
    let a = parse("api.js", "exports[name] = 7;", Language::JavaScript);
    assert_eq!(
        a.knowledge,
        SurfaceKnowledge::Unknown,
        "computed key was treated as property name"
    );
    let b = parse("api.js", "exports['name'] = 7;", Language::JavaScript);
    assert_eq!(b.knowledge, SurfaceKnowledge::Known);
}
#[test]
fn dynamic_or_order_dependent_module_bindings_are_not_certified() {
    for source in [
        "export let x = 1; x = runtime();",
        "const x = {}; x.key = 7; export { x };",
        "function f(): number {return 1;} function f(): number {return 2;} export {f};",
        "export { x as 'b\\\\u0062' } from './base';",
    ] {
        assert_eq!(ts(source).knowledge, SurfaceKnowledge::Unknown, "{source}");
    }
    for source in [
        "def f():\n    return 1\nx = f()\n",
        "def f(x):\n    pass\ndef f(y):\n    pass\n",
    ] {
        assert_eq!(py(source).knowledge, SurfaceKnowledge::Unknown, "{source}");
    }
}
#[test]
fn restricted_rust_forwarding_is_not_reported_as_unrestricted() {
    let a = rs("pub(crate) use crate::base::Foo as Bar;");
    assert!(!a.forwards.is_empty());
    assert!(a
        .forwards
        .iter()
        .all(|f| f.visibility == VisibilityDomain::Crate));
    assert_eq!(
        rs("#[allow_api_rewrite] pub fn f() {}").knowledge,
        SurfaceKnowledge::Unknown
    );
}

#[test]
fn surface_is_independent_of_thread_and_declaration_order() {
    let source =
        "export function z(): number { return 1; }\nexport function a(): number { return 2; }";
    let expected = fp(&ts(source));
    assert_eq!(
        expected,
        fp(&ts(
            "export function a(): number { return 2; }\nexport function z(): number { return 1; }"
        ))
    );
    let threads: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(move || (0..8).map(|_| fp(&ts(source))).collect::<Vec<_>>()))
        .collect();
    for thread in threads {
        assert!(thread.join().unwrap().iter().all(|h| *h == expected));
    }
}
