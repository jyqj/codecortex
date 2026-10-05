//! Planning-only B1 red proof: synthetic source, parser only, no production changes.
use cc_model::{id::StableId, Language, ParseOutcome, SymbolKind};
use cc_parsers::ParserRegistry;
use serde_json::{json, Value};

const FILE: &str = "qualified_owner.cpp";
const NS: &str = "namespace grove { int leaf(); } int grove::leaf() { return 1; }";
const CLASS: &str = "namespace grove { class Box { public: int member(); }; } int grove::Box::member() { return 2; }";
const STRUCT: &str =
    "namespace grove { struct Box { int member(); }; } int grove::Box::member() { return 3; }";
const GLOBAL_TYPE: &str = "struct Box { int member(); }; int Box::member() { return 4; }";
const NESTED_NS: &str = "namespace outer::inner { int leaf(); } namespace outer::inner {} int outer::inner::leaf() { return 5; }";
const NAME_CASE: &str = "namespace Upper { int free(); } struct lower { int member(); }; int Upper::free() { return 1; } int lower::member() { return 2; }";
const COMMENT: &str =
    "namespace grove { int leaf(); } int grove /* fake::Type */ :: leaf() { return 6; }";
const ABSOLUTE: &str = "namespace grove { int leaf(); } int ::grove::leaf() { return 7; }";
const TWIN_NS: &str = "namespace left::shared { int leaf(); } namespace right::shared { int leaf(); } int left::shared::leaf() { return 1; } int right::shared::leaf() { return 2; }";
const TWIN_TYPES: &str = "namespace left { struct Box { int member(); }; } namespace right { struct Box { int member(); }; } int left::Box::member() { return 1; } int right::Box::member() { return 2; }";
const UNKNOWN: &str = "#include \"owners.hpp\"\nint foreign::member() { return 8; } int run() { return foreign::member(); }";
const FORWARD: &str = "namespace grove { struct Box; } int grove::Box::member() { return 9; }";
const WRONG_PATH: &str = "namespace left { struct Box { int member(); }; } namespace right {} int right::Box::member() { return 10; }";
const ALIAS: &str =
    "namespace real { int leaf(); } namespace alias = real; int alias::leaf() { return 10; }";
const AMBIGUOUS: &str = "namespace owner { int leaf(); } struct owner { static int leaf(); }; int owner::leaf() { return 11; }";
const CONDITIONAL: &str = "#if OWNER_NS\nnamespace owner { int leaf(); }\n#else\nstruct owner { static int leaf(); };\n#endif\nint owner::leaf() { return 12; }";
const WRONG_NS_CALL: &str = "namespace grove { int leaf(); } namespace other { int leaf(); } int grove::leaf() { return 1; } int run() { return other::leaf(); }";
const WRONG_TYPE_CALL: &str = "struct Box { static int member(); }; struct Other { static int member(); }; int Box::member() { return 1; } int run() { return Other::member(); }";
const DEFERRED_RELATIVE: &str =
    "namespace grove { struct Box { int member(); }; int Box::member() { return 1; } }";
const PLAIN: &str = "int leaf() { return 1; } namespace grove { int other() { return 2; } struct Box { int member() { return 3; } }; }";

fn tree(text: &str) -> tree_sitter::Tree {
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
    tree
}
fn parse(text: &str) -> ParseOutcome {
    tree(text);
    ParserRegistry::new()
        .parse(FILE, text, Language::Cpp)
        .unwrap()
}
fn assert_identity(text: &str, name: &str, kind: SymbolKind, qname: &str) {
    let out = parse(text);
    let sym = out.symbols.iter().find(|s| s.name == name).unwrap();
    assert_eq!((sym.kind, sym.qname.as_deref()), (kind, Some(qname)));
    assert_eq!(
        sym.symbol_uid,
        Some(StableId::symbol_uid(
            FILE,
            qname,
            kind.as_str(),
            sym.signature.as_deref()
        ))
    );
    let boundary = out
        .source_structure
        .as_ref()
        .unwrap()
        .boundaries
        .iter()
        .find(|b| b.name.as_deref() == Some(name))
        .unwrap();
    assert_eq!(boundary.symbol_kind, Some(kind));
}
fn assert_no_identity(text: &str, name: &str) {
    let out = parse(text);
    let sym = out.symbols.iter().find(|s| s.name == name).unwrap();
    // SymbolKind has no Unknown variant. Do not assert a semantic owner kind.
    // Keep source-local display extraction, but publish no durable identity.
    assert_eq!(
        (sym.qname.as_deref(), sym.symbol_uid.as_deref()),
        (None, None)
    );
}
fn assert_distinct(text: &str, name: &str, kind: SymbolKind, qnames: &[&str]) {
    let out = parse(text);
    let symbols: Vec<_> = out.symbols.iter().filter(|s| s.name == name).collect();
    assert_eq!(symbols.len(), 2);
    assert_ne!(symbols[0].symbol_uid, symbols[1].symbol_uid);
    for (sym, qname) in symbols.iter().zip(qnames) {
        assert_eq!((sym.kind, sym.qname.as_deref()), (kind, Some(*qname)));
    }
}
fn assert_unresolved_call(text: &str, name: &str) {
    let out = parse(text);
    let call = out
        .call_edges
        .iter()
        .find(|e| e.callee_symbol == name)
        .unwrap();
    assert_eq!(call.resolution_kind, cc_model::ResolutionKind::Unresolved);
    assert!(
        call.target_symbol_id.is_none()
            && call.callee_symbol_uid.is_none()
            && call.target_file_path.is_none()
    );
    let reference = out
        .symbol_refs
        .iter()
        .find(|r| r.symbol_name == name)
        .unwrap();
    assert_eq!(
        reference.resolution_kind,
        cc_model::ResolutionKind::Unresolved
    );
    assert!(
        reference.target_symbol_id.is_none()
            && reference.target_symbol_uid.is_none()
            && reference.target_file_path.is_none()
    );
}

#[test]
fn red_known_namespace_full_identity() {
    assert_identity(NS, "leaf", SymbolKind::Function, "grove::leaf");
}
#[test]
fn red_known_class_full_identity() {
    assert_identity(CLASS, "member", SymbolKind::Method, "grove::Box::member");
}
#[test]
fn red_known_struct_full_identity() {
    assert_identity(STRUCT, "member", SymbolKind::Method, "grove::Box::member");
}
#[test]
fn red_known_global_struct_method() {
    assert_identity(GLOBAL_TYPE, "member", SymbolKind::Method, "Box::member");
}
#[test]
fn red_nested_reopened_namespace_full_identity() {
    assert_identity(
        NESTED_NS,
        "leaf",
        SymbolKind::Function,
        "outer::inner::leaf",
    );
}
#[test]
fn red_identifier_case_does_not_determine_owner_kind() {
    assert_identity(NAME_CASE, "free", SymbolKind::Function, "Upper::free");
    assert_identity(NAME_CASE, "member", SymbolKind::Method, "lower::member");
}
#[test]
fn red_comments_are_not_owner_segments() {
    assert_identity(COMMENT, "leaf", SymbolKind::Function, "grove::leaf");
}
#[test]
fn red_leading_global_separator_uses_same_canonical_identity() {
    assert_identity(ABSOLUTE, "leaf", SymbolKind::Function, "grove::leaf");
}
#[test]
fn red_same_tail_namespace_definitions_do_not_collide() {
    assert_distinct(
        TWIN_NS,
        "leaf",
        SymbolKind::Function,
        &["left::shared::leaf", "right::shared::leaf"],
    );
}
#[test]
fn red_same_tail_type_definitions_do_not_collide() {
    assert_distinct(
        TWIN_TYPES,
        "member",
        SymbolKind::Method,
        &["left::Box::member", "right::Box::member"],
    );
}
#[test]
fn red_unknown_owner_has_no_durable_identity() {
    assert_no_identity(UNKNOWN, "member");
}
#[test]
fn red_forward_only_type_is_not_body_evidence() {
    assert_no_identity(FORWARD, "member");
}
#[test]
fn red_same_tail_owner_in_another_namespace_is_not_proof() {
    assert_no_identity(WRONG_PATH, "member");
}
#[test]
fn red_namespace_alias_is_not_canonical_owner_proof() {
    assert_no_identity(ALIAS, "leaf");
}
#[test]
fn red_conflicting_owner_kind_fails_closed() {
    assert_no_identity(AMBIGUOUS, "leaf");
}
#[test]
fn red_conditional_owner_fails_closed() {
    assert_no_identity(CONDITIONAL, "leaf");
}
#[test]
fn red_different_namespace_call_cannot_bind_by_short_name() {
    assert_unresolved_call(WRONG_NS_CALL, "leaf");
}
#[test]
fn red_different_type_call_cannot_bind_by_short_name() {
    assert_unresolved_call(WRONG_TYPE_CALL, "member");
}
#[test]
fn red_unknown_owner_call_cannot_bind_by_short_name() {
    assert_unresolved_call(UNKNOWN, "member");
}

#[test]
fn control_scope_a_and_inline_methods_are_unchanged() {
    assert_identity(PLAIN, "leaf", SymbolKind::Function, "leaf");
    assert_identity(PLAIN, "other", SymbolKind::Function, "grove::other");
    assert_identity(PLAIN, "member", SymbolKind::Method, "Box::member");
}
#[test]
fn control_relative_namespace_definition_remains_deferred() {
    let out = parse(DEFERRED_RELATIVE);
    let sym = out.symbols.iter().find(|s| s.name == "member").unwrap();
    assert_eq!(
        (sym.kind, sym.qname.as_deref()),
        (SymbolKind::Method, Some("grove::member"))
    );
}
#[test]
fn control_scope_a_negative_resolution_stays_unresolved() {
    assert_unresolved_call("namespace grove { int leaf() { return 1; } } namespace other { int run() { return leaf(); } }", "leaf");
}
#[test]
fn control_ast_owner_nodes_have_full_same_file_paths() {
    let tree = tree(CLASS);
    let root = tree.root_node();
    let ns = root.named_child(0).unwrap();
    assert_eq!(ns.kind(), "namespace_definition");
    assert_eq!(
        ns.child_by_field_name("name")
            .unwrap()
            .utf8_text(CLASS.as_bytes())
            .unwrap(),
        "grove"
    );
    let body = ns.child_by_field_name("body").unwrap();
    let ty = body.named_child(0).unwrap();
    assert_eq!(ty.kind(), "class_specifier");
    assert_eq!(
        ty.child_by_field_name("name")
            .unwrap()
            .utf8_text(CLASS.as_bytes())
            .unwrap(),
        "Box"
    );
    assert!(ty.child_by_field_name("body").is_some());
    let definition = root.named_child(1).unwrap();
    assert_eq!(definition.kind(), "function_definition");
    let qualified = definition
        .child_by_field_name("declarator")
        .unwrap()
        .child_by_field_name("declarator")
        .unwrap();
    assert_eq!(qualified.kind(), "qualified_identifier");
    assert_eq!(
        qualified.utf8_text(CLASS.as_bytes()).unwrap(),
        "grove::Box::member"
    );
}

#[test]
fn control_write_synthetic_baseline() {
    let cases = [
        ("namespace", NS),
        ("class", CLASS),
        ("struct", STRUCT),
        ("global_type", GLOBAL_TYPE),
        ("nested_reopened_namespace", NESTED_NS),
        ("identifier_case", NAME_CASE),
        ("comment", COMMENT),
        ("leading_global", ABSOLUTE),
        ("same_tail_namespaces", TWIN_NS),
        ("same_tail_types", TWIN_TYPES),
        ("unknown_header_owner", UNKNOWN),
        ("forward_only", FORWARD),
        ("wrong_owner_path", WRONG_PATH),
        ("namespace_alias", ALIAS),
        ("conflicting_kind", AMBIGUOUS),
        ("conditional", CONDITIONAL),
        ("wrong_namespace_call", WRONG_NS_CALL),
        ("wrong_type_call", WRONG_TYPE_CALL),
        ("deferred_relative", DEFERRED_RELATIVE),
        ("unqualified_controls", PLAIN),
    ];
    let snapshots: Vec<Value> = cases.iter().map(|(label, source)| {
        let ast = tree(source);
        let out = parse(source);
        json!({"case": label, "source": source, "ast": ast.root_node().to_sexp(), "symbols": out.symbols,
               "calls": out.call_edges, "refs": out.symbol_refs, "data_flow_edges": out.data_flow_edges,
                   "semantic_edges": out.semantic_edges, "resolution": out.resolution,
               "boundaries": out.source_structure.as_ref().unwrap().boundaries})
    }).collect();
    if let Ok(path) = std::env::var("CPP_OWNER_SNAPSHOT") {
        std::fs::write(path, serde_json::to_string_pretty(&snapshots).unwrap()).unwrap();
    }
    assert_eq!(snapshots.len(), 20);
}

#[test]
fn b1_states_require_current_source_proofs_and_preserve_legacy_serialization() {
    use cc_model::{cpp_owner::CppQualifiedOwnerState as State, source::SourceSnapshot};
    let out = parse(CLASS);
    let member = out.symbols.iter().find(|s| s.name == "member").unwrap();
    assert_eq!(member.cpp_qualified_owner, State::ProvenType);
    assert_eq!(out.cpp_qualified_owner_proofs.len(), 1);
    let proof = &out.cpp_qualified_owner_proofs[0];
    assert!(proof.matches(
        &SourceSnapshot::new(CLASS.as_bytes()),
        proof.definition,
        member
    ));
    assert!(!proof.matches(
        &SourceSnapshot::new(format!(" {CLASS}").as_bytes()),
        proof.definition,
        member
    ));
    let mut poison = member.clone();
    poison.qname = Some("wrong::member".into());
    assert!(!proof.matches(
        &SourceSnapshot::new(CLASS.as_bytes()),
        proof.definition,
        &poison
    ));
    let plain = parse(PLAIN);
    assert!(plain.cpp_qualified_owner_proofs.is_empty());
    for symbol in plain.symbols {
        let json = serde_json::to_value(&symbol).unwrap();
        assert!(json.get("cpp_qualified_owner").is_none());
        let roundtrip: cc_model::SymbolRecord = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(roundtrip.cpp_qualified_owner, State::NonB1);
        assert_eq!(serde_json::to_value(&roundtrip).unwrap(), json);
        let mut malformed = json;
        malformed["cpp_qualified_owner"] = json!("future_positive");
        assert!(serde_json::from_value::<cc_model::SymbolRecord>(malformed).is_err());
    }
    for (source, expected) in [(UNKNOWN, State::Unproven), (AMBIGUOUS, State::Ambiguous)] {
        let out = parse(source);
        let symbol = out
            .symbols
            .iter()
            .find(|s| s.name == "member" || s.name == "leaf")
            .unwrap();
        assert_eq!(symbol.cpp_qualified_owner, expected);
        assert!(out.cpp_qualified_owner_proofs.is_empty());
    }
}

#[test]
fn b1_same_line_callers_and_environment_accesses_use_exact_spans() {
    let source = "int helper() { return 1; } namespace a { struct Box { int member(); }; int leaf(); } int a::Box::member() { getenv(\"TYPE\"); return helper(); } int a::leaf() { getenv(\"NS\"); return helper(); } int Unknown::leaf() { getenv(\"UNKNOWN\"); return helper(); }";
    let out = parse(source);
    let functions: Vec<_> = out
        .symbols
        .iter()
        .filter(|s| s.cpp_qualified_owner.is_b1())
        .collect();
    assert_eq!(functions.len(), 3);
    let calls: Vec<_> = out
        .call_edges
        .iter()
        .filter(|e| e.callee_symbol == "helper")
        .collect();
    let env: Vec<_> = out
        .data_flow_edges
        .iter()
        .filter(|e| e.flow_kind == "env_access")
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(env.len(), 3);
    for ((symbol, call), access) in functions.iter().zip(calls).zip(env) {
        assert_eq!(
            call.caller_symbol_id.as_deref(),
            Some(symbol.symbol_id.as_str())
        );
        assert_eq!(call.caller_symbol_uid, symbol.symbol_uid);
        assert_eq!(access.source_symbol_uid, symbol.symbol_uid);
    }
}

#[test]
fn b1_even_exact_qualified_and_bare_calls_remain_non_binding() {
    let out = parse("namespace grove { int leaf(); } int grove::leaf() { return 1; } int run() { return grove::leaf() + leaf(); }");
    for edge in out.call_edges.iter().filter(|e| e.callee_symbol == "leaf") {
        assert_eq!(
            edge.resolution_strategy,
            cc_model::resolution::CPP_QUALIFIED_OWNER_UNPROVEN_BINDING
        );
        assert!(
            edge.target_symbol_id.is_none()
                && edge.callee_symbol_uid.is_none()
                && edge.target_file_path.is_none()
        );
    }
}

#[test]
fn constructor_identifier_stays_outside_b1() {
    let out = parse("struct Box { Box(); }; Box::Box() {}");
    let ctor = out
        .symbols
        .iter()
        .find(|s| s.name == "Box" && s.kind == SymbolKind::Function)
        .unwrap();
    assert!(ctor.cpp_qualified_owner.is_non_b1());
    assert_eq!(ctor.qname.as_deref(), Some("Box"));
    assert!(out.cpp_qualified_owner_proofs.is_empty());
}

#[test]
fn unsupported_template_owner_still_blocks_conflicting_namespace_claim() {
    let source =
        "namespace clash {} template<class T> struct clash {}; int clash::leaf() { return 1; }";
    let out = parse(source);
    let leaf = out.symbols.iter().find(|s| s.name == "leaf").unwrap();
    assert_eq!(
        leaf.cpp_qualified_owner,
        cc_model::cpp_owner::CppQualifiedOwnerState::Ambiguous
    );
    assert!(leaf.qname.is_none() && leaf.symbol_uid.is_none());
    assert!(out.cpp_qualified_owner_proofs.is_empty());
}

#[test]
fn unsupported_typedef_and_union_owners_block_competing_namespace_claims() {
    for source in [
        "namespace clash {} typedef int clash; int clash::leaf() { return 1; }",
        "namespace clash {} union clash { int x; }; int clash::leaf() { return 1; }",
        "namespace clash {} enum clash { value }; int clash::leaf() { return 1; }",
    ] {
        let out = parse(source);
        let leaf = out.symbols.iter().find(|s| s.name == "leaf").unwrap();
        assert_eq!(
            leaf.cpp_qualified_owner,
            cc_model::cpp_owner::CppQualifiedOwnerState::Ambiguous
        );
        assert!(leaf.qname.is_none() && leaf.symbol_uid.is_none());
    }
}

#[test]
fn unsupported_owner_wrappers_and_all_typedef_declarators_fail_closed() {
    for source in [
        "namespace clash {} struct clash { int leaf(); } instance; int clash::leaf() { return 1; }",
        "namespace clash {} typedef int unrelated, clash; int clash::leaf() { return 1; }",
        "namespace clash {} typedef struct clash {} Alias; int clash::leaf() { return 1; }",
        "namespace clash {} template<> struct clash<int> {}; int clash::leaf() { return 1; }",
    ] {
        let out = parse(source);
        let leaf = out.symbols.iter().find(|s| s.name == "leaf").unwrap();
        assert!(leaf.cpp_qualified_owner.is_b1() && !leaf.cpp_qualified_owner.is_proven());
        assert!(leaf.qname.is_none() && leaf.symbol_uid.is_none());
        assert!(out.cpp_qualified_owner_proofs.is_empty());
    }
}

#[test]
fn template_forward_claim_cannot_merge_with_an_ordinary_type_body() {
    let out = parse("struct Box { int member(); }; template<class T> struct Box; int Box::member() { return 1; }");
    let member = out.symbols.iter().find(|s| s.name == "member").unwrap();
    assert_eq!(
        member.cpp_qualified_owner,
        cc_model::cpp_owner::CppQualifiedOwnerState::Ambiguous
    );
    assert!(member.qname.is_none() && member.symbol_uid.is_none());
}

#[test]
fn anonymous_namespace_visibility_prevents_incomplete_owner_proof() {
    let out =
        parse("namespace clash {} namespace { struct clash {}; } int clash::leaf() { return 1; }");
    let leaf = out.symbols.iter().find(|s| s.name == "leaf").unwrap();
    assert_eq!(
        leaf.cpp_qualified_owner,
        cc_model::cpp_owner::CppQualifiedOwnerState::Unproven
    );
    assert!(leaf.qname.is_none() && leaf.symbol_uid.is_none());
}

#[test]
fn zero_segment_global_owner_is_outside_b1_and_keeps_legacy_calls() {
    let out = parse("int f(); int ::f() { return 1; } int run() { return ::f(); }");
    let global = out.symbols.iter().find(|s| s.name == "f").unwrap();
    assert_eq!(global.kind, SymbolKind::Function);
    assert_eq!(global.qname.as_deref(), Some("f"));
    assert_eq!(
        global.symbol_uid,
        Some(StableId::symbol_uid(
            FILE,
            "f",
            "function",
            global.signature.as_deref()
        ))
    );
    assert!(global.cpp_qualified_owner.is_non_b1());
    assert!(out.cpp_qualified_owner_proofs.is_empty());
    assert_eq!(out.call_edges.len(), 1);
    assert_eq!(
        out.call_edges[0].resolution_kind,
        cc_model::ResolutionKind::Exact
    );
    assert_eq!(out.call_edges[0].callee_symbol_uid, global.symbol_uid);
    assert_eq!(
        out.call_edges[0].target_symbol_id.as_deref(),
        Some(global.symbol_id.as_str())
    );
    assert_eq!(out.symbol_refs.len(), 1);
    assert_eq!(out.symbol_refs[0].target_symbol_uid, global.symbol_uid);
}
