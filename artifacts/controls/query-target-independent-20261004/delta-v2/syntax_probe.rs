//! Compile the unchanged syntax recognizer into a review-only observer.
#[path = "/tmp/query-review-fixed/crates/cc-search/src/dsl.rs"]
mod dsl;
#[path = "/tmp/query-review-fixed/crates/cc-search/src/query_target.rs"]
mod query_target;
use query_target::QueryTarget;
fn main() {
    let root = std::path::Path::new(&std::env::args().nth(1).unwrap()).to_owned();
    let mut observations = Vec::new();
    for path in [root.join("matrix.json")] {
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for row in rows {
            let query = dsl::parse_search_dsl(row["query"].as_str().unwrap());
            let target = QueryTarget::parse(&query);
            let actual = match &target {
                QueryTarget::Ambiguous => "fallback".to_string(),
                QueryTarget::ContainerMethods => "none".to_string(),
                QueryTarget::Named { name, kind } => {
                    format!("named:{name}:{}", kind.map_or("any", |k| k.as_str()))
                }
            };
            let expected = row["expected"].as_str().unwrap();
            if !expected.ends_with("-ambiguous") {
                assert_eq!(actual, expected, "{}", row["id"]);
            }
            let permitted = ["Beacon", "pulse", "py", "Vessel", "rs"].map(|name| {
                (
                    name,
                    target.permits(name, Some("class"), &[name.to_lowercase()]),
                )
            });
            observations.push(serde_json::json!({"id":row["id"],"query":row["query"],"parsed_text":query.text,"name_filter":query.name_filter,"kind_filter":query.kind_filter,"actual":actual,"review_expectation":expected,"permitted_class_names":permitted}));
        }
    }
    std::fs::write(
        root.join("syntax-observations.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    println!(
        "{} syntax observations; all predeclared non-diagnostic expectations pass",
        observations.len()
    );
}
