//! Independent tiny source, real subprocess MCP, unchanged normalizer/scorer.
//! Diagnostic only: no public DEV gold, queries or product changes.
use cc_eval::benchmark::{
    adapters::{mcp_stdio::McpStdio, Backend},
    metrics, normalizer,
    schema::{Query, Row, ScoreProfile, SearchInput},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, time::Duration};

const SOURCE: &str = "def amber_probe(value):\n    return value + 7\n\nclass Lantern:\n    def violet_probe(self, value):\n        return value * 3\n\ndef outer_probe():\n    def inner_probe():\n        return 11\n    return inner_probe\n";

fn expectation(name: &str, qname: &str, kind: &str, start: usize, end: usize) -> Query {
    serde_json::from_value(json!({
        "id":"self-authored-analogue", "category":"diagnostic", "difficulty":1,
        "language":"python", "split":"dev", "query_family":"analogue", "query":name,
        "path_prefix":null,"no_answer":false,"expected_files":[], "annotations":{},
        "answers":[{"id":"definition", "primary":true,"grade":3,"alternatives":[{
            "path":"src/beacon/signals.py", "symbol":{"name":name,"qname":qname,"kind":kind},
            "span":{"start":start,"end":end}}]}]
    }))
    .unwrap()
}

#[tokio::test]
async fn actual_mcp_namespace_analogue() {
    let binary =
        std::env::var("REQUESTS_ZERO_PRODUCT_BINARY").expect("explicit fixed source binary");
    let out = std::env::var("REQUESTS_ZERO_OUTPUT").expect("diagnostic output directory");
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/beacon")).unwrap();
    std::fs::write(root.path().join("src/beacon/signals.py"), SOURCE).unwrap();
    let mut backend = McpStdio::spawn(Path::new(&binary), root.path(), Duration::from_secs(30))
        .await
        .unwrap();
    let prepare = backend.prepare(&[]).await.unwrap();
    std::fs::write(
        Path::new(&out).join("analogue-prepare.json"),
        serde_json::to_vec_pretty(&prepare).unwrap(),
    )
    .unwrap();
    let mut receipts = Vec::new();
    for (name, lexical, kind) in [
        ("amber_probe", "amber_probe", "function"),
        ("violet_probe", "Lantern.violet_probe", "method"),
        ("inner_probe", "outer_probe.inner_probe", "function"),
    ] {
        let symbols = backend
            .call("search", json!({"query":name,"top_k":10,"mode":"symbol"}))
            .await
            .unwrap();
        let (symbol_hits, _) = normalizer::mcp(&symbols).unwrap();
        let symbol = symbol_hits
            .iter()
            .find(|h| h.symbol_name.as_deref() == Some(name))
            .unwrap();
        assert_eq!(symbol.qname.as_deref(), Some(lexical));
        assert_eq!(symbol.kind.as_deref(), Some(kind));
        let raw = backend
            .search(&SearchInput {
                query: name.into(),
                top_k: 10,
                path_prefix: None,
            })
            .await
            .unwrap();
        std::fs::write(
            Path::new(&out).join(format!("analogue-{name}-raw.json")),
            serde_json::to_vec_pretty(&raw).unwrap(),
        )
        .unwrap();
        let (mut hits, status) = normalizer::mcp(&raw).unwrap();
        for hit in &mut hits {
            normalizer::verify_source(hit, root.path()).unwrap();
        }
        // Independent byte coordinates derived from this authored source, not a product span.
        let start = SOURCE.find(&format!("def {name}(")).unwrap();
        let end = SOURCE[start..].find('\n').unwrap() + start;
        let lexical_query = expectation(name, lexical, kind, start, end);
        let module_query =
            expectation(name, &format!("beacon.signals.{lexical}"), kind, start, end);
        let Some(direct) = hits.iter().find(|h| h.symbol_name.as_deref() == Some(name)) else {
            // A nested declaration can reside in its parent's chunk. Keep this miss;
            // never rename it, alter the query/budget, or fabricate a direct hit.
            assert_eq!(name, "inner_probe");
            receipts.push(json!({"name":name,"direct_retrieved":false,"status":status,
                "full_lexical":metrics::native(&hits,&lexical_query),
                "full_module":metrics::native(&hits,&module_query),"raw":raw,"normalized":hits,
                "symbol_mode_raw":symbols}));
            continue;
        };
        assert_eq!(direct.qname.as_deref(), Some(lexical));
        assert_eq!(direct.kind.as_deref(), Some(kind));
        assert_eq!(direct.evidence_valid, Some(true));
        let direct_lexical = metrics::native(std::slice::from_ref(direct), &lexical_query);
        let direct_module = metrics::native(std::slice::from_ref(direct), &module_query);
        assert_eq!(direct_lexical.top1, 1.0);
        assert_eq!(direct_module.top1, 0.0);
        assert_eq!(direct_module.ndcg10, 0.0);
        receipts.push(json!({"name":name,"actual_qname":direct.qname,
            "actual_kind":direct.kind,"verified":direct.evidence_valid,"status":status,
            "direct_lexical":direct_lexical,"direct_module":direct_module,
            "full_lexical":metrics::native(&hits,&lexical_query),
            "full_module":metrics::native(&hits,&module_query),
            "raw":raw,"normalized":hits,"symbol_mode_raw":symbols}));
    }
    backend.close().await.unwrap();
    std::fs::write(
        Path::new(&out).join("analogue.json"),
        serde_json::to_vec_pretty(&json!({"source":SOURCE,"prepare":prepare,"cases":receipts}))
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn all_fixed_raw_unchanged_normalizer_and_scorer() {
    let evidence = std::env::var("REQUESTS_ZERO_RAW").expect("fixed extracted archive");
    let corpus = std::env::var("REQUESTS_ZERO_SOURCE").expect("fixed original source");
    let mut totals = BTreeMap::new();
    for side in ["base", "candidate"] {
        for mode in ["native", "compat"] {
            let dir = Path::new(&evidence).join(side).join(mode);
            let manifest: Value =
                serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
            for file in manifest["input"]["files"].as_array().unwrap() {
                let bytes =
                    std::fs::read(Path::new(&corpus).join(file["path"].as_str().unwrap())).unwrap();
                assert_eq!(bytes.len() as u64, file["bytes"].as_u64().unwrap());
                assert_eq!(
                    blake3::hash(&bytes).to_hex().as_str(),
                    file["digest"].as_str().unwrap()
                );
            }
            let queries: BTreeMap<String, Query> =
                std::fs::read_to_string(dir.join("queries.jsonl"))
                    .unwrap()
                    .lines()
                    .map(|l| {
                        let q: Query = serde_json::from_str(l).unwrap();
                        (q.id.clone(), q)
                    })
                    .collect();
            let rows: Vec<Row> = std::fs::read_to_string(dir.join("normalized.jsonl"))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let profile = if mode == "native" {
                ScoreProfile::Native
            } else {
                ScoreProfile::OceCompat
            };
            let mut top1 = 0.0;
            let mut ndcg = 0.0;
            let saved_scores: Vec<metrics::Scores> =
                std::fs::read_to_string(dir.join("scores.jsonl"))
                    .unwrap()
                    .lines()
                    .map(|l| serde_json::from_str(l).unwrap())
                    .collect();
            assert_eq!(saved_scores.len(), rows.len());
            for (row, saved_score) in rows.iter().zip(saved_scores) {
                let raw: Value =
                    serde_json::from_slice(&std::fs::read(dir.join(&row.raw_path)).unwrap())
                        .unwrap();
                let (mut hits, status) = normalizer::mcp(&raw).unwrap();
                for hit in &mut hits {
                    normalizer::verify_source(hit, Path::new(&corpus)).unwrap();
                }
                assert_eq!(
                    serde_json::to_value(&hits).unwrap(),
                    serde_json::to_value(&row.hits).unwrap()
                );
                assert_eq!(status, row.status);
                let score = metrics::score(row, &queries[&row.case_id], profile).unwrap();
                assert_eq!(score, saved_score);
                top1 += score.top1;
                ndcg += score.ndcg10;
            }
            totals.insert(format!("{side}/{mode}"),json!({"rows":rows.len(),"top1":top1/rows.len() as f64,"ndcg10":ndcg/rows.len() as f64}));
        }
    }
    let out = std::env::var("REQUESTS_ZERO_OUTPUT").unwrap();
    std::fs::write(
        Path::new(&out).join("unchanged-replay.json"),
        serde_json::to_vec_pretty(&totals).unwrap(),
    )
    .unwrap();
}
