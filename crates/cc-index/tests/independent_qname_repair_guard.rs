//! Independent repair review: real multi-language inputs, no product edits.
use cc_model::{source::SourceSnapshot, Language, SymbolKind};
use cc_parsers::ParserRegistry;
#[test]
fn real_language_guard_snapshot() {
    let fixtures = [
        ("micro.rs", Language::Rust, "mod grove { pub const LIMIT: u32 = 1; pub struct Oak; pub trait View { fn see(&self) {} } impl View for Oak { fn see(&self) { fn leaf() {} leaf(); } } }\n"),
        ("micro.cpp", Language::Cpp, "namespace grove { struct Oak { int see(); }; int Oak::see() { return 1; } template<class T> T leaf(T x) { return x; } }\n"),
        ("micro.go", Language::Go, "package grove\ntype (\n Oak struct { Leaf int }\n View interface { See() int }\n Alias = int\n)\ntype Named int\nfunc (o *Oak) See() int { return 1 }\n"),
        ("micro.ts", Language::TypeScript, "export namespace grove { export interface View { see(): number; } export type Alias = string; export const leaf = (x: number) => x; export const value = 1; export class Oak { static see() { return leaf(1); } } }\n"),
        ("micro.js", Language::JavaScript, "export const leaf = function named(x) { return x; }; export function* seed() { yield 1; } class Oak { get value() { return 1; } static async see() { return leaf(1); } }\n"),
        ("micro.c", Language::C, "struct Oak { int leaf; }; static int see(int x) { return x; }\n"),
    ];
    let mut records = serde_json::json!({});
    for (path, language, text) in fixtures {
        let source = SourceSnapshot::new(text.as_bytes());
        let mut out = ParserRegistry::new().parse(path, text, language).unwrap();
        let structure = out.source_structure.as_ref().unwrap();
        assert!(structure.complete, "{path}");
        out.documents = Some(cc_index::documents::delta::prepare(&source, &out, &[]).unwrap());
        let identities = cc_index::documents::symbol_identity::prepare(&source, &out).unwrap();
        for proof in &identities {
            let candidates: Vec<_> = out
                .symbols
                .iter()
                .filter(|s| {
                    s.symbol_id == proof.symbol_id
                        && s.symbol_uid.as_deref() == Some(&proof.symbol_uid)
                })
                .collect();
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].kind, proof.kind);
            assert_eq!(candidates[0].qname.as_deref(), Some(proof.qname.as_str()));
        }
        records[path] = serde_json::json!({"text":text,"symbols":out.symbols,"structure":out.source_structure,"chunks":out.chunks,"identities":identities});
    }
    if let Ok(dest) = std::env::var("INDEPENDENT_GUARD_SNAPSHOT") {
        std::fs::write(dest, serde_json::to_vec_pretty(&records).unwrap()).unwrap();
    }
}
#[test]
fn decorator_owners_and_canonical_class_method_are_not_duplicated() {
    let text = "def annotate(x): return x\ndef maker(): return annotate\ndef factory():\n    @annotate\n    @maker()\n    class Local:\n        @annotate\n        @maker()\n        def see(self):\n            def leaf(): return maker()\n            return leaf()\n    return Local\n";
    let out = ParserRegistry::new()
        .parse("micro.py", text, Language::Python)
        .unwrap();
    for (q, k) in [
        ("factory.Local", SymbolKind::Class),
        ("factory.Local.see", SymbolKind::Method),
        ("factory.Local.see.leaf", SymbolKind::Function),
    ] {
        let found: Vec<_> = out
            .symbols
            .iter()
            .filter(|s| s.qname.as_deref() == Some(q))
            .collect();
        assert_eq!(found.len(), 1, "{q}");
        assert_eq!(found[0].kind, k);
    }
    let maker_calls: Vec<_> = out
        .call_edges
        .iter()
        .filter(|c| c.callee_symbol == "maker")
        .collect();
    assert_eq!(maker_calls.len(), 3);
    let owners: Vec<_> = maker_calls
        .iter()
        .map(|c| {
            c.caller_symbol_id
                .as_deref()
                .and_then(|id| out.symbols.iter().find(|s| s.symbol_id == id))
                .and_then(|s| s.qname.as_deref())
        })
        .collect();
    assert_eq!(
        owners,
        vec![Some("factory"), None, Some("factory.Local.see.leaf")]
    );
    let annotations: Vec<_> = out
        .symbol_refs
        .iter()
        .filter(|r| r.symbol_name == "annotate" && r.ref_kind == "identifier")
        .collect();
    assert!(annotations
        .iter()
        .any(|r| r.line == 4 && r.container.as_deref() == Some("factory")));
    assert!(annotations
        .iter()
        .any(|r| r.line == 7 && r.container.is_none()));
    let local = out
        .symbols
        .iter()
        .find(|s| s.qname.as_deref() == Some("factory.Local"))
        .unwrap();
    let function = out.symbols.iter().find(|s| s.name == "factory").unwrap();
    assert_eq!(
        local.parent_symbol_id.as_deref(),
        Some(function.symbol_id.as_str())
    );
    // Small local classes may live inside the whole function chunk. Assert the
    // real boundary; never manufacture an independent local class public hit.
    assert!(out
        .source_structure
        .as_ref()
        .unwrap()
        .boundaries
        .iter()
        .any(|b| b.name.as_deref() == Some("Local") && b.symbol_kind == Some(SymbolKind::Class)));
}
#[test]
fn cpp_rejected_kind_hint_keeps_actual_function_name_not_return_type() {
    let text = "namespace grove { template<class T> T leaf(T x) { return x; } }\n";
    let out = ParserRegistry::new()
        .parse("micro.cpp", text, Language::Cpp)
        .unwrap();
    let structure = out.source_structure.as_ref().unwrap();
    assert!(structure.complete);
    let boundary = structure
        .boundaries
        .iter()
        .find(|b| b.name.as_deref() == Some("leaf"))
        .expect("real declaration name leaf must survive kind rejection");
    assert_eq!(boundary.symbol_kind, Some(SymbolKind::Function));
    assert!(!structure
        .boundaries
        .iter()
        .any(|b| b.name.as_deref() == Some("T") && b.symbol_kind == Some(SymbolKind::Function)));
}
