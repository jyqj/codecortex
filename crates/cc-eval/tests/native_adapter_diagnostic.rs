//! Independently authored miniature source/gold; no admission corpus inputs.
use cc_eval::benchmark::{metrics, normalizer, schema::*};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Query, Vec<Hit>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("pay.py"), "def charge():\n    return 7\n").unwrap();
    std::fs::write(
        root.path().join("caller.py"),
        "from pay import charge\ncharge()\n",
    )
    .unwrap();
    let query: Query = serde_json::from_value(json!({
        "id":"handgold", "category":"call_chain", "difficulty":1,
        "language":"python", "split":"dev", "query_family":"handgold",
        "query":"charge and caller", "path_prefix":null, "no_answer":false,
        "expected_files":["pay.py","caller.py"],
        "answers":[
            {"id":"definition","primary":true,"grade":2,"alternatives":[
                {"path":"pay.py","symbol":{"name":"charge","qname":"pay.charge","kind":"function"},"span":{"start":0,"end":26}}]},
            {"id":"caller","primary":false,"grade":1,"alternatives":[
                {"path":"caller.py","symbol":null,"span":{"start":23,"end":31}}]}],
        "annotations":{"v19":{"facets":[{"id":"definition","group_id":"definition","required":true},
            {"id":"caller","group_id":"caller","required":true}],"graph_constraints":[{"authored":"not evaluated by native-v1"}]}}
    })).unwrap();
    let payload = json!({"machine_pack":{"hits":[
        {"file_path":"pay.py","symbol_name":"charge","qname":"pay.charge","symbol_kind":"function",
            "start_line":1,"end_line":2,"text":"def charge():\n    return 7"},
        {"file_path":"caller.py","start_line":2,"end_line":2,"text":"charge()"}]}});
    let (mut hits, _) = normalizer::mcp(&payload).unwrap();
    for h in &mut hits {
        normalizer::verify_source(h, root.path()).unwrap();
    }
    assert!(hits.iter().all(|h| h.evidence_valid == Some(true)));
    (root, query, hits)
}

#[test]
fn exact_identity_cross_file_spans_and_multiple_groups() {
    let (_root, q, hits) = fixture();
    let full = metrics::native(&hits, &q);
    assert_eq!(
        (full.top1, full.ndcg10, full.recall10),
        (1.0, 1.0, Some(1.0))
    );
    assert_eq!(
        (full.span_precision, full.span_recall),
        (Some(1.0), Some(1.0))
    );
    let one = metrics::native(&hits[..1], &q);
    assert_eq!((one.top1, one.recall10), (1.0, Some(0.5)));
    assert_eq!(one.span_recall, Some(26.0 / 34.0));
    let duplicate = metrics::native(&[hits[0].clone(), hits[0].clone()], &q);
    assert_eq!(duplicate.recall10, Some(0.5));
    // Graph annotation changes cannot establish or invalidate a chain: no metric exists.
    let mut changed = q.clone();
    changed.annotations.clear();
    assert_eq!(metrics::native(&hits, &changed), full);
}

#[test]
fn each_identity_predicate_can_zero_a_valid_primary() {
    let (_root, q, hits) = fixture();
    for field in ["path", "name", "qname", "kind", "span", "evidence"] {
        let mut h = hits[0].clone();
        match field {
            "path" => h.path = "other.py".into(),
            "name" => h.symbol_name = Some("other".into()),
            "qname" => h.qname = None,
            "kind" => h.kind = Some("method".into()),
            "span" => h.span = Some(ByteSpan { start: 26, end: 27 }),
            "evidence" => h.evidence_valid = Some(false),
            _ => unreachable!(),
        }
        let score = metrics::native(&[h], &q);
        assert_eq!(
            (score.top1, score.ndcg10, score.recall10),
            (0.0, 0.0, Some(0.0)),
            "{field}"
        );
    }
    assert_eq!(
        metrics::compatibility(&["pay.py".into()], &q.expected_files)
            .unwrap()
            .top1,
        1.0
    );
}

#[test]
fn adapter_preserves_explicit_identity_but_never_infers_qname() {
    let payload = json!({"machine_pack":{"hits":[{"file_path":"pay.py","symbol_name":"charge",
        "symbol_kind":"function","breadcrumb":"pay.charge","metadata":{"qname":"pay.charge"}}]}});
    assert_eq!(
        normalizer::mcp(&payload).unwrap().0[0].qname.as_deref(),
        Some("pay.charge")
    );
    let mut absent = payload;
    absent["machine_pack"]["hits"][0]["metadata"] = json!({});
    assert_eq!(normalizer::mcp(&absent).unwrap().0[0].qname, None);
}

#[test]
fn partial_positive_receives_credit_and_partial_empty_is_not_absence() {
    let (_root, q, hits) = fixture();
    let mut row = Row {
        case_id: q.id.clone(),
        repetition: 0,
        status: ResultStatus::Partial,
        hits,
        elapsed_us: 1,
        raw_path: "handgold.json".into(),
        raw_digest: "handgold".into(),
        diagnostic: None,
    };
    assert_eq!(
        metrics::score(&row, &q, ScoreProfile::Native).unwrap().top1,
        1.0
    );
    let mut absent = q;
    absent.no_answer = true;
    absent.answers.clear();
    absent.expected_files.clear();
    row.hits.clear();
    assert_eq!(
        metrics::score(&row, &absent, ScoreProfile::Native)
            .unwrap()
            .no_answer_correct,
        Some(false)
    );
    row.status = ResultStatus::NoMatch;
    assert_eq!(
        metrics::score(&row, &absent, ScoreProfile::Native)
            .unwrap()
            .no_answer_correct,
        Some(true)
    );
}

#[test]
fn actual_micro_repository_returns_method_identity_without_qname() {
    use cc_eval::runner::CodeIndexBackend;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("pay.py"),
        "class Worker:\n    def charge(self):\n        return 7\n",
    )
    .unwrap();
    let backend = CodeIndexBackend::new(root.path()).unwrap();
    backend.build_index_report(true).unwrap();
    let raw = backend
        .call_tool(
            "search",
            &json!({"query":"charge","mode":"hybrid","top_k":10}),
        )
        .unwrap();
    let (mut hits, _) = normalizer::mcp(&raw).unwrap();
    for hit in &mut hits {
        normalizer::verify_source(hit, root.path()).unwrap();
    }
    let hit = hits
        .iter()
        .find(|h| h.symbol_name.as_deref() == Some("charge"))
        .expect("micro method retrieved");
    assert_eq!(hit.kind.as_deref(), Some("method"));
    assert_eq!(hit.qname, None);
    assert_eq!(hit.evidence_valid, Some(true));
    let mut query = fixture().1;
    query.answers.truncate(1);
    let answer = &mut query.answers[0].alternatives[0];
    answer.symbol.as_mut().unwrap().qname = Some("Worker.charge".into());
    answer.symbol.as_mut().unwrap().kind = Some("method".into());
    answer.span = Some(ByteSpan { start: 14, end: 49 });
    assert_eq!(metrics::native(&[hit.clone()], &query).ndcg10, 0.0);
    // Separate handgold control only: prove which single missing field blocked credit.
    let mut explicit = hit.clone();
    explicit.qname = Some("Worker.charge".into());
    assert_eq!(metrics::native(&[explicit], &query).ndcg10, 1.0);
}
