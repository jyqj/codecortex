//! AST taxonomy, wrapper envelopes and lexical ownership for Python classes.
use cc_model::{source::SourceSnapshot, Language, ParseOutcome, StableId, SymbolKind};
use cc_parsers::ParserRegistry;
fn parsed(text: &str) -> ParseOutcome {
    let mut out = ParserRegistry::new()
        .parse("micro.py", text, Language::Python)
        .unwrap();
    out.documents = Some(
        cc_index::documents::delta::prepare(&SourceSnapshot::new(text.as_bytes()), &out, &[])
            .unwrap(),
    );
    out
}
#[test]
fn classes_keep_canonical_envelopes_members_uids_and_lexical_parents() {
    for decorators in ["", "@decorate\n", "@first(prepare())\n@second\n"] {
        let text = format!("def helper(): return 1\ndef factory():\n    if ready:\n{}        class 容器(Base):\n            if ready:\n                @decorate\n                class Inner:\n                    @staticmethod\n                    def pulse():\n                        def leaf(): return helper()\n                        return leaf()\n            @property\n            def value(self): return helper()\n    return 容器\n", decorators.lines().map(|l| format!("        {l}\n")).collect::<String>()).replace('\n', "\r\n");
        let out = parsed(&text);
        let source = SourceSnapshot::new(text.as_bytes());
        assert!(out.source_structure.as_ref().unwrap().complete);
        let identities = cc_index::documents::symbol_identity::prepare(&source, &out).unwrap();
        for (q, kind, parent, receiver) in [
            ("factory", SymbolKind::Function, None, None),
            ("factory.容器", SymbolKind::Class, Some("factory"), None),
            (
                "factory.容器.Inner",
                SymbolKind::Class,
                Some("factory.容器"),
                None,
            ),
            (
                "factory.容器.Inner.pulse",
                SymbolKind::Method,
                Some("factory.容器.Inner"),
                Some("factory.容器.Inner"),
            ),
            (
                "factory.容器.Inner.pulse.leaf",
                SymbolKind::Function,
                Some("factory.容器.Inner.pulse"),
                None,
            ),
            (
                "factory.容器.value",
                SymbolKind::Method,
                Some("factory.容器"),
                Some("factory.容器"),
            ),
        ] {
            let found: Vec<_> = out
                .symbols
                .iter()
                .filter(|s| s.qname.as_deref() == Some(q))
                .collect();
            assert_eq!(found.len(), 1, "{q}");
            let s = found[0];
            assert_eq!(s.kind, kind);
            assert_eq!(s.receiver_type.as_deref(), receiver);
            assert_eq!(s.container.as_deref(), parent);
            let pid = parent.map(|p| {
                out.symbols
                    .iter()
                    .find(|s| s.qname.as_deref() == Some(p))
                    .unwrap()
                    .symbol_id
                    .as_str()
            });
            assert_eq!(s.parent_symbol_id.as_deref(), pid);
            // Python has no emitted scope catalog; do not invent dangling IDs.
            assert!(s.scope_id.is_none());
            assert_eq!(
                s.symbol_uid,
                Some(StableId::symbol_uid(
                    "micro.py",
                    q,
                    kind.as_str(),
                    if kind == SymbolKind::Class {
                        None
                    } else if s.name == "value" {
                        Some("(self)")
                    } else {
                        Some("()")
                    }
                ))
            );
            let boundary = out
                .source_structure
                .as_ref()
                .unwrap()
                .boundaries
                .iter()
                .find(|b| {
                    source.point(b.span.start).unwrap()
                        == (s.start_line as usize, s.start_col as usize)
                        && source.point(b.span.end).unwrap()
                            == (s.end_line as usize, s.end_col as usize)
                        && b.name.as_deref() == Some(s.name.as_str())
                })
                .expect("exact canonical AST declaration");
            assert_eq!(boundary.symbol_kind, Some(kind));
            if out
                .chunks
                .iter()
                .any(|c| c.symbol_name.as_deref() == Some(s.name.as_str()))
            {
                assert!(
                    identities.iter().any(|i| i.qname == q),
                    "chunk must have proof {q}"
                );
            }
        }
        let class = out.symbols.iter().find(|s| s.name == "容器").unwrap();
        let start = source
            .line_range(class.start_line as usize, class.start_line as usize)
            .unwrap();
        let header = source.slice(start).unwrap();
        assert!(header.trim_start().starts_with(if decorators.is_empty() {
            "class 容器"
        } else if decorators.contains("first") {
            "@first"
        } else {
            "@decorate"
        }));
        let inner = out.symbols.iter().find(|s| s.name == "Inner").unwrap();
        assert!(source
            .slice(
                source
                    .line_range(inner.start_line as usize, inner.start_line as usize)
                    .unwrap()
            )
            .unwrap()
            .trim_start()
            .starts_with("@decorate"));
        assert!(out.symbol_refs.iter().any(|r| r.symbol_name == "decorate"));
        if decorators.contains("prepare") {
            let owner = out.symbols.iter().find(|s| s.name == "factory").unwrap();
            let call = out
                .call_edges
                .iter()
                .find(|e| e.callee_symbol == "prepare")
                .unwrap();
            assert_eq!(
                call.caller_symbol_id.as_deref(),
                Some(owner.symbol_id.as_str())
            );
        }
    }
}
#[test]
fn decorated_class_members_are_not_lost_in_small_or_split_chunks() {
    for body in [
        "        def pulse(self): return 1\n".to_string(),
        format!(
            "        def pulse(self):\n{}            return total\n",
            (0..260)
                .map(|i| format!("            total = {i}\n"))
                .collect::<String>()
        ),
    ] {
        let text = format!("@decorate\nclass Outer:\n    @decorate\n    class Inner:\n{body}");
        let out = parsed(&text);
        for (name, q, kind) in [
            ("Outer", "Outer", SymbolKind::Class),
            ("Inner", "Outer.Inner", SymbolKind::Class),
            ("pulse", "Outer.Inner.pulse", SymbolKind::Method),
        ] {
            assert_eq!(out.symbols.iter().filter(|s| s.name == name).count(), 1);
            let hits: Vec<_> = out
                .chunks
                .iter()
                .filter(|c| c.symbol_name.as_deref() == Some(name))
                .collect();
            assert!(!hits.is_empty(), "missing chunk {name}");
            for hit in hits {
                assert_eq!(hit.symbol_kind, Some(kind));
            }
            let identities = cc_index::documents::symbol_identity::prepare(
                &SourceSnapshot::new(text.as_bytes()),
                &out,
            )
            .unwrap();
            assert!(identities.iter().any(|i| i.qname == q));
        }
    }
}

#[test]
fn decorator_references_and_calls_are_real_and_evaluated_in_enclosing_scope() {
    let text = "def decorate(value): return value\ndef factory():\n    @decorate\n    @registry.decorate\n    @make(prepare())\n    class Local:\n        \"@ghost(phantom())\"\n        @decorate\n        def pulse(self): return 1\n    return Local\n";
    let out = parsed(text);
    let factory = out.symbols.iter().find(|s| s.name == "factory").unwrap();
    let bare: Vec<_> = out
        .symbol_refs
        .iter()
        .filter(|r| r.symbol_name == "decorate" && r.ref_kind == "identifier")
        .collect();
    assert_eq!(bare.len(), 2);
    assert_eq!(bare[0].container.as_deref(), Some("factory"));
    assert_eq!(bare[1].container, None); // class scope is not a function call owner
    assert!(out
        .symbol_refs
        .iter()
        .any(|r| r.symbol_name == "registry.decorate"));
    for name in ["make", "prepare"] {
        let calls: Vec<_> = out
            .call_edges
            .iter()
            .filter(|c| c.callee_symbol == name)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].caller_symbol_id.as_deref(),
            Some(factory.symbol_id.as_str())
        );
    }
    assert!(!out
        .symbol_refs
        .iter()
        .any(|r| r.symbol_name.contains("ghost") || r.symbol_name.contains("phantom")));
}
