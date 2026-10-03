//! Independent final P2 probes, not the author's four tests.
use cc_model::{source::SourceSnapshot, Language, SymbolKind};
use cc_parsers::{chunker::boundaries, ParserRegistry};
fn tree(text: &str, cpp: bool) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&if cpp {
            tree_sitter_cpp::LANGUAGE.into()
        } else {
            tree_sitter_c::LANGUAGE.into()
        })
        .unwrap();
    let tree = parser.parse(text, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    tree
}
#[test]
fn parenthesized_pointer_return_does_not_accept_return_or_parameter_poison() {
    let text="struct Token { int value; }; struct Token *(*harvest(unsigned decoy))(double argument) { return 0; }\n";
    let tree = tree(text, false);
    for name in ["Token", "decoy", "argument"] {
        // Native C symbol extraction does not catalog this nested pointer form.
        // Exercise the boundary hint guard explicitly, without inventing a
        // persisted/public identity or claiming parser support.
        let plain = ParserRegistry::new()
            .parse("final.c", "int seed(void) { return 0; }", Language::C)
            .unwrap();
        let mut hint = plain
            .symbols
            .iter()
            .find(|s| s.name == "seed")
            .unwrap()
            .clone();
        let source = SourceSnapshot::new(text.as_bytes());
        let unhinted = boundaries::extract(&tree, &source, &[]);
        let owner = unhinted
            .boundaries
            .iter()
            .find(|b| b.name.as_deref() == Some("harvest"))
            .unwrap();
        let start = source.point(owner.span.start).unwrap();
        hint.start_line = start.0 as u32;
        hint.start_col = start.1 as u32;
        hint.name = name.into();
        hint.kind = SymbolKind::Function;
        let hints = vec![hint];
        let out = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), &hints);
        let callable = out
            .boundaries
            .iter()
            .find(|b| b.symbol_kind == Some(SymbolKind::Function))
            .unwrap();
        assert_eq!(callable.name.as_deref(), Some("harvest"));
    }
}
#[test]
fn excessive_declarator_edges_omit_name_without_type_or_parameter_fallback() {
    let text = format!("long {}buried(int decoy) {{ return 0; }}\n", "*".repeat(70));
    let tree = tree(&text, false);
    let out = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), &[]);
    assert!(out.complete);
    let callable = out
        .boundaries
        .iter()
        .find(|b| b.symbol_kind == Some(SymbolKind::Function))
        .unwrap();
    assert!(
        callable.name.is_none(),
        "bounded unknown shape must omit name: {callable:?}"
    );
}
#[test]
fn unknown_conversion_operator_keeps_name_omitted_even_with_poisoned_native_hint() {
    let text="struct Capsule { explicit operator const char*() const { return nullptr; } int known(int decoy) { return decoy; } };\n";
    let parsed = ParserRegistry::new()
        .parse("final.cpp", text, Language::Cpp)
        .unwrap();
    let tree = tree(text, true);
    let initial = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), &[]);
    let conversion = initial
        .boundaries
        .iter()
        .find(|b| b.symbol_kind == Some(SymbolKind::Method) && b.name.is_none())
        .unwrap();
    let source = SourceSnapshot::new(text.as_bytes());
    let start = source.point(conversion.span.start).unwrap();
    let mut hint = parsed
        .symbols
        .iter()
        .find(|s| s.name == "known")
        .unwrap()
        .clone();
    hint.name = "char".into();
    hint.start_line = start.0 as u32;
    hint.start_col = start.1 as u32;
    let out = boundaries::extract(&tree, &source, &[hint]);
    let conversion = out
        .boundaries
        .iter()
        .find(|b| b.span == conversion.span)
        .unwrap();
    assert!(conversion.name.is_none());
    assert!(out
        .boundaries
        .iter()
        .any(|b| b.name.as_deref() == Some("known") && b.symbol_kind == Some(SymbolKind::Method)));
}
