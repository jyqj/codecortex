//! Algorithm-upgrade integration boundaries; local queries only, no provider.
use cc_model::{query::RetrievalStrategy, search::SearchRequest};
use cc_server::engine::CodeIndex;

#[tokio::test]
async fn pinned_handle_uses_rebuilt_generation_and_corrected_source() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let path = root.path().join("main.go");
    std::fs::write(
        &path,
        "package main\nfunc previous_marker() { a.B(); a.C() }\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let handle = index.query_handle().unwrap();
    assert_eq!(index.query_pins(), 1);
    let before = index.index_db().unwrap().reads().read_generation().unwrap();
    std::fs::write(
        &path,
        "package main\nfunc corrected_marker() { a.B().C().D() }\n",
    )
    .unwrap();
    index.build_index(true).unwrap();
    let after = index.index_db().unwrap().reads().read_generation().unwrap();
    assert_ne!(before.incarnation, after.incarnation);
    assert!(after.index_epoch > before.index_epoch);
    let envelope = handle
        .search_async(
            "corrected_marker".into(),
            5,
            None,
            SearchRequest {
                retrieval_strategy: Some(RetrievalStrategy::Local),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let wire = serde_json::to_string(&envelope).unwrap();
    assert!(wire.contains("corrected_marker"), "{wire}");
    assert!(!wire.contains("previous_marker"), "{wire}");
    assert_eq!(index.query_pins(), 1);
    drop(handle);
    assert_eq!(index.query_pins(), 0);
}
