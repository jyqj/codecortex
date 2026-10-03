//! Standalone receipt driver, linked to the unchanged locked cc-eval library.
use cc_eval::benchmark::{metrics, normalizer, schema::Query};
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let repo = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    let root = Path::new(&args[3]);
    std::fs::create_dir_all(root).unwrap();
    let text = "class Alpha:\n    def needle(self):\n        return '中文'\nclass Beta:\n    def needle(self):\n        return 'β'\n";
    std::fs::write(root.join("a.py"), text).unwrap();
    std::fs::write(
        root.join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    // The production API supplies qname; the driver never fills hit metadata.
    let current = serde_json::to_value(
        index
            .search()
            .search_in_context("needle", 10, None)
            .unwrap(),
    )
    .unwrap();
    let baseline: Value = serde_json::from_slice(&std::fs::read(repo.join("artifacts/checkpoints/qname-source-identity-design-20261003/public-context-baseline.json")).unwrap()).unwrap();
    let query: Query = serde_json::from_value(json!({
        "id":"synthetic-source-bound-methods","category":"exact-symbol","difficulty":1,"language":"python","split":"synthetic","query_family":"qname-association","query":"needle","path_prefix":null,"no_answer":false,"expected_files":[],
        "answers":[
            {"id":"alpha","primary":true,"grade":2,"alternatives":[{"path":"a.py","symbol":{"name":"needle","qname":"Alpha.needle","kind":"method"},"span":null}]},
            {"id":"beta","primary":true,"grade":2,"alternatives":[{"path":"a.py","symbol":{"name":"needle","qname":"Beta.needle","kind":"method"},"span":null}]}
        ]
    })).unwrap();
    let mut wrong = query.clone();
    for answer in &mut wrong.answers {
        answer.alternatives[0].symbol.as_mut().unwrap().qname = Some("Absent.needle".into());
    }
    let normalize = |payload: &Value| {
        let (mut hits, status) = normalizer::mcp(payload).unwrap();
        for hit in &mut hits {
            normalizer::verify_source(hit, root).unwrap();
            assert_eq!(hit.evidence_valid, Some(true));
        }
        (hits, status)
    };
    let (missing, missing_status) = normalize(&baseline);
    let (verified, status) = normalize(&current);
    let backend = cc_eval::runner::CodeIndexBackend::open_existing(root).unwrap();
    let wire_current = backend
        .call_tool(
            "search",
            &json!({"query":"needle","top_k":10,"mode":"hybrid"}),
        )
        .unwrap();
    let (wire_hits, wire_status) = normalize(&wire_current);
    let baseline_score = metrics::native(&missing, &query);
    let correct_score = metrics::native(&verified, &query);
    let wrong_score = metrics::native(&verified, &wrong);
    let wire_score = metrics::native(&wire_hits, &query);
    assert_eq!(wire_score, correct_score);
    assert_eq!(metrics::native(&wire_hits, &wrong).recall10, Some(0.0));
    assert_eq!(baseline_score.recall10, Some(0.0));
    assert_eq!(correct_score.recall10, Some(1.0));
    assert_eq!(wrong_score.recall10, Some(0.0));
    // Freeze every pre-existing public hit field except additive metadata.
    let old_hits = baseline["machine_pack"]["hits"].as_array().unwrap();
    let new_hits = current["machine_pack"]["hits"].as_array().unwrap();
    assert_eq!(old_hits.len(), new_hits.len());
    for (old, new) in old_hits.iter().zip(new_hits) {
        for (key, value) in old.as_object().unwrap() {
            if key != "metadata" {
                assert_eq!(value, &new[key], "changed pre-existing field {key}");
            }
        }
        for key in ["source_evidence", "document"] {
            assert_eq!(old["metadata"][key], new["metadata"][key]);
        }
    }
    std::fs::create_dir_all(out).unwrap();
    for (file, value) in [
        ("public-context-current.json", current),
        ("public-mcp-current.json", wire_current),
        ("micro-gold.json", serde_json::to_value(&query).unwrap()),
        (
            "wrong-qname-gold.json",
            serde_json::to_value(&wrong).unwrap(),
        ),
        (
            "native-scores.json",
            json!({"missing_identity":baseline_score,"correct_identity":correct_score,"wrong_qname":wrong_score,"mcp_wire_correct_identity":wire_score,"mcp_wire_status":wire_status,"baseline_status":missing_status,"current_status":status,"original_hit_fields_and_score_trace":"unchanged","source_proof":"verified_from_full_original_fixture_bytes"}),
        ),
    ] {
        std::fs::write(out.join(file), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    println!("native micro gold: missing recall10=0, correct recall10=1, wrong qname recall10=0; actual MCP JSON-RPC correct recall10=1 and wrong qname=0; all original hit fields/score traces/source/document identities unchanged");
}
