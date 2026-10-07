//! Self-authored neutral Go fixture; real index/engine/in-process MCP and unchanged scorer.
use cc_eval::benchmark::{metrics, normalizer, schema::Query};
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "fixture-root output-root variant");
    let root = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    assert!(!root.exists(), "fresh fixture destination required");
    assert!(!out.exists(), "fresh output destination required");
    std::fs::create_dir_all(root).unwrap();
    std::fs::create_dir_all(out).unwrap();
    let original = "package beacon\n\n// Beacon stores the garden state.\ntype Beacon struct {\n    Ready bool\n}\n\n// Commit changes the ready state.\nfunc (c *Beacon) Commit() {\n    c.Ready = true\n}\n\n// Sink accepts a seed.\ntype Sink interface {\n    Accept(seed int) error\n}\n\n// SeedCode names a seed.\ntype SeedCode string\n";
    let text = match args[3].as_str() {
        "base" => original.to_owned(),
        "receiver" => original.replace("    Ready bool", "    Ready bool\n    State bool"),
        "inversion" => original.replace("    Ready bool", "    // Commit API changes the ready state with the garden storage.\n    Ready bool"),
        _ => panic!("unknown self-authored fixture variant"),
    };
    std::fs::write(root.join("micro.go"), &text).unwrap();
    std::fs::write(root.join(".codecortex.json"), r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#).unwrap();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let backend = cc_eval::runner::CodeIndexBackend::open_existing(root).unwrap();
    let cases = [
        ("Which Beacon API is deprecated?", Some(("Beacon", "class"))),
        ("Which Beacon API rather than Sink accepts state?", None),
        ("Which Beacon API calls Commit/Accept?", None),
        ("Which Beacon API differs from Sink?", None),
        ("name:Beacon Which Beacon API calls Commit?", Some(("Beacon", "class"))),
        ("kind:class Which Beacon API calls Commit?", Some(("Beacon", "class"))),
        ("Which Beacon API RATHER THAN Sink accepts state?", None),
        ("Which Beacon API (rather than) Sink accepts state?", None),
        ("Which Beacon API rather\tthan Sink accepts state?", None),
        ("Which Beacon API instead of Sink accepts state?", None),
        ("Which API on Beacon rather than Sink accepts state?", None),
        ("Which Beacon API rather_than Sink accepts state?", None),
        ("Which Beacon API RatherThan Sink accepts state?", None),
        ("Which Beacon API rather.than Sink accepts state?", None),
        ("Which Beacon API rather more than Sink accepts state?", None),
        ("Which Beacon API instead_of Sink accepts state?", None),
        ("Commit", Some(("Commit", "method"))),
        ("Beacon", Some(("Beacon", "class"))),
        ("class Beacon", Some(("Beacon", "class"))),
        ("Sink", Some(("Sink", "interface"))),
        ("interface Sink", Some(("Sink", "interface"))),
        ("SeedCode", Some(("SeedCode", "type_alias"))),
        ("type SeedCode", Some(("SeedCode", "type_alias"))),
        ("method Commit on Beacon", Some(("Commit", "method"))),
        ("Which Beacon API changes the ready state with Commit?", Some(("Commit", "method"))),
        ("Which method on Beacon changes the ready state with Commit?", Some(("Commit", "method"))),
        ("Beacon.Commit", None),
        ("compare Beacon.Commit and Sink.Accept", None),
        ("name:Commit kind:method", Some(("Commit", "method"))),
        ("Which Beacon API calls Commit and Accept?", None),
        ("Which Beacon type stores the ready state?", Some(("Beacon", "class"))),
        ("methods on Beacon", None),
    ];
    let mut result = json!({"variant":args[3],"source_text":text,"queries":{}});
    for (query, expected) in cases {
        let engine = serde_json::to_value(index.search().search_in_context(query, 10, None).unwrap()).unwrap();
        let mcp = backend.call_tool("search", &json!({"query":query,"top_k":10,"mode":"hybrid"})).unwrap();
        for (api, payload) in [("engine", engine), ("mcp", mcp)] {
            let (mut hits, status) = normalizer::mcp(&payload).unwrap();
            for hit in &mut hits {
                normalizer::verify_source(hit, root).unwrap();
                assert_eq!(hit.evidence_valid, Some(true));
            }
            let mut entry = json!({"raw":payload,"normalized":hits,"status":status});
            if let Some((name, kind)) = expected {
                let gold: Query = serde_json::from_value(json!({
                    "id":"own-neutral-owner-role", "category":"exact-symbol", "difficulty":1,
                    "language":"Go", "split":"synthetic", "query_family":"own-neutral-owner-role",
                    "query":query, "path_prefix":null, "no_answer":false, "expected_files":[],
                    "answers":[{"id":"declaration","primary":true,"grade":3,"alternatives":[{
                        "path":"micro.go","symbol":{"name":name,"qname":null,"kind":kind},"span":null
                    }]}]
                })).unwrap();
                entry["scores"] = serde_json::to_value(metrics::native(&hits, &gold)).unwrap();
                let mut wrong = gold.clone();
                wrong.answers[0].alternatives[0].symbol.as_mut().unwrap().name = "Unrelated".into();
                assert_eq!(metrics::native(&hits, &wrong).recall10, Some(0.0));
            }
            result["queries"][query][api] = entry;
        }
    }
    let query = "Which Beacon API changes the ready state with Commit?";
    let small = serde_json::to_value(index.search().search_in_context(query, 1, None).unwrap()).unwrap();
    result["top_k_1"] = small;
    std::fs::write(out.join("results.json"), serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!("32 synthetic queries, engine + in-process MCP; all returned evidence source-verified; unrelated-name scorer controls rejected");
}
