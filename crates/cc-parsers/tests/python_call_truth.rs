//! Independent call-site truth, not merely full-vs-incremental agreement.
use cc_model::Language;
use cc_parsers::ParserRegistry;
fn parse(s: &str) -> cc_model::ParseOutcome {
    ParserRegistry::new()
        .parse("a.py", s, Language::Python)
        .unwrap()
}
#[test]
fn declarations_comments_and_strings_are_not_calls_or_references() {
    let o=parse("def target(x):\n    return x\n\ndef caller(x):\n    # target(x)\n    marker = \"target(x)\"\n    return x\n");
    assert!(o.call_edges.is_empty(), "{:?}", o.call_edges);
    assert!(!o.symbol_refs.iter().any(|r| r.symbol_name == "target"));
}
#[test]
fn multiline_recursion_and_formatted_string_expressions_are_real_calls() {
    let o=parse("def target(x):\n    return target(\n        x-1\n    ) if x else 0\n\ndef caller(x):\n    return f'value={target(x)}'\n");
    assert_eq!(o.call_edges.len(), 2, "{:?}", o.call_edges);
    assert_eq!(o.call_edges[0].line, 2);
    assert_eq!(o.call_edges[1].line, 7);
    assert!(o.call_edges.iter().all(|e| e.callee_symbol == "target"));
}
#[test]
fn nested_body_is_not_duplicated_as_a_call_of_its_outer_function() {
    let o=parse("def target():\n    pass\ndef outer():\n    def inner():\n        target()\n    return inner\n");
    assert_eq!(o.call_edges.len(), 1, "{:?}", o.call_edges);
    assert_eq!(o.call_edges[0].caller_symbol.as_deref(), Some("inner"));
}
#[test]
fn default_expression_belongs_to_outer_scope_and_dynamic_chains_have_unique_ids() {
    let o=parse("def make():\n    pass\ndef outer():\n    def inner(x=make()):\n        return x\n    return make()()\n");
    assert_eq!(o.call_edges.len(), 3, "{:?}", o.call_edges);
    assert!(o
        .call_edges
        .iter()
        .all(|c| c.caller_symbol.as_deref() == Some("outer")));
    let ids: std::collections::HashSet<_> = o.call_edges.iter().map(|c| &c.edge_id).collect();
    assert_eq!(ids.len(), 3);
    assert!(o
        .call_edges
        .iter()
        .any(|c| c.resolution_strategy == cc_model::resolution::PARSER_UNSUPPORTED_BINDING));
}

#[test]
fn parameter_shadow_and_receiver_method_are_not_parser_exact_global_functions() {
    let o = parse(
        "def target(x):\n    return x\ndef use(target, obj):\n    target(1)\n    obj.target(2)\n",
    );
    assert_eq!(o.call_edges.len(), 2);
    assert!(
        o.call_edges.iter().all(|c| c.target_symbol_id.is_none()),
        "{:?}",
        o.call_edges
    );
}
