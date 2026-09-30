use cc_eval::benchmark::{
    metrics, normalizer,
    readiness::{Readiness, State},
    sampler,
    schema::*,
    statistics, validation,
};
use serde_json::json;
#[test]
fn partial_empty_is_not_a_correct_no_answer() {
    let payload = json!({"machine_pack":{"hits":[]},"evidence_summary":{"retrieval":{"grep":{"status":"partial","reason":"scan_cap"}}}});
    let (hits, status) = normalizer::mcp(&payload).unwrap();
    assert_eq!(status, ResultStatus::Partial);
    let mut query = q();
    query.no_answer = true;
    query.answers.clear();
    let row = Row {
        case_id: query.id.clone(),
        repetition: 0,
        status,
        hits,
        elapsed_us: 1,
        raw_path: "raw/0.json".into(),
        raw_digest: "x".into(),
        diagnostic: None,
    };
    assert_eq!(
        metrics::score(&row, &query, ScoreProfile::Native)
            .unwrap()
            .no_answer_correct,
        Some(false)
    );
    assert!(normalizer::mcp(&json!({"machine_pack":{"hits":[]},"evidence_summary":{"retrieval":{"grep":{"status":"invented"}}}})).is_err());
}

#[test]
fn lane_only_incomplete_empty_never_scores_as_correct_no_answer() {
    use cc_model::retrieval::{LaneCoverage, LaneOutcome, LaneStatus};
    for state in [
        LaneStatus::Partial,
        LaneStatus::Timeout,
        LaneStatus::Unavailable,
        LaneStatus::Error,
        LaneStatus::Cancelled,
    ] {
        let mut lane = LaneOutcome::disabled("graph", 1.0);
        lane.status = state;
        lane.truncation_reason = Some("authored_limit_or_failure".into());
        lane.validate().unwrap();
        let payload = json!({"machine_pack":{"hits":[]},
            "evidence_summary":{"retrieval":{"lanes":[lane]}}});
        let (hits, status) = normalizer::mcp(&payload).unwrap();
        assert_eq!(status, ResultStatus::Partial, "lane={state:?}");
        let mut query = q();
        query.no_answer = true;
        query.answers.clear();
        let row = Row {
            case_id: query.id.clone(),
            repetition: 0,
            status,
            hits,
            elapsed_us: 1,
            raw_path: "raw/0.json".into(),
            raw_digest: "x".into(),
            diagnostic: None,
        };
        assert_eq!(
            metrics::score(&row, &query, ScoreProfile::Native)
                .unwrap()
                .no_answer_correct,
            Some(false)
        );
    }
    let mut complete = LaneOutcome::disabled("exact_symbol", 1.0);
    complete.status = LaneStatus::Complete;
    complete.coverage = LaneCoverage::complete(Some(0), 0);
    let valid =
        json!({"machine_pack":{"hits":[]},"evidence_summary":{"retrieval":{"lanes":[complete]}}});
    assert_eq!(normalizer::mcp(&valid).unwrap().1, ResultStatus::NoMatch);
    let mut corrupt = valid.clone();
    corrupt["evidence_summary"]["retrieval"]["lanes"][0]["status"] = json!("invented");
    assert!(normalizer::mcp(&corrupt).is_err());
    corrupt["evidence_summary"]["retrieval"]["grep"] =
        json!({"status":"partial","reason":"scan_cap"});
    assert!(
        normalizer::mcp(&corrupt).is_err(),
        "another partial lane must not bypass validation"
    );
    corrupt = valid.clone();
    corrupt["evidence_summary"]["retrieval"]["lanes"][0]["coverage"]["complete"] = json!(false);
    assert!(normalizer::mcp(&corrupt).is_err());
    corrupt = valid.clone();
    let duplicate = corrupt["evidence_summary"]["retrieval"]["lanes"][0].clone();
    corrupt["evidence_summary"]["retrieval"]["lanes"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(normalizer::mcp(&corrupt).is_err());
    assert_eq!(
        normalizer::mcp(&json!({"machine_pack":{"hits":[]}}))
            .unwrap()
            .1,
        ResultStatus::NoMatch,
        "legacy binaries without lane receipts remain supported"
    );
}

#[test]
fn machine_pack_preserves_public_symbol_identity() {
    let(h,_)=normalizer::mcp(&json!({"machine_pack":{"hits":[{"file_path":"a.rs","symbol_name":"target","symbol_kind":"function","start_line":1,"end_line":2,"text":"fn target() {}"}]},"nodes":[]})).unwrap();
    assert_eq!(h[0].symbol_name.as_deref(), Some("target"));
    assert_eq!(h[0].kind.as_deref(), Some("function"));
}
#[test]
fn independent_python_fnmatch_goldens() {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../benchmarks/goldens/fnmatch.json")).unwrap();
    assert!(rows.len() > 150);
    for r in rows {
        assert_eq!(
            metrics::path_matches(
                r["actual"].as_str().unwrap(),
                r["pattern"].as_str().unwrap()
            )
            .unwrap(),
            r["matches"].as_bool().unwrap(),
            "{r}"
        );
    }
}
fn q() -> Query {
    serde_json::from_value(json!({"id":"q1","category":"symbol_location","difficulty":2,"language":"rust","split":"dev","query_family":"definition","query":"charge","path_prefix":null,"no_answer":false,"expected_files":[],"answers":[{"id":"impl","primary":true,"grade":2,"alternatives":[{"path":"src/pay.rs","symbol":{"name":"charge","qname":null,"kind":null},"span":{"start":10,"end":20}}]},{"id":"caller","primary":false,"grade":1,"alternatives":[{"path":"src/caller.rs","symbol":null,"span":null}]}]})).unwrap()
}
fn hit() -> Hit {
    Hit {
        path: "src/pay.rs".into(),
        symbol_name: Some("charge".into()),
        span: Some(ByteSpan { start: 10, end: 20 }),
        evidence_valid: Some(true),
        ..Hit::default()
    }
}
#[test]
fn compatibility_supporting_is_top1_but_not_primary_grade() {
    let s = metrics::compatibility(&["b.rs".into()], &["a.rs".into(), "b.rs".into()]).unwrap();
    assert_eq!(s.top1, 1.0);
    assert!((s.ndcg10 - 1.0 / (2.0 + 1.0 / 3.0f64.log2())).abs() < 1e-12);
}
#[test]
fn compatibility_repeated_path_does_not_spend_rank() {
    let e = vec!["a.rs".into(), "b.rs".into()];
    let s = metrics::compatibility(&["a.rs".into(), "a.rs".into(), "b.rs".into()], &e).unwrap();
    assert!((s.ndcg10 - 1.0).abs() < 1e-12);
}
#[test]
fn compatibility_is_linear_gain() {
    let s = metrics::compatibility(
        &["b.rs".into(), "a.rs".into()],
        &["a.rs".into(), "b.rs".into()],
    )
    .unwrap();
    let expected = (1.0 + 2.0 / 3.0f64.log2()) / (2.0 + 1.0 / 3.0f64.log2());
    assert!((s.ndcg10 - expected).abs() < 1e-12);
}
#[test]
fn compatibility_glob_can_match_nested_path() {
    assert!(metrics::path_matches("src/nested/a.rs", "src/*.rs").unwrap());
    assert!(!metrics::path_matches("SRC/a.rs", "src/*.rs").unwrap());
}
#[test]
fn empty_expected_is_invalid() {
    assert!(metrics::compatibility(&[], &[]).is_err());
}
#[test]
fn native_wrong_file_same_name_gets_no_credit() {
    let mut h = hit();
    h.path = "src/other.rs".into();
    let s = metrics::native(&[h], &q());
    assert_eq!(s.top1, 0.0);
    assert_eq!(s.recall10, Some(0.0));
}
#[test]
fn native_supporting_top1_does_not_answer_primary() {
    let s = metrics::native(&[Hit::path_only("src/caller.rs".into())], &q());
    assert_eq!(s.top1, 0.0);
    assert_eq!(s.recall10, Some(0.5));
}
#[test]
fn native_duplicate_chunks_do_not_inflate_groups() {
    let s = metrics::native(&[hit(), hit()], &q());
    assert_eq!(s.recall10, Some(0.5));
    assert_eq!(s.mrr10, Some(1.0));
}
#[test]
fn whole_file_return_is_not_perfect_span_precision() {
    let mut h = hit();
    h.span = Some(ByteSpan { start: 0, end: 100 });
    let s = metrics::native(&[h], &q());
    assert_eq!(s.span_precision, Some(0.1));
    assert_eq!(s.span_recall, Some(1.0));
}
#[test]
fn malformed_public_result_is_not_no_match() {
    assert!(normalizer::mcp(&json!({})).is_err());
    assert!(normalizer::mcp(&json!({"nodes":[3]})).is_err());
    assert!(normalizer::oce(&json!({"formatted_retrieval":"unexpected new syntax"})).is_err());
}
#[test]
fn oce_path_parser_preserves_first_unique_order() {
    let (h, _) = normalizer::oce(
        &json!({"formatted_retrieval":"Path: a.rs\ncode\nPath: a.rs\nPath: b.rs\n"}),
    )
    .unwrap();
    assert_eq!(
        h.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
        vec!["a.rs", "b.rs"]
    );
}
#[test]
fn source_validation_catches_wrong_text_and_accepts_crlf() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("a.rs"), "α\r\nbeta\r\n").unwrap();
    let mut h = Hit {
        path: "a.rs".into(),
        text: Some("α\nbeta".into()),
        start_line: Some(1),
        end_line: Some(2),
        ..Hit::default()
    };
    normalizer::verify_source(&mut h, t.path()).unwrap();
    assert_eq!(h.evidence_valid, Some(true));
    assert_eq!(h.span, Some(ByteSpan { start: 0, end: 8 }));
    h.text = Some("not the source".into());
    normalizer::verify_source(&mut h, t.path()).unwrap();
    assert_eq!(h.evidence_valid, Some(false));
}
#[test]
fn invalid_paths_rejected() {
    for p in [
        "../a.rs",
        "/a.rs",
        "a/../b.rs",
        "a\\b.rs",
        "C:a.rs",
        "./a.rs",
        "a//b.rs",
    ] {
        assert!(validation::relative_path(p).is_err(), "{p}");
    }
}
#[test]
fn duplicate_ids_and_cross_split_family_rejected() {
    let a = q();
    assert!(validation::queries(&[a.clone(), a.clone()], ScoreProfile::Native).is_err());
    let mut b = a.clone();
    b.id = "q2".into();
    b.split = "holdout".into();
    assert!(validation::queries(&[a, b], ScoreProfile::Native).is_err());
}
#[test]
fn readiness_never_drops_failed_or_unknown_inputs() {
    assert_eq!(Readiness::new(3, 1, 1, 1, 0).unwrap().state, State::Partial);
    assert_eq!(Readiness::new(1, 0, 0, 0, 1).unwrap().state, State::Unknown);
    assert!(Readiness::new(3, 3, 1, 0, 0).is_err());
    assert!(Readiness::new(0, 0, 0, 0, 0).is_err());
}
#[test]
fn rss_units_and_small_sample_labels() {
    assert_eq!(sampler::high_water_bytes("macos", 20), Some(20));
    assert_eq!(sampler::high_water_bytes("linux", 20), Some(20480));
    assert_eq!(sampler::high_water_bytes("unknown", 20), None);
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    assert!(sampler::sample("unit", None)
        .runner_native_rss_bytes
        .is_some());
    assert_eq!(
        statistics::distribution(&[1, 2, 100]).tail_claim,
        "insufficient_for_tail_claim"
    );
}
#[test]
fn shuffle_and_bootstrap_reproducible() {
    assert_eq!(statistics::order(50, 7), statistics::order(50, 7));
    let a = statistics::bootstrap(&[1.0, 1.0], 17).unwrap();
    assert_eq!((a.low, a.high), (1.0, 1.0));
}
