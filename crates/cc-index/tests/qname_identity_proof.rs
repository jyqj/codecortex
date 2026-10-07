use cc_model::{source::SourceSnapshot, Language, ParseOutcome, SymbolKind};
use cc_parsers::ParserRegistry;

fn parsed(text: &str) -> ParseOutcome {
    let mut outcome = ParserRegistry::new()
        .parse("a.py", text, Language::Python)
        .unwrap();
    outcome.documents = Some(
        cc_index::documents::delta::prepare(&SourceSnapshot::new(text.as_bytes()), &outcome, &[])
            .unwrap(),
    );
    outcome
}

#[test]
fn parser_lexical_qnames_distinguish_function_locals_class_methods_and_async_decorated_scopes() {
    let text = "def outer():\n    @decorate\n    async def inner():\n        return 1\n    class Local:\n        def method(self):\n            def inner():\n                return 2\n            return inner()\n    return inner\nclass Alpha:\n    @decorate\n    async def method(self):\n        def inner():\n            return 3\n        return inner()\nclass Beta:\n    def method(self):\n        def inner():\n            return 4\n        return inner()\n";
    let outcome = parsed(text);
    for (qname, kind, receiver) in [
        ("outer", SymbolKind::Function, None),
        ("outer.inner", SymbolKind::Function, None),
        ("outer.Local", SymbolKind::Class, None),
        (
            "outer.Local.method",
            SymbolKind::Method,
            Some("outer.Local"),
        ),
        ("outer.Local.method.inner", SymbolKind::Function, None),
        ("Alpha.method", SymbolKind::Method, Some("Alpha")),
        ("Alpha.method.inner", SymbolKind::Function, None),
        ("Beta.method.inner", SymbolKind::Function, None),
    ] {
        let symbols: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.qname.as_deref() == Some(qname))
            .collect();
        assert!(!symbols.is_empty(), "missing {qname}");
        for s in symbols {
            assert_eq!(s.kind, kind, "{qname}");
            assert_eq!(s.receiver_type.as_deref(), receiver, "{qname}");
        }
    }
    let locals: Vec<_> = outcome
        .symbols
        .iter()
        .filter(|s| s.name == "inner" && s.qname.as_deref() != Some("outer.inner"))
        .collect();
    assert_eq!(locals.len(), 3);
    assert_eq!(
        locals
            .iter()
            .filter_map(|s| s.symbol_uid.as_deref())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
}

#[test]
fn association_requires_unique_exact_complete_source_and_matching_existing_labels() {
    let text = "# 中文\nclass Alpha:\n    def needle(self):\n        return 'β'\n";
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let outcome = parsed(text);
    let prepare =
        |o: &ParseOutcome| cc_index::documents::symbol_identity::prepare(&snapshot, o).unwrap();
    let identities = prepare(&outcome);
    assert!(identities.iter().any(|i| i.qname == "Alpha.needle"));
    let mut ambiguous = outcome.clone();
    ambiguous.symbols.push(
        ambiguous
            .symbols
            .iter()
            .find(|s| s.name == "needle")
            .unwrap()
            .clone(),
    );
    assert!(!prepare(&ambiguous)
        .iter()
        .any(|i| i.qname == "Alpha.needle"));
    let mut absent = outcome.clone();
    absent.symbols.clear();
    assert!(prepare(&absent).is_empty());
    let mut partial = outcome.clone();
    partial.source_structure.as_mut().unwrap().complete = false;
    assert!(prepare(&partial).is_empty());
    let mut legacy = outcome.clone();
    legacy.source_structure = None;
    assert!(prepare(&legacy).is_empty());
    let mut wrong_label = outcome.clone();
    for c in &mut wrong_label.chunks {
        if c.symbol_name.as_deref() == Some("needle") {
            c.symbol_name = Some("Alpha".into());
            c.symbol_kind = Some(SymbolKind::Class);
        }
    }
    assert!(!prepare(&wrong_label)
        .iter()
        .any(|i| i.qname == "Alpha.needle"));
    let mut wrong_owner = outcome.clone();
    for c in &mut wrong_owner.chunks {
        c.source.as_mut().unwrap().owner = Some(cc_model::source::ByteSpan { start: 3, end: 4 });
    }
    // Both byte endpoints are inside the first Chinese scalar.
    assert!(prepare(&wrong_owner).is_empty());
    let changed = SourceSnapshot::new(b"def other(): return 0\n");
    assert!(
        cc_index::documents::symbol_identity::prepare(&changed, &outcome)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn same_name_neighbor_owner_cannot_authorize_a_disjoint_body_chunk() {
    let text = "class Alpha:\n    def needle(self):\n        return 1\nclass Beta:\n    def needle(self):\n        return 2\n";
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let mut outcome = parsed(text);
    let methods: Vec<_> = outcome
        .chunks
        .iter()
        .filter(|c| c.symbol_name.as_deref() == Some("needle"))
        .map(|c| c.source.clone().unwrap())
        .collect();
    assert_eq!(methods.len(), 2);
    let alpha = outcome
        .chunks
        .iter_mut()
        .find(|c| c.symbol_name.as_deref() == Some("needle"))
        .unwrap();
    alpha.source.as_mut().unwrap().owner = methods[1].owner;
    alpha.source.as_mut().unwrap().signature = methods[1].signature;
    outcome.documents =
        Some(cc_index::documents::delta::prepare(&snapshot, &outcome, &[]).unwrap());
    // Keep name and kind identical: even exact neighbor coordinates must not
    // authorize bytes in this other declaration's body.
    let identities = cc_index::documents::symbol_identity::prepare(&snapshot, &outcome).unwrap();
    assert!(!identities
        .iter()
        .any(|i| i.owner == methods[1].owner.unwrap()
            && i.chunk_id
                == outcome
                    .chunks
                    .iter()
                    .find(|c| c.text.contains("return 1"))
                    .unwrap()
                    .chunk_id));
}

#[test]
fn decorated_body_call_owners_use_the_same_canonical_parser_identity() {
    let text = "def helper():\n    return 1\nclass Alpha:\n    @decorate\n    async def method(self):\n        helper()\n        def inner():\n            helper()\n        return inner()\n";
    let outcome = parsed(text);
    for qname in ["Alpha.method", "Alpha.method.inner"] {
        let owner = outcome
            .symbols
            .iter()
            .find(|s| s.qname.as_deref() == Some(qname))
            .unwrap();
        let calls: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.caller_symbol_uid == owner.symbol_uid && e.callee_symbol == "helper")
            .collect();
        assert_eq!(calls.len(), 1, "missing/crossed call owner {qname}");
        assert_eq!(
            calls[0].caller_symbol_id.as_deref(),
            Some(owner.symbol_id.as_str())
        );
    }
    assert_eq!(
        outcome
            .symbols
            .iter()
            .filter(|s| s.qname.as_deref() == Some("Alpha.method"))
            .count(),
        1
    );
}
