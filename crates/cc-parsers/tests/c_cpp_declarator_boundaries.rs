use cc_model::{source::SourceSnapshot, Language, SymbolKind};
use cc_parsers::{chunker::boundaries, ParserRegistry};

fn tree(text: &str, cpp: bool) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    let language = if cpp {
        tree_sitter_cpp::LANGUAGE.into()
    } else {
        tree_sitter_c::LANGUAGE.into()
    };
    parser.set_language(&language).unwrap();
    let tree = parser.parse(text, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    tree
}

#[test]
fn c_cpp_names_follow_declarators_without_parser_hints() {
    for (cpp, text, expected) in [
        (false, "struct Return {}; struct Return free_leaf(int x) { struct Return r; return r; }", vec!["free_leaf"]),
        (false, "int *pointer_leaf(void) { return 0; } int (*factory(void))(int) { return 0; }", vec!["pointer_leaf", "factory"]),
        (true, "namespace grove { template<class T> T leaf(T x) { return x; } }", vec!["leaf"]),
        (true, "namespace grove { namespace inner { int nested(int x) { return x; } } }", vec!["nested"]),
        (true, "struct Box { struct Return {}; Return member() { return {}; } ~Box() {} int operator+(int x) { return x; } };", vec!["member", "~Box", "operator+"]),
        (true, "int Box::member() { return 1; } Box::~Box() {} int Box::operator+(int x) { return x; }", vec!["member", "~Box", "operator+"]),
        (true, "template<> int leaf<int>(int x) { return x; }", vec!["leaf"]),
        (true, "int &reference_leaf() { static int x; return x; } int (*factory())(int) { return nullptr; }", vec!["reference_leaf", "factory"]),
    ] {
        let tree = tree(text, cpp);
        let out = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), &[]);
        let names: Vec<_> = out.boundaries.iter().filter(|b| matches!(b.symbol_kind, Some(SymbolKind::Function | SymbolKind::Method))).map(|b| b.name.as_deref()).collect();
        assert_eq!(names, expected.into_iter().map(Some).collect::<Vec<_>>(), "{text}");
    }
}

#[test]
fn rejected_namespace_kind_and_poisoned_callable_names_use_ast_name() {
    let text = "namespace grove { template<class T> T leaf(T x) { return x; } }";
    let parsed = ParserRegistry::new()
        .parse("micro.cpp", text, Language::Cpp)
        .unwrap();
    let native = parsed.symbols.iter().find(|s| s.name == "leaf").unwrap();
    assert_eq!(native.kind, SymbolKind::Function);
    let tree = tree(text, true);
    for poison in [None, Some("T"), Some("poison")] {
        let mut hints = parsed.symbols.clone();
        let hint = hints.iter_mut().find(|s| s.name == "leaf").unwrap();
        // Namespace symbols are now correct. Keep exercising the rejected
        // Method hint explicitly rather than depending on the old parser bug.
        hint.kind = SymbolKind::Method;
        if let Some(name) = poison {
            hint.name = name.into();
            hint.kind = SymbolKind::Function;
        }
        let out = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), &hints);
        let callable = out
            .boundaries
            .iter()
            .find(|b| b.symbol_kind == Some(SymbolKind::Function))
            .unwrap();
        assert_eq!(callable.name.as_deref(), Some("leaf"));
    }
}

#[test]
fn unknown_conversion_declarator_omits_name_even_with_native_hint() {
    let text = "struct Box { operator int() const { return 1; } };";
    let parsed = ParserRegistry::new()
        .parse("micro.cpp", text, Language::Cpp)
        .unwrap();
    let tree = tree(text, true);
    for hints in [&[][..], parsed.symbols.as_slice()] {
        let out = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), hints);
        let callable = out
            .boundaries
            .iter()
            .find(|b| b.symbol_kind == Some(SymbolKind::Method))
            .unwrap();
        assert_eq!(callable.name, None);
    }
}

#[test]
fn compatible_qualified_members_keep_native_names_and_kinds() {
    let text = "namespace grove { class Box { public: int member() { return 1; } }; } int Box::member() { return 2; } Box::~Box() {} int Box::operator+(int x) { return x; }";
    let parsed = ParserRegistry::new()
        .parse("micro.cpp", text, Language::Cpp)
        .unwrap();
    // The native parser labels top-level qualified definitions Function. Supply
    // the already-supported Method refinement to exercise its name guard.
    let mut hints = parsed.symbols.clone();
    for hint in &mut hints {
        hint.kind = if hint.kind == SymbolKind::Function {
            SymbolKind::Method
        } else {
            hint.kind
        };
    }
    let tree = tree(text, true);
    let out = boundaries::extract(&tree, &SourceSnapshot::new(text.as_bytes()), &hints);
    for name in ["member", "~Box", "operator+"] {
        assert!(
            out.boundaries
                .iter()
                .any(|b| b.name.as_deref() == Some(name)
                    && b.symbol_kind == Some(SymbolKind::Method)),
            "{name}: {out:#?}"
        );
    }
}
