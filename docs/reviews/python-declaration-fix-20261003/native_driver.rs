//! Independent real engine / MCP / unmodified native evaluator micro fixture.
use cc_eval::benchmark::{metrics, normalizer, schema::Query};
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = Path::new(&a[1]);
    let out = Path::new(&a[2]);
    let baseline = a[3] == "baseline";
    std::fs::create_dir_all(root).unwrap();
    std::fs::create_dir_all(out).unwrap();
    let text = "# independent fixture: 中文 / β\r\nclass Cedar:\r\n    # leading method comment\r\n    def pulse(self):\r\n        return 'cedar-β'\r\nclass Willow:\r\n    def pulse(self):\r\n        return 'willow-中'\r\n";
    std::fs::write(root.join("micro.py"), text).unwrap();
    std::fs::write(
        root.join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let engine =
        serde_json::to_value(index.search().search_in_context("pulse", 10, None).unwrap()).unwrap();
    let backend = cc_eval::runner::CodeIndexBackend::open_existing(root).unwrap();
    let mcp = backend
        .call_tool(
            "search",
            &json!({"query":"pulse","top_k":10,"mode":"hybrid"}),
        )
        .unwrap();
    let gold: Query = serde_json::from_value(json!({"id":"independent-cedar-willow","category":"exact-symbol","difficulty":1,"language":"python","split":"synthetic","query_family":"independent-qname","query":"pulse","path_prefix":null,"no_answer":false,"expected_files":[],"answers":[
        {"id":"cedar","primary":true,"grade":2,"alternatives":[{"path":"micro.py","symbol":{"name":"pulse","qname":"Cedar.pulse","kind":"method"},"span":null}]},
        {"id":"willow","primary":true,"grade":2,"alternatives":[{"path":"micro.py","symbol":{"name":"pulse","qname":"Willow.pulse","kind":"method"},"span":null}]}
    ]})).unwrap();
    let mut wrong = gold.clone();
    for answer in &mut wrong.answers {
        answer.alternatives[0].symbol.as_mut().unwrap().qname = Some("Unrelated.pulse".into());
    }
    let mut scores = json!({});
    for (label, payload) in [("engine", &engine), ("mcp", &mcp)] {
        let (mut hits, status) = normalizer::mcp(payload).unwrap();
        assert!(!hits.is_empty());
        for hit in &mut hits {
            normalizer::verify_source(hit, root).unwrap();
            assert_eq!(hit.evidence_valid, Some(true));
        }
        let correct = metrics::native(&hits, &gold);
        let incorrect = metrics::native(&hits, &wrong);
        assert_eq!(correct.recall10, Some(if baseline { 0.0 } else { 1.0 }));
        assert_eq!(incorrect.recall10, Some(0.0));
        scores[label] = json!({"correct":correct,"wrong":incorrect,"status":status});
    }
    for (name, value) in [
        ("engine.json", engine),
        ("mcp.json", mcp),
        ("scores.json", scores),
        ("micro-gold.json", serde_json::to_value(gold).unwrap()),
        ("wrong-gold.json", serde_json::to_value(wrong).unwrap()),
    ] {
        std::fs::write(out.join(name), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    let classes = root.join("class-counterexample");
    std::fs::create_dir_all(&classes).unwrap();
    std::fs::write(
        classes.join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(classes.join("classes.py"), "@decorate\nclass Outer:\n    @decorate\n    class Inner:\n        def pulse(self): return 1\n").unwrap();
    let mut ci = CodeIndex::new(Some(&classes)).unwrap();
    ci.build_index(true).unwrap();
    let class_backend = cc_eval::runner::CodeIndexBackend::open_existing(&classes).unwrap();
    let mut evidence = json!({});
    for q in ["Outer", "Inner"] {
        evidence[q] = json!({"engine":serde_json::to_value(ci.search().search_in_context(q, 20, None).unwrap()).unwrap(), "mcp":class_backend.call_tool("search", &json!({"query":q,"top_k":20,"mode":"hybrid"})).unwrap()});
    }
    std::fs::write(
        out.join("decorated-class-public.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    println!(
        "{} real engine/MCP source verification and native correct/wrong qname assertions passed",
        a[3]
    );
}
