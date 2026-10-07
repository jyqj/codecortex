//! A generated route node is a derived read model, not another parsed lookup
//! site. An API edit must not add it to the dirty-reloaded resolution manifest.
use cc_eval::benchmark::{
    mutation_case::{self, FactAssertion, MutationCase, Stage},
    mutations::Mutation,
};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn api_edit_keeps_route_manifest_equal_to_full_and_preserves_route_nodes() {
    let case = MutationCase {
        schema_version: 1,
        name: "p8-derived-route-reload".into(),
        initial: BTreeMap::from([
            (
                "api.ts".into(),
                "export function p8_target(x:number):number{return x;}\n".into(),
            ),
            (
                "routes.ts".into(),
                "import {Router} from 'express';\nimport {p8_target} from './api';\nconst router=Router();\nexport function handle(req:any,res:any){res.json(p8_target(1));}\nrouter.get('/p8',handle);\n".into(),
            ),
        ]),
        stages: vec![Stage {
            mutation: Mutation::Write {
                path: "api.ts".into(),
                content: "export function p8_target(x:number,offset:number=2):number{return x+offset;}\n".into(),
            },
            reopen: false,
            settle: true,
            assertions: vec![
                FactAssertion {
                    id: "parsed_and_normalized_route_are_still_stored".into(),
                    table: "routes".into(),
                    matches: BTreeMap::from([("route_path".into(), json!("/p8"))]),
                    count: 2,
                },
                FactAssertion {
                    id: "consumer_still_resolves_actual_api".into(),
                    table: "call_edges".into(),
                    matches: BTreeMap::from([
                        ("file_path".into(), json!("routes.ts")),
                        ("callee_symbol".into(), json!("p8_target")),
                        ("target_file_path".into(), json!("api.ts")),
                    ]),
                    count: 1,
                },
            ],
        }],
        dirty_budget: 1,
        max_resume_builds: 16,
    };
    let result = mutation_case::evaluate(&case).unwrap();
    if let Ok(path) = std::env::var("P8_ROUTE_RELOAD_EVIDENCE") {
        use std::io::Write;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        writeln!(output, "{}", serde_json::to_string_pretty(&result).unwrap()).unwrap();
    }
    assert_eq!(result["passed"], true, "{result}");
    let checkpoint = &result["checkpoints"][0];
    assert_eq!(checkpoint["status"], "compared");
    assert_eq!(checkpoint["different_tables"], json!([]));
    let row = checkpoint["incremental"]["resolution_manifests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["file_path"] == "routes.ts")
        .unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(row["payload"].as_str().unwrap()).unwrap();
    let routes: Vec<_> = payload["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["site_kind"] == "route")
        .collect();
    assert_eq!(routes.len(), 1, "only the actual parsed lookup is evidence");
    assert_eq!(routes[0]["query"], "handle");
    assert_eq!(routes[0]["outcome"]["target"]["file_path"], "routes.ts");
}
