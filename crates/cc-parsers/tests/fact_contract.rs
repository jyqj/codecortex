use cc_model::{Language, ParseOutcome};
use cc_parsers::ParserRegistry;

fn parse(src: &str, language: Language) -> ParseOutcome {
    ParserRegistry::new()
        .parse(
            if language == Language::Python {
                "facts.py"
            } else {
                "facts.ts"
            },
            src,
            language,
        )
        .unwrap()
}

#[test]
fn declarations_comments_and_strings_are_not_calls() {
    for (language, source) in [
        (Language::TypeScript, "export function transact() {\n // ghost()\n return \"ghost()\";\n}\nfunction ghost() { return 1; }"),
        (Language::Python, "def transact():\n    # ghost()\n    return 'ghost()'\ndef ghost():\n    return 1\n"),
    ] {
        let p = parse(source, language);
        assert!(p.call_edges.is_empty(), "{language:?}: {:?}", p.call_edges);
        assert!(!p.symbol_refs.iter().any(|r| r.ref_kind == "call"));
    }
}

#[test]
fn real_recursion_is_preserved_exactly_once() {
    for (language, source) in [
        (
            Language::TypeScript,
            "function f(n: number): number {\n return n ? f(n - 1) : 0;\n}",
        ),
        (
            Language::Python,
            "def f(n):\n    return f(n - 1) if n else 0\n",
        ),
    ] {
        let p = parse(source, language);
        assert_eq!(p.call_edges.len(), 1, "{language:?}: {:?}", p.call_edges);
        let c = &p.call_edges[0];
        assert_eq!(c.caller_symbol.as_deref(), Some("f"));
        assert_eq!(c.callee_symbol, "f");
        assert_eq!(c.caller_symbol_uid, c.callee_symbol_uid);
        assert!(c.callee_symbol_uid.is_some());
    }
}

#[test]
fn nested_call_belongs_to_innermost_function() {
    for (language, source) in [
        (
            Language::TypeScript,
            "function target() {}\nfunction outer() {\n function inner() { target(); }\n}",
        ),
        (
            Language::Python,
            "def target():\n    pass\ndef outer():\n    def inner():\n        target()\n",
        ),
    ] {
        let p = parse(source, language);
        let calls: Vec<_> = p
            .call_edges
            .iter()
            .filter(|e| e.callee_symbol == "target")
            .collect();
        assert_eq!(calls.len(), 1, "{language:?}: {calls:?}");
        assert_eq!(calls[0].caller_symbol.as_deref(), Some("inner"));
    }
}

#[test]
fn parameter_shadowing_does_not_claim_exact_global_target() {
    for (language, source) in [
        (
            Language::TypeScript,
            "function target() {}\nfunction run(target: () => void) { target(); }",
        ),
        (
            Language::Python,
            "def target():\n    pass\ndef run(target):\n    target()\n",
        ),
    ] {
        let p = parse(source, language);
        let call = p
            .call_edges
            .iter()
            .find(|e| e.caller_symbol.as_deref() == Some("run") && e.callee_symbol == "target")
            .unwrap();
        assert!(call.callee_symbol_uid.is_none(), "{language:?}: {call:?}");
        assert_eq!(call.resolution_strategy, "parser_shadowed");
    }
}

#[test]
fn nested_arguments_await_and_multiline_calls_are_retained() {
    let p = parse(
        "async function f() {\n await outer(\n  inner()\n );\n}\n",
        Language::TypeScript,
    );
    assert_eq!(p.call_edges.len(), 2, "{:?}", p.call_edges);
    assert!(p
        .call_edges
        .iter()
        .any(|c| c.callee_symbol == "outer" && c.is_awaited));
    assert!(p.call_edges.iter().any(|c| c.callee_symbol == "inner"));
}

#[test]
fn import_alias_does_not_declare_the_original_name_locally() {
    for (language,source) in [
        (Language::TypeScript,"import { target as remote } from './api';\nfunction target() {}\nfunction run() { target(); }\n"),
        (Language::Python,"from api import target as remote\ndef target():\n    pass\ndef run():\n    target()\n"),
    ] {
        let p=parse(source,language);
        let c=p.call_edges.iter().find(|e|e.caller_symbol.as_deref()==Some("run")).unwrap();
        assert_eq!(c.resolution_strategy,"parser_exact");assert!(c.callee_symbol_uid.is_some());
    }
}
#[test]
fn es_import_records_preserve_each_actual_binding() {
    let p = parse(
        "import Def, { first as local, second } from './a';\nimport * as ns from './b';",
        Language::TypeScript,
    );
    assert_eq!(p.imports.len(), 4);
    assert!(p
        .imports
        .iter()
        .any(|i| i.alias.as_deref() == Some("Def") && i.is_default));
    assert!(p.imports.iter().any(
        |i| i.alias.as_deref() == Some("local") && i.imported_name.as_deref() == Some("first")
    ));
    assert!(p
        .imports
        .iter()
        .any(|i| i.alias.as_deref() == Some("ns") && i.is_namespace));
}

#[test]
fn function_valued_declarators_have_distinct_identity_and_call_ownership() {
    let p = parse(
        "function target() {}\nconst first = () => target(), second = () => target();\n",
        Language::TypeScript,
    );
    let funcs: Vec<_> = p
        .symbols
        .iter()
        .filter(|s| s.name == "first" || s.name == "second")
        .collect();
    assert_eq!(funcs.len(), 2);
    assert_ne!(funcs[0].symbol_id, funcs[1].symbol_id);
    assert_eq!(p.call_edges.len(), 2);
    let owners: std::collections::BTreeSet<_> = p
        .call_edges
        .iter()
        .map(|c| c.caller_symbol.as_deref())
        .collect();
    assert_eq!(
        owners,
        [Some("first"), Some("second")].into_iter().collect()
    );
}
