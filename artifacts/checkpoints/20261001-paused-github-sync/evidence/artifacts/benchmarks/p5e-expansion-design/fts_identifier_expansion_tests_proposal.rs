//! First run uses the actual historical flat expansion to capture a red failure.
//! After helper implementation, replace match_query with compile_expanded_fts_query.
use cc_db::fts::{expand_query_text, sanitize_fts_query};
use rusqlite::Connection;
fn match_query(q: &str) -> String {
    sanitize_fts_query(&expand_query_text(q))
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
