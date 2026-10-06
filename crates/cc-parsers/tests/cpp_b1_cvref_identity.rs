//! Bounded definition-local cv/ref identity. No declaration matching or binding.
use cc_model::{
    cpp_owner::CppQualifiedOwnerState as State, id::StableId, source::SourceSnapshot, Language,
    ParseOutcome, SymbolKind, SymbolRecord,
};
use cc_parsers::ParserRegistry;
use std::collections::HashSet;

const FILE: &str = "cvref.cpp";
fn parse(source: &str) -> ParseOutcome {
    ParserRegistry::new()
        .parse(FILE, source, Language::Cpp)
        .unwrap()
}
fn methods(out: &ParseOutcome) -> Vec<&SymbolRecord> {
    out.symbols.iter().filter(|s| s.name == "f").collect()
}
fn checked_method<'a>(out: &'a ParseOutcome, source: &str, signature: &str) -> &'a SymbolRecord {
    let symbols = methods(out);
    assert_eq!(symbols.len(), 1, "{source}");
    let symbol = symbols[0];
    assert_eq!(symbol.cpp_qualified_owner, State::ProvenType, "{source}");
    assert_eq!(symbol.kind, SymbolKind::Method);
    assert_eq!(symbol.qname.as_deref(), Some("Box::f"));
    assert_eq!(symbol.signature.as_deref(), Some(signature), "{source}");
    assert_eq!(
        symbol.symbol_uid,
        Some(StableId::symbol_uid(
            FILE,
            "Box::f",
            SymbolKind::Method.as_str(),
            Some(signature),
        ))
    );
    let proofs = &out.cpp_qualified_owner_proofs;
    assert_eq!(proofs.len(), 1);
    assert!(proofs[0].matches(
        &SourceSnapshot::new(source.as_bytes()),
        proofs[0].definition,
        symbol,
    ));
    symbol
}
fn source(tail: &str) -> String {
    format!("struct Box {{ int f(); }}; int Box::f() {tail} {{ return 1; }}")
}

#[test]
fn all_cvref_combinations_have_canonical_unique_identity() {
    let mut uids = HashSet::new();
    for cv in ["", "const", "volatile", "const volatile"] {
        for reference in ["", "&", "&&"] {
            let tail = [cv, reference]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let text = source(&tail);
            let signature = if tail.is_empty() {
                "int f()".to_owned()
            } else {
                format!("int f() {tail}")
            };
            let out = parse(&text);
            let symbol = checked_method(&out, &text, &signature);
            assert!(uids.insert(symbol.symbol_uid.clone().unwrap()));
        }
    }
    assert_eq!(uids.len(), 12);
}

#[test]
fn cv_and_ref_overload_definitions_are_distinct_at_exact_coordinates() {
    for tails in [["", "const"], ["&", "&&"], ["const &", "const &&"]] {
        let text = format!(
            "struct Box {{ int f(); }};\nint Box::f() {} {{ return 1; }}\nint Box::f() {} {{ return 2; }}",
            tails[0], tails[1]
        );
        let out = parse(&text);
        let symbols = methods(&out);
        assert_eq!(symbols.len(), 2);
        assert_ne!(symbols[0].symbol_id, symbols[1].symbol_id);
        assert_ne!(symbols[0].symbol_uid, symbols[1].symbol_uid);
        assert_eq!(symbols[0].start_line, 2);
        assert_eq!(symbols[1].start_line, 3);
        assert_eq!(out.cpp_qualified_owner_proofs.len(), 2);
        for symbol in symbols {
            let proof = out
                .cpp_qualified_owner_proofs
                .iter()
                .find(|p| p.symbol_id == symbol.symbol_id)
                .unwrap();
            assert!(proof.matches(
                &SourceSnapshot::new(text.as_bytes()),
                proof.definition,
                symbol,
            ));
        }
    }
}

#[test]
fn empty_suffix_retains_the_accepted_uid_while_const_and_refs_change_it() {
    let legacy = StableId::symbol_uid(FILE, "Box::f", "method", Some("int f()"));
    let out = parse(&source(""));
    assert_eq!(
        methods(&out)[0].symbol_uid.as_deref(),
        Some(legacy.as_str())
    );
    for tail in ["const", "&", "&&"] {
        let out = parse(&source(tail));
        assert_ne!(
            methods(&out)[0].symbol_uid.as_deref(),
            Some(legacy.as_str())
        );
    }
}

#[test]
fn comments_and_cv_order_do_not_change_canonical_suffix() {
    let expected = "int f() const volatile &&";
    let mut uids = HashSet::new();
    for tail in [
        "const volatile &&",
        "volatile const &&",
        "volatile /* const & */ const /* && */ &&",
        "const\n// volatile &&\nvolatile\n&&",
    ] {
        let text = source(tail);
        let out = parse(&text);
        uids.insert(checked_method(&out, &text, expected).symbol_uid.clone());
    }
    assert_eq!(uids.len(), 1);
}

#[test]
fn signature_prefix_and_parameter_text_are_preserved_exactly() {
    let text = "struct Box { int f(const int&); }; int Box::f( const int & value /* && volatile */ ) volatile const & { return value; }";
    let out = parse(text);
    checked_method(
        &out,
        text,
        "int f( const int & value /* && volatile */ ) const volatile &",
    );
}

#[test]
fn return_parameters_noexcept_and_direct_attributes_cannot_supply_qualifiers() {
    for (text, signature) in [
        ("struct Box { int f(); }; const int Box::f() { return 1; }", "int f()"),
        ("struct Box { int f(const int&); }; int Box::f(const int& value) { return value; }", "int f(const int& value)"),
        ("struct Box { int f(); }; int Box::f() noexcept(sizeof(const int&) > 0) { return 1; }", "int f()"),
        ("struct Box { int f(); }; int Box::f() const noexcept(sizeof(volatile int&) > 0) { return 1; }", "int f() const"),
        ("struct Box { int f(); }; int Box::f() __attribute__((annotate(\"const volatile &&\"))) { return 1; }", "int f()"),
        ("struct Box { int f(); }; int Box::f() volatile __attribute__((annotate(\"const &&\"))) { return 1; }", "int f() volatile"),
        ("struct Box { int f(); }; auto Box::f() -> const int& { static int value; return value; }", "auto f()"),
        ("struct Box { int f(); }; auto Box::f() const -> volatile int& { static int value; return value; }", "auto f() const"),
    ] {
        let out = parse(text);
        checked_method(&out, text, signature);
    }
}

#[test]
fn prototypes_do_not_affect_definition_qualifiers_and_are_not_extracted() {
    let mut uids = HashSet::new();
    for declarations in [
        "int f(); int f() const; int f() &; int f() &&;",
        "int f() &&; int f() &; int f() const; int f();",
        "int f() volatile;",
        "",
    ] {
        let text = format!("struct Box {{ {declarations} }}; int Box::f() const & {{ return 1; }}");
        let out = parse(&text);
        uids.insert(
            checked_method(&out, &text, "int f() const &")
                .symbol_uid
                .clone(),
        );
        let only = parse(&format!(
            "struct Box {{ {declarations} }}; int Box::f() const &;"
        ));
        assert!(methods(&only).is_empty());
        assert!(only.cpp_qualified_owner_proofs.is_empty());
    }
    assert_eq!(uids.len(), 1);
}

#[test]
fn owner_failures_do_not_rewrite_or_publish_qualified_identity() {
    for (text, state) in [
        ("int Missing::f() const { return 1; }", State::Unproven),
        (
            "struct Box; int Box::f() const { return 1; }",
            State::Unproven,
        ),
        (
            "int Box::f() const { return 1; } struct Box { int f(); };",
            State::Unproven,
        ),
        (
            "namespace Box {} struct Box { int f(); }; int Box::f() const { return 1; }",
            State::Ambiguous,
        ),
        (
            "struct Box { int f(); }; struct Box { int f(); }; int Box::f() const { return 1; }",
            State::Ambiguous,
        ),
    ] {
        let out = parse(text);
        let symbols = methods(&out);
        assert_eq!(symbols.len(), 1, "{text}");
        let symbol = symbols[0];
        assert_eq!(symbol.cpp_qualified_owner, state, "{text}");
        assert!(symbol.qname.is_none() && symbol.symbol_uid.is_none());
        assert_eq!(symbol.signature.as_deref(), Some("int f()"));
        assert!(out.cpp_qualified_owner_proofs.is_empty());
    }
}

#[test]
fn unsupported_duplicate_and_malformed_qualifiers_fail_closed() {
    for tail in [
        "restrict",
        "const restrict",
        "alignas(8)",
        "const const",
        "volatile volatile",
        "& &",
        "&& &&",
        "const noexcept(",
    ] {
        let text = source(tail);
        let out = parse(&text);
        for symbol in methods(&out) {
            assert_eq!(symbol.cpp_qualified_owner, State::Unproven, "{text}");
            assert!(
                symbol.qname.is_none() && symbol.symbol_uid.is_none(),
                "{text}"
            );
            assert_eq!(symbol.signature.as_deref(), Some("int f()"));
        }
        assert!(out.cpp_qualified_owner_proofs.is_empty(), "{text}");
    }
}

#[test]
fn duplicate_definitions_with_the_same_signature_still_share_uid() {
    let text = "struct Box { int f(); }; int Box::f() volatile const & { return 1; } int Box::f() const volatile & { return 2; }";
    let out = parse(text);
    let symbols = methods(&out);
    assert_eq!(symbols.len(), 2);
    assert_ne!(symbols[0].symbol_id, symbols[1].symbol_id);
    assert_eq!(symbols[0].symbol_uid, symbols[1].symbol_uid);
    assert!(symbols
        .iter()
        .all(|s| s.signature.as_deref() == Some("int f() const volatile &")));
}

#[test]
fn namespaces_inline_and_wrapped_definitions_keep_accepted_signatures() {
    for text in [
        "namespace Box {} int Box::f() const { return 1; }",
        "struct Box { int f() const { return 1; } };",
        "struct Box { int *f() const; }; int *Box::f() const { return nullptr; }",
        "struct Box { int f() const; }; int Box::f() const [[gnu::always_inline]] { return 1; }",
    ] {
        let out = parse(text);
        let symbols = methods(&out);
        assert_eq!(symbols.len(), 1, "{text}");
        assert_eq!(symbols[0].signature.as_deref(), Some("int f()"), "{text}");
        assert_ne!(symbols[0].cpp_qualified_owner, State::ProvenType, "{text}");
    }
}

#[test]
fn proven_type_definitions_remain_ineligible_call_and_reference_targets() {
    let text = "struct Box { int f() const; }; int Box::f() const { return 1; } int run() { return Box::f(); }";
    let out = parse(text);
    checked_method(&out, text, "int f() const");
    let calls: Vec<_> = out
        .call_edges
        .iter()
        .filter(|e| e.callee_symbol == "f")
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].resolution_kind,
        cc_model::ResolutionKind::Unresolved
    );
    assert!(calls[0].target_symbol_id.is_none() && calls[0].callee_symbol_uid.is_none());
    let references: Vec<_> = out
        .symbol_refs
        .iter()
        .filter(|r| r.symbol_name == "f")
        .collect();
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].resolution_kind,
        cc_model::ResolutionKind::Unresolved
    );
    assert!(references[0].target_symbol_id.is_none() && references[0].target_symbol_uid.is_none());
}

#[test]
fn definition_local_virtual_specifiers_cannot_publish_positive_identity() {
    for tail in ["override", "final", "const override", "const final"] {
        let text = source(tail);
        let out = parse(&text);
        let symbols = methods(&out);
        assert_eq!(symbols.len(), 1, "{text}");
        assert_eq!(symbols[0].cpp_qualified_owner, State::Unproven, "{text}");
        assert!(symbols[0].qname.is_none() && symbols[0].symbol_uid.is_none());
        assert_eq!(symbols[0].signature.as_deref(), Some("int f()"));
        assert!(out.cpp_qualified_owner_proofs.is_empty());
    }
}

#[test]
fn valid_earlier_virtual_prototypes_and_inline_definitions_keep_their_context() {
    for prototype in [
        "virtual int f() const;",
        "int f() const override;",
        "int f() const final;",
    ] {
        let text = format!(
            "struct Base {{ virtual int f() const; }}; struct Box : Base {{ {prototype} }}; int Box::f() const {{ return 1; }}"
        );
        let out = parse(&text);
        checked_method(&out, &text, "int f() const");
    }
    for specifier in ["override", "final"] {
        let text = format!(
            "struct Base {{ virtual int f() const; }}; struct Box : Base {{ int f() const {specifier} {{ return 1; }} }};"
        );
        let out = parse(&text);
        let symbols = methods(&out);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].cpp_qualified_owner, State::NonB1);
        assert_eq!(symbols[0].signature.as_deref(), Some("int f()"));
        assert!(symbols[0].symbol_uid.is_some());
        assert!(out.cpp_qualified_owner_proofs.is_empty());
    }
}
