use cc_model::{
    public_surface::{SurfaceKnowledge, VisibilityDomain},
    Language,
};
use cc_parsers::ParserRegistry;
fn go(source: &str) -> cc_model::public_surface::PublicSurface {
    ParserRegistry::new()
        .parse("pkg/api.go", source, Language::Go)
        .unwrap()
        .public_surface
}
#[test]
fn go_declared_package_surface_is_known_and_body_independent() {
    let a=go("package pkg\nfunc Exported(x int) int { return x }\nfunc internal() { local := 1; _ = local }\n");
    assert_eq!(a.knowledge, SurfaceKnowledge::Known);
    assert!(a
        .entries
        .iter()
        .any(|e| e.exported_name == "Exported" && e.visibility == VisibilityDomain::Exported));
    assert!(!a.entries.iter().any(|e| e.qualified_name == "local"));
    let b=go("package pkg\nfunc Exported(x int) int { y := x + 1; return y }\nfunc internal() { other := 2; _ = other }\n");
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_ne!(
        a.fingerprint(),
        go("package pkg\nfunc Exported(x string) int { return 0 }\nfunc internal() {}\n")
            .fingerprint()
    );
}
#[test]
fn go_types_methods_aliases_and_package_name_change_surface() {
    for (a, b) in [
        (
            "package pkg\ntype Item struct { Value int }\n",
            "package pkg\ntype Item struct { Value string }\n",
        ),
        (
            "package pkg\ntype Item int\nfunc (i *Item) Get() int { return 1 }\n",
            "package pkg\ntype Item int\nfunc (i *Item) Get() string { return \"\" }\n",
        ),
        (
            "package pkg\ntype Name = string\n",
            "package pkg\ntype Name string\n",
        ),
        ("package pkg\nconst A = 1\n", "package other\nconst A = 1\n"),
    ] {
        let x = go(a);
        let y = go(b);
        assert!(x.fingerprint().is_some(), "{x:?}");
        assert_ne!(x.fingerprint(), y.fingerprint(), "{a}");
    }
    assert_eq!(
        go("package pkg\ntype Item int\nfunc (i *Item) Get() int { return 1 }\n").fingerprint(),
        go("package pkg\ntype Item int\nfunc (i *Item) Get() int { return 2 }\n").fingerprint()
    );
}
#[test]
fn go_unmodeled_conditions_and_cgo_are_unknown() {
    for s in [
        "//go:build linux\n\npackage pkg\nfunc Exported() {}\n",
        "package pkg\nimport \"C\"\n",
        "package pkg\nimport . \"example.org/lib\"\n",
    ] {
        let v = go(s);
        assert_eq!(v.knowledge, SurfaceKnowledge::Unknown, "{s}");
        assert!(v.fingerprint().is_none());
    }
}
#[test]
fn es_import_records_preserve_individual_binding_identity() {
    let o=ParserRegistry::new().parse("use.ts","import Default, { make as build, type Shape } from './api'; import * as ns from './other'; import './side';",Language::TypeScript).unwrap();
    assert_eq!(o.imports.len(), 5);
    let build = o
        .imports
        .iter()
        .find(|i| i.alias.as_deref() == Some("build"))
        .unwrap();
    assert_eq!(build.imported_name.as_deref(), Some("make"));
    assert_eq!(build.import_string, "./api");
    assert!(o
        .imports
        .iter()
        .any(|i| i.alias.as_deref() == Some("Default") && i.is_default));
    assert!(o
        .imports
        .iter()
        .any(|i| i.alias.as_deref() == Some("ns") && i.is_namespace));
    assert!(o
        .imports
        .iter()
        .any(|i| i.import_string == "./side" && i.imported_name.is_none()));
}

#[test]
fn unsupported_surface_names_its_language_and_capability() {
    for (path, lang, source) in [
        ("Api.java", Language::Java, "public class Api {}"),
        ("api.cpp", Language::Cpp, "int api();"),
        ("api.cs", Language::CSharp, "class Api {}"),
    ] {
        let s = ParserRegistry::new()
            .parse(path, source, lang)
            .unwrap()
            .public_surface;
        assert_eq!(s.knowledge, SurfaceKnowledge::Unknown);
        assert_eq!(s.language, lang.as_str());
        assert_eq!(s.module, path);
        assert!(s.extractor_version.starts_with("conservative-"));
    }
}
