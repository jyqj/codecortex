//! Scope A: synthetic, parser-only namespace free-function contracts.
use cc_model::{id::StableId, symbol::SymbolRecord, Language, ParseOutcome, SymbolKind};
use cc_parsers::ParserRegistry;
use serde_json::{json, Value};

fn parse(text: &str) -> ParseOutcome {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(text, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    ParserRegistry::new()
        .parse("taxonomy.cpp", text, Language::Cpp)
        .unwrap()
}

fn find<'a>(out: &'a ParseOutcome, name: &str) -> &'a SymbolRecord {
    out.symbols.iter().find(|s| s.name == name).unwrap()
}

fn fields(s: &SymbolRecord) -> Value {
    json!({
        "name": s.name, "kind": s.kind, "container": s.container, "qname": s.qname,
        "signature": s.signature, "symbol_id": s.symbol_id, "symbol_uid": s.symbol_uid,
        "start_line": s.start_line, "start_col": s.start_col, "end_line": s.end_line, "end_col": s.end_col
    })
}

#[test]
fn namespace_repair_changes_only_approved_baseline_fields() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/cpp_namespace_baseline.json")).unwrap();
    for case in cases {
        let label = case["case"].as_str().unwrap();
        let text = case["source"].as_str().unwrap();
        let mut expected = case["symbols"].as_array().unwrap().clone();
        for (index, sym) in expected.iter_mut().enumerate() {
            let qname = match (label, sym["name"].as_str()) {
                ("namespace" | "template_namespace", Some("leaf")) => Some("grove::leaf"),
                ("nested_namespace" | "cpp17_namespace", Some("leaf")) => {
                    Some("grove::inner::leaf")
                }
                ("inline_namespace", Some("leaf")) => Some("grove::v1::leaf"),
                ("same_tail_namespaces", Some("leaf")) if index == 2 => Some("left::shared::leaf"),
                ("same_tail_namespaces", Some("leaf")) => Some("right::shared::leaf"),
                _ => None,
            };
            if let Some(qname) = qname {
                sym["kind"] = json!("function");
                sym["qname"] = json!(qname);
                sym["symbol_uid"] = json!(StableId::symbol_uid(
                    "taxonomy.cpp",
                    qname,
                    "function",
                    sym["signature"].as_str()
                ));
            }
        }
        let out = parse(text);
        assert_eq!(
            out.symbols.iter().map(fields).collect::<Vec<_>>(),
            expected,
            "{label}"
        );
        // Kind repair admits only the already existing Function boundary hint.
        let boundaries: Vec<_> = out
            .source_structure
            .as_ref()
            .unwrap()
            .boundaries
            .iter()
            .filter(|b| b.name.is_some() || b.symbol_kind.is_some())
            .map(|b| json!({"name": b.name, "kind": b.symbol_kind, "span": b.span}))
            .collect();
        assert_eq!(
            json!(boundaries),
            case["boundaries"],
            "unchanged AST fields: {label}"
        );
    }
}

#[test]
fn namespace_templates_and_type_templates_keep_different_kinds() {
    let out = parse("namespace n { template<class T> T free(T x) { return x; } template<class T> struct Box { T member(T x) { return x; } }; }");
    assert_eq!(find(&out, "free").kind, SymbolKind::Function);
    assert_eq!(find(&out, "free").qname.as_deref(), Some("n::free"));
    assert_eq!(find(&out, "member").kind, SymbolKind::Method);
    assert_eq!(find(&out, "member").qname.as_deref(), Some("Box::member"));
}

#[test]
fn cpp17_nested_inline_and_reopened_namespaces_use_ast_paths() {
    let out = parse("namespace a::b { namespace c { int first() { return 1; } } } namespace a { inline namespace v1 { int second() { return 2; } } } namespace a::b::c { int third() { return 3; } }");
    for (name, qname) in [
        ("first", "a::b::c::first"),
        ("second", "a::v1::second"),
        ("third", "a::b::c::third"),
    ] {
        let sym = find(&out, name);
        assert_eq!(sym.kind, SymbolKind::Function);
        assert_eq!(sym.qname.as_deref(), Some(qname));
    }
}

#[test]
fn unicode_crlf_and_line_drift_preserve_namespace_uid() {
    let text = "namespace 森 {\r\nnamespace 内 {\r\nint 葉(int x) { return x; }\r\n}\r\n}\r\n";
    let first = parse(text);
    let moved = parse(&format!("\r\n{text}"));
    let a = find(&first, "葉");
    let b = find(&moved, "葉");
    assert_eq!(a.kind, SymbolKind::Function);
    assert_eq!(a.qname.as_deref(), Some("森::内::葉"));
    assert_eq!(a.container.as_deref(), Some("内"));
    assert_eq!(a.symbol_uid, b.symbol_uid);
    assert_ne!(a.symbol_id, b.symbol_id);
    assert_eq!(a.start_line + 1, b.start_line);
    assert_eq!(a.start_col, b.start_col);
}

#[test]
fn same_tail_namespace_functions_have_distinct_uids() {
    let out = parse("namespace left { namespace shared { int leaf() { return 1; } } } namespace right { namespace shared { int leaf() { return 2; } } }");
    let leaves: Vec<_> = out.symbols.iter().filter(|s| s.name == "leaf").collect();
    assert_eq!(leaves.len(), 2);
    assert_ne!(leaves[0].symbol_uid, leaves[1].symbol_uid);
    assert_eq!(
        leaves
            .iter()
            .map(|s| s.qname.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("left::shared::leaf"), Some("right::shared::leaf")]
    );
}

#[test]
fn qualified_specialized_and_operator_names_do_not_enter_scope_a() {
    let out = parse("namespace n { struct Box { int member(); }; int Box::member() { return 1; } template<class T> T leaf(T x); template<> int leaf<int>(int x) { return x; } struct Token {}; Token operator+(Token a, Token b) { return a; } } int n::Box::member() { return 2; }");
    let members: Vec<_> = out.symbols.iter().filter(|s| s.name == "member").collect();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].kind, SymbolKind::Method);
    assert_eq!(members[0].qname.as_deref(), Some("n::member"));
    assert_eq!(members[1].kind, SymbolKind::Function);
    assert_eq!(members[1].qname.as_deref(), Some("member"));
    assert_eq!(find(&out, "leaf<int>").kind, SymbolKind::Method);
    assert_eq!(find(&out, "operator+").kind, SymbolKind::Method);
}

#[test]
fn namespace_path_budget_exhaustion_does_not_publish_partial_function_identity() {
    let namespaces = (0..65)
        .map(|i| format!("namespace n{i} {{ "))
        .collect::<String>();
    let out = parse(&format!(
        "{namespaces}int leaf() {{ return 1; }} {}",
        "}".repeat(65)
    ));
    let leaf = find(&out, "leaf");
    assert_eq!(
        leaf.kind,
        SymbolKind::Method,
        "unsupported context retains old non-admitted native result"
    );
    let boundary = out
        .source_structure
        .as_ref()
        .unwrap()
        .boundaries
        .iter()
        .find(|b| b.name.as_deref() == Some("leaf"))
        .unwrap();
    assert_eq!(boundary.symbol_kind, Some(SymbolKind::Function));
    assert_ne!(Some(leaf.kind), boundary.symbol_kind);
}

#[test]
fn namespace_call_and_ref_edges_use_corrected_symbol_uid() {
    let out = parse("namespace grove { namespace inner { int callee() { return 1; } int caller() { return callee(); } } }");
    let caller = find(&out, "caller");
    let callee = find(&out, "callee");
    let edge = out
        .call_edges
        .iter()
        .find(|e| e.callee_symbol == "callee")
        .unwrap();
    assert_eq!(caller.kind, SymbolKind::Function);
    assert_eq!(callee.kind, SymbolKind::Function);
    assert_eq!(edge.caller_symbol_uid, caller.symbol_uid);
    assert_eq!(edge.callee_symbol_uid, callee.symbol_uid);
    let reference = out
        .symbol_refs
        .iter()
        .find(|r| r.target_symbol_uid == callee.symbol_uid)
        .unwrap();
    assert_eq!(reference.container, caller.qname);
    assert_eq!(reference.target_symbol_uid, callee.symbol_uid);
}

#[test]
fn ordinary_pointer_return_is_corrected_without_new_declarator_support() {
    let out = parse("namespace n { int *leaf() { return nullptr; } }");
    let leaf = find(&out, "leaf");
    assert_eq!(leaf.kind, SymbolKind::Function);
    assert_eq!(leaf.qname.as_deref(), Some("n::leaf"));
}

#[test]
fn namespace_comments_and_inline_shorthand_are_ast_trivia() {
    for (text, qname) in [
        (
            "namespace outer /* decoy::owner */ :: inner { int leaf() { return 3; } }",
            "outer::inner::leaf",
        ),
        (
            "namespace exterior::inline api { int leaf() { return 1; } }",
            "exterior::api::leaf",
        ),
    ] {
        let out = parse(text);
        assert_eq!(find(&out, "leaf").kind, SymbolKind::Function);
        assert_eq!(find(&out, "leaf").qname.as_deref(), Some(qname));
    }
}

#[test]
fn malformed_namespace_does_not_gain_partial_function_identity() {
    let text = "namespace outer:: { int leaf() { return 1; } }";
    let out = ParserRegistry::new()
        .parse("taxonomy.cpp", text, Language::Cpp)
        .unwrap();
    assert!(!out
        .symbols
        .iter()
        .any(|s| s.name == "leaf" && s.kind == SymbolKind::Function));
}

#[test]
fn same_line_namespace_callers_use_their_own_source_coordinates() {
    let out = parse("int helper(int x) { return x; }\nnamespace a { namespace tail { int leaf(int x) { return helper(x); } } } namespace b { namespace tail { int leaf(int x) { return helper(x); } } }");
    let leaves: Vec<_> = out.symbols.iter().filter(|s| s.name == "leaf").collect();
    let calls: Vec<_> = out
        .call_edges
        .iter()
        .filter(|e| e.callee_symbol == "helper")
        .collect();
    assert_eq!(leaves.len(), 2);
    assert_eq!(calls.len(), 2);
    for (sym, edge) in leaves.iter().zip(calls) {
        assert_eq!(
            edge.caller_symbol_id.as_deref(),
            Some(sym.symbol_id.as_str())
        );
        assert_eq!(edge.caller_symbol_uid, sym.symbol_uid);
    }
}

#[test]
fn ambiguous_namespace_callees_remain_unresolved_without_owner_lookup() {
    let out = parse("namespace east { namespace common { int leaf(int x) { return x; } int grow(int x) { return leaf(x); } } } namespace west { namespace common { int leaf(int x) { return x + 2; } int flourish(int x) { return leaf(x); } } }");
    for edge in out.call_edges.iter().filter(|e| e.callee_symbol == "leaf") {
        assert_eq!(
            edge.resolution_kind,
            cc_model::edge::ResolutionKind::Unresolved
        );
        assert_eq!(edge.target_symbol_id, None);
        assert_eq!(edge.callee_symbol_uid, None);
        assert_eq!(edge.target_file_path, None);
        assert_eq!(edge.resolution_confidence, 0.0);
    }
    assert_eq!(
        out.call_edges
            .iter()
            .filter(|e| e.callee_symbol == "leaf")
            .count(),
        2
    );
    for reference in out.symbol_refs.iter().filter(|r| r.symbol_name == "leaf") {
        assert_eq!(
            reference.resolution_kind,
            cc_model::edge::ResolutionKind::Unresolved
        );
        assert_eq!(reference.target_symbol_id, None);
        assert_eq!(reference.target_symbol_uid, None);
    }
}

#[test]
fn qualified_calls_require_exact_ast_namespace_path() {
    let out = parse("namespace north { template<class T> T leaf(T x) { return x; } } int wrong(int x) { return south::leaf<int>(x); } int global(int x) { return ::leaf<int>(x); } int correct(int x) { return north::leaf<int>(x); } int absolute(int x) { return ::north::leaf<int>(x); }");
    let leaf = find(&out, "leaf");
    for edge in out.call_edges.iter().filter(|e| e.callee_symbol == "leaf") {
        match edge.caller_symbol.as_deref().unwrap() {
            "wrong" | "global" => {
                assert_eq!(edge.callee_symbol_uid, None);
                assert_eq!(edge.target_symbol_id, None);
                assert_eq!(
                    edge.resolution_kind,
                    cc_model::edge::ResolutionKind::Unresolved
                );
            }
            "correct" | "absolute" => assert_eq!(edge.callee_symbol_uid, leaf.symbol_uid),
            name => panic!("unexpected caller {name}"),
        }
    }
    assert_eq!(out.call_edges.len(), 4);
}

#[test]
fn field_and_constructor_calls_cannot_bind_namespace_free_functions() {
    let out = parse("namespace north { int leaf(int x) { return x; } int Box() { return 1; } } struct Other { int leaf(int); }; struct Box {}; int wrong(Other obj) { return obj.leaf(1); } Box *make() { return new Box(); }");
    assert_eq!(out.call_edges.len(), 2);
    for edge in &out.call_edges {
        assert_eq!(edge.target_symbol_id, None);
        assert_eq!(edge.callee_symbol_uid, None);
        assert_eq!(
            edge.resolution_kind,
            cc_model::edge::ResolutionKind::Unresolved
        );
    }
}

#[test]
fn same_line_environment_accesses_use_exact_namespace_function_spans() {
    let out = parse("namespace a { namespace tail { const char *leaf() { return getenv(\"FIRST\"); } } } namespace b { namespace tail { const char *leaf() { return getenv(\"SECOND\"); } } } const char *global = getenv(\"OUTSIDE\");");
    let leaves: Vec<_> = out.symbols.iter().filter(|s| s.name == "leaf").collect();
    let edges: Vec<_> = out
        .data_flow_edges
        .iter()
        .filter(|e| e.flow_kind == "env_access")
        .collect();
    assert_eq!(edges.len(), 3);
    assert_eq!(edges[0].env_key.as_deref(), Some("FIRST"));
    assert_eq!(edges[0].source_symbol_uid, leaves[0].symbol_uid);
    assert_eq!(edges[1].env_key.as_deref(), Some("SECOND"));
    assert_eq!(edges[1].source_symbol_uid, leaves[1].symbol_uid);
    assert_eq!(edges[2].env_key.as_deref(), Some("OUTSIDE"));
    assert_eq!(edges[2].source_symbol_uid, None);
}

#[test]
fn catalog_namespace_predicate_matches_only_scope_a_with_scanner_languages() {
    let source = "namespace n { int free() { return 1; } template<class T> T generic(T x) { return x; } struct Box { int member() { return 1; } int outside(); }; int Box::outside() { return 1; } template<> int generic<int>(int x) { return x; } } int n::free() { return 2; } namespace { int hidden() { return 1; } }";
    for extension in ["cpp", "cc", "cxx", "h", "hpp", "hxx"] {
        let file = format!("typed.{extension}");
        let language = cc_parsers::detect_language(&file);
        assert_eq!(language, Language::Cpp);
        let out = ParserRegistry::new()
            .parse(&file, source, language)
            .unwrap();
        let selected: Vec<_> = out
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function && s.container.is_some())
            .map(|s| (s.name.as_str(), s.qname.as_deref()))
            .collect();
        assert_eq!(
            selected,
            [("free", Some("n::free")), ("generic", Some("n::generic"))]
        );
    }
    for (file, text) in [
        ("legacy.c", "int plain() { return 1; }"),
        (
            "legacy.py",
            "def outer():\n    def inner():\n        return 1\n    return inner()\n",
        ),
        ("legacy.rs", "fn outer() { fn inner() {} inner(); }"),
        ("unknown.weird", source),
    ] {
        let language = cc_parsers::detect_language(file);
        assert_ne!(language, Language::Cpp);
        let out = ParserRegistry::new().parse(file, text, language).unwrap();
        assert!(!out
            .symbols
            .iter()
            .any(
                |s| cc_parsers::detect_language(&s.file_path) == Language::Cpp
                    && s.kind == SymbolKind::Function
                    && s.container.is_some()
            ));
    }
}
