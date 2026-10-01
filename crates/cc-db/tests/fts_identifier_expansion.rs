//! Public compiler MATCH behavior through real SQLite FTS5.
//! Historical flat-expansion failures are preserved in the evidence archive.
use cc_db::fts::compile_expanded_fts_query;
use rusqlite::Connection;
fn match_query(q: &str) -> String {
    compile_expanded_fts_query(q).query
}
fn find(query: &str, docs: &[(&str, &str)]) -> Vec<String> {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE VIRTUAL TABLE docs USING fts5(name UNINDEXED, body, tokenize='unicode61');",
    )
    .unwrap();
    for (name, body) in docs {
        db.execute("INSERT INTO docs(name,body) VALUES(?1,?2)", [name, body])
            .unwrap();
    }
    let mut s = db
        .prepare("SELECT name FROM docs WHERE docs MATCH ?1 ORDER BY name")
        .unwrap();
    let rows = s
        .query_map([match_query(query)], |r| r.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    rows
}
#[test]
fn unknown_identifier_does_not_buy_recall_from_a_generic_fragment() {
    assert_eq!(
        find(
            "ghostModuleInUnrelatedPackage9071",
            &[("loop", "for value in items return value")]
        ),
        Vec::<String>::new()
    );
}
#[test]
fn expansion_requires_all_components_not_just_one() {
    assert_eq!(
        find(
            "renderWidget",
            &[("widget_only", "widget"), ("all", "render useful widget")]
        ),
        vec!["all"]
    );
}
#[test]
fn full_identifier_and_all_components_are_both_useful() {
    assert_eq!(
        find(
            "renderWidget",
            &[("literal", "renderWidget"), ("components", "widget render")]
        ),
        vec!["components", "literal"]
    );
}
#[test]
fn independent_natural_words_still_have_or_semantics() {
    assert_eq!(find("in", &[("loop", "for value in items")]), vec!["loop"]);
    assert_eq!(
        find(
            "render widget",
            &[("render", "render"), ("widget", "widget")]
        ),
        vec!["render", "widget"]
    );
}
#[test]
fn snake_whole_preserves_phrase_and_parts_support_reordering() {
    assert_eq!(
        find(
            "get_user_name",
            &[
                ("whole", "get_user_name"),
                ("parts", "name useful user get"),
                ("missing", "user name")
            ]
        ),
        vec!["parts", "whole"]
    );
}
#[test]
fn one_distinct_component_never_degrades_to_generic_fragment() {
    assert_eq!(
        find("nameName", &[("whole", "nameName"), ("part", "name")]),
        vec!["whole"]
    );
}

#[test]
fn literal_fts_operators_and_punctuation_cannot_control_match_syntax() {
    for operator in ["OR", "AND", "NOT", "NEAR"] {
        assert_eq!(
            find(
                operator,
                &[("literal", operator), ("unrelated", "unrelated")]
            ),
            vec!["literal"]
        );
    }
    assert_eq!(
        find(
            "\" OR * : ()",
            &[("literal", "OR"), ("unrelated", "unrelated")]
        ),
        vec!["literal"]
    );
    assert_eq!(
        find("\"*():", &[("unrelated", "unrelated")]),
        Vec::<String>::new()
    );
}

#[test]
fn unicode_original_terms_coexist_with_compound_all_components() {
    assert_eq!(
        find(
            "用户 renderWidget",
            &[
                ("chinese", "用户"),
                ("all", "render widget"),
                ("onepart", "widget")
            ]
        ),
        vec!["all", "chinese"]
    );
}

#[test]
fn original_terms_have_priority_and_an_expansion_is_never_truncated() {
    let query = "renderWidget a0 a1 a2 a3 a4 a5 a6 a7 a8 a9 tail";
    let compiled = compile_expanded_fts_query(query);
    assert_eq!(compiled.source_tokens, 12);
    assert_eq!(compiled.atom_count, 12);
    assert!(!compiled.source_tokens_omitted);
    assert_eq!(compiled.identifier_groups_omitted, 1);
    assert_eq!(
        find(
            query,
            &[
                ("whole", "renderWidget"),
                ("last_original", "tail"),
                ("parts", "render widget")
            ]
        ),
        vec!["last_original", "whole"]
    );
}

#[test]
fn multiple_groups_share_one_atom_budget_without_dropping_originals() {
    let query = "alphaBeta gammaDelta one_two_three four_five_six";
    let c = compile_expanded_fts_query(query);
    assert_eq!(c.source_tokens, 4);
    assert_eq!(c.atom_count, 11);
    assert_eq!(c.identifier_groups_omitted, 1);
    assert_eq!(
        find(
            query,
            &[
                ("onepart", "alpha"),
                ("all_first", "beta alpha"),
                ("whole_last", "four_five_six"),
                ("omitted_parts", "six words five here four")
            ]
        ),
        vec!["all_first", "whole_last"]
    );
}

#[test]
fn oversized_identifier_group_is_omitted_as_a_whole() {
    let q = "alphaBetaGammaDeltaEpsilonZetaEtaThetaIotaKappaLambdaMu";
    let c = compile_expanded_fts_query(q);
    assert_eq!(c.atom_count, 1);
    assert_eq!(c.identifier_groups_omitted, 1);
    assert_eq!(
        find(
            q,
            &[
                (
                    "parts",
                    "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu"
                ),
                ("whole", q)
            ]
        ),
        vec!["whole"]
    );
}

#[test]
fn source_token_limit_is_explicit_and_does_not_claim_full_input_coverage() {
    let q = "word0 word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12";
    let c = compile_expanded_fts_query(q);
    assert_eq!(c.source_tokens, 12);
    assert_eq!(c.atom_count, 12);
    assert!(c.source_tokens_omitted);
    assert_eq!(
        find(q, &[("last_admitted", "word11"), ("omitted", "word12")]),
        vec!["last_admitted"]
    );
}

#[test]
fn acronym_and_number_boundaries_preserve_every_distinct_component() {
    assert_eq!(
        find(
            "HTTPServer",
            &[
                ("all", "server http"),
                ("whole", "httpserver"),
                ("onepart", "server")
            ]
        ),
        vec!["all", "whole"]
    );
    assert_eq!(
        find(
            "cache2Reader9071",
            &[
                ("all", "CACHE2 reader9071"),
                ("missing_suffix", "cache2 reader"),
                ("onepart", "reader9071")
            ]
        ),
        vec!["all"]
    );
}
