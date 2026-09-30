//! Independent syntax facts: an identical full/inc graph can still be wrong.
use cc_model::{Language, ParseOutcome};
use cc_parsers::ParserRegistry;
fn parse(source: &str, language: Language) -> ParseOutcome {
    ParserRegistry::new()
        .parse("test.ts", source, language)
        .unwrap()
}
#[test]
fn declarations_comments_and_string_bodies_cannot_create_jsts_calls() {
    for language in [Language::JavaScript, Language::TypeScript] {
        let out=parse("function target(x) { return x; }\nfunction caller(x) {\n  // target(x)\n  const text = 'target(x)';\n  return x;\n}\n",language);
        assert!(out.call_edges.is_empty(), "{:?}", out.call_edges);
        assert!(
            !out.symbol_refs.iter().any(|r| r.symbol_name == "target"),
            "{:?}",
            out.symbol_refs
        );
    }
}
#[test]
fn real_calls_are_preserved_without_duplicate_member_leaf_calls() {
    let out = parse(
        "function run(x) {\n return target(\n x\n ) + obj.target(x);\n}\n",
        Language::TypeScript,
    );
    let mut calls: Vec<_> = out
        .call_edges
        .iter()
        .map(|c| c.callee_symbol.as_str())
        .collect();
    calls.sort();
    assert_eq!(calls, vec!["obj.target", "target"]);
    assert_eq!(
        out.call_edges
            .iter()
            .map(|c| &c.edge_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        2
    );
}
#[test]
fn nested_and_dynamic_calls_have_real_owners_and_distinct_positions() {
    let out=parse("function outer(){ function inner(){ target(); } return inner; }\nfunction run(){ factory()(); obj.first().second(); }\n",Language::TypeScript);
    let targets: Vec<_> = out
        .call_edges
        .iter()
        .filter(|c| c.callee_symbol == "target")
        .collect();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].caller_symbol.as_deref(), Some("inner"));
    assert_eq!(
        out.call_edges
            .iter()
            .map(|c| &c.edge_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        out.call_edges.len()
    );
    assert!(out.call_edges.iter().any(|c| c.callee_symbol == "factory()"
        && c.resolution_strategy == cc_model::resolution::PARSER_UNSUPPORTED_BINDING));
}

#[test]
fn template_substitution_and_await_arguments_keep_real_calls() {
    let out=parse("async function run(x) {\n const value = `text target(x) ${target(x)}`;\n return await wrap(target(x));\n}\n",Language::TypeScript);
    let mut calls: Vec<_> = out
        .call_edges
        .iter()
        .map(|c| c.callee_symbol.as_str())
        .collect();
    calls.sort();
    assert_eq!(
        calls,
        vec!["target", "target", "wrap"],
        "{:?}",
        out.call_edges
    );
}
