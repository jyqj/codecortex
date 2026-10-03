use cc_model::{query::RetrievalStrategy, search::SearchRequest};
use cc_server::engine::CodeIndex;
use std::{io::{self, BufRead, Write}, path::Path};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let index = CodeIndex::new(Some(Path::new(&root))).unwrap();
    // Preserve the existing actual v23 rows, including missing type evidence.
    let handle = index.query_handle().unwrap();
    let request = || SearchRequest { retrieval_strategy: Some(RetrievalStrategy::Local), ..Default::default() };
    let warm = runtime.block_on(handle.search_async("previous_marker".into(), 5, None, request())).unwrap();
    let before = index.index_db().unwrap().reads().read_generation().unwrap();
    println!("{}", serde_json::json!({"phase":"pinned_v22","schema":index.index_db().unwrap().reads().schema_version().unwrap(),"generation":format!("{before:?}"),"warm":format!("{warm:?}"),"pins":index.query_pins()}));
    io::stdout().flush().unwrap();
    let mut command = String::new();
    io::stdin().lock().read_line(&mut command).unwrap();
    assert_eq!(command.trim(), "after");
    let generation = index.index_db().unwrap().reads().read_generation().unwrap();
    let manifests = index.index_db().unwrap().reads().resolution_manifests(&["main.py".into()]);
    let query = runtime.block_on(handle.search_async("corrected_marker".into(), 5, None, request()));
    println!("{}", serde_json::json!({"phase":"old_handle_after_upgrade","generation":format!("{generation:?}"),"manifest_error":manifests.err().map(|e|e.to_string()),"query":query.as_ref().ok().map(|value|format!("{value:?}")),"query_error":query.as_ref().err().map(|e|e.to_string()),"pins":index.query_pins()}));
}
