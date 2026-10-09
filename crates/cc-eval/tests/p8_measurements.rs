use cc_eval::benchmark::{
    self as b,
    adapters::Backend,
    manifest::{self, FileRecord},
    readiness::Readiness,
    report,
    sampler::{self, CostBasis, CostReceipt, DiskComponent, DiskPartition},
    schema::{ResultStatus, SearchInput},
    statistics::{
        self, LatencyEvidence, LatencySample, LatencyStratum, MeasurementOperation,
        ResultCacheObservation,
    },
};
use serde_json::{json, Value};

#[test]
fn distribution_and_interval_keep_one_nearest_rank_convention() {
    for n in [1usize, 2, 3, 19, 20, 21, 199, 200, 201] {
        // Deliberately unsorted, with ties and zero timings. Expected ranks
        // use integer arithmetic, independently of the implementation.
        let samples: Vec<u64> = (0..n).rev().map(|i| (i / 3) as u64).collect();
        let summary = statistics::distribution(&samples);
        let expected_p50 = ((n.div_ceil(2) - 1) / 3) as u64;
        let expected_p95 = (((n * 95).div_ceil(100) - 1) / 3) as u64;
        assert_eq!(summary.p50_us, Some(expected_p50));
        assert_eq!(summary.p95_us, Some(expected_p95));
        assert_eq!(
            statistics::quantile_interval(&samples, 0.5)
                .unwrap()
                .estimate_us,
            expected_p50
        );
        assert_eq!(
            statistics::quantile_interval(&samples, 0.95)
                .unwrap()
                .estimate_us,
            expected_p95
        );
    }
    assert_eq!(statistics::distribution(&[]).p95_us, None);
    for quantile in [0.0, 1.0, f64::NAN, f64::INFINITY] {
        assert!(statistics::quantile_interval(&[0, 1], quantile).is_none());
    }
    assert!(statistics::quantile_interval(&[], 0.5).is_none());
}

fn warm(cache: ResultCacheObservation) -> LatencyEvidence {
    LatencyEvidence {
        operation: MeasurementOperation::Query,
        index_was_empty: Some(false),
        first_query_in_process: Some(false),
        warmup_completed: Some(true),
        result_cache: cache,
        ..LatencyEvidence::default()
    }
}

#[test]
fn lifecycle_requires_direct_consistent_evidence() {
    assert_eq!(
        LatencyEvidence::default().classify(),
        LatencyStratum::Unknown
    );
    let mut evidence = LatencyEvidence {
        operation: MeasurementOperation::Build,
        index_was_empty: Some(true),
        parse_cache_was_empty: Some(true),
        ..LatencyEvidence::default()
    };
    assert_eq!(evidence.classify(), LatencyStratum::ColdBuild);
    evidence.reopened_existing_index = Some(true);
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    evidence.reopened_existing_index = None;
    evidence.parse_cache_was_empty = None;
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    evidence.parse_cache_was_empty = Some(false);
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    evidence = LatencyEvidence {
        operation: MeasurementOperation::Query,
        index_was_empty: Some(false),
        first_query_in_process: Some(true),
        warmup_completed: Some(false),
        ..LatencyEvidence::default()
    };
    // First query after an in-process cold build is not evidence of reopening.
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    evidence.reopened_existing_index = Some(false);
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    evidence.reopened_existing_index = Some(true);
    assert_eq!(evidence.classify(), LatencyStratum::ProcessReopen);
    evidence.warmup_completed = Some(true);
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    evidence.warmup_completed = Some(false);
    evidence.result_cache = ResultCacheObservation::Hit;
    assert_eq!(evidence.classify(), LatencyStratum::Unknown);
    assert_eq!(
        warm(ResultCacheObservation::Miss).classify(),
        LatencyStratum::WarmUncached
    );
    assert_eq!(
        warm(ResultCacheObservation::Hit).classify(),
        LatencyStratum::CacheHit
    );
    assert_eq!(
        warm(ResultCacheObservation::Unknown).classify(),
        LatencyStratum::Unknown
    );
    let mut contradictory = warm(ResultCacheObservation::Hit);
    contradictory.index_was_empty = Some(true);
    assert_eq!(contradictory.classify(), LatencyStratum::Unknown);
}

#[test]
fn full_attempt_denominator_retains_timeouts_errors_partials_and_missing_times() {
    let statuses = [
        ResultStatus::Success,
        ResultStatus::NoMatch,
        ResultStatus::Partial,
        ResultStatus::Timeout,
        ResultStatus::ToolError,
        ResultStatus::ProtocolError,
        ResultStatus::Cancelled,
    ];
    let samples: Vec<_> = statuses
        .into_iter()
        .enumerate()
        .map(|(i, status)| LatencySample {
            evidence: warm(ResultCacheObservation::Miss),
            status,
            elapsed_us: if i == 6 {
                None
            } else {
                Some((i + 1) as u64 * 100)
            },
        })
        .collect();
    let result = statistics::latency_layers(&samples, 9);
    assert_eq!(result.recorded_samples, 7);
    assert_eq!(result.missing_samples, 2);
    let layer = result
        .layers
        .iter()
        .find(|l| l.stratum == LatencyStratum::WarmUncached)
        .unwrap();
    assert_eq!(
        (
            layer.completed,
            layer.partial,
            layer.errors,
            layer.timeouts,
            layer.cancelled
        ),
        (2, 1, 2, 1, 1)
    );
    assert_eq!(layer.missing_timings, 1);
    assert_eq!(layer.all_attempt_elapsed.samples, 6);
    assert_eq!(layer.all_attempt_elapsed.max_us, Some(600));
    assert_eq!(layer.completed_elapsed.samples, 2);
    assert_eq!(layer.timeout_rate, Some(1.0 / 7.0));
    assert_eq!(layer.error_rate, Some(2.0 / 7.0));
    assert!(layer.statistical_status.starts_with("inconclusive"));
    assert_eq!(
        statistics::latency_layers(&samples, 3).unexpected_samples,
        4
    );
}

#[test]
fn tail_intervals_do_not_disguise_small_sample_or_unbounded_p99() {
    assert!(statistics::quantile_interval(&[], 0.95).is_none());
    for p in [0.0, 1.0, -0.1, f64::NAN, f64::INFINITY] {
        assert!(statistics::quantile_interval(&[1, 2], p).is_none());
    }
    let values: Vec<_> = (1..=200).collect();
    let p95 = statistics::quantile_interval(&values, 0.95).unwrap();
    assert!(p95.lower_us.unwrap() < 190);
    assert!(p95.upper_us.unwrap() > 190);
    let p99 = statistics::quantile_interval(&values, 0.99).unwrap();
    assert_eq!(p99.estimate_us, 198);
    assert!(p99.upper_us.is_none()); // 0.99^200 > 0.025: no finite upper bound.
    let samples: Vec<_> = values
        .iter()
        .map(|v| LatencySample {
            evidence: warm(ResultCacheObservation::Miss),
            status: ResultStatus::Success,
            elapsed_us: Some(*v),
        })
        .collect();
    let layers = statistics::latency_layers(&samples, 200);
    assert_eq!(
        layers.layers[2].statistical_status,
        "inconclusive_unbounded_tail_interval"
    );
    assert_eq!(
        statistics::latency_layers(&samples[..30], 30).layers[2].statistical_status,
        "inconclusive_insufficient_tail_samples"
    );
    let p99_large = statistics::quantile_interval(&(1..=1000).collect::<Vec<_>>(), 0.99).unwrap();
    assert!(p99_large.upper_us.is_some());
    // A constant sample may have equal endpoints, but is not a passed gate.
    let constant = vec![samples[0].clone(); 1000];
    assert_eq!(
        statistics::latency_layers(&constant, 1000).layers[2].statistical_status,
        "observations_only_requires_controlled_release_profile"
    );
}

#[test]
fn binomial_interval_matches_enumerated_small_sample_coverage() {
    // Independent binomial coefficients for B~Binomial(10, 1/2). A 95%
    // central interval uses X_2..X_9; each outside tail has mass 11/1024.
    let interval = statistics::quantile_interval(&(1..=10).collect::<Vec<_>>(), 0.5).unwrap();
    assert_eq!(interval.lower_us, Some(2));
    assert_eq!(interval.upper_us, Some(9));
    let mut reversed: Vec<_> = (1..=10).rev().collect();
    assert_eq!(
        serde_json::to_value(interval).unwrap(),
        serde_json::to_value(statistics::quantile_interval(&reversed, 0.5).unwrap()).unwrap()
    );
    reversed.push(u64::MAX);
    assert!(statistics::quantile_interval(&reversed, 0.5).is_some());
}

#[test]
fn ps_tree_uses_checked_sum_and_never_discards_unreadable_children() {
    let good = sampler::ps_resources(
        "query",
        10,
        Some(20),
        Some("10 1 100\n20 10 200\n21 20 300\n30 1 999\n"),
    );
    assert_eq!(good.runner_rss_bytes, Some(102_400));
    assert_eq!(good.server_rss_bytes, Some(204_800));
    assert_eq!(good.server_tree_rss_bytes, Some(512_000));
    for table in [
        "10 1 100\n20 10 200\n21 20 unreadable\n".to_owned(),
        "10 1 100\n20 10 200\nunknown topology\n".to_owned(),
        format!("10 1 100\n20 10 200\n21 20 {}\n", u64::MAX),
        format!(
            "10 1 100\n20 10 {}\n21 20 {}\n",
            u64::MAX / 1024,
            u64::MAX / 1024
        ),
        "10 1 100\n20 10 200\n21 20 300\n21 20 300\n".to_owned(),
        "10 1 100\n20 21 200\n21 20 300\n".to_owned(),
    ] {
        let partial = sampler::ps_resources("query", 10, Some(20), Some(&table));
        assert_eq!(partial.runner_rss_bytes, Some(102_400));
        assert_eq!(
            partial.server_tree_rss_bytes, None,
            "accepted partial/overflow: {table}"
        );
        assert!(partial.method.contains("partial"));
    }
    let missing = sampler::ps_resources("query", 10, Some(42), Some("10 1 100\n20 10 200\n"));
    assert!(missing.server_rss_bytes.is_none());
    assert!(missing.server_tree_rss_bytes.is_none());
    let unavailable = sampler::ps_resources("query", 10, Some(20), None);
    assert!(unavailable.runner_rss_bytes.is_none());
    assert!(unavailable.server_tree_rss_bytes.is_none());
}

#[test]
fn measured_zero_differs_from_missing_and_memory_alternatives_are_not_added() {
    let mut sample = sampler::ps_resources("before", 10, Some(20), Some("10 1 0\n20 10 0\n"));
    assert_eq!(sample.runner_rss_bytes, Some(0));
    assert_eq!(sample.server_tree_rss_bytes, Some(0));
    sample.runner_native_rss_bytes = Some(300);
    let other = sampler::ps_resources("after", 10, Some(20), None);
    let ledger = sampler::memory_ledger(&[sample, other]);
    assert_eq!(ledger.snapshots, 2);
    assert!(ledger.total_rss_bytes.is_none());
    assert!(ledger.sampling_interval_ms.is_none());
    assert_eq!(ledger.roles[0].peak_observed_bytes, Some(0));
    assert_eq!(ledger.roles[1].peak_observed_bytes, Some(300));
    assert_eq!(ledger.roles[0].root_pids, vec![10]);
    assert_eq!(ledger.roles[3].root_pids, vec![20]);
    assert!(ledger.roles[5].root_pids.is_empty());
    assert_eq!(ledger.roles[0].status, "partial");
    assert_eq!(ledger.roles[0].unavailable_samples, 1);
    assert_eq!(ledger.roles[5].status, "unavailable");
    assert_eq!(ledger.roles[6].status, "unavailable");
}

fn disk(identity: &str, component: DiskComponent, bytes: Option<u64>) -> DiskPartition {
    DiskPartition {
        storage_id: identity.into(),
        component,
        bytes,
    }
}

#[test]
fn disk_layout_counts_shared_storage_once_and_never_totals_partial_coverage() {
    let shared = disk("inode-1", DiskComponent::Shared, Some(100));
    let cache = disk("inode-2", DiskComponent::ParseCache, Some(20));
    let rows = [shared.clone(), shared.clone(), cache];
    let result = sampler::disk_ledger(&rows, true).unwrap();
    assert_eq!(result.duplicate_observations, 1);
    assert_eq!(result.total_bytes, Some(120));
    assert_eq!(
        sampler::disk_ledger(&rows, false).unwrap().total_bytes,
        None
    );
    let unavailable = sampler::disk_ledger(&[], false).unwrap();
    assert_eq!(unavailable.status, "unavailable");
    assert!(unavailable.total_bytes.is_none());
    let partial = sampler::disk_ledger(
        &[shared, disk("vector-1", DiskComponent::Vector, None)],
        true,
    )
    .unwrap();
    assert_eq!(partial.measured_subtotal_bytes, Some(100));
    assert_eq!(partial.total_bytes, None);
    assert_eq!(partial.status, "partial");
}

#[test]
fn disk_conflicts_empty_identity_and_overflow_cannot_produce_valid_total() {
    assert!(sampler::disk_ledger(
        &[
            disk("same", DiskComponent::Index, Some(1)),
            disk("same", DiskComponent::Fts, Some(1))
        ],
        true
    )
    .is_err());
    assert!(sampler::disk_ledger(&[disk("", DiskComponent::Index, Some(1))], true).is_err());
    let overflow = sampler::disk_ledger(
        &[
            disk("a", DiskComponent::Index, Some(u64::MAX)),
            disk("b", DiskComponent::Fts, Some(1)),
        ],
        true,
    )
    .unwrap();
    assert_eq!(overflow.status, "overflow");
    assert_eq!(overflow.measured_subtotal_bytes, None);
    assert_eq!(overflow.total_bytes, None);
}

fn receipt(id: &str, basis: CostBasis, amount: Option<u64>) -> CostReceipt {
    CostReceipt {
        receipt_id: id.into(),
        basis,
        currency: "USD".into(),
        amount_microunits: amount,
        requests: Some(1),
        input_tokens: None,
        output_tokens: None,
        cache_hits: None,
        duplicate_charge_uncertain: false,
    }
}

#[test]
fn costs_separate_currency_basis_and_deduplicate_receipts_without_assuming_usage() {
    let reported = receipt("bill-1", CostBasis::Reported, Some(100));
    let estimated = receipt("estimate-1", CostBasis::Estimated, Some(500));
    let mut other_currency = receipt("bill-2", CostBasis::Reported, Some(700));
    other_currency.currency = "CNY".into();
    let result =
        sampler::cost_ledger(&[reported.clone(), reported, estimated, other_currency]).unwrap();
    assert_eq!(result.duplicate_observations, 1);
    assert_eq!(result.totals.len(), 3);
    let usd = result
        .totals
        .iter()
        .find(|t| t.currency == "USD" && t.basis == CostBasis::Reported)
        .unwrap();
    assert_eq!(usd.amount_microunits, Some(100));
    assert_eq!(usd.requests, Some(1));
    assert!(usd.input_tokens.is_none());
    assert!(usd.cache_hits.is_none());
    assert_eq!(sampler::cost_ledger(&[]).unwrap().status, "unavailable");
}

#[test]
fn costs_reject_conflicts_and_preserve_missing_overflow_and_duplicate_charge_uncertainty() {
    assert!(sampler::cost_ledger(&[
        receipt("same", CostBasis::Reported, Some(1)),
        receipt("same", CostBasis::Reported, Some(2))
    ])
    .is_err());
    let mut invalid = receipt("valid", CostBasis::Reported, Some(1));
    invalid.currency = "usd".into();
    assert!(sampler::cost_ledger(&[invalid]).is_err());
    let missing = sampler::cost_ledger(&[
        receipt("1", CostBasis::Reported, Some(1)),
        receipt("2", CostBasis::Reported, None),
    ])
    .unwrap();
    assert!(missing.totals[0].amount_microunits.is_none());
    let overflow = sampler::cost_ledger(&[
        receipt("1", CostBasis::Reported, Some(u64::MAX)),
        receipt("2", CostBasis::Reported, Some(1)),
    ])
    .unwrap();
    assert!(overflow.totals[0].amount_microunits.is_none());
    let mut uncertain = receipt("1", CostBasis::Reported, Some(0));
    uncertain.duplicate_charge_uncertain = true;
    let result = sampler::cost_ledger(&[uncertain]).unwrap();
    assert_eq!(result.totals[0].amount_microunits, Some(0));
    assert_eq!(result.totals[0].duplicate_charge_uncertain_receipts, 1);
    assert_eq!(result.totals[0].status, "duplicate_charge_uncertain");
}

struct TimedStub {
    calls: usize,
}
impl Backend for TimedStub {
    fn name(&self) -> &'static str {
        "fixture-stub-not-product"
    }
    async fn prepare(&mut self, _: &[FileRecord]) -> b::Result<Value> {
        Ok(json!({"fixture":true}))
    }
    async fn readiness(&mut self, f: &[FileRecord]) -> b::Result<Readiness> {
        Readiness::new(f.len(), f.len(), 0, 0, 0)
    }
    async fn search(&mut self, _: &SearchInput) -> b::Result<Value> {
        self.calls += 1;
        if self.calls == 2 {
            Err(b::BenchError::Timeout("fixture deadline".into()))
        } else {
            Ok(json!({"nodes":[],"cache_hit":true}))
        } // Unversioned/unsupported hint must be ignored.
    }
    async fn close(&mut self) -> b::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn existing_runner_and_replay_emit_all_attempt_and_resource_ledgers() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::create_dir(base.join("source")).unwrap();
    std::fs::write(base.join("source/a.py"), "def target():\n    return 1\n").unwrap();
    let q = json!({"id":"q","category":"symbol_location","difficulty":1,"language":"python","split":"dev","query_family":"target","query":"missing","path_prefix":null,"no_answer":true,"expected_files":[],"answers":[]});
    std::fs::write(base.join("queries.jsonl"), format!("{q}\n")).unwrap();
    let suite = base.join("suite.json");
    report::json(&suite, &json!({"schema_version":1,"name":"cache-hit-name-is-not-proof","source":{"root":"source","commit":null,"digest":"","files":["a.py"]},"queries":"queries.jsonl","queries_digest":"","scoring":"codecortex-native-v1","repetitions":3,"warmup":0,"seed":7,"timeout_ms":1000,"top_k":10,"engine_config":{"auto_index":{"enabled":false}}})).unwrap();
    report::json(&suite, &manifest::freeze(&suite).unwrap()).unwrap();
    let loaded = manifest::load(&suite).unwrap();
    let work = manifest::materialize(&loaded).unwrap();
    let out = base.join("run");
    std::fs::create_dir(&out).unwrap();
    let gate = b::runner::run(
        &loaded,
        TimedStub { calls: 0 },
        work.path(),
        &out,
        json!({"fixture":true}),
        "warm-cache-hit",
    )
    .await
    .unwrap();
    assert_ne!(gate.exit_code, 0);
    let layers: Value = manifest::json_file(&out.join("latency-strata.json")).unwrap();
    assert_eq!(layers["recorded_samples"], 3);
    assert_eq!(layers["layers"][4]["samples"], 3);
    assert_eq!(layers["layers"][4]["timeouts"], 1);
    assert_eq!(layers["layers"][4]["all_attempt_elapsed"]["samples"], 3);
    assert_eq!(layers["layers"][4]["completed_elapsed"]["samples"], 2);
    assert_eq!(layers["layers"][3]["samples"], 0);
    let legacy: Value = manifest::json_file(&out.join("latency-summary.json")).unwrap();
    assert_eq!(legacy["samples"], 2);
    let ledger: Value = manifest::json_file(&out.join("resource-ledger.json")).unwrap();
    assert_eq!(ledger["memory"]["snapshots"], 5);
    assert_eq!(ledger["disk"]["status"], "unavailable");
    assert_eq!(ledger["cost"]["status"], "unavailable");
    assert!(ledger["memory"]["total_rss_bytes"].is_null());
    let files = [
        "latency-strata.json",
        "resource-ledger.json",
        "latency-summary.json",
        "metrics.json",
        "costs.jsonl",
    ];
    let before: Vec<_> = files
        .iter()
        .map(|p| std::fs::read(out.join(p)).unwrap())
        .collect();
    assert_eq!(report::replay(&out).unwrap().exit_code, gate.exit_code);
    for (file, bytes) in files.iter().zip(before) {
        assert_eq!(
            bytes,
            std::fs::read(out.join(file)).unwrap(),
            "replay drift: {file}"
        );
    }
    // Older artifacts can omit resources; no missing measurement becomes zero.
    std::fs::remove_file(out.join("resources.jsonl")).unwrap();
    report::replay(&out).unwrap();
    let unavailable: Value = manifest::json_file(&out.join("resource-ledger.json")).unwrap();
    assert!(unavailable["resource_source_digest"].is_null());
    assert_eq!(unavailable["memory"]["snapshots"], 0);
    assert!(unavailable["memory"]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .all(|role| role["peak_observed_bytes"].is_null()));
    std::fs::write(out.join("resources.jsonl"), "{\"stage\":\"corrupt\"}\n").unwrap();
    assert!(report::replay(&out).is_err());
}
