use cc_model::{source::SourceSnapshot, Language, SymbolKind};
use cc_parsers::{chunker::boundaries, ParserRegistry};

#[test]
fn wrong_wrapper_and_inner_hints_cannot_reclassify_python_declarations() {
    let text = "@decorate\nclass Outer:\n    @decorate\n    class Inner:\n        @staticmethod\n        def pulse():\n            def local(): return 1\n            return local()\n";
    let parsed = ParserRegistry::new()
        .parse("a.py", text, Language::Python)
        .unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(text, None).unwrap();
    let source = SourceSnapshot::new(text.as_bytes());
    for wrong in [
        SymbolKind::Function,
        SymbolKind::Method,
        SymbolKind::Variable,
    ] {
        let mut hints = parsed.symbols.clone();
        for hint in hints.iter_mut().filter(|s| s.kind == SymbolKind::Class) {
            hint.kind = wrong;
            hint.name = "poison".into();
        }
        // A valid inner hint must still work after a rejected wrapper hint.
        let mut inner = parsed
            .symbols
            .iter()
            .find(|s| s.name == "Inner")
            .unwrap()
            .clone();
        inner.start_line += 1;
        hints.push(inner);
        let out = boundaries::extract(&tree, &source, &hints);
        assert!(out.complete);
        for (name, kind) in [
            ("Outer", SymbolKind::Class),
            ("Inner", SymbolKind::Class),
            ("pulse", SymbolKind::Method),
            ("local", SymbolKind::Function),
        ] {
            let b = out
                .boundaries
                .iter()
                .find(|b| b.name.as_deref() == Some(name))
                .unwrap();
            assert_eq!(b.symbol_kind, Some(kind));
        }
        assert!(!out
            .boundaries
            .iter()
            .any(|b| b.name.as_deref() == Some("poison")));
    }
    for poison_name in [false, true] {
        let mut hints = parsed.symbols.clone();
        for hint in &mut hints {
            if poison_name {
                hint.name = "poison".into();
            } else if hint.kind == SymbolKind::Method {
                hint.kind = SymbolKind::Function;
            } else if hint.kind == SymbolKind::Function {
                hint.kind = SymbolKind::Method;
            }
        }
        let out = boundaries::extract(&tree, &source, &hints);
        for (name, kind) in [
            ("Outer", SymbolKind::Class),
            ("Inner", SymbolKind::Class),
            ("pulse", SymbolKind::Method),
            ("local", SymbolKind::Function),
        ] {
            assert_eq!(
                out.boundaries
                    .iter()
                    .find(|b| b.name.as_deref() == Some(name))
                    .unwrap()
                    .symbol_kind,
                Some(kind)
            );
        }
    }
    let mut hints = parsed.symbols.clone();
    for hint in &mut hints {
        hint.kind = if hint.kind == SymbolKind::Class {
            SymbolKind::Method
        } else {
            SymbolKind::Class
        };
    }
    let out = boundaries::extract(&tree, &source, &hints);
    for (name, kind) in [
        ("Outer", SymbolKind::Class),
        ("Inner", SymbolKind::Class),
        ("pulse", SymbolKind::Method),
        ("local", SymbolKind::Function),
    ] {
        assert_eq!(
            out.boundaries
                .iter()
                .find(|b| b.name.as_deref() == Some(name))
                .unwrap()
                .symbol_kind,
            Some(kind)
        );
    }
}
#[test]
fn boundary_guard_preserves_other_languages_native_declaration_kinds() {
    for (path, language, text, expected) in [
        ("a.rs", Language::Rust, "struct Boxed; impl Boxed { fn pulse(&self) {} } fn free() {}", vec![("Boxed", SymbolKind::Class), ("pulse", SymbolKind::Method), ("free", SymbolKind::Function)]),
        ("a.ts", Language::TypeScript, "export class Boxed { pulse() {} } export const arrow = () => 1; interface View {} type Alias = string;", vec![("Boxed", SymbolKind::Class), ("pulse", SymbolKind::Method), ("arrow", SymbolKind::Function), ("View", SymbolKind::Interface), ("Alias", SymbolKind::TypeAlias)]),
        ("a.go", Language::Go, "package a\ntype Boxed struct {}\ntype View interface { Pulse() }\ntype Alias = string\nfunc (b Boxed) pulse() {}\nfunc free() {}", vec![("Boxed", SymbolKind::Class), ("View", SymbolKind::Interface), ("Alias", SymbolKind::TypeAlias), ("pulse", SymbolKind::Method), ("free", SymbolKind::Function)]),
        ("A.java", Language::Java, "class Boxed { void pulse() {} } interface View {}", vec![("Boxed", SymbolKind::Class), ("pulse", SymbolKind::Method), ("View", SymbolKind::Interface)]),
        ("a.cpp", Language::Cpp, "namespace N { class Boxed { public: int pulse() { return 1; } }; }", vec![("N", SymbolKind::Namespace), ("Boxed", SymbolKind::Class), ("pulse", SymbolKind::Method)]),
    ] {
        let parsed = ParserRegistry::new().parse(path, text, language).unwrap();
        let structure = parsed.source_structure.as_ref().unwrap();
        assert!(structure.complete, "{path}");
        for (name, kind) in expected {
            let b = structure.boundaries.iter().find(|b| b.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("{path} missing {name}: {structure:#?}"));
            assert_eq!(b.symbol_kind, Some(kind), "{path} {name}");
        }
    }
}
