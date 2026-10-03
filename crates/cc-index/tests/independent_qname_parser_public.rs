//! Independently authored review cases at product 4e3e5355; no product changes.
use cc_model::{source::SourceSnapshot, Language, ParseOutcome, SymbolKind};
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
fn nearest_declaration_and_conditional_classes_preserve_native_scope() {
    let text = "def factory():\n    if ready:\n        class Local:\n            @staticmethod\n            def static():\n                def leaf():\n                    return 1\n                return leaf()\n            @classmethod\n            def build(cls):\n                return cls\n            @property\n            def value(self):\n                return 2\n            async def wait(self):\n                return 3\n            if ready:\n                class Deep:\n                    def run(self):\n                        return 4\n    return Local\n";
    let out = parsed(text);
    assert!(out.source_structure.as_ref().unwrap().complete);
    for (q, kind, receiver) in [
        ("factory.Local", SymbolKind::Class, None),
        (
            "factory.Local.static",
            SymbolKind::Method,
            Some("factory.Local"),
        ),
        ("factory.Local.static.leaf", SymbolKind::Function, None),
        (
            "factory.Local.build",
            SymbolKind::Method,
            Some("factory.Local"),
        ),
        (
            "factory.Local.value",
            SymbolKind::Method,
            Some("factory.Local"),
        ),
        (
            "factory.Local.wait",
            SymbolKind::Method,
            Some("factory.Local"),
        ),
        ("factory.Local.Deep", SymbolKind::Class, None),
        (
            "factory.Local.Deep.run",
            SymbolKind::Method,
            Some("factory.Local.Deep"),
        ),
    ] {
        let found: Vec<_> = out
            .symbols
            .iter()
            .filter(|s| s.qname.as_deref() == Some(q))
            .collect();
        assert_eq!(found.len(), 1, "{q}");
        assert_eq!(found[0].kind, kind, "{q}");
        assert_eq!(found[0].receiver_type.as_deref(), receiver, "{q}");
    }
    let leaf = out.symbols.iter().find(|s| s.name == "leaf").unwrap();
    let call = out
        .call_edges
        .iter()
        .find(|c| c.callee_symbol == "leaf")
        .unwrap();
    assert_eq!(
        call.target_symbol_id.as_deref(),
        Some(leaf.symbol_id.as_str())
    );
}
#[test]
fn unicode_crlf_decorator_is_single_wrapper_and_body_call_owner() {
    let text = "# 注释\r\ndef helper(): return 7\r\nclass 容器:\r\n    @decorate\r\n    async def 方法(self):\r\n        def local():\r\n            return helper()\r\n        return helper() + local()\r\n";
    let out = parsed(text);
    let method: Vec<_> = out.symbols.iter().filter(|s| s.name == "方法").collect();
    assert_eq!(method.len(), 1);
    assert_eq!(method[0].start_line, 4);
    assert_eq!(method[0].qname.as_deref(), Some("容器.方法"));
    for q in ["容器.方法", "容器.方法.local"] {
        let sym = out
            .symbols
            .iter()
            .find(|s| s.qname.as_deref() == Some(q))
            .unwrap();
        let calls: Vec<_> = out
            .call_edges
            .iter()
            .filter(|c| {
                c.callee_symbol == "helper" && c.caller_symbol_id.as_deref() == Some(&sym.symbol_id)
            })
            .collect();
        assert_eq!(calls.len(), 1, "{q}");
        assert_eq!(calls[0].caller_symbol_uid, sym.symbol_uid);
    }
    let ids =
        cc_index::documents::symbol_identity::prepare(&SourceSnapshot::new(text.as_bytes()), &out)
            .unwrap();
    assert!(ids.iter().any(|i| i.qname == "容器.方法"));
}
#[test]
fn malformed_and_missing_proofs_omit_identity() {
    let text = "def okay(): return 1\ndef broken(:\n";
    let out = parsed(text);
    assert!(!out.source_structure.as_ref().unwrap().complete);
    assert!(cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(text.as_bytes()),
        &out
    )
    .unwrap()
    .is_empty());
    // Compound Python declarations cannot legally follow semicolons. Recovery
    // must not publish same-line homonyms as complete declaration proof.
    let same_line = "def same(): return 1; def same(): return 2\n";
    let out = parsed(same_line);
    assert!(!out.source_structure.as_ref().unwrap().complete);
    assert!(cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(same_line.as_bytes()),
        &out
    )
    .unwrap()
    .is_empty());
    let unknown = "def guessed(): return 1\n";
    let mut fallback = ParserRegistry::new()
        .parse("micro.unknown", unknown, Language::Unknown)
        .unwrap();
    fallback.documents = Some(
        cc_index::documents::delta::prepare(
            &SourceSnapshot::new(unknown.as_bytes()),
            &fallback,
            &[],
        )
        .unwrap(),
    );
    assert!(cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(unknown.as_bytes()),
        &fallback
    )
    .unwrap()
    .is_empty());
    let valid = "def same(): return 1\ndef same(): return 2\n";
    let mut out = parsed(valid);
    let source = SourceSnapshot::new(valid.as_bytes());
    out.source_structure.as_mut().unwrap().capability = "unknown".into();
    assert!(cc_index::documents::symbol_identity::prepare(&source, &out)
        .unwrap()
        .is_empty());
    out.source_structure = None;
    assert!(cc_index::documents::symbol_identity::prepare(&source, &out)
        .unwrap()
        .is_empty());
}
#[test]
fn decorated_classes_are_class_declarations_not_function_or_method() {
    let out = parsed("@decorate\nclass Outer:\n    @decorate\n    class Inner:\n        def run(self): return 1\n");
    for name in ["Outer", "Inner"] {
        let symbols: Vec<_> = out.symbols.iter().filter(|s| s.name == name).collect();
        assert_eq!(symbols.len(), 1, "decorated class {name}: {symbols:#?}");
        assert_eq!(symbols[0].kind, SymbolKind::Class);
    }
}
