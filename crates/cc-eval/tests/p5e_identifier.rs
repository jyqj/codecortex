//! End-to-end lexical expansion contracts, not fixture-id special cases.
use cc_server::engine::CodeIndex;

#[test]
fn unknown_compound_identifier_does_not_recall_a_generic_loop_word() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("session.py"), "def retry_connection(connect, attempts=3):\n    for _ in range(attempts):\n        return connect()\n").unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    for query in [
        "zzNoSuchSymbolInThisFixture4938",
        "missingWidgetInRemoteForest",
        "missing_widget_in_remote_forest",
    ] {
        let envelope = index.search().search_in_context(query, 10, None).unwrap();
        assert!(
            envelope.machine_pack["hits"].as_array().unwrap().is_empty(),
            "absent identifier recalled unrelated source: {query}"
        );
    }
    let natural = index.search().search_in_context("in", 10, None).unwrap();
    assert!(
        !natural.machine_pack["hits"].as_array().unwrap().is_empty(),
        "independent natural words retain their ordinary lexical meaning"
    );
}

#[test]
fn lexical_atom_budget_is_visible_not_a_complete_absence_claim() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("needle.py"),
        "def thirteenth():\n    return 13\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let query = "first second third fourth fifth sixth seventh eighth ninth tenth eleventh twelfth thirteenth";
    let envelope = index.search().search_in_context(query, 10, None).unwrap();
    let retrieval = &envelope.evidence_summary["retrieval"];
    let lanes = retrieval["lanes"]
        .as_array()
        .or_else(|| retrieval["lane_receipts"].as_array())
        .unwrap();
    let lexical = lanes
        .iter()
        .find(|lane| lane["lane_id"] == "lexical")
        .unwrap();
    assert_eq!(lexical["status"], "partial");
    assert_eq!(lexical["truncation_reason"], "query_expansion_atom_budget");
    assert_eq!(lexical["coverage"]["complete"], false);
}
